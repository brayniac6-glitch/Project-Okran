//! A melee strike landing: does the defender dodge or block it, and what an unarmed strike does to
//! the striker's own limb.
//!
//! Specs: sheet rows F040724 defendOutcome, F033831 blockDirection, F039783 canDefend,
//! F040731 punchFeedback, F033862 Character::onMeleeHit (the outcome-by-material table),
//! F039792 CombatClass::updateBlock, F047048 CharStats::defenceLevel.

use crate::WeaponCategory;
use kenshi_char::medical::lerp;
use kenshi_char::{Constants, Damage, Medical};

/// How a strike ended for the defender (F040724's return values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defence {
    Dodged = 0,
    Hit = 2,
    Blocked = 5,
}

/// What the defender is doing when the strike arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Blocking; `direction` is the block action's last direction entry.
    Block {
        direction: i32,
    },
    Dodge,
    Other,
}

/// @fn F033831
///
/// The block direction that stops attack type `attack`; `variant` is the attacker relation the
/// original tests (== 1).
pub fn block_direction(attack: i32, variant: bool) -> i32 {
    match (variant, attack) {
        (true, 1) => 7,
        (true, 2 | 3) => 9,
        _ => attack,
    }
}

/// @fn F040724
///
/// `animal` is F039783 (animals can't defend); `progress` is the defender's action's normalised
/// time (F038894).
pub fn defend_outcome(
    animal: bool,
    action: Action,
    progress: f32,
    attack: i32,
    variant: bool,
) -> Defence {
    if animal {
        return Defence::Hit;
    }
    match action {
        Action::Dodge if (0.1..=0.98).contains(&progress) => Defence::Dodged,
        Action::Block { direction }
            if direction == block_direction(attack, variant) && 0.5 < progress =>
        {
            Defence::Blocked
        }
        _ => Defence::Hit,
    }
}

/// @fn F047048
///
/// The defence level blocking uses. `skill` is CharStats +0xf0; with `buffed`: `unarmed_bonus`
/// (unarmed and the original's condition) adds 20, encumbrance (`load`, +0x190) and injury
/// (`injury`, the character's factor) lower it, `mult` (+0x38) scales it.
pub fn defence_level(
    skill: f32,
    buffed: bool,
    unarmed_bonus: bool,
    load: f32,
    injury: f32,
    mult: f32,
) -> f32 {
    if !buffed {
        return skill;
    }
    let base = if unarmed_bonus { skill + 20.0 } else { skill };
    let l = load.clamp(0.4, 1.0);
    let i = injury.clamp(0.5, 1.0);
    base - (1.0 - l) * skill - (1.0 - i) * skill + (skill * mult - skill)
}

/// An attacker as the block decision sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Incoming {
    /// The attacker's swing progress at this defender (F040709; 0 when aimed elsewhere).
    pub progress: f32,
    /// The attacker's attack skill (F047043).
    pub skill: f32,
}

/// What the block decision did this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlockDecision {
    /// Nothing to react to (or still cooling down).
    None,
    /// Start blocking (combat state 3).
    Block,
    /// The roll failed: train defence against `skill`; the cooldown restarted.
    Failed { skill: f32 },
}

/// @fn F039792
///
/// One tick of a defender's block decision. `cooldown` is CombatClass +0x124; `defence` the
/// defender's defence level; `roll` uniform in [0, 1) (F050500).
pub fn update_block(
    cooldown: &mut f32,
    dt: f32,
    defending: bool,
    attackers: &[Incoming],
    defence: f32,
    c: &crate::CombatConstants,
    roll: f32,
) -> BlockDecision {
    *cooldown -= dt;
    if !defending || 0.0 <= *cooldown {
        return BlockDecision::None;
    }
    let mut best: Option<f32> = None;
    for a in attackers {
        if (0.1..0.99).contains(&a.progress) && best.is_none_or(|b| b < a.skill) && 0.0 < a.skill {
            best = Some(a.skill);
        }
    }
    let Some(skill) = best else {
        return BlockDecision::None;
    };
    if crate::block_chance(defence - skill, c) <= roll * 100.0 {
        *cooldown = 1.2;
        BlockDecision::Failed { skill }
    } else {
        BlockDecision::Block
    }
}

/// What an unarmed strike landed on, for the striker's own limb (F033862's outcome codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Struck {
    /// Flesh, cloth or leather (2).
    Soft = 2,
    /// Chain (6).
    Chain = 6,
    /// Plate, a replaced limb or a robot (1).
    Plate = 1,
    /// A blocking weapon (5).
    Blocked = 5,
}

impl Struck {
    /// From resolveHit's hit material (-1 flesh, else ArmourType).
    pub fn from_material(material: i32) -> Struck {
        match material {
            2 => Struck::Chain,
            3 => Struck::Plate,
            _ => Struck::Soft,
        }
    }
}

/// The striker's state punch feedback reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Striker {
    pub category: WeaponCategory,
    pub robot: bool,
    pub martial_arts: f32,
    pub toughness: f32,
    pub toughness_scale: f32,
    /// CharStats +0x4c.
    pub self_damage_mult: f32,
    /// The global damage factor (G2).
    pub damage_mult: f32,
}

/// @fn F040731
///
/// Damage the striker's limb in `slot` (the strike action's limb) takes from an unarmed strike;
/// returns the part hit and the total damage. The caller trains toughness by `struck`.
/// (The original also skips a part whose +0x28 is set; that field isn't identified yet.)
pub fn punch_feedback(
    m: &mut Medical,
    s: &Striker,
    mut struck: Struck,
    victim_robot: bool,
    slot: i32,
    c: &Constants,
) -> Option<(usize, f32)> {
    if s.category != WeaponCategory::SkillUnarmed || s.robot {
        return None;
    }
    if victim_robot {
        struck = Struck::Plate;
    }
    if slot == 4 {
        return None;
    }
    let i = m.part_for_slot(slot)?;
    let mut d = Damage {
        blunt: 1.0,
        ..Default::default()
    };
    match struck {
        Struck::Chain => d.cut = 1.0,
        Struck::Plate => {
            d.cut = 2.0;
            d.blunt = 2.0;
        }
        Struck::Blocked => {
            d.cut = 5.0;
            d.bleed = 1.0;
        }
        Struck::Soft => {}
    }
    let t = (s.martial_arts * 0.01 + s.toughness * s.toughness_scale * 0.01) * 0.5;
    let k = s.damage_mult * lerp(t, 1.0, 0.0) * s.self_damage_mult;
    d.cut *= k;
    d.blunt *= k;
    d.pierce *= k;
    d.stun *= k;
    m.parts[i].damage(d, c.blunt_permanent_organ_damage);
    Some((i, d.blunt + d.cut + d.pierce + d.stun))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenshi_char::BodyPart;

    #[test]
    fn dodges_and_blocks_need_timing() {
        let block = Action::Block { direction: 2 };
        assert_eq!(
            defend_outcome(false, block, 0.6, 2, false),
            Defence::Blocked
        );
        assert_eq!(defend_outcome(false, block, 0.4, 2, false), Defence::Hit); // too early
        assert_eq!(defend_outcome(false, block, 0.6, 1, false), Defence::Hit); // wrong side
        assert_eq!(
            defend_outcome(false, Action::Block { direction: 9 }, 0.6, 3, true),
            Defence::Blocked
        );
        assert_eq!(
            defend_outcome(false, Action::Dodge, 0.5, 1, false),
            Defence::Dodged
        );
        assert_eq!(
            defend_outcome(false, Action::Dodge, 0.99, 1, false),
            Defence::Hit
        );
        assert_eq!(
            defend_outcome(true, Action::Dodge, 0.5, 1, false),
            Defence::Hit
        ); // animals
    }

    #[test]
    fn burdens_and_injuries_lower_defence() {
        assert_eq!(defence_level(50.0, false, true, 0.0, 0.0, 2.0), 50.0);
        assert_eq!(defence_level(50.0, true, false, 1.0, 1.0, 1.0), 50.0);
        assert_eq!(defence_level(50.0, true, true, 1.0, 1.0, 1.0), 70.0);
        // heavily burdened (0.4 floor) and badly hurt (0.5 floor)
        assert!(
            (defence_level(50.0, true, false, 0.0, 0.0, 1.0) - (50.0 - 30.0 - 25.0)).abs() < 1e-4
        );
    }

    #[test]
    fn blocks_the_most_skilled_swing_in_progress() {
        let c = crate::CombatConstants::default();
        let mut cd = 0.0;
        let foes = [
            Incoming {
                progress: 0.5,
                skill: 30.0,
            },
            Incoming {
                progress: 0.05,
                skill: 90.0,
            }, // not swinging yet
        ];
        // defence 30 vs skill 30: 70%
        assert_eq!(
            update_block(&mut cd, 0.1, true, &foes, 30.0, &c, 0.69),
            BlockDecision::Block
        );
        assert_eq!(
            update_block(&mut cd, 0.1, true, &foes, 30.0, &c, 0.7),
            BlockDecision::Failed { skill: 30.0 }
        );
        assert!((cd - 1.2).abs() < 1e-6);
        // cooling down
        assert_eq!(
            update_block(&mut cd, 0.1, true, &foes, 30.0, &c, 0.0),
            BlockDecision::None
        );
    }

    #[test]
    fn punching_plate_hurts_untrained_hands() {
        let c = Constants::from_record(&Default::default());
        let hand = BodyPart {
            kind: 1,
            side: 1,
            flesh: 50.0,
            max_a: 50.0,
            max_b: 1.0,
            max_c: 1.0,
            ..Default::default()
        };
        let mut m = Medical {
            parts: vec![BodyPart::default(), hand],
            ..Default::default()
        };
        let mut s = Striker {
            category: WeaponCategory::SkillUnarmed,
            robot: false,
            martial_arts: 0.0,
            toughness: 0.0,
            toughness_scale: 1.0,
            self_damage_mult: 1.0,
            damage_mult: 1.0,
        };
        let (i, total) = punch_feedback(&mut m, &s, Struck::Plate, false, 2, &c).unwrap();
        assert_eq!((i, total), (1, 4.0));
        // a master takes nothing
        s.martial_arts = 100.0;
        s.toughness = 100.0;
        assert_eq!(
            punch_feedback(&mut m, &s, Struck::Soft, false, 2, &c)
                .unwrap()
                .1,
            0.0
        );
        s.category = WeaponCategory::SkillKatanas;
        assert_eq!(
            punch_feedback(&mut m, &s, Struck::Plate, false, 2, &c),
            None
        );
    }
}
