//! Encumbrance and movement speeds: how carried weight, athletics and injuries set a character's
//! speed.
//!
//! Specs: sheet rows F040360 CharStats::strengthLevel, F047034 CharStats::computeLoad,
//! F047455 CharStats::strengthXpFromWalking, F047018 CharStats::runSpeed, F047058 CharStats::swimSpeed,
//! F047064 CharStats::updateMoveSpeed, F047037 legFitness, F040424 MedicalSystem::hungerSpeed;
//! type T00025 GlobalConstants.

use crate::medical::invlerp;

/// The constants encumbrance reads (GLOBAL CONSTANTS, shipped values as defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncumbranceConstants {
    pub carry_weight_mult: f32,
    pub encumbrance_base: f32,
    /// "carry person weight": what one carried adds to the carrier's weight (F039146).
    pub carry_person_weight: f32,
    pub xp_rate_strength_from_walking: f32,
}

impl Default for EncumbranceConstants {
    fn default() -> Self {
        EncumbranceConstants {
            carry_weight_mult: 1.2,
            encumbrance_base: 15.0,
            carry_person_weight: 30.0,
            xp_rate_strength_from_walking: 0.5,
        }
    }
}

/// @fn F039146
///
/// The weight a character carries (computeLoad's): its inventory's, plus "carry person weight"
/// while it carries someone (Character +0x348).
pub fn carried_weight(inventory: f32, carrying_someone: bool, c: &EncumbranceConstants) -> f32 {
    if carrying_someone { inventory + c.carry_person_weight } else { inventory }
}

/// @fn F040360
pub fn strength_level(strength: f32, scale: f32) -> f32 {
    strength * scale
}

/// @fn F047034
///
/// The load factor (CharStats +0x190): 1 while `carried` is within capacity, falling to 0 at
/// twenty times it. `arms` is the arms' fitness (F040433, activity 1).
pub fn load(carried: f32, arms: f32, strength_level: f32, c: &EncumbranceConstants) -> f32 {
    let capacity = c.carry_weight_mult * arms * strength_level + c.encumbrance_base;
    let l = invlerp(capacity, carried * 0.05, carried);
    // carrying nothing divides by zero: +inf clamps to 1; NaN would pass through, as in the original
    #[allow(clippy::manual_clamp)]
    {
        let l = if 0.0 <= l { l } else { 0.0 };
        if l <= 1.0 { l } else { 1.0 }
    }
}

/// @fn F047455
///
/// Strength training from walking with a load: `carrying_someone` doubles it.
pub fn strength_xp_from_walking(
    load: f32,
    carrying_someone: bool,
    c: &EncumbranceConstants,
) -> f32 {
    let p = 1.0 - load;
    if p <= 0.0 {
        return 0.0;
    }
    let v = (p * p).min(0.5);
    let v = if carrying_someone { v * 2.0 } else { v };
    c.xp_rate_strength_from_walking * v
}

/// @fn F047018
///
/// Running speed: athletics (0..100) moves it from the race's "speed min skill" to "speed max
/// skill"; `maturity` (Character vtable 0x398: 1 for people, an animal's growth 0..1) slows it to half.
pub fn run_speed(athletics: f32, race_min: f32, race_max: f32, maturity: f32) -> f32 {
    let s = crate::medical::lerp(athletics * 0.01, race_min, race_max);
    ((maturity as f64 * 0.5 + 0.5) * s as f64) as f32
}

/// @fn F047058
///
/// Swimming speed: `legs` is the legs' fitness, `equipment` the product of the limb slots' swim
/// modifiers, `extra` the hunger's (F040424 MedicalSystem::hungerSpeed, through thunk
/// 0x140022179); `legs_out` both legs unusable.
#[allow(clippy::too_many_arguments)]
pub fn swim_speed(
    swim_mult: f32,
    swimming: f32,
    legs: f32,
    left_leg_ok: bool,
    right_leg_ok: bool,
    equipment: f32,
    load: f32,
    extra: f32,
) -> f32 {
    let base = crate::medical::lerp(swim_mult * swimming * 0.02, 6.0, 25.0);
    let mut l = legs;
    if !right_leg_ok {
        l *= 0.6;
    }
    if !left_leg_ok {
        l *= 0.6;
    }
    let s = extra * equipment * base * l * load;
    if !left_leg_ok && !right_leg_ok {
        return 1.0;
    }
    if s < 4.0 { 4.0 } else { s }
}

/// @fn F047037
///
/// The legs' fitness: the worse leg's health fraction * 1.8, clamped to [0, 1].
pub fn leg_fitness(left: f32, right: f32) -> f32 {
    let l = if right < left { right } else { left };
    (l * 1.8).clamp(0.0, 1.0)
}

/// @fn F040424
///
/// Hunger slows: full speed from hunger 2 up, half at hunger 1 or below.
pub fn hunger_speed(hunger: f32) -> f32 {
    crate::medical::lerp((hunger - 1.0).clamp(0.0, 1.0), 0.5, 1.0)
}

/// Where a character stands in water (F039118).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Water {
    Dry = 0,
    Shallow = 1,
    Wading = 2,
    Deep = 3,
}

/// The inputs of the move speed (F047064).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveInputs {
    pub run_speed: f32,
    pub legs: f32,
    pub hunger: f32,
    /// Product of the leg slots' equipment speed modifiers.
    pub equipment: f32,
    pub load: f32,
    /// CharStats +0x18.
    pub mult: f32,
    /// Sneaking, with the stealth level (0..1, F047035).
    pub sneaking: Option<f32>,
    pub water: Water,
    pub swimmer: bool,
    pub walk_speed: f32,
    pub swim_speed: f32,
    pub prone: bool,
    /// Medical flag +0x160.
    pub slowed: bool,
}

/// @fn F047064
pub fn move_speed(m: &MoveInputs) -> f32 {
    let mut s = (m.run_speed - 11.0) * m.legs * m.hunger * m.equipment * m.load * m.mult;
    if let Some(stealth) = m.sneaking {
        let t = crate::medical::invlerp(stealth, 0.1, 0.9).max(0.0);
        let cap = crate::medical::lerp(t, 12.0, s);
        if cap < s {
            s = cap;
        }
    }
    s += 11.0;
    if s < 11.0 {
        s = 11.0;
    }
    match m.water {
        Water::Dry => {}
        Water::Shallow => s = s.min(45.0),
        _ if !m.swimmer => s = m.walk_speed * 2.0,
        Water::Wading => s = m.walk_speed,
        Water::Deep => s = m.swim_speed,
    }
    if m.prone {
        s = 7.0;
    }
    if m.slowed {
        s = 5.0;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads() {
        let c = EncumbranceConstants::default();
        // strength 50: capacity 1.2 * 50 + 15 = 75
        let s = strength_level(50.0, 1.0);
        assert_eq!(load(70.0, 1.0, s, &c), 1.0);
        assert_eq!(load(75.0 * 20.0, 1.0, s, &c), 0.0);
        let half = load(150.0, 1.0, s, &c); // (75 - 7.5) / 142.5
        assert!((half - 67.5 / 142.5).abs() < 1e-6);
        assert_eq!(strength_xp_from_walking(1.0, false, &c), 0.0);
        // Greenlander: 70 at no athletics, 120 at 100; half speed with no fitness
        assert_eq!(run_speed(0.0, 70.0, 120.0, 1.0), 70.0);
        assert_eq!(run_speed(100.0, 70.0, 120.0, 1.0), 120.0);
        assert_eq!(run_speed(50.0, 70.0, 120.0, 0.0), 47.5);
        assert_eq!(swim_speed(1.0, 0.0, 1.0, true, true, 1.0, 1.0, 1.0), 6.0);
        let m = MoveInputs {
            run_speed: 95.0,
            legs: leg_fitness(1.0, 0.5),
            hunger: hunger_speed(2.5),
            equipment: 1.0,
            load: 1.0,
            mult: 1.0,
            sneaking: None,
            water: Water::Dry,
            swimmer: true,
            walk_speed: 15.0,
            swim_speed: 10.0,
            prone: false,
            slowed: false,
        };
        assert_eq!(move_speed(&m), 84.0 * 0.9 + 11.0); // a leg at half health
        assert_eq!(
            move_speed(&MoveInputs {
                sneaking: Some(0.1),
                ..m
            }),
            23.0
        ); // unskilled sneak: 12 + 11
        assert_eq!(
            move_speed(&MoveInputs {
                water: Water::Deep,
                ..m
            }),
            10.0
        );
        assert_eq!(
            move_speed(&MoveInputs {
                water: Water::Wading,
                swimmer: false,
                ..m
            }),
            30.0
        );
        assert_eq!(hunger_speed(0.5), 0.5);

        assert_eq!(swim_speed(1.0, 0.0, 0.1, true, true, 1.0, 1.0, 1.0), 4.0); // floor

        assert!((strength_xp_from_walking(0.5, true, &c) - 0.5 * 0.25 * 2.0).abs() < 1e-6);
    }
}
