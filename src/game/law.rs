//! Crime and law (port_map `law`): crimes seen by witnesses, bounties, guards arresting the
//! wanted, prison cages and sentences, escaping by picking locks, slavers enslaving the knocked
//! out, stealing and stealth knockouts.
//!
//! Specs:
//! - Committing (F046495, F046496): a crime against a faction, on a victim, kept for a time
//!   (`commit_crime_on`); a crime committed in a fight or caught red-handed also blows the
//!   criminal's disguise for 250 s (`commit_crime_for`).
//! - Witnesses (F046520, `witness`): whoever sees a criminal weighs its crime by the crime and
//!   the factions, raises the crime's event, tags the criminal ST_CRIMINAL, keeps the crime 20 s
//!   (F046479) and, a town's guard (F038593) of a faction that "offers bounties", catches it
//!   (F046511: the crime's bounty with its faction, once a crime; none for bandits, diplomats or
//!   law enforcement, nor for enslaving). Seeing someone carry a slave or a captive is a crime of
//!   freeing slaves or kidnapping (F046519).
//! - F046477 (bounty by crime): enslaving / trespassing 100, assault / looting / farm eating 500,
//!   lockpicking / fencing 1000, stealing / escaping prison / uniform theft 2500, murder / slave
//!   freeing / smuggling / terrorism / kidnapping 5000, assault on a VIP 50000.
//! - F046478: a bounty expires that many x 0.04 hours after it was last raised; 10000 or more
//!   never ("Notorious").
//! - F046518: a prison sentence is 16 hours without a bounty, the bounty x 0.02 x GLOBAL
//!   CONSTANTS "prison time" hours up to 25000, else for life.
//! - F029315 (a lock picked): each attempt (made as the picker's speed fills the lock's attempt
//!   time; speed lerp(lockpicking / 100, 0.55, 2.0), F031344) opens it with chance 0.9, or 0.9 /
//!   2^((lock level - skill) x 0.1) when the lock is better than the skill; held in a cage the
//!   skill counts 10 less (F031344). Picking is a crime each time: a cage, lockpicking against
//!   its faction (F031344); shackles, freeing a slave against its owners (F031345).
//! - Escaping (F031326): out of an open cage, a prisoner is an escaped prisoner (the state
//!   object +0xe8) until it leaves town (F038706), a slave is escaping, and it commits the crime
//!   of escaping prison. Lifted out of a cage by the player's people: the lifter frees a slave
//!   and the prisoner escapes prison (F031291). Caged again it is no escaped prisoner, and one
//!   with an owner caged outside a prison town is a slave again (F030719).
//! - F047050 (stealth knockout chance): the attacker's assassination less the target's best
//!   guard (the most of its assassination, half its thievery, half its stealth (F047035), half
//!   its melee defence, its toughness), d: (d + 30) / 60 when d >= 0, (d + 15) / 30 when below;
//!   none on a target that is fighting.
//! - Factions' "cages lock level" and "lock level random" (FACTION records), cages are
//!   furniture of function 8 (BUILDING_FUNCTIONALITY 'Cage').
//!
//! (assumption) A witness must see the criminal as the sensor sees (F046539, the criminal's
//! sneaking and stealth counted); guards are the armed people of the faction who are not
//! shopkeepers; they arrest the wanted they see (F046512's bounty, F050281's targets) by
//! beating them down, then carry them to the
//! nearest free cage of their faction's towns (none: they leave them); prisoners keep to their
//! cage until the sentence is served (the bounty with that faction then cleared) or they pick
//! its lock (lock level: the faction's "cages lock level" +- its "lock level random"); a lock's
//! attempt comes every 2 to 4 seconds of pick work; slavers take the knocked out they carry to
//! their faction's nearest cage and shackle them (shackles' lock 40); a shackled slave picks
//! the shackles, then the cage; paying a bounty to a guard of the faction clears it.
//! Not ported: the guards' "halt" talk and searches, doors' locks
//! (slaves' work, sale and recognition are crate::slavery's).

use crate::sim::Sim;
use kenshi_ai::CrimeEnum;
use kenshi_ai::perception::{Observer, Stance, Target as Seen, daylight, sees};
use kenshi_char::StatsEnumerated;
use kenshi_data::ItemType;

/// GLOBAL CONSTANTS "prison time" (GlobalConstants +0x16c, shipped 1).
pub const PRISON_TIME: f32 = 1.0;
/// How long a crime stays one once seen (seconds; F046479, F046495's assault).
const CRIME_SECONDS: f32 = 20.0;
/// EventTriggerEnum values (fcs.def).
pub const EV_THIEF_CAUGHT_STEALING_FROM_ME: i32 = 8;
pub const EV_UNLOCK_MY_CAGE_OR_SHACKLES: i32 = 13;
pub const EV_UNLOCK_MY_CAGE_ATTEMPT: i32 = 14;
pub const EV_PRISONER_FREE_TO_GO: i32 = 46;
pub const EV_EATING_MY_CROPS: i32 = 57;
pub const EV_KIDNAPPING_MY_ALLY: i32 = 58;
/// The lock level of shackles (assumption).
const SHACKLES_LOCK: f32 = 40.0;
/// How much less the lockpicking skill counts for one held in a cage (F031344).
const CAGED_PICK_PENALTY: f32 = 10.0;

/// @fn F046477
///
/// The bounty a crime adds.
pub fn bounty_for(c: CrimeEnum) -> i32 {
    use CrimeEnum::*;
    match c {
        CrimeEnslaving | CrimeTresspassing => 100,
        CrimeAssault | CrimeLooting | CrimeFarmEating => 500,
        CrimeLockpicking | CrimeFencing => 1000,
        CrimeStealing | CrimeEscapePrison | CrimeUniformTheft => 2500,
        CrimeMurder | CrimeSlaveFreeing | CrimeSmuggling | CrimeTerrorism | CrimeKidnapping => 5000,
        CrimeAssaultVip => 50000,
        _ => 0,
    }
}

/// @fn F046478
///
/// How long a bounty lasts (hours): its amount x 0.04; 10000 or more: forever.
pub fn bounty_hours(amount: i32) -> f64 {
    if amount >= 10000 { 1e7 } else { amount as f64 * 0.04 }
}

/// @fn F046518
///
/// A prison sentence (hours) for a bounty.
pub fn sentence_hours(bounty: i32) -> f64 {
    if bounty == 0 {
        16.0
    } else if bounty < 25001 {
        bounty as f64 * 0.02 * PRISON_TIME as f64
    } else {
        999_999.0
    }
}

/// @fn F029315
///
/// The chance one attempt picks a lock of `lock` level at lockpicking `skill`.
pub fn lockpick_chance(lock: f32, skill: f32) -> f32 {
    if lock - skill > 0.0 { 0.9 / 2f32.powf((lock - skill) * 0.1) } else { 0.9 }
}

/// @fn F047050
///
/// The chance (0..1, not capped) of a stealth knockout: assassination against the target's
/// best guard.
pub fn stealth_ko_chance(assassination: f32, guard: f32) -> f32 {
    let d = assassination - guard;
    let c = if d < 0.0 { 15.0 } else { 30.0 };
    (d + c) / (2.0 * c)
}

/// A bounty with one faction.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bounty {
    pub faction: String,
    pub amount: i32,
    /// The crimes in it (1 << CrimeEnum).
    pub crimes: u32,
    /// When last raised (game hours).
    pub stamp: f64,
}

/// A crime being committed (CharacterCrimes +0x58..): what, against whom (the faction, +0x60;
/// empty for none), and how long it stays one (+0x90).
#[derive(Debug, Clone, PartialEq)]
pub struct Crime {
    pub kind: CrimeEnum,
    pub faction: String,
    pub left: f32,
    /// Seen already (CharacterCrimes +0xa4): the bounty is added once while the same crime goes
    /// on (F046511), cleared when another starts (F046495).
    pub caught: bool,
    /// The one it is committed on (+0x78: the victim, or whoever stands for the place).
    pub victim: Option<usize>,
}

/// What the law reads of each step: the melee hits that landed in it (indices into the
/// arena's hit list; crate::sim marks them round the arena's step).
#[derive(Debug, Clone, Default)]
pub struct LawState {
    pub melee_hits: std::ops::Range<usize>,
    /// Prisoners lately released and until when (game hours; Character +0x138 / +0x140,
    /// Task_ReleasePrisoner): no trespassers or intruders meanwhile.
    pub released: std::collections::HashMap<usize, f64>,
    /// New slaves being processed (crate::slavery `processing_tick`).
    pub processing: Vec<NewSlave>,
    /// The homes' "locked up" flags (the squad's AI +0xe8, F044498) and each door's phase
    /// last seen (crate::intruders `locked_up_tick`).
    pub locked_up: std::collections::HashMap<usize, bool>,
    pub door_phase: Vec<i32>,
}

/// A new slave being processed by `taker` for `faction` (Task_ProcessNewSlave), seconds left.
#[derive(Debug, Clone)]
pub struct NewSlave {
    pub taker: usize,
    pub slave: usize,
    pub faction: String,
    pub left: f32,
}

/// A lock being picked or a cage held in.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lock {
    pub level: f32,
    /// Pick work done toward the next attempt, and that attempt's time.
    pub progress: f32,
    pub next: f32,
}

/// A prisoner's cage.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Prison {
    /// The cage (furniture index) and its faction.
    pub cage: usize,
    pub faction: String,
    /// When the sentence ends (game hours).
    pub until: f64,
    /// A slave (no sentence).
    pub slave: bool,
    pub lock: Lock,
    /// A slave let out of its cage (to work, to follow its master): not kept in it.
    pub out: bool,
}

/// What a lawman or a slaver is doing with someone: beating them down, carrying them to a
/// cage.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Arrest {
    pub target: usize,
    pub slaver: bool,
    /// Whose cage the catch goes to, when not the arrester's own faction's (a bounty hunter
    /// takes a wanted one to the faction that posted the bounty).
    #[serde(default)]
    pub to: Option<String>,
}

/// What a character's noise reads (F046538 / F046476): sneaking, fighting, its speed (units /
/// s), its stealth level (0 .. 1) and the building it is in.
#[derive(Debug, Clone, Copy)]
pub struct Noise {
    pub sneaking: bool,
    pub fighting: bool,
    pub speed: f32,
    pub stealth: f32,
    pub inside: Option<usize>,
}

/// The noise roll of `Sim::heard` for observer `seer` (awake, in building `inside`) and target
/// `seen` at squared distance `d2`, at sim time `t` (seconds).
pub fn hear_roll(seer: usize, seen: usize, t: f32, walk: f32, inside: Option<usize>, d2: f32, n: &Noise) -> bool {
    const NOISE: f32 = 400.0;
    if seer == seen || (!n.sneaking && !n.fighting) || inside != n.inside {
        return false;
    }
    let mode = if n.speed > walk * 1.1 {
        0.25
    } else if n.speed > 1.0 {
        0.75
    } else {
        1.0
    };
    let mut r = (1.0 + (mode - 1.0) * NOISE).abs() * 0.5;
    if inside.is_some() {
        r *= 0.5;
    }
    let near = 1.0 - d2 / (r * r);
    if near <= 0.0 {
        return false;
    }
    let k = if n.sneaking && !n.fighting { 0.05 * (1.0 - n.stealth) } else { 0.75 };
    let window = (t / 2.0).max(0.0) as u64;
    let roll = |w: u64| {
        // splitmix64 of the pair and the window: the same roll for the whole window
        let mut z = (seer as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (seen as u64).rotate_left(29) ^ w.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    };
    let mut chance = near * k;
    if window > 0 && roll(window - 1) < chance {
        chance *= 2.0;
    }
    roll(window) < chance
}

impl Sim {
    /// A person's name as shown: theirs, else (not the player's: their template is no name of
    /// theirs) their character's, else "Character n".
    pub fn display_name(&self, id: usize) -> String {
        let p = &self.folk[id];
        if !p.name.is_empty() {
            return p.name.clone();
        }
        if p.player {
            return format!("Character {id}");
        }
        p.character.as_deref().and_then(|c| self.data.get(c)).map_or(format!("Character {id}"), |r| r.name.clone())
    }

    /// Game hours now.
    pub(crate) fn now_hours(&self) -> f64 {
        self.hours_gone()
    }

    /// Faction +0x1d0 "not real", +0x34 "fundamental type", +0x8 "anti slavery", +0x40 "offers
    /// bounties" (the FACTION record, F045587).
    pub fn faction_not_real(&self, faction: &str) -> bool {
        self.data.get(faction).is_some_and(|f| f.bools.get("not real").copied().unwrap_or(false))
    }

    pub fn fundamental_type(&self, faction: &str) -> i32 {
        self.data.get(faction).and_then(|f| f.ints.get("fundamental type").copied()).unwrap_or(0)
    }

    pub fn anti_slavery(&self, faction: &str) -> bool {
        faction == crate::sim::PLAYER_FACTION || self.data.get(faction).is_some_and(|f| f.bools.get("anti slavery").copied().unwrap_or(false))
    }

    pub fn offers_bounties(&self, faction: &str) -> bool {
        self.data.get(faction).is_some_and(|f| f.bools.get("offers bounties").copied().unwrap_or(false))
    }

    /// The faction person `id` belongs to (Character vtable +0x58): the player's for the
    /// player's people.
    pub fn faction_id(&self, id: usize) -> Option<String> {
        let p = self.folk.get(id)?;
        if p.player { Some(crate::sim::PLAYER_FACTION.to_string()) } else { p.faction.clone() }
    }

    /// Person `id`'s NPC class (CharacterTypeEnum, the state object +0xdc: its CHARACTER's "NPC
    /// class").
    pub fn npc_class(&self, id: usize) -> i32 {
        self.folk[id].character.as_deref().and_then(|c| self.data.get(c)).and_then(|c| c.ints.get("NPC class").copied()).unwrap_or(0)
    }

    /// Person `id` commits crime `kind` against `faction` (CharacterCrimes::commit, F046495;
    /// `commit_crime_on` with no victim).
    pub fn commit_crime(&mut self, id: usize, kind: CrimeEnum, faction: Option<&str>) {
        self.commit_crime_on(id, kind, faction, None);
    }

    /// @fn F046495
    ///
    /// Person `id` commits crime `kind` against `faction`, on `victim` (the crime's handle,
    /// CharacterCrimes +0x78: the one wronged, or whoever stands for the place). No crime by a
    /// town's guard (F038593), by one held in a cage, against its own faction, against the
    /// faction it passes as (F046494) unless it is a slave of any state, nor against a faction
    /// "not real" or of "fundamental type" 8. An assault while another crime goes on keeps that
    /// crime (its time lifted to 20 s). Another crime than the last is not caught yet (+0xa4);
    /// the crime lasts at least 3 s (+0x90). Whether it was committed.
    /// (not ported) An assault on a character F044534 picks (its NPC class diplomat, the
    /// faction's +0x88 object asked) becomes crime 9.
    pub fn commit_crime_on(&mut self, id: usize, kind: CrimeEnum, faction: Option<&str>, victim: Option<usize>) -> bool {
        if id >= self.folk.len() || self.guards_town(id) || self.folk[id].prison.as_ref().is_some_and(|pr| !pr.out) {
            return false;
        }
        if faction.is_some() && self.faction_id(id).as_deref() == faction {
            return false;
        }
        if faction.is_some() && self.apparent_faction(id).as_deref() == faction && self.slave_state(id) == 0 {
            return false;
        }
        if faction.is_some_and(|f| self.faction_not_real(f) || self.fundamental_type(f) == 8) {
            return false;
        }
        // (a crime against no faction is kept with an empty one)
        let f = faction.unwrap_or("");
        let p = &mut self.folk[id];
        if kind == CrimeEnum::CrimeAssault
            && let Some(c) = p.crime.as_mut()
            && c.kind != CrimeEnum::CrimeAssault
        {
            c.left = c.left.max(CRIME_SECONDS);
            return false;
        }
        let caught = p.crime.as_ref().is_some_and(|c| c.kind == kind && c.caught);
        let left = p.crime.as_ref().map_or(0.0, |c| c.left).max(3.0);
        p.crime = Some(Crime { kind, faction: f.to_string(), left, caught, victim });
        true
    }

    /// @fn F046496
    ///
    /// Person `id` commits crime `kind` for `seconds` (assault in a fight 30, stealing caught
    /// 30, fencing 30, kidnapping 10, a uniform imposter 20): not while it is down or an
    /// animal, nor against a faction "not real"; its disguise is blown (250 s) and, committed
    /// (F046495), the crime lasts `seconds`.
    /// (not ported) An escape from prison stamps its time (CharacterCrimes +0x98) and the
    /// state object's +0xea is cleared: nothing here reads them.
    pub fn commit_crime_for(&mut self, id: usize, kind: CrimeEnum, faction: Option<&str>, victim: Option<usize>, seconds: f32) -> bool {
        if self.folk[id].down || self.folk[id].race.is_some() || faction.is_some_and(|f| self.faction_not_real(f)) {
            return false;
        }
        self.folk[id].disguise_blown = crate::disguise::BLOWN_SECONDS;
        self.folk[id].disguised = None;
        if !self.commit_crime_on(id, kind, faction, victim) {
            return false;
        }
        if let Some(c) = self.folk[id].crime.as_mut() {
            c.left = seconds;
        }
        true
    }

    /// @fn F030782
    ///
    /// `attacker` goes for `target` unprovoked (Unprovoked_Mixin, the attack tasks flagged +8):
    /// a target with no bounty with the attacker's faction, not one that started a fight itself
    /// (the state object +0x114), not a bandit, committing no crime and not fighting makes the
    /// attacker the aggressor (its +0x114) and its attack an assault on the target's faction, on
    /// the target (F046495).
    /// The player's attack orders call it (crate::sim `order_attack`).
    /// (assumption) Only the player's orders count as unprovoked (the mixin's +8 flag is not
    /// decompiled for each task). (not ported) The aggressor flag (+0x114) is not kept: no
    /// target is excused for it.
    pub fn unprovoked_attack(&mut self, attacker: usize, target: usize) -> bool {
        let Some(af) = self.faction_id(attacker) else { return false };
        let t = &self.folk[target];
        if 0 < self.bounty_with(target, &af) || self.npc_class(target) == 8 || t.crime.is_some() || self.arena.f[target].active {
            return false;
        }
        let tf = self.faction_id(target);
        self.commit_crime_on(attacker, CrimeEnum::CrimeAssault, tf.as_deref(), Some(target))
    }

    /// @fn F031266
    ///
    /// The crime of `cuffer` putting shackles on `target` (Task_CuffTarget): enslaving, against
    /// the target's faction, when the target is no slave, has no bounty and stands in a town of
    /// the player's or of an "anti slavery" faction (F046495).
    pub fn cuffing_crime(&mut self, cuffer: usize, target: usize) {
        if self.slave_state(target) != 0 || self.folk[target].bounties.iter().any(|b| b.amount > 0) {
            return;
        }
        let town = self.folk[target].town.and_then(|t| self.towns.get(t)).and_then(|t| t.faction.clone());
        if town.is_some_and(|f| self.anti_slavery(&f)) {
            let tf = self.faction_id(target);
            self.commit_crime_on(cuffer, CrimeEnum::CrimeEnslaving, tf.as_deref(), Some(target));
        }
    }

    /// @fn F031308
    /// @fn F031346
    ///
    /// The crime of each update `worker` spends on `target`'s shackles (Task_BruteForceShackles,
    /// Task_CutShackles; Task_PickLock_Shackles F031345 alike): freeing a slave against the
    /// target's owners (on the target) for another's, lockpicking against no one for its own
    /// (F046495).
    pub fn shackle_crime(&mut self, worker: usize, target: usize) {
        if worker == target {
            self.commit_crime_on(worker, CrimeEnum::CrimeLockpicking, None, None);
        } else {
            let owners = self.folk[target].slave.as_ref().map(|s| s.faction.clone());
            self.commit_crime_on(worker, CrimeEnum::CrimeSlaveFreeing, owners.as_deref(), Some(target));
        }
    }

    /// @fn F030932
    ///
    /// The strength shackles are broken and cut with: the body's fitness for strength
    /// (F040433, activity 1, as the damage curves ask it) x the strength level (F040360).
    pub fn shackle_strength(&self, id: usize) -> f32 {
        let f = &self.arena.f[id];
        f.fitness(kenshi_char::fitness::STRENGTH) * f.strength_level()
    }

    /// @fn F046479
    ///
    /// Person `id`'s crime seen: unless it is down, the crime lasts 20 s more from now and its
    /// disguise is blown (250 s).
    pub fn crime_seen(&mut self, id: usize) {
        if self.folk[id].down {
            return;
        }
        if let Some(c) = self.folk[id].crime.as_mut() {
            c.left = CRIME_SECONDS;
        }
        self.folk[id].disguise_blown = crate::disguise::BLOWN_SECONDS;
        self.folk[id].disguised = None;
    }

    /// @fn F046510
    ///
    /// A bounty of `amount` with `faction` put on person `id` (CharacterCrimes::addBounty: the
    /// amount added, the time stamped).
    pub fn add_bounty_amount(&mut self, id: usize, faction: &str, amount: i32) {
        let now = self.now_hours();
        let p = &mut self.folk[id];
        let b = match p.bounties.iter_mut().position(|b| b.faction == faction) {
            Some(k) => &mut p.bounties[k],
            None => {
                p.bounties.push(Bounty { faction: faction.to_string(), amount: 0, crimes: 0, stamp: now });
                p.bounties.last_mut().unwrap()
            }
        };
        b.amount += amount;
        b.stamp = now;
        let name = self.data.get(faction).map_or(faction.to_string(), |r| r.name.clone());
        self.messages.push(format!("{} now has a bounty with {name}: c.{}", self.display_name(id), b_amount(&self.folk[id], faction)));
    }

    /// @fn F046511
    ///
    /// Person `id`'s crime `kind` caught by `faction` (CharacterCrimes::caught): once a crime
    /// (+0xa4), not for bandits, diplomats or law enforcement (the criminal's NPC class, 8, 5,
    /// 1), and no bounty for enslaving; else the crime's bounty (F046477) added with `faction`,
    /// the crime marked in it, the time stamped and the disguise blown (250 s).
    /// (port) With no crime of that kind going on, the bounty is added as for a caught one.
    pub fn add_bounty(&mut self, id: usize, faction: &str, kind: CrimeEnum) {
        if matches!(self.npc_class(id), 1 | 5 | 8) || kind == CrimeEnum::CrimeNone {
            return;
        }
        if let Some(c) = self.folk[id].crime.as_mut().filter(|c| c.kind == kind) {
            if c.caught {
                return;
            }
            c.caught = true;
        }
        if kind == CrimeEnum::CrimeEnslaving {
            return;
        }
        let now = self.now_hours();
        self.folk[id].disguise_blown = crate::disguise::BLOWN_SECONDS;
        self.folk[id].disguised = None;
        let p = &mut self.folk[id];
        let b = match p.bounties.iter_mut().position(|b| b.faction == faction) {
            Some(k) => &mut p.bounties[k],
            None => {
                p.bounties.push(Bounty { faction: faction.to_string(), amount: 0, crimes: 0, stamp: now });
                p.bounties.last_mut().unwrap()
            }
        };
        b.amount += bounty_for(kind);
        b.crimes |= 1 << (kind as i32);
        b.stamp = now;
        let name = self.data.get(faction).map_or(faction.to_string(), |r| r.name.clone());
        self.messages.push(format!("{} has been caught {} - bounty with {name}: c.{}", self.display_name(id), crime_words(kind), b_amount(&self.folk[id], faction)));
    }

    /// @fn F046512
    ///
    /// The bounty on `id` that `observer` sees (CharacterCrimes::bountyWith): its bounty with
    /// the observer's faction, only while its disguise is blown (the state object +0x110:
    /// caught or seen at a crime lately) or while it is down and the observer takes it for an
    /// enemy (F044572).
    pub fn bounty_seen_by(&self, id: usize, observer: usize) -> i32 {
        let Some(f) = self.faction_id(observer) else { return 0 };
        let amount = self.bounty_with(id, &f);
        if amount == 0 {
            return 0;
        }
        if self.folk[id].disguise_blown <= 0.0 && !(self.folk[id].down && self.takes_for_enemy(observer, id, true)) {
            return 0;
        }
        amount
    }

    /// Person `id`'s bounty with `faction` (0: none).
    /// The faction with the biggest bounty on `id`, if any.
    pub fn top_bounty(&self, id: usize) -> Option<String> {
        self.folk.get(id)?.bounties.iter().filter(|b| b.amount > 0).max_by_key(|b| b.amount).map(|b| b.faction.clone())
    }

    pub fn bounty_with(&self, id: usize, faction: &str) -> i32 {
        self.folk.get(id).and_then(|p| p.bounties.iter().find(|b| b.faction == faction)).map_or(0, |b| b.amount)
    }

    /// Whether `seer` sees `seen` now (the sensor's test, F046539, with `seen`'s sneaking and
    /// stealth).
    pub fn can_see(&self, seer: usize, seen: usize) -> bool {
        self.visibility(seer, seen) > kenshi_ai::perception::SEEN_THRESHOLD || self.heard(seer, seen)
    }

    /// F046498: how visible `seen` is to `seer` now, 0 .. 1 (0: not seen).
    pub fn visibility(&self, seer: usize, seen: usize) -> f32 {
        let (p, q) = (&self.folk[seer], &self.folk[seen]);
        let hour = ((self.start_hour + self.hours_gone()).rem_euclid(24.0)) as f32;
        let dl = daylight(hour, self.sunrise, self.sunset);
        let observer = Observer {
            position: p.mover.position_units(),
            facing: if p.facing == [0.0; 3] { [0.0, 0.0, 1.0] } else { p.facing },
            vision_mult: self.vision_mult,
            in_combat: self.arena.f[seer].active,
            indoors: p.inside.is_some(),
            flag_1e0: false,
            out: false,
            light: dl,
            has_78: false,
        };
        let target = Seen {
            position: q.mover.position_units(),
            stance: if q.down { Stance::Prone } else { Stance::Standing },
            sneaking: q.sneaking,
            moving: q.mover.velocity.iter().map(|v| v * v).sum::<f32>() > 1e-4,
            stealth: (self.arena.f[seen].level(StatsEnumerated::StatStealth) * 0.01).clamp(0.0, 1.0),
            light: dl,
        };
        kenshi_ai::perception::visibility(&observer, &target, dl)
    }

    /// @fn F046538
    ///
    /// Whether `seer` notices `seen` by its noise: a sneaking or fighting character, moving,
    /// is heard within its noise radius on a roll, fresh every 2 s for each pair (the sensor's
    /// per-target timer: game hours x 3600 / 33 x 0.5, about 2 real seconds); the chance is
    /// (1 - distance^2 / radius^2) x 0.05 x (1 - its stealth level) while it sneaks, x 0.75
    /// while it fights, doubled for one already noticed (the last roll's). The radius (F046476)
    /// is lerp(the noise, 1, 0.25 running / 0.75 walking / 1 still), halved for an awake observer
    /// and again inside a building; nobody hears through a building's walls.
    /// (stand-in) The noise (Character +0x448's +0x1a0 and mode +0x198, F039135 / F039134) is
    /// set by the movement-sound update (F039251 -> F037128: a footstep sound's radius from the
    /// ground's sound data), not ported: 400 units, running past the walk speed, walking above
    /// 1 unit/s. A sleeper (state
    /// 1: chance 0.1, woken by it) hears nothing here.
    pub fn heard(&self, seer: usize, seen: usize) -> bool {
        let (p, q) = (&self.folk[seer], &self.folk[seen]);
        if seer == seen || p.down || p.dead || p.bed.is_some() || q.dead || p.away || q.away {
            return false;
        }
        let (a, b) = (p.mover.position_units(), q.mover.position_units());
        let d2 = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
        hear_roll(seer, seen, self.t, self.walk, p.inside, d2, &self.noise_of(seen))
    }

    /// What `hear_roll` reads of person `id`.
    pub fn noise_of(&self, id: usize) -> Noise {
        let q = &self.folk[id];
        Noise {
            sneaking: q.sneaking,
            fighting: self.arena.f[id].active,
            speed: q.speed,
            stealth: (self.arena.f[id].level(StatsEnumerated::StatStealth) * 0.01).clamp(0.0, 1.0),
            inside: q.inside,
        }
    }

    /// F042119's name tag eye (its state +0x24): 0 not sneaking (no eye); 1 hidden (no one
    /// sees them, NameTagStealthHidden); 2 almost seen (someone would see them but for their
    /// sneaking, NameTagStealthAlmostSeen); 3 seen (NameTagStealthSeen).
    /// (assumption) Who sets the state is not decompiled: the watchers are the conscious people
    /// not of the player's squad within 1000 units, each tested with `can_see`; "almost" is
    /// their seeing the character if it stood up.
    pub fn stealth_state(&self, id: usize) -> u8 {
        let q = &self.folk[id];
        if !q.sneaking || q.dead || q.down {
            return 0;
        }
        let at = q.mover.position_units();
        let mut almost = false;
        for w in 0..self.folk.len() {
            let p = &self.folk[w];
            if w == id || p.player || p.dead || p.down || p.away {
                continue;
            }
            let r = p.mover.position_units();
            if (r[0] - at[0]).hypot(r[2] - at[2]) > 1000.0 {
                continue;
            }
            if self.can_see(w, id) {
                return 3;
            }
            if !almost && self.can_see_standing(w, id) {
                almost = true;
            }
        }
        if almost { 2 } else { 1 }
    }

    /// `can_see`, the seen standing and not sneaking.
    fn can_see_standing(&self, seer: usize, seen: usize) -> bool {
        let (p, q) = (&self.folk[seer], &self.folk[seen]);
        let hour = ((self.start_hour + self.hours_gone()).rem_euclid(24.0)) as f32;
        let dl = daylight(hour, self.sunrise, self.sunset);
        let observer = Observer {
            position: p.mover.position_units(),
            facing: if p.facing == [0.0; 3] { [0.0, 0.0, 1.0] } else { p.facing },
            vision_mult: self.vision_mult,
            in_combat: self.arena.f[seer].active,
            indoors: p.inside.is_some(),
            flag_1e0: false,
            out: false,
            light: dl,
            has_78: false,
        };
        let target = Seen {
            position: q.mover.position_units(),
            stance: Stance::Standing,
            sneaking: false,
            moving: q.mover.velocity.iter().map(|v| v * v).sum::<f32>() > 1e-4,
            stealth: (self.arena.f[seen].level(StatsEnumerated::StatStealth) * 0.01).clamp(0.0, 1.0),
            light: dl,
        };
        sees(&observer, &target, dl)
    }

    /// The light a character stands in (0 dark .. 1 full day; the name tag's sun, F042054).
    /// (stand-in) the daylight of the hour: lamps and shade are not counted.
    pub fn light_on(&self, _id: usize) -> f32 {
        let hour = ((self.start_hour + self.hours_gone()).rem_euclid(24.0)) as f32;
        daylight(hour, self.sunrise, self.sunset).clamp(0.0, 1.0)
    }

    /// Whether person `id` is a guard of their faction: armed, not a shopkeeper, not the
    /// player's.
    pub(crate) fn is_guard(&self, id: usize) -> bool {
        let p = &self.folk[id];
        !p.player && !p.down && !p.dead && p.faction.is_some() && p.shop.is_none() && p.inventory.equipped().any(|i| i.kind == Some(ItemType::Weapon))
    }

    /// The faction owning cage furniture `k`: the town round it.
    pub(crate) fn cage_faction(&self, k: usize) -> Option<String> {
        let at = self.furn[k].position;
        self.towns.iter().find(|t| t.contains(at, 1.0)).and_then(|t| t.faction.clone())
    }

    /// The nearest free cage to `at` of `faction` (any cage when `any`).
    pub fn free_cage(&self, faction: &str, at: [f32; 3], any: bool) -> Option<usize> {
        let taken: Vec<usize> = self.folk.iter().filter_map(|p| p.prison.as_ref().map(|q| q.cage)).collect();
        (0..self.furn.len())
            .filter(|&k| self.furn[k].function == kenshi_world::furniture::BF_CAGE && !taken.contains(&k))
            .filter(|&k| any || self.cage_faction(k).as_deref() == Some(faction))
            .min_by(|&a, &b| {
                let d = |k: usize| (self.furn[k].position[0] - at[0]).hypot(self.furn[k].position[2] - at[2]);
                d(a).total_cmp(&d(b))
            })
    }

    /// Put person `id` in cage `cage` for `faction`: for their sentence, or as a slave
    /// (shackled, no sentence). In a cage it is no escaped prisoner (F030719).
    pub fn imprison(&mut self, id: usize, cage: usize, faction: &str, slave: bool) {
        let rec = self.data.get(faction);
        let level = rec.and_then(|r| r.ints.get("cages lock level").copied()).unwrap_or(30) as f32;
        let random = rec.and_then(|r| r.ints.get("lock level random").copied()).unwrap_or(0) as f32;
        let lock = level + (self.rng.uniform() * 2.0 - 1.0) * random;
        let until = if slave { f64::MAX } else { self.now_hours() + sentence_hours(self.bounty_with(id, faction)) };
        let at = self.furn[cage].position;
        let hours = until - self.hours_gone();
        let who = self.display_name(id);
        let p = &mut self.folk[id];
        p.carried_by = None;
        p.mover.position = at.map(|v| v * 0.1);
        self.arena.f[id].mover.position = p.mover.position;
        p.ai.orders.clear();
        p.ai.queue.clear();
        p.shackled = slave;
        p.crime = None;
        p.surrendered = false;
        p.picking = false;
        p.escaped = None;
        p.prison = Some(Prison { cage, faction: faction.to_string(), until, slave, lock: Lock { level: lock, progress: 0.0, next: 3.0 }, out: false });
        // F030719: one with an owner caged in a town that is no prison (TOWN_PRISON 7) is a
        // slave again, of the cage's town
        let town = self.towns.iter().position(|t| t.contains(at, 1.0));
        if self.folk[id].slave.is_some() && town.is_some_and(|t| self.towns[t].kind != 7) {
            if let Some(s) = self.folk[id].slave.as_mut() {
                s.faction = faction.to_string();
            }
            self.set_slave_state(id, crate::slavery::IS_SLAVE);
        }
        let name = self.data.get(faction).map_or(faction.to_string(), |r| r.name.clone());
        self.messages.push(if slave {
            format!("{} has been enslaved by {name}.", who)
        } else {
            format!("{} has been imprisoned by {name} for {:.0} hours.", who, hours)
        });
    }

    /// Person `id` picks their way out: the shackles first, then the cage ("Lockpicking
    /// Shackles", "Escape cage").
    pub fn order_escape(&mut self, id: usize) {
        if let Some(p) = self.folk.get_mut(id) {
            p.picking = p.prison.is_some();
        }
    }

    /// "Free Prisoner": `picker` goes to `prisoner`'s cage and picks its lock from outside.
    pub fn order_free(&mut self, picker: usize, prisoner: usize) {
        if self.folk.get(prisoner).is_some_and(|p| p.prison.is_some()) {
            self.folk[picker].freeing = Some((prisoner, Lock { level: 0.0, progress: 0.0, next: 3.0 }));
        }
    }

    /// Where a cage lets out: (assumption) beside it on its front (its local +z), past half its
    /// size (`cage_reach`, else 15 units), onto the navmesh.
    pub fn cage_exit(&self, cage: usize) -> [f32; 3] {
        let Some(f) = self.furn.get(cage) else { return [0.0; 3] };
        let reach = self.cage_reach.get(&cage).copied().unwrap_or(15.0) + 8.0;
        let d = kenshi_world::nodes::rotate(f.rotation, [0.0, 0.0, 1.0]);
        let to = [f.position[0] + d[0] * reach, f.position[1], f.position[2] + d[2] * reach];
        self.nav
            .closest_point(to.map(|v| v * kenshi_world::navmesh::METRES), 5.0)
            .map_or(to, |(_, q)| q.map(|v| v / kenshi_world::navmesh::METRES))
    }

    /// Person `id` steps out of cage `cage` (its door, `cage_exit`): the cage's walls cut the
    /// navmesh, so it starts its way outside them.
    pub fn leave_cage(&mut self, id: usize, cage: usize) {
        let at = self.cage_exit(cage).map(|v| v * kenshi_world::navmesh::METRES);
        self.folk[id].mover.position = at;
        self.arena.f[id].mover.position = at;
    }

    /// Cages are solid: anyone inside one's reach (`cage_reach`) who is not held in it is pushed
    /// out to its edge. (The navmesh cut round cages keeps paths outside; indoors a building's own
    /// floor is not cut, so this keeps the bars solid there.)
    pub fn cage_push(&mut self) {
        let cages: Vec<(usize, [f32; 3], f32)> = self
            .furn
            .iter()
            .enumerate()
            .filter(|(_, f)| f.function == kenshi_world::furniture::BF_CAGE)
            .map(|(k, f)| (k, f.position, self.cage_reach.get(&k).copied().unwrap_or(12.0)))
            .collect();
        for id in 0..self.folk.len() {
            let held = self.folk[id].prison.as_ref().filter(|pr| !pr.out).map(|pr| pr.cage);
            if self.folk[id].carried_by.is_some() || self.folk[id].dead {
                continue;
            }
            let at = self.folk[id].mover.position_units();
            for &(k, c, r) in &cages {
                if held == Some(k) {
                    continue;
                }
                let (dx, dz) = (at[0] - c[0], at[2] - c[2]);
                let d = dx.hypot(dz);
                if d < r && (at[1] - c[1]).abs() < 25.0 {
                    let (nx, nz) = if d > 0.01 { (dx / d, dz / d) } else { (1.0, 0.0) };
                    let to = [c[0] + nx * r, at[1], c[2] + nz * r].map(|v| v * kenshi_world::navmesh::METRES);
                    self.folk[id].mover.position = to;
                    self.arena.f[id].mover.position = to;
                    break;
                }
            }
        }
    }

    /// A lock being picked by `id` (a prisoner at its own, a freer at a cage): the chance of each
    /// attempt and how far the next attempt is (0 .. 1), for "Lockpicking (Chance {1}%)".
    pub fn pick_status(&self, id: usize) -> Option<(f32, f32)> {
        let p = &self.folk[id];
        let skill = self.arena.f[id].level(StatsEnumerated::StatLockpicking);
        if let Some((c, lock)) = &p.freeing {
            let level = self.folk[*c].prison.as_ref()?.lock.level;
            return Some((lockpick_chance(level, skill), lock.progress / lock.next.max(0.1)));
        }
        let pr = p.prison.as_ref().filter(|_| p.picking)?;
        let level = if p.shackled { SHACKLES_LOCK } else { pr.lock.level };
        Some((lockpick_chance(level, skill), pr.lock.progress / pr.lock.next.max(0.1)))
    }

    /// "Pay Bounty": person `id` pays off their bounty with `faction` from the player's money.
    pub fn pay_bounty(&mut self, id: usize, faction: &str) -> bool {
        let amount = self.bounty_with(id, faction);
        if amount == 0 || self.money < amount {
            return false;
        }
        self.money -= amount;
        self.folk[id].bounties.retain(|b| b.faction != faction);
        true
    }

    /// Stealth knockout (Task_StealthKO): person `id` creeps up on `target` and tries it.
    pub fn order_stealth_ko(&mut self, id: usize, target: usize) {
        let at = self.folk[target].mover.position_units();
        self.order_move(&[id], at);
        self.folk[id].ko_target = Some(target);
    }

    /// The target's best guard against a stealth knockout (F047050).
    pub fn ko_guard(&self, target: usize) -> f32 {
        let f = &self.arena.f[target];
        let l = |s| f.level(s);
        let stealth = (l(StatsEnumerated::StatStealth) * 0.01).clamp(0.0, 1.0) * 100.0 * 0.5;
        [
            l(StatsEnumerated::StatAssassination),
            l(StatsEnumerated::StatThieving) * 0.5,
            stealth,
            l(StatsEnumerated::StatMeleeDefence) * 0.5,
            l(StatsEnumerated::StatToughness),
        ]
        .into_iter()
        .fold(0.0, f32::max)
    }

    /// Whether faction `a` takes faction `b` for an ally (F041495) or an enemy (F041496).
    fn allied(&self, a: &str, b: &str) -> bool {
        self.relations.get(a).is_some_and(|r| r.is_ally(Some(b)))
    }

    fn at_war(&self, a: &str, b: &str) -> bool {
        self.relations.get(a).is_some_and(|r| r.is_enemy(Some(b)))
    }

    /// F038591: whether `me` owes `other` (long-term tags LT_MY_LIFESAVER, LT_FREED_ME).
    fn owes(&self, me: usize, other: usize) -> bool {
        self.dialog_memory.tags.get(&(me, other)).is_some_and(|t| t & 0xc != 0)
    }

    /// Once a second each, the people about who see someone committing a crime or carrying
    /// someone (the sensor's witness call, F046520, for each one it senses).
    /// (assumption) The sensor's reach: 1500 units, the witness seeing as `can_see` does.
    fn witness_tick(&mut self) {
        let tick = self.tick;
        let suspects: Vec<usize> = (0..self.folk.len())
            .filter(|&i| tick % 60 == (i as u64 * 7) % 60)
            .filter(|&i| !self.folk[i].dead && (self.folk[i].crime.is_some() || self.folk[i].carrying.is_some()))
            .collect();
        for c in suspects {
            let at = self.folk[c].mover.position_units();
            let seen_by: Vec<usize> = (0..self.folk.len())
                .filter(|&w| {
                    let p = &self.folk[w];
                    let q = p.mover.position_units();
                    w != c && !p.player && !p.dead && !p.away && p.race.is_none() && (q[0] - at[0]).hypot(q[2] - at[2]) < 1500.0
                })
                .filter(|&w| self.can_see(w, c))
                .collect();
            for w in seen_by {
                self.witness(w, c);
            }
        }
    }

    /// @fn F046520
    ///
    /// `w` sees `c` (Character::witness). Nothing comes of it when they are of one faction, `c`
    /// is carried or held in a cage, `w` is held, a slave or escaping one, an escaped prisoner,
    /// a criminal itself or down. A crime seen is weighed by its faction: not one `w` is
    /// enslaved by or at war with, not "not real", not of "fundamental type" 8 when `w`'s
    /// faction "offers bounties"; from an ally only one against `w`'s own faction. By the crime
    /// (the event `w` raises):
    /// - enslaving: seen by the player's or an "anti slavery" faction, a friend of the victim
    ///   (EV_KIDNAPPING_MY_ALLY);
    /// - lockpicking, not by a friend of the criminal (EV_WITNESS_THIEF_OR_LOCKPICK): a slave
    ///   picks locks seen by its owners or their allies: EV_ESCAPING_SLAVE_SPOTTED and it is
    ///   escaping; slaves are not minded by the player's or "anti slavery" factions;
    /// - stealing, fencing, looting (not by a friend): seen by a friend of the victim
    ///   (EV_WITNESS_LOOTING_ALLY; EV_THIEF_CAUGHT_STEALING_FROM_ME from the victim's squad);
    /// - assault (on a slaver, not minded by "anti slavery" factions while its squad holds the
    ///   criminal's faction), assault on a VIP: not when `w` takes the victim for an enemy
    ///   (EV_WITNESS_GENERIC_ASSAULT);
    /// - freeing slaves: not by the player's or an "anti slavery" faction
    ///   (EV_WITNESS_THIEF_OR_LOCKPICK);
    /// - trespassing (not by a friend): EV_INTRUDER_FOUND, the victim's squad tagging the
    ///   intruder ST_INTRUDER;
    /// - eating crops (a friend of the victim, not of the criminal): EV_EATING_MY_CROPS,
    ///   ST_TEMPORARY_ENEMY, only while it eats (task 231);
    /// - kidnapping: a friend of the victim (EV_KIDNAPPING_MY_ALLY);
    /// - a uniform imposter: a friend of the victim, not of the criminal, with words for it
    ///   (EV_I_SEE_UNIFORM_IMPOSTER);
    /// - murder and the rest: EV_WITNESS_GENERIC_ASSAULT.
    ///
    /// A friend of both: the criminal is a temporary enemy. Then `w`'s squad tags the criminal
    /// ST_CRIMINAL, the crime lasts 20 s more and its disguise is blown (F046479), `w` raises
    /// the event and, a town's guard of a faction that "offers bounties", catches the crime
    /// (F046511: the bounty with its faction).
    /// (not ported) What the player's own people make of a crime against the player (F039340),
    /// the relation lost for a friend's crime, and the victim's own reaction (F039209).
    fn witness(&mut self, w: usize, c: usize) {
        use CrimeEnum::*;
        use kenshi_ai::PerceptionShortTerm::*;
        let (Some(wf), cf) = (self.faction_id(w), self.faction_id(c)) else { return };
        if Some(&wf) == cf.as_ref() {
            return;
        }
        let (pw, pc) = (&self.folk[w], &self.folk[c]);
        if pc.carried_by.is_some() || pc.prison.as_ref().is_some_and(|pr| !pr.out) || pw.prison.as_ref().is_some_and(|pr| !pr.out) {
            return;
        }
        if matches!(self.slave_state(w), 1 | 2) || pw.escaped.is_some() || pw.crime.is_some() || pw.down {
            return;
        }
        let ally = cf.as_deref().is_some_and(|f| self.allied(&wf, f));
        if !ally {
            self.witness_carrying(w, c);
        }
        let Some(crime) = self.folk[c].crime.clone() else { return };
        if crime.kind == CrimeNone || self.guards_town(c) {
            return;
        }
        let f = crime.faction.as_str();
        if !f.is_empty() {
            if self.folk[w].slave.as_ref().is_some_and(|s| s.faction == f) || self.at_war(&wf, f) {
                return;
            }
            if (self.offers_bounties(&wf) && self.fundamental_type(f) == 8) || self.faction_not_real(f) || (ally && f != wf) {
                return;
            }
        }
        let friend = self.takes_for_friend(w, c, true);
        let victim = crime.victim.filter(|&v| v < self.folk.len());
        let mut victim_friend = match victim {
            Some(v) => self.takes_for_friend(w, v, true),
            None => !f.is_empty() && self.allied(&wf, f),
        };
        if friend {
            if f != wf || self.owes(w, c) {
                return;
            }
            victim_friend = true;
        }
        let same_squad = |sim: &Sim, v: usize| sim.folk[v].platoon.is_some() && sim.folk[v].platoon == sim.folk[w].platoon;
        let mut tail = false;
        let event = match crime.kind {
            CrimeEnslaving | CrimeKidnapping => {
                if crime.kind == CrimeKidnapping && victim.is_none() {
                    return;
                }
                if crime.kind == CrimeEnslaving && !self.anti_slavery(&wf) {
                    return;
                }
                if !victim_friend {
                    return;
                }
                EV_KIDNAPPING_MY_ALLY
            }
            CrimeLockpicking => {
                if friend {
                    return;
                }
                let state = self.slave_state(c);
                if state != 0 && self.anti_slavery(&wf) {
                    return;
                }
                if state == crate::slavery::IS_SLAVE {
                    let owners = self.folk[c].slave.as_ref().map(|s| s.faction.clone());
                    if owners.as_deref().is_none_or(|o| self.allied(&wf, o)) {
                        if !self.folk[c].down {
                            self.bark(w, c, crate::disguise::EV_ESCAPING_SLAVE_SPOTTED);
                        }
                        self.set_slave_state(c, crate::slavery::ESCAPING_SLAVE);
                        return;
                    }
                }
                tail = true;
                crate::speech::EV_WITNESS_THIEF_OR_LOCKPICK
            }
            CrimeStealing | CrimeFencing | CrimeLooting => {
                if crime.kind == CrimeLooting && friend {
                    return;
                }
                if !victim_friend {
                    return;
                }
                if let Some(v) = victim
                    && same_squad(self, v)
                {
                    self.tag_short(w, c, StTemporaryEnemy);
                    EV_THIEF_CAUGHT_STEALING_FROM_ME
                } else {
                    crate::speech::EV_WITNESS_LOOTING_ALLY
                }
            }
            CrimeAssault | CrimeAssaultVip => {
                let Some(v) = victim else { return };
                if crime.kind == CrimeAssault && !victim_friend && self.npc_class(v) == 7 && self.anti_slavery(&wf) {
                    let squad = self.folk[v].platoon;
                    if squad.is_some() && self.folk.iter().any(|q| q.platoon == squad && cf.is_some() && q.faction == cf) {
                        return;
                    }
                }
                if self.takes_for_enemy(w, v, true) {
                    return;
                }
                crate::speech::EV_WITNESS_GENERIC_ASSAULT
            }
            CrimeSlaveFreeing => {
                if self.anti_slavery(&wf) {
                    return;
                }
                crate::speech::EV_WITNESS_THIEF_OR_LOCKPICK
            }
            CrimeTresspassing => {
                if friend {
                    return;
                }
                if let Some(v) = victim
                    && same_squad(self, v)
                {
                    self.tag_short(w, c, StIntruder);
                }
                tail = true;
                crate::intruders::EV_INTRUDER_FOUND
            }
            CrimeFarmEating => {
                if !victim_friend || friend {
                    return;
                }
                self.tag_short(w, c, StTemporaryEnemy);
                if self.folk[c].ai.current.as_ref().is_none_or(|t| t.task != 231) {
                    return;
                }
                tail = true;
                EV_EATING_MY_CROPS
            }
            CrimeUniformTheft => {
                if !victim_friend || friend || !self.has_event_dialogue(w, crate::disguise::EV_I_SEE_UNIFORM_IMPOSTER) {
                    return;
                }
                tail = true;
                crate::disguise::EV_I_SEE_UNIFORM_IMPOSTER
            }
            _ => crate::speech::EV_WITNESS_GENERIC_ASSAULT,
        };
        if !tail && friend && victim_friend {
            self.tag_short(w, c, StTemporaryEnemy);
        }
        self.tag_short(w, c, StCriminal);
        self.crime_seen(c);
        if !self.folk[c].down {
            self.bark(w, c, event);
        }
        if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
            eprintln!("law: {w} saw {c} commit {:?} against {} (event {event})", crime.kind, crime.faction);
        }
        if self.guards_town(w) && self.offers_bounties(&wf) {
            self.add_bounty(c, &wf, crime.kind);
        }
    }

    /// @fn F046519
    ///
    /// `w` sees `c` carrying someone. A slave (or escaping one) carried: when it is `w`'s
    /// faction's (Character::isPrisonerOf, F030709) `c` frees slaves (20 s, against `w`'s
    /// faction); else, `w` of the player's or an "anti slavery" faction, a friend of the slave
    /// and not in a town of a faction it is no ally of, `c` kidnaps it (20 s, against the
    /// slave's faction). One carried who is no slave, `w` not its enemy: unless `w` is its
    /// friend, only in a town, of a real faction, with no bounty with `w`'s faction; then
    /// kidnapping it (F046495, its time 20 s, `c`'s disguise blown).
    /// (assumption) One carried as a lawful prisoner (F044460: no bounty, not a bandit, its
    /// state +0xe9) is not told apart here: anyone carried who is no slave counts.
    fn witness_carrying(&mut self, w: usize, c: usize) {
        let Some(k) = self.folk[c].carrying else { return };
        if self.folk[k].dead {
            return;
        }
        let Some(wf) = self.faction_id(w) else { return };
        let kf = self.faction_id(k);
        if matches!(self.slave_state(k), 1 | 2) {
            let owned = self.folk[k].slave.as_ref().is_some_and(|s| s.faction == wf);
            if owned {
                self.commit_crime_for(c, CrimeEnum::CrimeSlaveFreeing, Some(&wf), Some(w), CRIME_SECONDS);
                return;
            }
            if !self.anti_slavery(&wf) || !self.takes_for_friend(w, k, true) {
                return;
            }
            if let Some(tf) = self.folk[w].town.and_then(|t| self.towns.get(t)).and_then(|t| t.faction.clone())
                && !self.allied(&wf, &tf)
            {
                return;
            }
            self.commit_crime_for(c, CrimeEnum::CrimeKidnapping, kf.as_deref(), Some(k), CRIME_SECONDS);
            return;
        }
        if self.takes_for_enemy(w, k, true) {
            return;
        }
        if !self.takes_for_friend(w, k, true) {
            if self.folk[c].town.is_none() || kf.as_deref().is_some_and(|f| self.faction_not_real(f)) || 0 < self.bounty_with(k, &wf) {
                return;
            }
        }
        if self.folk[c].down || self.folk[c].race.is_some() || kf.as_deref().is_some_and(|f| self.faction_not_real(f)) {
            return;
        }
        self.folk[c].disguise_blown = crate::disguise::BLOWN_SECONDS;
        self.folk[c].disguised = None;
        if self.commit_crime_on(c, CrimeEnum::CrimeKidnapping, kf.as_deref(), Some(k))
            && let Some(cr) = self.folk[c].crime.as_mut()
        {
            cr.left = CRIME_SECONDS;
        }
    }

    /// @fn F040724
    ///
    /// The melee hits landed this step (defendOutcome's end): a fight with a town's guard
    /// (F038593) is the other side's assault on the guard's faction, 30 s (F046496), caught at
    /// once by that faction (F046511): the attacker's when the guard is hit, the one hit's when
    /// a guard hits it; between others no crime.
    fn fight_crimes(&mut self) {
        let range = self.law_state.melee_hits.clone();
        self.law_state.melee_hits = 0..0;
        let hits: Vec<(usize, usize)> = self.arena.sh.hits.get(range).map(|h| h.iter().map(|x| (x.0, x.1)).collect()).unwrap_or_default();
        for (attacker, hit) in hits {
            if attacker >= self.folk.len() || hit >= self.folk.len() || attacker == hit {
                continue;
            }
            if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                eprintln!("law: {attacker} hits {hit} (guards {} / {})", self.guards_town(attacker), self.guards_town(hit));
            }
            let (criminal, guard) = if self.guards_town(hit) {
                (attacker, hit)
            } else if self.guards_town(attacker) {
                (hit, attacker)
            } else {
                continue;
            };
            let Some(f) = self.faction_id(guard) else { continue };
            self.commit_crime_for(criminal, CrimeEnum::CrimeAssault, Some(&f), Some(guard), 30.0);
            if let Some(kind) = self.folk[criminal].crime.as_ref().map(|c| c.kind) {
                self.add_bounty(criminal, &f, kind);
            }
        }
    }

    /// @fn F031336
    ///
    /// Prisoner `id`'s sentence served (Task_ReleasePrisoner): out of its cage, released (the
    /// state object +0xea) with its bounty with the cage's faction cleared; one of that faction
    /// near (the releaser) tells it it is free to go (EV_PRISONER_FREE_TO_GO); for an hour it is
    /// taken for no trespasser or intruder (+0x138 / +0x140: now + 1.0000002 hours).
    /// (assumption) The releaser is the nearest of the cage's faction within 1500 units, who
    /// does not walk to the cage; with none the prisoner just leaves.
    pub fn release_prisoner(&mut self, id: usize) {
        let Some(pr) = self.folk[id].prison.take() else { return };
        self.leave_cage(id, pr.cage);
        self.folk[id].bounties.retain(|b| b.faction != pr.faction);
        self.folk[id].escaped = None;
        let now = self.now_hours();
        self.law_state.released.insert(id, now + 1.000_000_163_9);
        let at = self.folk[id].mover.position_units();
        let releaser = self
            .folk
            .iter()
            .filter(|q| q.id != id && !q.player && !q.dead && !q.down && q.faction.as_deref() == Some(pr.faction.as_str()))
            .map(|q| {
                let r = q.mover.position_units();
                (q.id, (r[0] - at[0]).hypot(r[2] - at[2]))
            })
            .filter(|x| x.1 < 1500.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0);
        if let Some(r) = releaser {
            self.bark(r, id, EV_PRISONER_FREE_TO_GO);
        }
        self.messages.push(format!("{} has been released from prison.", self.display_name(id)));
    }

    /// Whether `id` was released from prison within the hour (Task_ReleasePrisoner's grace).
    pub fn lately_released(&self, id: usize) -> bool {
        self.law_state.released.get(&id).is_some_and(|&until| self.now_hours() < until)
    }

    /// @fn F031326
    ///
    /// Prisoner `id` gets out of its open cage, escaping (Task_GetOutOfCage's end): out of the
    /// cage (F030719); an escaped prisoner (the state object +0xe8, "escap") when the cage's
    /// faction is real; a slave is escaping (setSlaveState 2); and it commits the crime of
    /// escaping prison against the cage's faction (F046495).
    pub fn escape_cage(&mut self, id: usize) {
        let Some(pr) = self.folk[id].prison.take() else { return };
        self.leave_cage(id, pr.cage);
        let f = pr.faction.clone();
        if !self.faction_not_real(&f) {
            self.folk[id].escaped = Some((f.clone(), self.now_hours()));
        }
        if self.slave_state(id) == crate::slavery::IS_SLAVE {
            self.set_slave_state(id, crate::slavery::ESCAPING_SLAVE);
        }
        self.commit_crime_on(id, CrimeEnum::CrimeEscapePrison, Some(&f), None);
    }

    /// @fn F031291
    ///
    /// Prisoner `id` lifted out of its cage by `lifter` (Task_LiftSomething): one of the
    /// player's lifting from a cage of a real faction not its own frees a slave (F046495
    /// against the cage's faction) and the prisoner, escaping prison, is an escaped prisoner.
    /// It is out of the cage.
    fn lifted_from_cage(&mut self, id: usize, lifter: usize) {
        let Some(pr) = self.folk[id].prison.take() else { return };
        let f = pr.faction.clone();
        if self.folk[lifter].player && self.faction_id(lifter).as_deref() != Some(f.as_str()) && !self.faction_not_real(&f) {
            self.commit_crime_on(lifter, CrimeEnum::CrimeSlaveFreeing, Some(&f), None);
            self.commit_crime_on(id, CrimeEnum::CrimeEscapePrison, Some(&f), None);
            self.folk[id].escaped = Some((f, self.now_hours()));
        }
        if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
            eprintln!("law: {lifter} lifts {id} out of cage {}", pr.cage);
        }
    }

    /// Crime and law each tick (see the module notes).
    pub fn law_tick(&mut self, dt: f32) {
        let now = self.now_hours();
        let tick = self.tick;
        // bounties lapse; heat cools; crimes run out
        for p in self.folk.iter_mut() {
            p.bounties.retain(|b| now - b.stamp < bounty_hours(b.amount));
            p.heat = (p.heat - dt).max(0.0);
            if let Some(c) = p.crime.as_mut() {
                c.left -= dt;
                if c.left <= 0.0 {
                    p.crime = None;
                }
            }
        }
        self.fight_crimes();
        self.locked_up_tick();
        self.witness_tick();
        // guards take on the wanted they see (once a second each): those whose squad's packages
        // hold "Law enforcer" (Attacking trouble makers, 34: score 1) or "Arrest bounties"
        // (Bounty hunting, 204: scored by F038567 only while a cage of their faction is free to
        // put the arrested in, F047589 / F047588, a building of functionality 8, BF_CAGE). The
        // targets as the tasks' finders take them: one not carried, held, an animal or of a
        // faction "not real" with a bounty the guard sees (F046512: while its disguise is blown,
        // or down and an enemy), or that its squad tagged ST_CRIMINAL; a bounty hunter (F050281)
        // also any it takes for an enemy.
        // (not ported) F050280's troublemakers (the state object +0xac, F038606, F050279) and
        // its town rules: the law enforcers' targets here are the bounty hunters' less enemies.
        use crate::sim::{DUTY_ARRESTS, DUTY_ENFORCES};
        let guards: Vec<usize> = (0..self.folk.len())
            .filter(|&g| tick % 60 == (g as u64 * 13) % 60 && self.folk[g].arrest.is_none() && self.folk[g].duties & (DUTY_ARRESTS | DUTY_ENFORCES) != 0 && self.is_guard(g))
            .collect();
        for g in guards {
            let Some(f) = self.folk[g].faction.clone() else { continue };
            let me = self.folk[g].mover.position_units();
            if self.folk[g].duties & DUTY_ENFORCES == 0 && self.free_cage(&f, me, false).is_none() {
                continue;
            }
            let hunter = self.folk[g].duties & DUTY_ARRESTS != 0;
            let wanted = (0..self.folk.len()).find(|&c| {
                let p = &self.folk[c];
                if c == g || p.prison.is_some() || p.dead || p.carried_by.is_some() || p.race.is_some() || self.faction_id(c).is_some_and(|k| self.faction_not_real(&k)) {
                    return false;
                }
                let q = p.mover.position_units();
                if 1500.0 <= (q[0] - me[0]).hypot(q[2] - me[2]) || !(p.down || self.can_see(g, c)) {
                    return false;
                }
                let target = 0 < self.bounty_seen_by(c, g) || self.has_short(g, c, kenshi_ai::PerceptionShortTerm::StCriminal) || (hunter && self.takes_for_enemy(g, c, true));
                target && !self.folk.iter().any(|o| o.arrest.as_ref().is_some_and(|a| a.target == c))
            });
            if let Some(c) = wanted {
                self.folk[g].arrest = Some(Arrest { target: c, slaver: false, to: None });
                if !self.folk[c].down && !self.folk[c].surrendered {
                    self.order_attack(&[g], c);
                }
                if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                    eprintln!("law: guard {g} goes to arrest {c}");
                }
            }
        }
        // slavers carrying someone take them in
        for s in 0..self.folk.len() {
            let p = &self.folk[s];
            // (a won raid's catch is taken home: crate::raid)
            let raiding = p.world_squad.is_some_and(|k| self.world.squads.get(k).is_some_and(|w| w.won.is_some()));
            if p.captures && p.arrest.is_none() && !raiding && let Some(c) = p.carrying {
                self.folk[s].arrest = Some(Arrest { target: c, slaver: true, to: None });
            }
        }
        // arrests under way
        for g in 0..self.folk.len() {
            let Some(a) = self.folk[g].arrest.clone() else { continue };
            let c = a.target;
            if self.folk[g].down || self.folk[g].dead || self.folk[c].dead || self.folk[c].prison.is_some() {
                self.folk[g].arrest = None;
                continue;
            }
            let Some(f) = a.to.clone().or_else(|| self.folk[g].faction.clone()) else { continue };
            if std::env::var("KENSHI_LAW_DEBUG").is_ok() && tick % 600 == 0 {
                let (x, y) = (self.folk[g].mover.position_units(), self.folk[c].mover.position_units());
                eprintln!(
                    "law: guard {g} on {c}: {:.0} apart, target down {}, carrying {:?}, fighting {}",
                    (x[0] - y[0]).hypot(x[2] - y[2]),
                    self.folk[c].down,
                    self.folk[g].carrying,
                    self.arena.f[g].active
                );
            }
            if self.folk[g].carrying == Some(c) {
                let at = self.folk[g].mover.position_units();
                let Some(cage) = self.free_cage(&f, at, a.slaver) else {
                    if std::env::var("KENSHI_LAW_DEBUG").is_ok() && tick % 600 == 0 {
                        eprintln!("law: no free cage of {f}");
                    }
                    continue;
                };
                let cp = self.furn[cage].position;
                if (cp[0] - at[0]).hypot(cp[2] - at[2]) < 40.0 {
                    self.folk[g].carrying = None;
                    // a slaver's catch, or an escaped slave caught by its masters' guards
                    if a.slaver || self.folk[c].slave.as_ref().is_some_and(|s| s.faction == f) {
                        // the slaver's squad owns them (crate::slavery)
                        let owner = self.folk[g].platoon;
                        self.enslave(c, owner, &f);
                        self.remember(c, g, crate::dialogue::LT_MY_CAPTOR);
                    } else {
                        self.imprison(c, cage, &f, false);
                        self.remember(c, g, crate::dialogue::LT_MY_CAPTOR);
                    }
                    self.folk[g].arrest = None;
                } else if tick % 60 == (g as u64 * 3) % 60 {
                    self.order_move(&[g], cp);
                }
            } else if (self.folk[c].down || self.folk[c].surrendered) && self.folk[c].carried_by.is_none() && self.folk[g].pickup.is_none() {
                if self.folk[c].surrendered {
                    // (assumption) one who gives up lies down to be taken
                    self.knock_out(c);
                    self.arena.f[c].medical.knockout_time = 5.0;
                }
                self.order_pickup(&[g], c);
            }
        }
        // prisoners: kept in their cage; lifted out of it (F031291); the sentence served,
        // released; picking their way out (Task_PickLock F031344, Task_PickLock_Shackles F031345)
        for id in 0..self.folk.len() {
            let Some(pr) = self.folk[id].prison.clone() else { continue };
            if self.folk[id].dead {
                self.folk[id].prison = None;
                continue;
            }
            if let Some(lifter) = self.folk[id].carried_by
                && !pr.out
            {
                self.lifted_from_cage(id, lifter);
                continue;
            }
            let cage = self.furn.get(pr.cage).map(|f| f.position);
            if let Some(cp) = cage
                && self.folk[id].carried_by.is_none()
                && !pr.out
            {
                let p = &mut self.folk[id];
                p.mover.position = cp.map(|v| v * 0.1);
                self.arena.f[id].mover.position = p.mover.position;
            }
            if now >= pr.until {
                self.release_prisoner(id);
                continue;
            }
            if !self.folk[id].picking || self.folk[id].down {
                continue;
            }
            // each update a crime: the shackles, freeing a slave (against its owners, F031345);
            // the cage, lockpicking (against the cage's faction, F031344)
            let shackles = self.folk[id].shackled;
            if shackles {
                let owners = self.folk[id].slave.as_ref().map(|s| (s.faction.clone(), s.owner));
                let witness = owners.as_ref().and_then(|(_, o)| *o).and_then(|o| self.folk.iter().position(|q| q.platoon == Some(o) && q.slave.is_none() && !q.dead));
                self.commit_crime_on(id, CrimeEnum::CrimeSlaveFreeing, owners.as_ref().map(|(f, _)| f.as_str()), witness);
            } else {
                let f = pr.faction.clone();
                self.commit_crime_on(id, CrimeEnum::CrimeLockpicking, Some(&f), None);
            }
            // held in the cage, the skill counts 10 less (F031344)
            let skill = self.arena.f[id].level(StatsEnumerated::StatLockpicking);
            let speed = 0.55 + (2.0 - 0.55) * (skill * 0.01).clamp(0.0, 1.0);
            let level = if shackles { self.shackles_lock(id).unwrap_or(SHACKLES_LOCK) } else { pr.lock.level };
            let mut lock = pr.lock.clone();
            lock.progress += speed * dt;
            self.arena.f[id].skills.train(StatsEnumerated::StatLockpicking, dt * 0.05, 1.0);
            if lock.progress >= lock.next {
                lock.progress = 0.0;
                lock.next = 2.0 + self.rng.uniform() * 2.0;
                let roll = self.rng.uniform();
                let held = if pr.out { 0.0 } else { CAGED_PICK_PENALTY };
                let chance = lockpick_chance(level, skill - held);
                if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                    eprintln!("law: {id} picks a lock of {level:.0} at skill {skill:.0}: chance {chance:.2}, roll {roll:.2}");
                }
                self.pick_events.push((id, roll < chance));
                if roll < chance {
                    if shackles {
                        self.remove_shackles(id);
                        self.messages.push(format!("{}: Shackles broken!", self.display_name(id)));
                    } else {
                        self.folk[id].picking = false;
                        self.escape_cage(id);
                        self.messages.push(format!("{} has escaped from the cage.", self.display_name(id)));
                        continue;
                    }
                }
            }
            if let Some(p) = self.folk[id].prison.as_mut() {
                p.lock = lock;
            }
        }
        // those picking a cage's lock from outside (F031344): at its door, the lock's attempts,
        // each a crime of lockpicking against the cage's faction; the one held in it calls out
        // on each (EV_UNLOCK_MY_CAGE_ATTEMPT) and, open, thanks the picker
        // (EV_UNLOCK_MY_CAGE_OR_SHACKLES) and gets out, escaping (Task_GetOutOfCage F031326)
        for id in 0..self.folk.len() {
            let Some((c, mut lock)) = self.folk[id].freeing.clone() else { continue };
            let Some(pr) = self.folk[c].prison.clone() else {
                self.folk[id].freeing = None;
                continue;
            };
            if self.folk[id].down || self.folk[id].dead || self.folk[c].dead {
                self.folk[id].freeing = None;
                continue;
            }
            let door = self.cage_exit(pr.cage);
            let at = self.folk[id].mover.position_units();
            if 20.0 < (door[0] - at[0]).hypot(door[2] - at[2]) {
                if tick % 60 == (id as u64 * 5) % 60 {
                    self.order_move(&[id], door);
                }
                continue;
            }
            let f = pr.faction.clone();
            self.commit_crime_on(id, CrimeEnum::CrimeLockpicking, Some(&f), None);
            let skill = self.arena.f[id].level(StatsEnumerated::StatLockpicking);
            lock.progress += (0.55 + (2.0 - 0.55) * (skill * 0.01).clamp(0.0, 1.0)) * dt;
            self.arena.f[id].skills.train(StatsEnumerated::StatLockpicking, dt * 0.05, 1.0);
            if lock.progress >= lock.next {
                lock.progress = 0.0;
                lock.next = 2.0 + self.rng.uniform() * 2.0;
                let ok = self.rng.uniform() < lockpick_chance(pr.lock.level, skill);
                self.pick_events.push((id, ok));
                let theirs = self.faction_id(id).as_deref() == Some(f.as_str());
                if !theirs {
                    self.bark(c, id, if ok { EV_UNLOCK_MY_CAGE_OR_SHACKLES } else { EV_UNLOCK_MY_CAGE_ATTEMPT });
                }
                if ok {
                    self.folk[id].freeing = None;
                    self.escape_cage(c);
                    self.remember(c, id, crate::dialogue::LT_FREED_ME);
                    self.messages.push(format!("{} has been freed.", self.display_name(c)));
                    continue;
                }
            }
            self.folk[id].freeing = Some((c, lock));
        }
        // F038706: an escaped prisoner's status lasts until it is out of town
        for p in self.folk.iter_mut() {
            if p.escaped.is_some() && p.town.is_none() {
                p.escaped = None;
            }
        }
        // stealth knockouts: at the target's back, the roll
        for id in 0..self.folk.len() {
            let Some(t) = self.folk[id].ko_target else { continue };
            if self.folk[id].down || self.folk[t].down || self.folk[t].dead {
                self.folk[id].ko_target = None;
                continue;
            }
            let (a, b) = (self.folk[id].mover.position_units(), self.folk[t].mover.position_units());
            if (a[0] - b[0]).hypot(a[2] - b[2]) > 20.0 {
                if tick % 30 == 0 {
                    self.order_move(&[id], b);
                    self.folk[id].ko_target = Some(t);
                }
                continue;
            }
            self.folk[id].ko_target = None;
            let assassination = self.arena.f[id].level(StatsEnumerated::StatAssassination);
            let chance = if self.arena.f[t].active { 0.0 } else { stealth_ko_chance(assassination, self.ko_guard(t)) };
            let roll = self.rng.uniform();
            self.arena.f[id].skills.train(StatsEnumerated::StatAssassination, 5.0, 1.0);
            let faction = self.folk[t].faction.clone();
            self.commit_crime(id, CrimeEnum::CrimeAssault, faction.as_deref());
            if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                eprintln!("law: {id} stealth knocks out {t}: chance {chance:.2}, roll {roll:.2}");
            }
            if roll < chance {
                self.knock_out(t);
                self.messages.push(format!("{} knocked {} out.", self.display_name(id), self.display_name(t)));
            } else {
                self.order_attack(&[t], id);
                self.messages.push(format!("{} failed to knock {} out!", self.display_name(id), self.display_name(t)));
            }
        }
    }
}

/// The amount of person `p`'s bounty with `faction`.
fn b_amount(p: &crate::sim::Person, faction: &str) -> i32 {
    p.bounties.iter().find(|b| b.faction == faction).map_or(0, |b| b.amount)
}

/// A crime as the messages say it.
pub fn crime_words(c: CrimeEnum) -> &'static str {
    use CrimeEnum::*;
    match c {
        CrimeEnslaving => "enslaving",
        CrimeLockpicking => "lockpicking",
        CrimeStealing => "stealing",
        CrimeMurder => "murdering",
        CrimeAssault | CrimeAssaultVip => "assaulting",
        CrimeSlaveFreeing => "freeing slaves",
        CrimeSmuggling => "smuggling",
        CrimeTerrorism => "terrorism",
        CrimeLooting => "looting",
        CrimeTresspassing => "trespassing",
        CrimeEscapePrison => "escaping prison",
        CrimeFencing => "fencing",
        CrimeFarmEating => "eating from a farm",
        CrimeKidnapping => "kidnapping",
        CrimeUniformTheft => "wearing a stolen uniform",
        _ => "committing a crime",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounties_and_sentences() {
        assert_eq!(bounty_for(CrimeEnum::CrimeStealing), 2500);
        assert_eq!(bounty_hours(2500), 100.0);
        assert_eq!(bounty_hours(10000), 1e7);
        assert_eq!(sentence_hours(0), 16.0);
        assert_eq!(sentence_hours(2500), 50.0);
        assert_eq!(sentence_hours(30000), 999_999.0);
    }

    #[test]
    fn locks_and_knockouts() {
        assert_eq!(lockpick_chance(20.0, 30.0), 0.9);
        assert!((lockpick_chance(40.0, 30.0) - 0.45).abs() < 1e-6);
        assert_eq!(stealth_ko_chance(30.0, 30.0), 0.5);
        assert_eq!(stealth_ko_chance(60.0, 30.0), 1.0);
        assert_eq!(stealth_ko_chance(15.0, 30.0), 0.0);
    }

    #[test]
    fn noise() {
        let n = |speed: f32, stealth: f32| Noise { sneaking: true, fighting: false, speed, stealth, inside: None };
        let heard = |n: &Noise, d: f32| (0..2000).filter(|&w| hear_roll(1, 2, w as f32 * 2.0, 15.0, None, d * d, n)).count();
        // standing still: no noise; walking: within 50 units; running: within 150
        assert_eq!(heard(&n(0.0, 0.0), 10.0), 0);
        assert!(heard(&n(10.0, 0.0), 10.0) > 0);
        assert_eq!(heard(&n(10.0, 0.0), 60.0), 0);
        assert!(heard(&n(30.0, 0.0), 100.0) > 0);
        // the stealthier, the less
        assert!(heard(&n(30.0, 0.8), 20.0) < heard(&n(30.0, 0.0), 20.0));
        assert_eq!(heard(&n(30.0, 1.0), 20.0), 0);
        // not sneaking or fighting: not by noise
        assert_eq!(heard(&Noise { sneaking: false, ..n(30.0, 0.0) }, 20.0), 0);
    }
}
