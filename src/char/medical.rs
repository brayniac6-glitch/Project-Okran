//! The limb-based medical system: knockout point and time, damage to body parts, bleeding, max
//! blood, the per-part healing tick and the per-character medical update.
//!
//! Specs: sheet rows F040402 getKOPoint, F040419 computeKnockoutTime, F040472 applyBodyPartDamage,
//! F040473 BodyPart::update, F040422 BodyPart::bleedAmount, F040438 MedicalSystem::bleed,
//! F040410 MedicalSystem::maxBlood, F040477 MedicalSystem::update, F050479 lerp, F050480 invlerp;
//! types T00025 GlobalConstants, T00026 CharStats, T00027 BodyPart, T00028 MedicalSystem.

use kenshi_data::GameData;

/// @fn F050479
pub fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}

/// @fn F050480
pub fn invlerp(x: f32, a: f32, b: f32) -> f32 {
    (x - a) / (b - a)
}

/// The GLOBAL CONSTANTS values the medical system reads (T00025, by record key).
#[derive(Debug, Clone, PartialEq)]
pub struct Constants {
    pub min_toughness_ko_point: f32,
    pub max_toughness_ko_point: f32,
    pub knockout_time_base: f32,
    pub knockout_mult_1: f32,
    pub knockout_mult_99: f32,
    pub immediate_blood_loss: f32,
    pub blunt_permanent_organ_damage: f32,
    pub robot_wear_rate: f32,
    pub heal_rate_mult: f32,
    pub resting_heal_rate_mult: f32,
    pub bodypart_degeneration_rate: f32,
    pub stun_recovery_rate: f32,
    pub extra_blood_loss_from_bodyparts: f32,
    pub bleeding_clot_rate: f32,
    pub blood_recovery_rate: f32,
    pub degeneration_mult_1: f32,
    pub degeneration_mult_99: f32,
    /// The options' "Dismemberment" (settings.cfg, 0x142133504; not a GLOBAL CONSTANTS key):
    /// 0 Never, 1 Rare (a limb a hit takes below minus its most health, F040472), 2 Frequent
    /// (any limb under that, F040473). Set by the game from its options.
    pub dismemberment: i32,
}

impl Constants {
    /// @type GlobalConstants
    ///
    /// From the merged GLOBAL CONSTANTS record (110-gamedata.quack). Missing keys keep the shipped
    /// base-game values.
    pub fn from_record(r: &GameData) -> Self {
        let f = |k: &str, d: f32| r.floats.get(k).copied().unwrap_or(d);
        Constants {
            min_toughness_ko_point: f("min toughness ko point", 10.0),
            max_toughness_ko_point: f("max toughness ko point", 85.0),
            knockout_time_base: f("knockout time base", 20.0),
            knockout_mult_1: f("knockout mult 1", 3.0),
            knockout_mult_99: f("knockout mult 99", 0.75),
            immediate_blood_loss: f("immediate blood loss", 0.2),
            blunt_permanent_organ_damage: f("blunt permanent organ damage", 0.0),
            robot_wear_rate: f("robot wear rate", 0.0),
            heal_rate_mult: f("heal rate mult", 0.25),
            resting_heal_rate_mult: f("resting heal rate mult", 2.0),
            bodypart_degeneration_rate: f("bodypart degeneration rate", 0.0),
            stun_recovery_rate: f("stun recovery rate", 1.0),
            extra_blood_loss_from_bodyparts: f("extra blood loss from bodyparts", 1.0),
            bleeding_clot_rate: f("bleeding clot rate", 0.0085),
            blood_recovery_rate: f("blood recovery rate", 0.4),
            degeneration_mult_1: f("degeneration mult 1", 1.0),
            degeneration_mult_99: f("degeneration mult 99", 1.0),
            dismemberment: 1,
        }
    }
}

/// The stats the medical system uses (T00026).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// CharStats +0x90 ("toughness2").
    pub toughness: f32,
    /// CharStats +0x34: scales toughness in the KO formulas; not yet identified (1.0 = none).
    pub toughness_scale: f32,
}

impl Stats {
    /// Stat level as a 0..1 lerp factor: toughness * scale * 0.01.
    fn t(&self) -> f32 {
        self.toughness * self.toughness_scale * 0.01
    }
}

/// @type BodyPart
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct BodyPart {
    pub vital: bool,
    /// Part kind (+0x08, LOCATIONAL_DAMAGE "body part type"): 0 torso, 1 leg, 2 arm, 3 head.
    pub kind: i32,
    /// Which of a limb pair (+0x20; 1 = the first).
    pub side: i32,
    /// The limb's slot state (limbSlotState F022536; 0 = none). State 1 bleeds once the flesh is
    /// gone and can't be hit; state 2 hits sound as material 3.
    pub slot: i32,
    /// Relative chance of being the part hit (+0x38).
    pub hit_weight: f32,
    pub ko_weight: f32,
    pub flesh: f32,
    pub stun: f32,
    /// +0x4c: a splint's support (F040455 raises it to at most the stun and treatment on the
    /// part), counted in the part's health; capped so the health with it stays at most 50
    /// (F040473), it wears off as the limb heals.
    pub bonus: f32,
    pub max_loss: f32,
    pub max_a: f32,
    pub max_b: f32,
    pub max_c: f32,
    /// Hit-weight multiplier (+0x3c): grows as the part is hit, decays back toward 1.
    pub focus: f32,
    pub treatment: f32,
    pub treated: bool,
    pub robotic: bool,
    pub health_frac: f32,
    /// Losing it collapses the character (+0x31, "collapses").
    pub collapses: bool,
    /// The race heals itself (+0x30, the race's "self healing").
    pub self_healing: bool,
    /// The LOCATIONAL_DAMAGE record's string id (what animations refer to the part by).
    pub id: String,
}

impl BodyPart {
    /// @fn F040462
    ///
    /// A part from its LOCATIONAL_DAMAGE record and the race's "combat anatomy" reference values
    /// (`hit_weight`, `max_health`: below 1 means 100); `size` is the character's size (vtable
    /// 0x390) and `health_mult` its factor at vtable 0x238 (1 for now: not identified).
    pub fn from_record(
        r: &kenshi_data::GameData,
        hit_weight: f32,
        max_health: f32,
        size: f32,
        health_mult: f32,
        self_healing: bool,
    ) -> Self {
        let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
        let i = |k: &str| r.ints.get(k).copied().unwrap_or(0);
        let side = match i("collapse part") {
            4 | 0x20 => 1,
            2 | 0x10 => 2,
            _ => 0,
        };
        let max_a = if max_health < 1.0 { 100.0 } else { max_health };
        let mut p = BodyPart {
            vital: b("death"),
            kind: i("body part type"),
            side,
            hit_weight,
            ko_weight: r.floats.get("KO mult").copied().unwrap_or(0.0),
            max_a,
            max_b: size,
            max_c: health_mult,
            focus: 1.0,
            collapses: b("collapses"),
            self_healing,
            id: r.string_id.clone(),
            ..Default::default()
        };
        p.flesh = max_a * size * health_mult - p.max_loss;
        p.health_frac = 1.0;
        p
    }

    /// Max health = a * b * c - permanent loss.
    pub fn max_health(&self) -> f32 {
        self.max_a * self.max_b * self.max_c - self.max_loss
    }

    /// Effective health = flesh - stun + bonus.
    pub fn health(&self) -> f32 {
        self.flesh - self.stun + self.bonus
    }

    pub fn update_frac(&mut self) {
        self.health_frac = self.health() / self.max_health();
    }

    /// @fn F040421
    ///
    /// Damage this part directly: `organ` is GLOBAL CONSTANTS "blunt permanent organ damage", the
    /// share of blunt damage that hurts the flesh.
    pub fn damage(&mut self, d: Damage, organ: f32) {
        self.flesh -= organ * d.blunt + d.cut + d.pierce;
        self.stun += (1.0 - organ) * d.blunt + d.stun;
        self.update_frac();
    }

    /// An arm or leg: part kind 1 or 2.
    pub fn is_limb(&self) -> bool {
        self.kind == 1 || self.kind == 2
    }

    /// The limb slot this part fills: legs 2/3, arms 0/1 (by side), else 4 (none).
    pub fn limb_slot(&self) -> i32 {
        match self.kind {
            1 => 2 + i32::from(self.side != 1),
            2 => i32::from(self.side != 1),
            _ => 4,
        }
    }

    /// @fn F040422
    pub fn bleed_amount(&self) -> f32 {
        let deficit = self.flesh + self.treatment;
        if !self.vital && self.is_limb() && self.slot == 1 && deficit < 0.0 {
            deficit * -0.002
        } else {
            0.0
        }
    }
}

/// One hit's damage, split by kind (F040472's damage vector).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Damage {
    pub cut: f32,
    pub blunt: f32,
    pub pierce: f32,
    pub stun: f32,
    pub bleed: f32,
}

/// @type MedicalSystem
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Medical {
    /// Limbs (part indices) severed by hits not yet seen to by the game (F040471's effects).
    #[serde(skip)]
    pub severed: Vec<usize>,
    pub blood: f32,
    pub max_blood: f32,
    pub parts: Vec<BodyPart>,
    /// Remaining amount of each open wound (clots over time).
    pub wounds: Vec<f32>,
    pub limb_bleed: f32,
    pub bleed_rate: f32,
    pub knocked_out: bool,
    pub blood_ko: bool,
    /// A vital part fell below the KO point (F040415 +0x162): a recovery coma until it heals
    /// back above zero.
    pub coma: bool,
    pub dead: bool,
    pub knockout_time: f32,
    pub hunger: crate::hunger::Hunger,
}

/// What a body-part tick reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartState {
    Ok,
    /// A vital part fell below -max health.
    Dead,
    /// A non-vital part fell below -max health.
    LimbLost,
}

/// Inputs to one part's healing tick (F040473 parameters plus the character state it reads).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartTick {
    pub dt: f32,
    /// Healing rate while awake / while asleep.
    pub rate: f32,
    pub sleep_rate: f32,
    pub degeneration: f32,
    /// Character size: sets the part's middle max-health factor.
    pub size: f32,
    /// Resting heal (> 0 restores permanent loss).
    pub rest: f32,
    pub asleep: bool,
    /// Awake and able to be treated.
    pub conscious: bool,
    /// The global degeneration/bleed factor (a game setting at 0x142133578; 1.0 = default).
    pub g: f32,
}

/// The race and situation a character's medical update runs with (F040477).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedicalTick {
    pub dt: f32,
    pub race_heal: f32,
    pub race_bleed: f32,
    pub race_blood_recovery: f32,
    /// Healing rate multipliers chosen by the situation (bed, healer, ...); 1.0 normally.
    pub rate: f32,
    pub sleep_rate: f32,
    pub rest: f32,
    pub resting: bool,
    pub asleep: bool,
    pub size: f32,
    /// Has blood at all (bleeding knockouts apply).
    pub bleeds: bool,
    pub g: f32,
}

impl Medical {
    /// @fn F040468
    ///
    /// The body parts of a race: one per "combat anatomy" reference, in the race's order, with the
    /// reference's values as hit weight and max health.
    pub fn from_race(
        race: &kenshi_data::GameData,
        data: &kenshi_data::GameDataContainer,
        size: f32,
        health_mult: f32,
    ) -> Self {
        let self_healing = race.bools.get("self healing").copied().unwrap_or(false);
        let parts = race
            .refs("combat anatomy")
            .iter()
            .filter_map(|(sid, v)| {
                data.get(sid).map(|r| {
                    BodyPart::from_record(
                        r,
                        v[0] as f32,
                        v[1] as f32,
                        size,
                        health_mult,
                        self_healing,
                    )
                })
            })
            .collect();
        Medical {
            parts,
            ..Default::default()
        }
    }

    /// @fn F040500
    pub fn part_for_slot(&self, slot: i32) -> Option<usize> {
        self.parts.iter().position(|p| p.limb_slot() == slot)
    }

    /// @fn F040402
    ///
    /// The (negative) health a part may fall to before the character is knocked out.
    pub fn ko_point(stats: &Stats, c: &Constants) -> f32 {
        -lerp(
            stats.t(),
            c.min_toughness_ko_point,
            c.max_toughness_ko_point,
        )
    }

    /// @fn F040410
    pub fn max_blood(race_min: f32, race_max: f32, stat_factor: f32, a: f32, size: f32) -> f32 {
        size * (race_max - race_min) * stat_factor * a + race_min
    }

    /// @fn F040419
    pub fn compute_knockout_time(&mut self, stats: &Stats, c: &Constants) -> f32 {
        let m = lerp(stats.t(), c.knockout_mult_1, c.knockout_mult_99);
        let mut t = c.knockout_time_base;
        for p in &self.parts {
            let max = p.max_health();
            if p.vital {
                t += 100.0 * 0.25 * m * (max - (p.flesh + p.stun)) / max;
            }
            let diff = p.flesh - p.stun;
            if diff < 0.0 {
                t += 100.0 * m * p.ko_weight * diff.abs() / max;
            }
        }
        if self.max_blood > 0.0 {
            t += 2.0 * 100.0 * 0.25 * m * (self.max_blood - self.blood).abs() / self.max_blood;
        }
        self.knockout_time = t;
        t
    }

    fn blood_threshold(&self) -> f32 {
        if self.blood_ko { -25.0 } else { 0.0 }
    }

    /// @fn F040472
    ///
    /// Apply one hit to part `i`; `race_bleed` is the race's bleed factor.
    pub fn apply_damage(&mut self, i: usize, d: Damage, race_bleed: f32, c: &Constants) {
        self.blood -= (d.cut + d.pierce) * c.immediate_blood_loss * d.bleed * race_bleed;
        let p = &mut self.parts[i];
        p.damage(d, c.blunt_permanent_organ_damage);
        // limb loss (the options' "Dismemberment" not Never, F040472): a limb hit below minus
        // its most health is lost (F040471: slot state 1), but not a robotic one (state 2)
        if c.dismemberment > 0 && p.is_limb() && p.slot != 1 && p.slot != 2 && p.health() < -p.max_health() {
            p.slot = 1;
            self.severed.push(i);
        }
        let p = &mut self.parts[i];
        if p.robotic {
            p.max_loss += (d.cut + d.pierce + d.blunt) * 0.04 * c.robot_wear_rate;
        }
        if p.health() < 0.0 && p.vital {
            self.knocked_out = true;
            self.coma = true;
        }
        if self.blood < self.blood_threshold() {
            self.knocked_out = true;
            self.blood_ko = true;
        }
    }

    /// @fn F040438
    pub fn bleed(&mut self, dt: f32, race_bleed: f32, g: f32, bleeds: bool, c: &Constants) {
        let before = self.blood;
        self.limb_bleed = self.parts.iter().map(BodyPart::bleed_amount).sum();
        for w in &mut self.wounds {
            *w -= c.bleeding_clot_rate * dt;
        }
        self.wounds.retain(|w| *w > 0.0);
        let open: f32 = self.wounds.iter().sum();
        self.bleed_rate =
            ((c.extra_blood_loss_from_bodyparts * self.limb_bleed + open) * g * 1.2 * race_bleed)
                .max(0.0);
        self.blood = (self.blood - self.bleed_rate * dt).min(self.max_blood);
        let threshold = self.blood_threshold();
        if bleeds && self.blood < threshold && before >= threshold {
            self.knocked_out = true;
            self.blood_ko = true;
        }
    }

    /// @fn F040477
    ///
    /// One medical tick. Returns false once the character is dead.
    pub fn update(&mut self, t: &MedicalTick, stats: &Stats, c: &Constants) -> bool {
        if self.dead {
            return false;
        }
        self.bleed(t.dt, t.race_bleed, t.g, t.bleeds, c);
        self.knockout_time = (self.knockout_time - t.dt).max(0.0);
        // dies when blood falls below -max blood or hunger (+0x60) below zero (starvation)
        if self.blood < -self.max_blood || self.hunger.starving() {
            self.dead = true;
            return false;
        }
        let tick = PartTick {
            dt: t.dt,
            rate: t.race_heal * t.rate,
            sleep_rate: t.race_heal * t.sleep_rate,
            degeneration: lerp(stats.t(), c.degeneration_mult_1, c.degeneration_mult_99),
            size: t.size,
            rest: t.rest,
            asleep: t.asleep,
            conscious: !self.knocked_out && !self.dead,
            g: t.g,
        };
        for (i, p) in self.parts.iter_mut().enumerate() {
            match p.update(&tick, c) {
                PartState::Dead => {
                    self.dead = true;
                    return false;
                }
                // F040473: "Dismemberment" Frequent, a whole limb (slot state 0) under minus its
                // most health is lost as a hit would take it
                PartState::LimbLost if c.dismemberment > 1 && p.is_limb() && p.slot == 0 => {
                    p.slot = 1;
                    self.severed.push(i);
                }
                _ => {}
            }
        }
        if self.blood < self.max_blood && self.bleed_rate <= 0.0 {
            let mut r = c.blood_recovery_rate;
            if t.resting {
                r *= c.resting_heal_rate_mult;
            }
            self.blood += r * t.dt * t.race_blood_recovery;
        }
        if self.blood > 25.0 {
            self.blood_ko = false;
        }
        true
    }
}

impl BodyPart {
    /// @fn F040473
    ///
    /// Advance this part by one tick.
    pub fn update(&mut self, t: &PartTick, c: &Constants) -> PartState {
        self.max_b = t.size;
        let max = self.max_health();
        let r = max / 100.0 * if t.asleep { t.sleep_rate } else { t.rate };
        let dt = t.dt;
        if self.focus > 1.0 {
            if self.flesh >= 0.0 {
                self.focus -= dt * 0.01;
            } else {
                self.focus = 1.0;
            }
        }
        if self.flesh - self.stun < -max {
            return if self.vital {
                PartState::Dead
            } else {
                PartState::LimbLost
            };
        }
        let mut treating = false;
        if self.treated && t.conscious && self.flesh - self.stun < max - self.treatment {
            self.treatment += dt * 0.05 * c.heal_rate_mult * r * 0.4;
            treating = true;
        }
        if !treating && self.flesh < max {
            let mut d = invlerp(
                100.0 * (max - self.flesh - self.treatment) / max,
                20.0,
                90.0,
            )
            .clamp(0.0, 1.0);
            if self.flesh + self.treatment > 0.0 {
                d *= 0.25;
            }
            if self.flesh > -3.0 * max {
                self.flesh -=
                    dt * 0.05 * c.bodypart_degeneration_rate * t.g * t.degeneration * d * 1.2;
            }
        }
        if self.flesh < max && self.treatment > 0.0 {
            let mut h = dt * 0.05 * c.heal_rate_mult * r;
            if t.asleep || self.robotic {
                h *= 250.0;
            }
            let h = h.min(self.treatment);
            self.treatment -= h;
            self.flesh = (self.flesh + h).min(max - 0.01);
        }
        if t.rest > 0.0 {
            let x = dt * 0.05 * c.resting_heal_rate_mult * t.rest;
            self.max_loss = (self.max_loss - x).max(0.0);
            self.treatment += x;
            let room = self.max_health() - self.flesh;
            if self.treatment > room {
                self.treatment = room;
            }
        }
        if self.bonus > 0.0 && self.bonus + self.flesh - self.stun > 50.0 {
            self.bonus = 50.0 - (self.flesh - self.stun);
        }
        self.bonus = self.bonus.max(0.0);
        self.stun = (self.stun - dt * 0.05 * c.stun_recovery_rate * c.heal_rate_mult * r).max(0.0);
        self.update_frac();
        PartState::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constants() -> Constants {
        Constants {
            min_toughness_ko_point: 10.0,
            max_toughness_ko_point: 85.0,
            knockout_time_base: 20.0,
            knockout_mult_1: 3.0,
            knockout_mult_99: 0.75,
            immediate_blood_loss: 0.2,
            blunt_permanent_organ_damage: 0.0,
            robot_wear_rate: 0.5,
            heal_rate_mult: 0.25,
            resting_heal_rate_mult: 2.0,
            bodypart_degeneration_rate: 1.0,
            stun_recovery_rate: 1.0,
            extra_blood_loss_from_bodyparts: 1.0,
            bleeding_clot_rate: 0.0085,
            blood_recovery_rate: 0.4,
            degeneration_mult_1: 1.0,
            degeneration_mult_99: 1.0,
            dismemberment: 1,
        }
    }

    fn part(vital: bool) -> BodyPart {
        BodyPart {
            vital,
            ko_weight: 1.0,
            flesh: 100.0,
            max_a: 100.0,
            max_b: 1.0,
            max_c: 1.0,
            ..Default::default()
        }
    }

    fn tick(dt: f32) -> PartTick {
        PartTick {
            dt,
            rate: 1.0,
            sleep_rate: 1.0,
            degeneration: 1.0,
            size: 1.0,
            rest: 0.0,
            asleep: false,
            conscious: true,
            g: 1.0,
        }
    }

    fn medical_tick(dt: f32) -> MedicalTick {
        MedicalTick {
            dt,
            race_heal: 1.0,
            race_bleed: 1.0,
            race_blood_recovery: 1.0,
            rate: 1.0,
            sleep_rate: 1.0,
            rest: 0.0,
            resting: false,
            asleep: false,
            size: 1.0,
            bleeds: true,
            g: 1.0,
        }
    }

    fn healthy(n_vital: usize, n_limbs: usize) -> Medical {
        let mut parts: Vec<BodyPart> = (0..n_vital).map(|_| part(true)).collect();
        parts.extend((0..n_limbs).map(|_| BodyPart {
            kind: 1,
            ..part(false)
        }));
        Medical {
            blood: 100.0,
            max_blood: 100.0,
            parts,
            ..Default::default()
        }
    }

    #[test]
    fn dismemberment_option_decides_limb_loss() {
        let hit = Damage { cut: 250.0, blunt: 0.0, pierce: 0.0, stun: 0.0, bleed: 0.0 };
        // Never: a hit far below minus the limb's most health takes nothing
        let mut c = constants();
        c.dismemberment = 0;
        let mut m = healthy(1, 1);
        m.apply_damage(1, hit, 1.0, &c);
        assert!(m.severed.is_empty() && m.parts[1].slot == 0);
        // Rare: the hit takes it
        c.dismemberment = 1;
        let mut m = healthy(1, 1);
        m.apply_damage(1, hit, 1.0, &c);
        assert_eq!((m.severed.clone(), m.parts[1].slot), (vec![1], 1));
        // Rare: a limb under minus its most health without a hit stays
        let mut m = healthy(1, 1);
        m.parts[1].flesh = -150.0;
        m.update(&medical_tick(0.1), &Stats { toughness: 50.0, toughness_scale: 1.0 }, &c);
        assert!(m.severed.is_empty());
        // Frequent: it goes
        c.dismemberment = 2;
        m.update(&medical_tick(0.1), &Stats { toughness: 50.0, toughness_scale: 1.0 }, &c);
        assert_eq!((m.severed.clone(), m.parts[1].slot), (vec![1], 1));
    }

    #[test]
    fn ko_point_scales_with_toughness() {
        let c = constants();
        assert_eq!(
            Medical::ko_point(
                &Stats {
                    toughness: 0.0,
                    toughness_scale: 1.0
                },
                &c
            ),
            -10.0
        );
        assert_eq!(
            Medical::ko_point(
                &Stats {
                    toughness: 100.0,
                    toughness_scale: 1.0
                },
                &c
            ),
            -85.0
        );
        assert_eq!(
            Medical::ko_point(
                &Stats {
                    toughness: 50.0,
                    toughness_scale: 1.0
                },
                &c
            ),
            -47.5
        );
    }

    #[test]
    fn knockout_time_grows_with_damage_and_falls_with_toughness() {
        let c = constants();
        let mut m = healthy(1, 0);
        let weak = Stats {
            toughness: 0.0,
            toughness_scale: 1.0,
        };
        let tough = Stats {
            toughness: 100.0,
            toughness_scale: 1.0,
        };
        assert_eq!(m.compute_knockout_time(&weak, &c), 20.0);
        m.parts[0].stun = 120.0;
        let t_weak = m.compute_knockout_time(&weak, &c);
        let t_tough = m.compute_knockout_time(&tough, &c);
        assert!((t_weak - (-10.0)).abs() < 1e-4, "{t_weak}");
        assert!(t_tough > t_weak);
    }

    #[test]
    fn damage_splits_flesh_stun_blood_and_knocks_out() {
        let c = constants();
        let mut m = healthy(1, 1);
        m.apply_damage(
            1,
            Damage {
                cut: 30.0,
                blunt: 10.0,
                pierce: 5.0,
                stun: 2.0,
                bleed: 1.0,
            },
            1.0,
            &c,
        );
        assert_eq!((m.parts[1].flesh, m.parts[1].stun), (65.0, 12.0));
        assert!((m.blood - (100.0 - 35.0 * 0.2)).abs() < 1e-4);
        assert!(!m.knocked_out);
        m.apply_damage(
            0,
            Damage {
                cut: 120.0,
                ..Default::default()
            },
            1.0,
            &c,
        );
        assert!(m.knocked_out);
    }

    #[test]
    fn blood_loss_knocks_out_and_robots_wear() {
        let c = constants();
        let mut robot = part(false);
        robot.robotic = true;
        let mut m = Medical {
            blood: 1.0,
            max_blood: 100.0,
            parts: vec![robot],
            ..Default::default()
        };
        m.apply_damage(
            0,
            Damage {
                cut: 10.0,
                bleed: 1.0,
                ..Default::default()
            },
            1.0,
            &c,
        );
        assert!(m.knocked_out && m.blood_ko);
        assert!((m.parts[0].max_loss - 10.0 * 0.04 * 0.5).abs() < 1e-6);
    }

    #[test]
    fn treated_wounds_heal_untreated_ones_worsen_and_stun_recovers() {
        let c = constants();
        let mut untreated = part(false);
        untreated.flesh = 20.0;
        untreated.stun = 10.0;
        let before = untreated.flesh;
        assert_eq!(untreated.update(&tick(1.0), &c), PartState::Ok);
        assert!(untreated.flesh < before);
        assert!(untreated.stun < 10.0);

        let mut treated = part(false);
        treated.flesh = 20.0;
        treated.treatment = 50.0;
        let before = treated.flesh;
        treated.update(
            &PartTick {
                asleep: true,
                ..tick(1.0)
            },
            &c,
        );
        assert!(treated.flesh > before);
        assert!(treated.flesh <= treated.max_health());

        let mut lost = part(false);
        lost.flesh = -150.0;
        assert_eq!(lost.update(&tick(1.0), &c), PartState::LimbLost);
        lost.vital = true;
        assert_eq!(lost.update(&tick(1.0), &c), PartState::Dead);
    }

    #[test]
    fn open_limbs_and_wounds_bleed_and_wounds_clot() {
        let c = constants();
        let mut m = healthy(1, 1);
        m.parts[1].slot = 1;
        m.parts[1].flesh = -50.0;
        assert!((m.parts[1].bleed_amount() - 0.1).abs() < 1e-6);
        assert_eq!(m.parts[0].bleed_amount(), 0.0); // vital parts bleed through wounds, not here
        m.wounds = vec![0.5, 0.001];
        m.bleed(1.0, 1.0, 1.0, true, &c);
        assert_eq!(m.wounds.len(), 1, "the small wound clotted");
        let expected = (0.1 + (0.5 - 0.0085)) * 1.2;
        assert!((m.bleed_rate - expected).abs() < 1e-5, "{}", m.bleed_rate);
        assert!((m.blood - (100.0 - expected)).abs() < 1e-4);
    }

    #[test]
    fn update_bleeds_out_recovers_blood_and_dies() {
        let c = constants();
        let stats = Stats {
            toughness: 50.0,
            toughness_scale: 1.0,
        };
        // not bleeding, below max: blood recovers
        let mut m = healthy(1, 2);
        m.blood = 50.0;
        assert!(m.update(&medical_tick(1.0), &stats, &c));
        assert!((m.blood - 50.4).abs() < 1e-4);
        // knockout timer runs down
        m.knockout_time = 0.5;
        m.update(&medical_tick(1.0), &stats, &c);
        assert_eq!(m.knockout_time, 0.0);
        // blood far below -max blood: dead
        m.blood = -150.0;
        assert!(!m.update(&medical_tick(1.0), &stats, &c));
        assert!(m.dead);
        assert!(!m.update(&medical_tick(1.0), &stats, &c), "stays dead");
        // starvation kills
        let mut m = healthy(1, 0);
        m.hunger.level = -0.01;
        assert!(!m.update(&medical_tick(1.0), &stats, &c));
    }

    #[test]
    fn max_blood_interpolates_race_range() {
        assert_eq!(Medical::max_blood(80.0, 120.0, 0.5, 1.0, 1.0), 100.0);
        assert_eq!(Medical::max_blood(80.0, 120.0, 0.0, 1.0, 1.0), 80.0);
    }
    #[test]
    fn first_aid_closes_wounds_then_treats_parts() {
        let c = FirstAidConstants::default();
        let mut m = healthy(1, 2);
        m.parts[0].flesh = 40.0;
        m.wounds = vec![0.01, 0.02];
        let kit = Some(Kit { quality: 40.0, robot: false });
        // the last wound first: 3 * 2 * lerp(0.4, 0.0005, 0.03) = 0.0738 a second
        let r = m.first_aid(80.0, kit, true, false, &c, 0.1);
        assert!(!r.done);
        assert_eq!(m.wounds.len(), 2);
        assert!((m.wounds[1] - (0.02 - 0.00738)).abs() < 1e-5);
        // the kit drains at lerp((100 - 80) / 100, 1, 20) = 4.8 a second
        assert!((r.drained - 0.48).abs() < 1e-4);
        for _ in 0..10 {
            m.first_aid(80.0, kit, true, false, &c, 0.1);
        }
        assert!(m.wounds.is_empty());
        // then the part's treatment fills at lerp(0.4, 1, 15) * 3 = 19.8 a second, up to the 60
        // the flesh lacks
        let t0 = m.parts[0].treatment;
        m.first_aid(80.0, kit, true, false, &c, 1.0);
        assert!((m.parts[0].treatment - t0 - 19.8).abs() < 1e-3);
        let mut done = false;
        for _ in 0..10 {
            done = m.first_aid(80.0, kit, true, false, &c, 1.0).done;
        }
        assert!(done);
        assert!((m.parts[0].treatment - 60.0).abs() < 1e-3);
    }

    #[test]
    fn a_players_medic_without_a_kit_only_stops_bleeding() {
        let c = FirstAidConstants::default();
        let mut m = healthy(1, 0);
        m.parts[0].flesh = 40.0;
        let r = m.first_aid(80.0, None, true, false, &c, 1.0);
        assert!(r.done);
        assert_eq!(m.parts[0].treatment, 0.0);
        // someone else's medic treats at 1 a second
        m.first_aid(80.0, None, false, false, &c, 1.0);
        assert!((m.parts[0].treatment - 1.0).abs() < 1e-5);
    }

}

/// The GLOBAL CONSTANTS first aid reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FirstAidConstants {
    /// "medic speed mult" (0x142133e28).
    pub speed: f32,
    /// "robot medic speed mult" (0x142133e58).
    pub robot_speed: f32,
    /// "medkit drain 1" and "medkit drain 99": kit charges a second at medic skill 0 and 100.
    pub drain: [f32; 2],
}

impl Default for FirstAidConstants {
    fn default() -> Self {
        FirstAidConstants { speed: 3.0, robot_speed: 0.33, drain: [20.0, 1.0] }
    }
}

/// The kit a medic works with (its quality, whether a robot kit), if any.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kit {
    pub quality: f32,
    pub robot: bool,
}

/// What one tick of first aid did.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FirstAidTick {
    /// Everything treated: the bleeding stopped and every part's treatment full.
    pub done: bool,
    /// Kit charges used.
    pub drained: f32,
}

/// @fn F040413
///
/// Kit charges used in `dt` by a medic of `skill`: lerp((100 - skill) / 100, drain 99, drain 1),
/// times the robot medic speed for a robot kit.
pub fn kit_drain(skill: f32, kit: Kit, c: &FirstAidConstants, dt: f32) -> f32 {
    let t = ((100.0 - skill.clamp(0.0, 100.0)) * 0.01).max(0.0);
    let mut d = lerp(t, c.drain[1], c.drain[0]);
    if kit.robot {
        d *= c.robot_speed;
    }
    d * dt
}

/// How much first aid a body wants (F040427, MedicalSystem +0xac flesh / +0xa8 robotic): each
/// side's worst vital part and half its worst part, 1 - the health share; a bleeding body's
/// (1 - its blood share) x the bleeding over 1.5 x its limbs' x 10 when more.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AidNeed {
    pub flesh: f32,
    pub robotic: f32,
}

impl Medical {
    /// @fn F040427
    ///
    /// What this body wants of first aid (see [`AidNeed`]); `robot_race` a robot's body (its
    /// bleeding counts on the robotic side). Each part's share is (treatment + flesh) / its max
    /// health, a share over 0.999 counted as whole; a dead body wants -0.1 of each (F040406).
    pub fn aid_need(&self, robot_race: bool) -> AidNeed {
        if self.dead {
            return AidNeed { flesh: -0.1, robotic: -0.1 };
        }
        let (mut vital_f, mut worst_f, mut vital_r, mut worst_r) = (1.0f32, 1.0f32, 1.0f32, 1.0f32);
        for p in &self.parts {
            let share = (p.treatment + p.flesh) / p.max_health();
            let (vital, worst) = if robot_race || p.robotic { (&mut vital_r, &mut worst_r) } else { (&mut vital_f, &mut worst_f) };
            if p.vital {
                *vital = vital.min(share);
            }
            *worst = worst.min(share);
        }
        let whole = |v: f32| if 0.999 < v { 1.0 } else { v };
        let side = |vital: f32, worst: f32| (1.0 - whole(vital)).max((1.0 - whole(worst)) * 0.5);
        let blood = if 0.0 < self.max_blood { self.blood / self.max_blood } else { 1.0 };
        let bleeding = (1.0 - blood) * (self.bleed_rate - self.limb_bleed * 1.5).max(0.0) * 10.0;
        let mut need = AidNeed { flesh: side(vital_f, worst_f), robotic: side(vital_r, worst_r) };
        let own = if robot_race { &mut need.robotic } else { &mut need.flesh };
        if *own < bleeding && 0.0 < bleeding {
            *own = bleeding;
        }
        need
    }

    /// @fn F031038
    ///
    /// One tick of first aid on this (the patient) by a medic of medic skill `skill` (CharStats
    /// +0x98; robotics +0xdc on a robot), with `kit` (None: none), `player` the medic being the
    /// player's. The skill used is the kit's quality at most, 5 without a kit; the bleeding is
    /// stopped first (F040465), then the parts are treated (F040454).
    pub fn first_aid(&mut self, skill: f32, kit: Option<Kit>, player: bool, robot: bool, c: &FirstAidConstants, dt: f32) -> FirstAidTick {
        let mut out = FirstAidTick::default();
        if self.dead || kit.is_some_and(|k| k.robot != robot) {
            out.done = true;
            return out;
        }
        let level = skill.min(kit.map_or(5.0, |k| k.quality));
        let f = level * 0.01;
        // F040465: the wounds, the last first, then the bleeding
        let stopped = if let Some(w) = self.wounds.last_mut() {
            *w -= c.speed * 2.0 * lerp(f, 0.0005, 0.03) * dt;
            if *w < 0.0 {
                self.wounds.pop();
            }
            if let Some(k) = kit {
                out.drained += kit_drain(skill, k, c, dt);
            }
            false
        } else {
            let mut stopped = true;
            if 0.0 < self.bleed_rate {
                let (_, d) = self.treat_parts(level, kit, player, robot, skill, c, dt);
                out.drained += d;
                self.bleed_rate = (self.bleed_rate - f * dt).max(0.0);
                stopped = self.bleed_rate <= self.limb_bleed * 2.0;
            }
            stopped
        };
        if !stopped {
            return out;
        }
        // F040454: each part's treatment pool filled to what its flesh lacks
        let (all, d) = self.treat_parts(level, kit, player, robot, skill, c, dt);
        out.drained += d;
        out.done = all;
        out
    }

    /// @fn F040454
    ///
    /// The first part short of treatment (pool + flesh under max) gets `rate * dt` more, rate =
    /// lerp(level / 100, 1, 15) x medic speed (x robot medic speed with a robot kit), 1 without
    /// a kit; capped at what the flesh lacks. Done (true) when no part lacks any. A player's medic
    /// needs a kit for this and uses its charges.
    #[allow(clippy::too_many_arguments)]
    fn treat_parts(&mut self, level: f32, kit: Option<Kit>, player: bool, robot: bool, skill: f32, c: &FirstAidConstants, dt: f32) -> (bool, f32) {
        if self.dead || (kit.is_none() && player) {
            return (true, 0.0);
        }
        let rate = match kit {
            Some(k) => lerp(level * 0.01, 1.0, 15.0) * c.speed * if k.robot { c.robot_speed } else { 1.0 },
            None => 1.0,
        };
        let mut drained = 0.0;
        for p in &mut self.parts {
            let max = p.max_health();
            if p.treatment >= 100_000.0 || p.treatment + p.flesh >= max {
                continue;
            }
            // (assumption) a part's robot-ness is the character's (a robot limb on a person
            // aside)
            if kit.is_some_and(|k| k.robot != (robot || p.robotic)) {
                continue;
            }
            p.treatment += rate * dt;
            let capped = max < p.treatment + p.flesh;
            if capped {
                p.treatment = max - p.flesh;
            }
            p.update_frac();
            if player && let Some(k) = kit {
                drained += kit_drain(skill, k, c, dt);
            }
            if !capped {
                return (false, drained);
            }
        }
        (true, drained)
    }

}
