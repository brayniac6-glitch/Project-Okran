//! How fast a character attacks and defends: weapon weight against strength sets a handling
//! factor, and dexterity with the attack (or defence) level sets the speed within the original's
//! bounds.
//!
//! Specs: sheet rows F047010 CharStats::weaponStrain, F047011 CharStats::weaponHandling,
//! F047012 CharStats::combatSpeed, F047065 CharStats::update (the speeds part); type T00025
//! GlobalConstants.

use crate::medical::lerp;

/// The GLOBAL CONSTANTS the weapon strain reads (shipped values as defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrainConstants {
    /// "min strength xp mult"
    pub min_strength_xp_mult: f32,
    /// "weight strength diff 1x" (stored as a float)
    pub weight_strength_diff_1x: f32,
    /// "weight strength diff max"
    pub weight_strength_diff_max: f32,
}

impl Default for StrainConstants {
    fn default() -> Self {
        StrainConstants {
            min_strength_xp_mult: 0.1,
            weight_strength_diff_1x: 20.0,
            weight_strength_diff_max: 1.0,
        }
    }
}

impl StrainConstants {
    /// @type GlobalConstants
    pub fn from_record(r: &kenshi_data::GameData) -> Self {
        let d = Self::default();
        let f = |k: &str, v: f32| {
            r.floats
                .get(k)
                .copied()
                .or_else(|| r.ints.get(k).map(|i| *i as f32))
                .unwrap_or(v)
        };
        StrainConstants {
            min_strength_xp_mult: f("min strength xp mult", d.min_strength_xp_mult),
            weight_strength_diff_1x: f("weight strength diff 1x", d.weight_strength_diff_1x),
            weight_strength_diff_max: f("weight strength diff max", d.weight_strength_diff_max),
        }
    }
}

/// @fn F047010
///
/// How much a weapon of `weight` strains its wielder (CharStats +0x184; also the strength XP
/// multiplier): the weight beyond `arms` (the arms' fitness for strength, F040433) * the strength
/// level (CharStats vtable 0x10), per "weight strength diff 1x", capped, plus the minimum.
pub fn weapon_strain(weight: f32, arms: f32, strength_level: f32, c: &StrainConstants) -> f32 {
    let excess =
        ((1.0 / c.weight_strength_diff_1x as f64) * (weight - arms * strength_level) as f64) as f32;
    let excess = if excess < 0.0 { 0.0 } else { excess };
    let excess = if c.weight_strength_diff_max < excess {
        c.weight_strength_diff_max
    } else {
        excess
    };
    c.min_strength_xp_mult + excess
}

/// @fn F047011
///
/// The weapon handling factor (CharStats +0x180): strain slows from 1 toward 0.5, and so does a
/// heavy load (`load` +0x190, counted from half); `mult` is CharStats +0x1c.
pub fn weapon_handling(strain: f32, load: f32, mult: f32, c: &StrainConstants) -> f32 {
    let f = (strain / c.weight_strength_diff_max).clamp(0.0, 1.0);
    let l = ((load as f64 + 0.5) as f32).min(1.0);
    lerp(l * (1.0 - f), 0.5, 1.0) * mult
}

/// @fn F047012
///
/// The attack speed (CharStats +0x188, `defence` false) or defence speed (+0x18c, true):
/// lerp((dexterity * 0.6 + level * 0.4) * 0.01) between 0.8 and 1.2 (attack) or 0.6 and 4
/// (defence), times the handling factor, capped at the top. `dexterity` is the dexterity level:
/// the limbs' dexterity fitness (F040433) * dexterity (+0x88) * CharStats +0x34.
pub fn combat_speed(handling: f32, dexterity: f32, level: f32, defence: bool) -> f32 {
    let (lo, hi) = if defence { (0.6, 4.0) } else { (0.8, 1.2) };
    let s = lerp((dexterity * 0.6 + level * 0.4) * 0.01, lo, hi) * handling;
    if hi < s { hi } else { s }
}

/// @fn F047065
///
/// Both speeds as CharStats::update sets them: `attack` is the attack level (martial arts when
/// unarmed, else (weapon skill + attack bonus +0x120) * maturity), `defence` the defence level
/// (+0x24, plus +0x28 with a weapon drawn, plus the defence bonus +0x124, plus 20 when the
/// character fights unarmed-style, F? 1cf35); deep water (water state 3) slows both to 0.8.
pub fn speeds(
    handling: f32,
    dexterity: f32,
    attack: f32,
    defence: f32,
    deep_water: bool,
) -> (f32, f32) {
    let w = if deep_water { 0.8 } else { 1.0 };
    (
        w * combat_speed(handling, dexterity, attack, false),
        w * combat_speed(handling, dexterity, defence, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_weapons_strain_weak_wielders() {
        let c = StrainConstants::default();
        // well within strength: only the minimum
        assert_eq!(weapon_strain(6.0, 1.0, 30.0, &c), 0.1);
        // 10 kg beyond strength: half of "1x"
        assert!((weapon_strain(40.0, 1.0, 30.0, &c) - 0.6).abs() < 1e-6);
        // capped at max
        assert!((weapon_strain(200.0, 1.0, 30.0, &c) - 1.1).abs() < 1e-6);
        // handling: unburdened, light weapon
        assert!((weapon_handling(0.1, 1.0, 1.0, &c) - 0.95).abs() < 1e-6);
        assert_eq!(weapon_handling(1.1, 1.0, 1.0, &c), 0.5);
    }

    #[test]
    fn speeds_stay_in_bounds() {
        assert!((combat_speed(1.0, 0.0, 0.0, false) - 0.8).abs() < 1e-6);
        assert!((combat_speed(1.0, 100.0, 100.0, false) - 1.2).abs() < 1e-6);
        assert_eq!(combat_speed(2.0, 100.0, 100.0, false), 1.2);
        assert!((combat_speed(1.0, 50.0, 50.0, true) - 2.3).abs() < 1e-5);
        let (a, d) = speeds(1.0, 0.0, 0.0, 0.0, true);
        assert!((a - 0.64).abs() < 1e-6 && (d - 0.48).abs() < 1e-6);
    }
}
