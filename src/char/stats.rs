//! Stats training: Kenshi's "improve by doing", with diminishing returns as a stat approaches
//! its cap.
//!
//! Specs: sheet rows F047450 CharStats::addXP, F047460 train, F047461 trainUpTo,
//! F047457 trainEngineering, F047027 statById, F047454 skillDiffMult, F047466 trainToughness,
//! F047459 trainDefence;
//! ids T00029 StatsEnumerated.

use crate::StatsEnumerated;
use std::collections::HashMap;

/// The cap every normal training curve uses (F047460).
pub const STAT_CAP: f32 = 101.0;
/// Base gain factor applied to training amounts (a data global read by the train functions).
pub const XP_SCALE: f32 = 0.02;
/// Stored "exp gain multiplier" = record value * this (T00025 field 0x138).
pub const EXP_GAIN_STORE_SCALE: f32 = 0.25;
/// Extra factor engineering training applies (F047457).
pub const ENGINEERING_FACTOR: f32 = 0.75;

/// @fn F047450
///
/// Raise `level` by `gain` scaled by ((cap - level) / cap)^2. Gains outside (0, 20] change nothing.
pub fn add_xp(level: &mut f32, gain: f32, cap: f32) {
    let factor = ((cap - *level) * (1.0 / cap)).powi(2);
    if gain > 0.0 && factor > 0.0 && gain <= 20.0 && factor <= 20.0 {
        let old = *level;
        *level = factor * gain + old;
        if level.is_nan() {
            *level = if old.is_nan() { 20.0 } else { old };
        }
    }
}

/// A character's stats by id with the race's per-stat XP multipliers.
#[derive(Debug, Clone, Default)]
pub struct Skills {
    pub levels: HashMap<StatsEnumerated, f32>,
    /// Race XP multiplier per stat (race +0xa0); missing = 1.0.
    pub race_xp: HashMap<StatsEnumerated, f32>,
    /// The GLOBAL CONSTANTS "exp gain multiplier" as stored (record value * 0.25).
    pub exp_gain: f32,
    /// Multipliers on stats' levels (robotic limbs'); missing = 1.0.
    pub mults: HashMap<StatsEnumerated, f32>,
}

impl Skills {
    pub fn new(exp_gain_multiplier: f32) -> Self {
        Skills {
            exp_gain: exp_gain_multiplier * EXP_GAIN_STORE_SCALE,
            ..Default::default()
        }
    }

    /// @fn F047027
    /// A stat's level, with its multiplier (robotic limbs).
    pub fn level(&self, s: StatsEnumerated) -> f32 {
        self.levels.get(&s).copied().unwrap_or(0.0) * self.mults.get(&s).copied().unwrap_or(1.0)
    }

    fn gain(&self, s: StatsEnumerated, amount: f32, mult: f32) -> f32 {
        XP_SCALE * amount * self.exp_gain * mult * self.race_xp.get(&s).copied().unwrap_or(1.0)
    }

    /// @fn F047460
    pub fn train(&mut self, s: StatsEnumerated, amount: f32, mult: f32) {
        let g = self.gain(s, amount, mult);
        add_xp(self.levels.entry(s).or_insert(0.0), g, STAT_CAP);
    }

    /// @fn F047461
    pub fn train_up_to(&mut self, s: StatsEnumerated, amount: f32, mult: f32, limit: f32) {
        if self.level(s) < limit {
            let g = self.gain(s, amount, mult);
            add_xp(self.levels.entry(s).or_insert(0.0), g, limit);
        }
    }

    /// @fn F047457
    pub fn train_engineering(&mut self, amount: f32) {
        let s = StatsEnumerated::StatEngineering;
        let g = self.gain(s, amount, ENGINEERING_FACTOR);
        add_xp(self.levels.entry(s).or_insert(0.0), g, STAT_CAP);
    }
}

/// @fn F047454
///
/// How much a fight teaches, by the level gap to the opponent: +1x per `bonus_gap` levels the
/// opponent is above, down to nothing `penalty_gap` levels below (GLOBAL CONSTANTS "skill diff xp
/// 2x bonus" / "skill diff xp 0x penalty"), clamped to [0.1, 6]; `group` (> 1) adds (group-1)/10.
pub fn skill_diff_mult(mine: f32, other: f32, bonus_gap: f32, penalty_gap: f32, group: u32) -> f32 {
    let gap = if mine > other { penalty_gap } else { bonus_gap };
    let mut d = (other - mine) * 0.01 * (100.0 / gap) + 1.0;
    d = if d.is_nan() { 0.1 } else { d.clamp(0.1, 6.0) };
    let n = group.saturating_sub(1);
    let f = if n > 0 { n as f32 / 10.0 + 1.0 } else { 1.0 };
    (f * d).clamp(0.01, 10.0)
}

/// What kind of hit trains toughness (F047466's kind argument).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToughnessKind {
    Kind1,
    Kind5,
    Other,
}

impl Skills {
    /// @fn F047466
    ///
    /// `g1` is the combat XP base (exp gain multiplier stored * 0.1), `g2` the global XP factor.
    pub fn train_toughness(
        &mut self,
        kind: ToughnessKind,
        xp_rate_toughness: f32,
        g1: f32,
        g2: f32,
    ) {
        let k = match kind {
            ToughnessKind::Kind1 => 0.25,
            ToughnessKind::Kind5 => 0.5,
            ToughnessKind::Other => 0.05,
        };
        let s = StatsEnumerated::StatToughness;
        let g = g1 * k * xp_rate_toughness * self.race_xp.get(&s).copied().unwrap_or(1.0) * g2;
        add_xp(self.levels.entry(s).or_insert(0.0), g, STAT_CAP);
    }
}

impl Skills {
    /// @fn F047459
    ///
    /// Train the defence skill (+0xf0; see below) after a block roll against an attacker of skill
    /// `other`; `mine` is the defender's defence level (F047048), `load` CharStats +0x190, `g1`/`g2`
    /// the combat XP base and global XP factor, `group` the skillDiffMult group size.
    #[allow(clippy::too_many_arguments)]
    pub fn train_defence(
        &mut self,
        mine: f32,
        other: f32,
        blocked: bool,
        load: f32,
        g1: f32,
        g2: f32,
        (bonus_gap, penalty_gap, group): (f32, f32, u32),
    ) {
        let k = if blocked { 0.4 } else { 0.8 };
        // (assumption) the field +0xf0 taken as melee defence (see the combat crate's
        // defence_level)
        let s = StatsEnumerated::StatMeleeDefence;
        let race = self.race_xp.get(&s).copied().unwrap_or(1.0);
        let g =
            load * k * g1 * race * g2 * skill_diff_mult(mine, other, bonus_gap, penalty_gap, group);
        add_xp(self.levels.entry(s).or_insert(0.0), g, STAT_CAP);
    }
}

/// A STATS record key's stat (the CharStats fields of T00026 with a stat id; "endurance",
/// "bluff", "climbing", "tracking", "xp" have none).
pub fn stat_of_key(key: &str) -> Option<StatsEnumerated> {
    use StatsEnumerated::*;
    Some(match key {
        "strength" => StatStrength,
        "attack" => StatMeleeAttack,
        "labouring" => StatLabouring,
        "science" => StatScience,
        "engineer" => StatEngineering,
        "robotics" => StatRobotics,
        "weapon smith" => StatSmithingWeapon,
        "armour smith" => StatSmithingArmour,
        "medic" => StatMedic,
        "thievery" => StatThieving,
        "turrets" => StatTurrets,
        "farming" => StatFarming,
        "cooking" => StatCooking,
        "stealth" => StatStealth,
        "athletics" => StatAthletics,
        "dexterity" => StatDexterity,
        "defence" => StatMeleeDefence,
        "toughness2" => StatToughness,
        "assassin" => StatAssassination,
        "swimming" => StatSwimming,
        "perception" => StatPerception,
        "katana" => StatKatanas,
        "sabres" => StatSabres,
        "hackers" => StatHackers,
        "heavy weapons" => StatHeavyweapons,
        "blunt" => StatBlunt,
        "unarmed" => StatMartialarts,
        "mass combat" => StatMasscombat,
        "dodge" => StatDodge,
        "survival" => StatSurvival,
        "poles" => StatPolearms,
        "bow" => StatCrossbows,
        "ff" => StatFriendlyFire,
        "lockpicking" => StatLockpicking,
        "bow smith" => StatSmithingBow,
        _ => return None,
    })
}

/// @fn F033689
///
/// A race's stat multipliers (RaceData +0xa0): 1.2 for its "stats good<i>", 0.8 for its
/// "stats bad<i>" (each a stat id, 0 none), 1 for the rest. They scale both the starting levels
/// (F040461) and the XP each stat gains.
pub fn race_multipliers(race: &kenshi_data::GameData) -> HashMap<StatsEnumerated, f32> {
    let mut m = HashMap::new();
    for (prefix, v) in [("stats good", 1.2f32), ("stats bad", 0.8)] {
        for i in 0.. {
            let Some(&id) = race.ints.get(&format!("{prefix}{i}")) else { break };
            if let Some(s) = StatsEnumerated::from_i32(id).filter(|_| id != 0) {
                m.insert(s, v);
            }
        }
    }
    m
}

/// @fn F040461
///
/// A character's starting stats from its CHARACTER record: its STATS record's levels, or,
/// when it has none, the record's templates ("combat stats", "unarmed stats", "stealth
/// stats", "strength", "ranged stats", each only when above 0); then "stats randomise", then
/// each stat times the race's multiplier. `rand` gives uniform [0, 1).
///
/// (stand-in) The randomise helper is not ported: each templated stat moves by up to
/// +-"stats randomise" levels, never below 0.
pub fn character_levels(
    data: &kenshi_data::GameDataContainer,
    character: &kenshi_data::GameData,
    race_mult: &HashMap<StatsEnumerated, f32>,
    rand: &mut dyn FnMut() -> f32,
) -> HashMap<StatsEnumerated, f32> {
    use StatsEnumerated::*;
    let mut lv: HashMap<StatsEnumerated, f32> = HashMap::new();
    let stats = character.refs("stats").first().and_then(|(sid, _)| data.get(sid));
    match stats {
        Some(r) => {
            for (k, v) in &r.floats {
                if let Some(s) = stat_of_key(k) {
                    lv.insert(s, *v);
                }
            }
            for (k, v) in &r.ints {
                if let Some(s) = stat_of_key(k) {
                    lv.insert(s, *v as f32);
                }
            }
        }
        None => {
            let int = |k: &str| character.ints.get(k).copied().unwrap_or(0);
            let c = int("combat stats");
            if c > 0 {
                let c = c as f32;
                for s in [
                    StatStrength, StatDexterity, StatToughness, StatMeleeAttack, StatMeleeDefence,
                    StatKatanas, StatSabres, StatBlunt, StatPolearms, StatHeavyweapons, StatHackers,
                    StatTurrets, StatAthletics, StatSwimming, StatMedic, StatFriendlyFire,
                ] {
                    lv.insert(s, c);
                }
                lv.insert(StatAssassination, c * 0.5);
                lv.insert(StatMartialarts, c * 0.1);
                lv.insert(StatDodge, c * 0.1);
            }
            let u = int("unarmed stats");
            if u > 0 {
                for s in [StatMartialarts, StatDodge, StatDexterity] {
                    lv.insert(s, u as f32);
                }
            }
            let st = int("stealth stats");
            if st > 0 {
                lv.insert(StatDexterity, u.max(st) as f32);
                for s in [StatStealth, StatThieving, StatAssassination, StatLockpicking, StatMedic] {
                    lv.insert(s, st as f32);
                }
            }
            let strength = int("strength");
            if strength > 0 {
                lv.insert(StatStrength, strength as f32);
            }
            let r = int("ranged stats");
            if r > 0 {
                for s in [StatCrossbows, StatPerception, StatFriendlyFire, StatTurrets] {
                    lv.insert(s, r as f32);
                }
            }
            let spread = character.ints.get("stats randomise").copied().unwrap_or(0) as f32;
            if spread > 0.0 {
                for v in lv.values_mut() {
                    *v = (*v + (rand() * 2.0 - 1.0) * spread).max(0.0);
                }
            }
        }
    }
    for (s, v) in lv.iter_mut() {
        *v *= race_mult.get(s).copied().unwrap_or(1.0);
    }
    lv
}

/// The GLOBAL CONSTANTS training reads (shipped values as defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XpConstants {
    /// "exp gain multiplier" as stored (record value x 0.25).
    pub exp_gain: f32,
    pub xp_rate_strength: f32,
    pub xp_rate_toughness: f32,
    pub xp_rate_athletics: f32,
    /// "skill diff xp 2x bonus" / "skill diff xp 0x penalty".
    pub bonus_gap: f32,
    pub penalty_gap: f32,
    /// (assumption) the global XP factor (0x142133580) is not read yet: 1.
    pub global: f32,
}

impl Default for XpConstants {
    fn default() -> Self {
        XpConstants {
            exp_gain: 3.0 * EXP_GAIN_STORE_SCALE,
            xp_rate_strength: 0.5,
            xp_rate_toughness: 1.33,
            xp_rate_athletics: 1.0,
            bonus_gap: 10.0,
            penalty_gap: 25.0,
            global: 1.0,
        }
    }
}

impl XpConstants {
    /// From the GLOBAL CONSTANTS record's floats (`f` reads one by name).
    pub fn from_record(f: impl Fn(&str) -> Option<f32>) -> XpConstants {
        let d = XpConstants::default();
        XpConstants {
            exp_gain: f("exp gain multiplier").map_or(d.exp_gain, |v| v * EXP_GAIN_STORE_SCALE),
            xp_rate_strength: f("xp rate strength").unwrap_or(d.xp_rate_strength),
            xp_rate_toughness: f("xp rate toughness").unwrap_or(d.xp_rate_toughness),
            xp_rate_athletics: f("xp rate athletics").unwrap_or(d.xp_rate_athletics),
            bonus_gap: f("skill diff xp 2x bonus").unwrap_or(d.bonus_gap),
            penalty_gap: f("skill diff xp 0x penalty").unwrap_or(d.penalty_gap),
            global: d.global,
        }
    }
}

/// What happened in a fight, for trainFromCombat's event type (F047467). (assumption) Which
/// game event raises which type: 0 my attack landed, 1 my attack was blocked, 2 I blocked an
/// attack, 4 I was hit; type 6 (a weaker attack event) is not raised yet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CombatEvent {
    Landed,
    Blocked,
    Parried,
    Hit { damage: f32 },
}

/// A fighter's side of a combat event: its weapon's skill (None unarmed), the opponent's level
/// the gap is taken against, the load factor (CharStats +0x190), the share of its weight the
/// armour is (dexterity training) and the weapon strain (CharStats +0x184, assumption).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatTraining {
    pub weapon: Option<StatsEnumerated>,
    pub mine: f32,
    pub theirs: f32,
    pub load: f32,
    pub armour_share: f32,
    pub strain: f32,
    pub group: u32,
}

impl Skills {
    fn race(&self, s: StatsEnumerated) -> f32 {
        self.race_xp.get(&s).copied().unwrap_or(1.0)
    }

    fn add(&mut self, s: StatsEnumerated, g: f32) {
        add_xp(self.levels.entry(s).or_insert(0.0), g, STAT_CAP);
    }

    /// @fn F047467
    ///
    /// Train from one combat event: attack (martial arts unarmed), the weapon's skill and
    /// dexterity for attacks; melee defence for blocks; melee defence and toughness for hits
    /// taken; then strength from the effort.
    pub fn train_from_combat(&mut self, e: CombatEvent, t: &CombatTraining, c: &XpConstants) {
        use StatsEnumerated::*;
        let g1 = c.exp_gain * 0.1;
        let g2 = c.global;
        let diff = skill_diff_mult(t.mine, t.theirs, c.bonus_gap, c.penalty_gap, t.group);
        let attack = if t.weapon.is_some() { StatMeleeAttack } else { StatMartialarts };
        let (a, w) = match e {
            CombatEvent::Landed => (1.0, 1.0),
            CombatEvent::Blocked => (0.5, 0.4),
            CombatEvent::Parried => (0.0, 0.1),
            CombatEvent::Hit { .. } => (0.0, 0.0),
        };
        if a > 0.0 {
            self.add(attack, g1 * a * diff * self.race(attack) * g2);
            self.add(StatDexterity, g1 * a * diff * t.armour_share * self.race(StatDexterity));
        }
        if let Some(ws) = t.weapon
            && w > 0.0
        {
            self.add(ws, g1 * w * diff * self.race(ws) * g2);
        }
        match e {
            CombatEvent::Parried => {
                self.add(StatMeleeDefence, 0.25 * g1 * diff * self.race(StatMeleeDefence));
            }
            CombatEvent::Hit { damage } => {
                self.add(StatMeleeDefence, 2.0 * g1 * diff * self.race(StatMeleeDefence));
                let tdiff = skill_diff_mult(self.level(StatToughness), damage, c.bonus_gap, c.penalty_gap, 1);
                self.add(StatToughness, c.xp_rate_toughness * g1 * tdiff * self.race(StatToughness));
            }
            _ => {}
        }
        let k = match e {
            CombatEvent::Parried => 0.4,
            _ => 1.0,
        };
        let effort = if t.weapon.is_none() { 1.0 - t.load } else { t.strain };
        self.add(StatStrength, g1 * 0.55 * 2.0 * c.xp_rate_strength * k * effort * self.race(StatStrength));
    }

    /// @fn F047463
    ///
    /// Moving trains athletics (swimming in water: at 5); (assumption) `amount` is the
    /// distance-and-speed factor the caller works out per tick.
    pub fn train_athletics(&mut self, amount: f32, swimming: bool, c: &XpConstants) {
        if swimming {
            self.train(StatsEnumerated::StatSwimming, 5.0 * amount, 1.0);
        } else {
            self.train(StatsEnumerated::StatAthletics, amount * c.xp_rate_athletics, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diminishing_returns_toward_the_cap() {
        let mut low = 0.0;
        add_xp(&mut low, 1.0, STAT_CAP);
        let mut high = 90.0;
        add_xp(&mut high, 1.0, STAT_CAP);
        assert!((low - 1.0).abs() < 1e-6);
        let expected = ((101.0f32 - 90.0) / 101.0).powi(2);
        assert!((high - 90.0 - expected).abs() < 1e-4); // f32 resolution near 90 is ~8e-6
        // out-of-range gains do nothing
        let mut x = 10.0;
        add_xp(&mut x, 0.0, STAT_CAP);
        add_xp(&mut x, 25.0, STAT_CAP);
        assert_eq!(x, 10.0);
    }

    #[test]
    fn training_uses_exp_gain_and_race_multipliers() {
        let mut s = Skills::new(3.0); // shipped exp gain multiplier
        s.race_xp.insert(StatsEnumerated::StatMedic, 2.0);
        s.train(StatsEnumerated::StatMedic, 10.0, 1.0);
        // 0.02 * 10 * 0.75 * 1 * 2 = 0.3 at level 0 (factor 1)
        assert!((s.level(StatsEnumerated::StatMedic) - 0.3).abs() < 1e-6);
        // capped training stops at its limit
        s.levels.insert(StatsEnumerated::StatDodge, 15.0);
        s.train_up_to(StatsEnumerated::StatDodge, 100.0, 1.0, 15.0);
        assert_eq!(s.level(StatsEnumerated::StatDodge), 15.0);
        s.train_engineering(10.0);
        assert!(
            (s.level(StatsEnumerated::StatEngineering) - 0.02 * 10.0 * 0.75 * 0.75).abs() < 1e-6
        );
    }

    #[test]
    fn fighting_trains_the_right_stats() {
        use StatsEnumerated::*;
        let c = XpConstants::default();
        let t = CombatTraining {
            weapon: Some(StatKatanas),
            mine: 10.0,
            theirs: 10.0,
            load: 1.0,
            armour_share: 0.5,
            strain: 0.5,
            group: 1,
        };
        let mut s = Skills::new(3.0);
        s.train_from_combat(CombatEvent::Landed, &t, &c);
        assert!(s.level(StatMeleeAttack) > 0.0);
        assert!(s.level(StatKatanas) > 0.0);
        assert!(s.level(StatDexterity) > 0.0);
        assert_eq!(s.level(StatToughness), 0.0);
        let mut d = Skills::new(3.0);
        d.train_from_combat(CombatEvent::Hit { damage: 20.0 }, &t, &c);
        assert!(d.level(StatToughness) > 0.0);
        assert!(d.level(StatMeleeDefence) > 0.0);
        assert_eq!(d.level(StatMeleeAttack), 0.0);
        // unarmed attacks train martial arts, not melee attack
        let mut u = Skills::new(3.0);
        u.train_from_combat(CombatEvent::Landed, &CombatTraining { weapon: None, ..t }, &c);
        assert!(u.level(StatMartialarts) > 0.0);
        assert_eq!(u.level(StatMeleeAttack), 0.0);
    }

    #[test]
    fn stronger_opponents_teach_more() {
        // shipped constants: 2x at +10 levels, 0x at -25 levels
        assert_eq!(skill_diff_mult(20.0, 20.0, 10.0, 25.0, 1), 1.0);
        assert!((skill_diff_mult(20.0, 30.0, 10.0, 25.0, 1) - 2.0).abs() < 1e-5);
        assert_eq!(skill_diff_mult(50.0, 25.0, 10.0, 25.0, 1), 0.1); // clamped at the floor
        assert_eq!(skill_diff_mult(0.0, 100.0, 10.0, 25.0, 1), 6.0); // clamped at the top
        assert!((skill_diff_mult(20.0, 20.0, 10.0, 25.0, 11) - 2.0).abs() < 1e-5); // group of 11
    }

    #[test]
    fn failed_blocks_teach_twice_as_much() {
        let mut a = Skills::new(3.0);
        let mut b = Skills::new(3.0);
        a.train_defence(10.0, 10.0, true, 1.0, 1.0, 1.0, (10.0, 25.0, 1));
        b.train_defence(10.0, 10.0, false, 1.0, 1.0, 1.0, (10.0, 25.0, 1));
        let (la, lb) = (
            a.level(StatsEnumerated::StatMeleeDefence),
            b.level(StatsEnumerated::StatMeleeDefence),
        );
        assert!((la - 0.4).abs() < 1e-6 && (lb - 0.8).abs() < 1e-6);
    }

    #[test]
    fn toughness_training_by_kind() {
        let mut s = Skills::new(3.0);
        s.train_toughness(ToughnessKind::Kind5, 1.33, 1.0, 1.0);
        assert!((s.level(StatsEnumerated::StatToughness) - 0.5 * 1.33).abs() < 1e-6);
    }
}
