//! The damage one swing carries: weapon (or unarmed) damage, scaled for the kind and race of the
//! target.
//!
//! Specs: sheet rows F047066 CharStats::attackDamage, F047041 CharStats::martialArts; type
//! T00026 CharStats.

use crate::hit::Hit;
use crate::weapon::Wielded;
use crate::{Attacker, CombatConstants, WeaponCategory, blunt_damage, cut_damage, pierce_damage};
use kenshi_char::medical::lerp;

/// What kind of character is being hit (picks the weapon's damage multiplier).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetKind {
    Robot,
    Animal,
    Human,
    Other,
}

/// The target of a swing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target<'a> {
    pub kind: TargetKind,
    /// The target's race string id (for the weapon's race damage).
    pub race: &'a str,
}

/// Inputs for unarmed damage that come from the character rather than the weapon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unarmed {
    /// Martial arts level (F047041).
    pub level: f32,
    /// CharStats +0x90 and +0x34.
    pub toughness: f32,
    pub toughness_scale: f32,
    /// GLOBAL CONSTANTS "unarmed damage mult".
    pub unarmed_damage_mult: f32,
    /// The arms' fitness for fighting (F040433, activity 1).
    pub limbs: f32,
    /// The strength level (CharStats vtable 0x10: strength * [+0x34], F040360).
    pub char_factor: f32,
}

/// @fn F047041
///
/// `load` is CharStats +0x190, `bonus` the attack bonus (F047030), `unarmed` the unarmed skill,
/// `char_factor` the character's factor (vtable 0x398).
pub fn martial_arts(load: f32, bonus: i32, unarmed: f32, char_factor: f32) -> f32 {
    lerp(load, 0.2, 1.0) * (bonus as f32 + unarmed) * char_factor
}

/// @fn F047066
///
/// `damage_mult` is CharStats +0x48. The swing carries no stun; resolveHit builds stun from armour.
pub fn attack_damage(
    w: &Wielded,
    a: &Attacker,
    c: &CombatConstants,
    unarmed: &Unarmed,
    target: Option<Target>,
    damage_mult: f32,
) -> Hit {
    let mut h = Hit {
        cut: cut_damage(a, c),
        blunt: blunt_damage(a, c),
        pierce: pierce_damage(a, c),
        bleed: w.bleed,
        penetration: 0.0,
    };
    if w.category == WeaponCategory::SkillUnarmed {
        let u = unarmed.level * 0.01;
        let mut total = lerp(u, 0.25, 1.0)
            * (unarmed.toughness * unarmed.toughness_scale * unarmed.unarmed_damage_mult
                + unarmed.limbs * unarmed.char_factor
                + 10.0);
        if total < 1.0 {
            total = 1.0;
        }
        let f = lerp(u, 1.0, 0.4);
        h.blunt = f * total;
        h.cut = (1.0 - f) * total;
        h.pierce = 0.0;
    }
    if let Some(t) = target {
        let mut k = match t.kind {
            TargetKind::Robot => w.robot_damage_mult,
            TargetKind::Animal => w.animal_damage_mult,
            TargetKind::Human => w.human_damage_mult,
            TargetKind::Other => 1.0,
        };
        h.penetration = w.armour_penetration;
        if let Some(r) = w.race_damage.get(t.race) {
            k *= r;
        }
        h.cut *= k;
        h.blunt *= k;
        h.pierce *= k;
    }
    if damage_mult != 1.0 && 0.0 < damage_mult {
        h.cut *= damage_mult;
        h.blunt *= damage_mult;
        h.pierce *= damage_mult;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weapon::set_weapon;

    fn no_unarmed() -> Unarmed {
        Unarmed {
            level: 0.0,
            toughness: 0.0,
            toughness_scale: 1.0,
            unarmed_damage_mult: 1.0,
            limbs: 0.0,
            char_factor: 1.0,
        }
    }

    #[test]
    fn fists_turn_from_blunt_to_cut_with_skill() {
        let w = set_weapon(None, 20.0, 20.0);
        let a = w.attacker(0.0, (0.0, 1.0), (0.0, 1.0), 1.0);
        let c = CombatConstants::default();
        let novice = attack_damage(&w, &a, &c, &no_unarmed(), None, 1.0);
        // lerp(0, 0.25, 1) * 10 = 2.5, all blunt
        assert_eq!((novice.blunt, novice.cut, novice.pierce), (2.5, 0.0, 0.0));
        let master = Unarmed {
            level: 100.0,
            ..no_unarmed()
        };
        let m = attack_damage(&w, &a, &c, &master, None, 1.0);
        assert!((m.blunt - 4.0).abs() < 1e-5 && (m.cut - 6.0).abs() < 1e-5);
    }

    #[test]
    fn target_kind_and_race_scale_damage() {
        let mut w = set_weapon(None, 20.0, 20.0);
        w.category = WeaponCategory::SkillKatanas;
        w.cut_factor = 1.0;
        w.cut_base = 20.0;
        w.robot_damage_mult = 0.5;
        w.armour_penetration = 0.2;
        w.race_damage.insert("skeleton".into(), 0.5);
        let a = w.attacker(100.0, (100.0, 1.0), (0.0, 1.0), 1.0);
        let c = CombatConstants::default();
        let t = Target {
            kind: TargetKind::Robot,
            race: "skeleton",
        };
        let h = attack_damage(&w, &a, &c, &no_unarmed(), Some(t), 2.0);
        assert_eq!(h.cut, 80.0 * 0.5 * 0.5 * 2.0);
        assert_eq!(h.penetration, 0.2);
    }
}
