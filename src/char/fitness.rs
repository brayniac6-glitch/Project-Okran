//! A body's fitness for an activity (MedicalSystem::fitness F040433): how hunger, the health of
//! the arms, the head and the torso, the arms' use and the armour's modifiers scale what a
//! character does with it (its damage, its speed, its skills' levels).
//!
//! Specs: F040433 (fitness), F040427 (the parts' aggregates it reads: MedicalSystem +0xb8 the best
//! arm, +0xbc the head, +0xc0 the worst torso part), F040423 (the limbs' factor), F040415 (a
//! part's state), F040452 (the arms' use flags +0x165 / +0x166 and the legs' collapse +0x160).

use crate::medical::{Medical, lerp};

/// A part's state (F040415): whole, lost (its limb slot state 1) or collapsed (2).
pub const PART_OK: u8 = 0;
pub const PART_LOST: u8 = 1;
pub const PART_COLLAPSED: u8 = 2;

/// The activities (F040433's case labels) by what reads them: 1 strength (blunt damage, unarmed
/// damage, carrying), 0x12 dexterity (cut and pierce damage, combat speed), 0x23 / 0x24 crossbows
/// and precision shooting.
pub const STRENGTH: i32 = 1;
pub const DEXTERITY: i32 = 0x12;
pub const CROSSBOWS: i32 = 0x23;
pub const PRECISION: i32 = 0x24;

/// The armour's modifiers (CharStats +0x40, +0x44, +0x48, +0x50) some activities end with when
/// asked to (the eighth argument).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mods {
    /// +0x40 (activity 0x16).
    pub x40: f32,
    /// +0x44 the dexterity mult (activity 0x12).
    pub dexterity: f32,
    /// +0x48 the damage output mult (activities 0x19 .. 0x1d answer it alone).
    pub damage_output: f32,
    /// +0x50 the ranged skill mult (activities 0x23, 0x24).
    pub ranged: f32,
}

impl Default for Mods {
    fn default() -> Self {
        Mods { x40: 1.0, dexterity: 1.0, damage_output: 1.0, ranged: 1.0 }
    }
}

/// F040433's flag arguments: what it takes into account.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Considering {
    /// The hunger (+0x60).
    pub hunger: bool,
    /// The parts' health (the aggregates).
    pub parts: bool,
    /// (not ported) The character's +0xd8 (a weariness, x 2.5).
    pub weariness: bool,
    /// The limbs' factor (F040423).
    pub limbs: bool,
    /// (not ported) F0357ab, only for activities 0x18 / 0x24.
    pub effort: bool,
    /// The armour's modifier ([`Mods`]).
    pub mods: bool,
}

/// The flags the damage curves (F047014, F047016, F047017) and unarmed damage (F047066) pass:
/// hunger, parts, the limbs and the armour's modifier.
pub const FOR_DAMAGE: Considering =
    Considering { hunger: true, parts: true, weariness: false, limbs: true, effort: false, mods: true };

/// The flags a stat's buffed level asks with (CharStats::getStatLevel F047040: all of them;
/// (not ported) the weariness and the effort).
pub const FOR_LEVEL: Considering =
    Considering { hunger: true, parts: true, weariness: true, limbs: true, effort: true, mods: true };

/// MedicalSystem's part aggregates (F040427): the best arm's health fraction (+0xb8; -1 with no
/// arm, 1 for an animal), the head's (+0xbc; 0 with none) and the worst torso part's (+0xc0; 100
/// with none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aggregates {
    pub arm: f32,
    pub head: f32,
    pub torso: f32,
}

impl Medical {
    /// @fn F040427
    ///
    /// The parts' aggregates fitness reads (parts by kind: 2 arm, 3 head, 0 torso; the health
    /// fraction is the part's +0x60).
    pub fn aggregates(&self, animal: bool) -> Aggregates {
        let mut a = Aggregates { arm: -1.0, head: 0.0, torso: 100.0 };
        for p in &self.parts {
            let f = p.health_frac;
            if p.kind == 2 && a.arm < f {
                a.arm = f;
            }
            if p.kind == 3 {
                a.head = f;
            }
            if p.kind == 0 && f < a.torso {
                a.torso = f;
            }
        }
        if animal {
            a.arm = 1.0;
        }
        a
    }

    /// @fn F040415
    ///
    /// Part `i`'s state, against the KO point `ko` (F040402): an animal's limbs are always
    /// whole; a person's arm or leg lost (limb slot state 1) is lost, one whose flesh less stun
    /// is below minus its most health collapsed. A leg: collapsed when its flesh less stun plus
    /// its treatment pool is below the KO point. Any other part (an arm too): whole at or above
    /// zero health (with its splint), or between the KO point and zero when vital and not in a
    /// coma; else collapsed. The second value: a vital part below the KO point puts the body in
    /// its coma (+0x162).
    pub fn part_state(&self, i: usize, animal: bool, ko: f32) -> (u8, bool) {
        let p = &self.parts[i];
        let h = p.flesh - p.stun;
        if p.kind == 1 || p.kind == 2 {
            if animal {
                return (PART_OK, false);
            }
            if p.slot == 1 {
                return (PART_LOST, false);
            }
            if h < -p.max_health() {
                return (PART_COLLAPSED, false);
            }
        }
        if p.kind == 1 {
            return (if ko <= h + p.treatment { PART_OK } else { PART_COLLAPSED }, false);
        }
        if ko <= h + p.bonus {
            if 0.0 <= h + p.bonus || (!self.coma && p.vital) {
                return (PART_OK, false);
            }
            return (PART_COLLAPSED, false);
        }
        (PART_COLLAPSED, p.vital)
    }

    /// The arms' use flags (MedicalSystem +0x165, +0x166) as the medical refresh (F040452)
    /// leaves them: set, then cleared by a part not whole ([`Medical::part_state`]) whose
    /// "collapse part" is 2 (+0x165) or 4 (+0x166), its arm (side 2 or 1). Returns (+0x165,
    /// +0x166).
    pub fn arms_usable(&self, animal: bool, ko: f32) -> (bool, bool) {
        let down = |side: i32| (0..self.parts.len()).any(|i| self.parts[i].kind == 2 && self.parts[i].side == side && self.part_state(i, animal, ko).0 != PART_OK);
        (!down(2), !down(1))
    }

    /// The legs' collapse (MedicalSystem +0x160, F040452): a part not whole whose "collapse
    /// part" is 0x10 or 0x20 (a leg). The character then lies prone (Character vtable 0x370
    /// with 2: crawling) and moves at most 5 (F047064's "slowed").
    pub fn legs_collapsed(&self, animal: bool, ko: f32) -> bool {
        (0..self.parts.len()).any(|i| self.parts[i].kind == 1 && self.parts[i].side != 0 && self.part_state(i, animal, ko).0 != PART_OK)
    }

    /// @fn F040423
    ///
    /// The limbs' factor of an activity: the worn robotic limbs' own factors ((not ported): 1),
    /// and x 0.66 while either arm is out of use, for the activities that use the arms (1, 2, 6,
    /// 9, 10, 0xb, 0x12, 0x13, 0x16, 0x17, 0x1e, 0x23, 0x25; the legs' 0x10, 0x11 only by their
    /// limbs); others 1.
    pub fn limbs_factor(&self, activity: i32, animal: bool, ko: f32) -> f32 {
        match activity {
            1 | 2 | 6 | 9 | 10 | 0xb | 0x12 | 0x13 | 0x16 | 0x17 | 0x1e | 0x23 | 0x25 => {
                let (a, b) = self.arms_usable(animal, ko);
                if a && b { 1.0 } else { 0.66 }
            }
            _ => 1.0,
        }
    }

    /// @fn F040433
    ///
    /// The fitness for `activity` (a 0 .. 1 factor, times the armour's modifier for some):
    /// each activity has a body measure (the best arm times the worst torso part, the head, or
    /// none), how far hunger may lower it and how far the measure may; the hunger share (hunger
    /// - 1, within 0 .. 1, lifted to its floor) times the measure (lifted to its floor) is
    /// lifted to the activity's floor, kept within 0 .. 1, then times the limbs' factor. With
    /// neither arm in use the arm activities' measure is a hundredth.
    /// `ko` is the KO point (F040402) the parts' states are measured against.
    pub fn fitness(&self, activity: i32, animal: bool, ko: f32, with: Considering, mods: &Mods) -> f32 {
        let ag = self.aggregates(animal);
        let (arm_b, arm_a) = self.arms_usable(animal, ko);
        let no_arms = !arm_b && !arm_a;
        let mut measure = ag.arm;
        let (mut hunger_floor, mut measure_floor, mut floor, mut extra) = (0.0f32, 0.0f32, 0.25f32, 1.0f32);
        match activity {
            1 => {
                measure *= ag.torso;
                floor = 0.1;
                measure_floor = 0.75;
            }
            2 | 0x1e => {
                hunger_floor = 0.75;
                measure *= ag.torso;
                floor = 0.7;
            }
            3 | 5 | 7 | 8 | 0xc | 0x26 => {
                measure *= ag.torso;
                hunger_floor = 0.5;
                if no_arms {
                    measure *= 0.01;
                }
            }
            4 | 6 | 0xd | 9 | 0xb | 0xe | 0xf => {
                measure = ag.head;
                hunger_floor = 0.6;
                measure_floor = 0.4;
            }
            10 | 0x25 => {
                hunger_floor = 0.75;
                measure_floor = 0.4;
                if no_arms {
                    measure *= 0.01;
                }
            }
            0x10 => {
                hunger_floor = 0.9;
                floor = 0.8;
                measure_floor = 0.9;
            }
            0x11 => {
                hunger_floor = 0.7;
                measure = 1.0;
                floor = 0.5;
                measure_floor = 1.0;
            }
            0x12 => {
                measure *= ag.torso;
                hunger_floor = 0.7;
                floor = 0.4;
                if with.mods {
                    extra = mods.dexterity;
                }
            }
            0x13 => {
                hunger_floor = 0.75;
                measure *= ag.head;
                floor = 0.8;
            }
            0x16 => {
                measure = if no_arms { measure * 0.01 } else { 1.0 };
                hunger_floor = 1.0;
                floor = 0.5;
                if with.mods {
                    extra = mods.x40;
                }
            }
            0x17 => {
                hunger_floor = 0.5;
                measure *= ag.torso;
                floor = 0.01;
            }
            0x19..=0x1d => {
                let v = mods.damage_output;
                return if !with.mods || v == 1.0 || v <= 0.0 { 1.0 } else { v };
            }
            0x23 => {
                hunger_floor = 0.6;
                measure = ag.head;
                measure_floor = 0.4;
                if with.mods {
                    extra = mods.ranged;
                }
            }
            0x18 | 0x24 => {
                if activity == 0x24 && with.mods {
                    extra = mods.ranged;
                }
                hunger_floor = 0.6;
                measure = ag.head;
                floor = 0.33;
                measure_floor = 0.5;
            }
            _ => return 1.0,
        }
        let mut f = 1.0;
        if with.hunger {
            f = lerp((self.hunger.level - 1.0).clamp(0.0, 1.0), hunger_floor, 1.0);
        }
        if with.parts {
            f *= lerp(measure, measure_floor, 1.0);
        }
        let mut v = lerp(f, floor, 1.0).clamp(0.0, 1.0);
        if with.limbs {
            v *= self.limbs_factor(activity, animal, ko);
        }
        extra * v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BodyPart;

    fn part(kind: i32, side: i32, frac: f32) -> BodyPart {
        BodyPart { kind, side, health_frac: frac, flesh: frac * 100.0, max_a: 100.0, max_b: 1.0, max_c: 1.0, collapses: kind == 2, ..Default::default() }
    }

    #[test]
    fn healthy_and_hurt_bodies() {
        let mut m = Medical { parts: vec![part(0, 0, 1.0), part(2, 1, 1.0), part(2, 2, 1.0), part(3, 0, 1.0)], ..Default::default() };
        m.hunger.level = 2.5;
        let mods = Mods { dexterity: 1.2, ..Default::default() };
        assert_eq!(m.fitness(STRENGTH, false, -50.0, FOR_DAMAGE, &mods), 1.0);
        assert!((m.fitness(DEXTERITY, false, -50.0, FOR_DAMAGE, &mods) - 1.2).abs() < 1e-6);
        // starving: strength's hunger share falls to its floor 0, lifted to 0.1
        m.hunger.level = 0.5;
        assert!((m.fitness(STRENGTH, false, -50.0, FOR_DAMAGE, &mods) - 0.1).abs() < 1e-6);
        m.hunger.level = 2.5;
        // one arm down to nothing: the other carries the arms' measure, but it is out of use
        m.parts[1] = part(2, 1, -0.5);
        m.parts[1].flesh = -50.0;
        assert_eq!(m.aggregates(false).arm, 1.0);
        assert!((m.fitness(STRENGTH, false, -50.0, FOR_DAMAGE, &mods) - 0.66).abs() < 1e-6);
        // a hurt torso: half health -> strength lerp(0.5, 0.75, 1) lifted from 0.1
        m.parts[1] = part(2, 1, 1.0);
        m.parts[0] = part(0, 0, 0.5);
        let v = lerp(lerp(0.5, 0.75, 1.0), 0.1, 1.0);
        assert!((m.fitness(STRENGTH, false, -50.0, FOR_DAMAGE, &mods) - v).abs() < 1e-6);
    }

    #[test]
    fn legs_collapse_below_the_ko_point() {
        let mut m = Medical { parts: vec![part(0, 0, 1.0), part(1, 1, 1.0), part(1, 2, 1.0)], ..Default::default() };
        assert!(!m.legs_collapsed(false, -30.0));
        // a leg at -20: above the KO point -30, still standing
        m.parts[1].flesh = -20.0;
        assert!(!m.legs_collapsed(false, -30.0));
        m.parts[1].flesh = -40.0;
        assert!(m.legs_collapsed(false, -30.0));
        // its treatment pool holds it up
        m.parts[1].treatment = 15.0;
        assert!(!m.legs_collapsed(false, -30.0));
        // an animal's legs never collapse
        m.parts[1].treatment = 0.0;
        assert!(!m.legs_collapsed(true, -30.0));
        // an arm below zero is out of use
        m.parts.push(part(2, 1, -0.1));
        m.parts[3].flesh = -10.0;
        assert_eq!(m.arms_usable(false, -30.0), (true, false));
    }
}
