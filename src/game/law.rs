//! Crime and law (port_map `law`): crimes seen by witnesses, bounties, guards arresting the
//! wanted, prison cages and sentences, escaping by picking locks, slavers enslaving the knocked
//! out, stealing and stealth knockouts.
//!
//! Specs:
//! - F046520 (Character::witness): a witness of the crime's faction (or an ally of it) seeing the
//!   criminal commit it raises it; F046511 then adds the crime's bounty (F046477) to the
//!   criminal's bounty with that faction (none for factions of "fundamental type" 1, 5 or 8, nor
//!   for enslaving), marks the crime, stamps the time and sets the heat to 250.
//! - F046477 (bounty by crime): enslaving / trespassing 100, assault / looting / farm eating 500,
//!   lockpicking / fencing 1000, stealing / escaping prison / uniform theft 2500, murder / slave
//!   freeing / smuggling / terrorism / kidnapping 5000, assault on a VIP 50000.
//! - F046478: a bounty expires that many x 0.04 hours after it was last raised; 10000 or more
//!   never ("Notorious").
//! - F046518: a prison sentence is 16 hours without a bounty, the bounty x 0.02 x GLOBAL
//!   CONSTANTS "prison time" hours up to 25000, else for life.
//! - F029315 (a lock picked): each attempt (made as the picker's speed fills the lock's attempt
//!   time; speed lerp(lockpicking / 100, 0.55, 2.0), F031344) opens it with chance 0.9, or 0.9 /
//!   2^((lock level - skill) x 0.1) when the lock is better than the skill; indoors the skill
//!   counts 10 less.
//! - F047050 (stealth knockout chance): the attacker's assassination less the target's best
//!   guard (the most of its assassination, half its thievery, half its stealth (F047035), half
//!   its melee defence, its toughness), d: (d + 30) / 60 when d >= 0, (d + 15) / 30 when below;
//!   none on a target that is fighting.
//! - Factions' "cages lock level" and "lock level random" (FACTION records), cages are
//!   furniture of function 8 (BUILDING_FUNCTIONALITY 'Cage').
//!
//! (assumption) A crime lasts 20 seconds of being seen (F046520's notoriety timer 20); a witness
//! must see the criminal as the sensor sees (F046539, the criminal's sneaking and stealth
//! counted); guards are the armed people of the faction who are not shopkeepers; they arrest
//! the wanted they see by beating them down, then carry them to the nearest free cage of their
//! faction's towns (none: they leave them); prisoners keep to their cage until the sentence is
//! served (the bounty with that faction then cleared) or they pick its lock (lock level: the
//! faction's "cages lock level" +- its "lock level random"); a lock's attempt comes every 2 to
//! 4 seconds of pick work; slavers take the knocked out they carry to their faction's nearest
//! cage and shackle them (shackles' lock 40); a shackled slave picks the shackles, then the cage;
//! escaping is the crime of escaping prison; paying a bounty to a guard of the faction clears it.
//! Not ported: the guards' "halt" talk and searches, bounty hunters, trespassing, doors' locks
//! (slaves' work, sale and recognition are crate::slavery's).

use crate::sim::Sim;
use kenshi_ai::CrimeEnum;
use kenshi_ai::perception::{Observer, Stance, Target as Seen, daylight, sees};
use kenshi_char::StatsEnumerated;
use kenshi_data::ItemType;

/// GLOBAL CONSTANTS "prison time" (GlobalConstants +0x16c, shipped 1).
pub const PRISON_TIME: f32 = 1.0;
/// How long a crime stays one (seconds; F046520's timer).
const CRIME_SECONDS: f32 = 20.0;
/// The lock level of shackles (assumption).
const SHACKLES_LOCK: f32 = 40.0;

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

/// A crime being committed: what, and against whom (the faction), and how long it stays one.
#[derive(Debug, Clone, PartialEq)]
pub struct Crime {
    pub kind: CrimeEnum,
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
    /// A person's name as shown: theirs, else their character's, else "Character n".
    pub fn display_name(&self, id: usize) -> String {
        let p = &self.folk[id];
        if !p.name.is_empty() {
            return p.name.clone();
        }
        p.character.as_deref().and_then(|c| self.data.get(c)).map_or(format!("Character {id}"), |r| r.name.clone())
    }

    /// Game hours now.
    fn now_hours(&self) -> f64 {
        self.hours_gone()
    }

    /// Person `id` commits crime `kind` against `faction` (it stays a crime a while; seen by a
    /// witness, a bounty).
    pub fn commit_crime(&mut self, id: usize, kind: CrimeEnum, faction: Option<&str>) {
        let Some(f) = faction else { return };
        if let Some(p) = self.folk.get_mut(id) {
            p.crime = Some(Crime { kind, faction: f.to_string(), left: CRIME_SECONDS });
        }
    }

    /// @fn F046511
    ///
    /// Add crime `kind`'s bounty with `faction` to person `id`.
    pub fn add_bounty(&mut self, id: usize, faction: &str, kind: CrimeEnum) {
        let fundamental = self.data.get(faction).and_then(|r| r.ints.get("fundamental type").copied()).unwrap_or(0);
        if matches!(fundamental, 1 | 5 | 8) || kind == CrimeEnum::CrimeEnslaving {
            return;
        }
        let now = self.now_hours();
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
        p.heat = 250.0;
        let name = self.data.get(faction).map_or(faction.to_string(), |r| r.name.clone());
        self.messages.push(format!("{} has been caught {} - bounty with {name}: c.{}", self.display_name(id), crime_words(kind), b_amount(&self.folk[id], faction)));
    }

    /// Person `id`'s bounty with `faction` (0: none).
    pub fn bounty_with(&self, id: usize, faction: &str) -> i32 {
        self.folk.get(id).and_then(|p| p.bounties.iter().find(|b| b.faction == faction)).map_or(0, |b| b.amount)
    }

    /// Whether `seer` sees `seen` now (the sensor's test, F046539, with `seen`'s sneaking and
    /// stealth).
    pub fn can_see(&self, seer: usize, seen: usize) -> bool {
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
        sees(&observer, &target, dl) || self.heard(seer, seen)
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
    /// (stand-in) The noise (the animation object +0x1a0, its movement mode +0x198) is not
    /// traced: 400 units, running past the walk speed, walking above 1 unit/s. A sleeper (state
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
    pub(crate) fn free_cage(&self, faction: &str, at: [f32; 3], any: bool) -> Option<usize> {
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
    /// (shackled, no sentence).
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
        p.prison = Some(Prison { cage, faction: faction.to_string(), until, slave, lock: Lock { level: lock, progress: 0.0, next: 3.0 }, out: false });
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
        // F046520: witnesses (each criminal tested once a second)
        let criminals: Vec<usize> = (0..self.folk.len()).filter(|&i| self.folk[i].crime.is_some() && tick % 60 == (i as u64 * 7) % 60).collect();
        for c in criminals {
            let Some(crime) = self.folk[c].crime.clone() else { continue };
            let at = self.folk[c].mover.position_units();
            let witness = (0..self.folk.len()).find(|&w| {
                let p = &self.folk[w];
                if w == c || p.player || p.down || p.dead {
                    return false;
                }
                let of_side = p.faction.as_deref() == Some(crime.faction.as_str())
                    || p.faction.as_ref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_ally(Some(&crime.faction)));
                let q = p.mover.position_units();
                of_side && (q[0] - at[0]).hypot(q[2] - at[2]) < 1500.0 && self.can_see(w, c)
            });
            if let Some(w) = witness {
                self.folk[c].crime = None;
                self.add_bounty(c, &crime.faction, crime.kind);
                // the witness calls it out
                let event = match crime.kind {
                    CrimeEnum::CrimeStealing | CrimeEnum::CrimeLockpicking => crate::speech::EV_WITNESS_THIEF_OR_LOCKPICK,
                    CrimeEnum::CrimeLooting => crate::speech::EV_WITNESS_LOOTING_ALLY,
                    _ => crate::speech::EV_WITNESS_GENERIC_ASSAULT,
                };
                self.bark(w, c, event);
                if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                    eprintln!("law: {w} saw {c} commit {:?} against {}", crime.kind, crime.faction);
                }
            }
        }
        self.slave_recognition_tick();
        // guards take on the wanted they see (once a second each)
        let guards: Vec<usize> = (0..self.folk.len()).filter(|&g| tick % 60 == (g as u64 * 13) % 60 && self.folk[g].arrest.is_none() && self.is_guard(g)).collect();
        for g in guards {
            let Some(f) = self.folk[g].faction.clone() else { continue };
            let me = self.folk[g].mover.position_units();
            let wanted = (0..self.folk.len()).find(|&c| {
                let p = &self.folk[c];
                p.prison.is_none() && !p.dead && p.carried_by.is_none() && self.bounty_with(c, &f) > 0 && !self.passes_as(c, &f) && {
                    let q = p.mover.position_units();
                    (q[0] - me[0]).hypot(q[2] - me[2]) < 1500.0
                } && (p.down || self.can_see(g, c))
                    && !self.folk.iter().any(|o| o.arrest.as_ref().is_some_and(|a| a.target == c))
            });
            if let Some(c) = wanted {
                self.folk[g].arrest = Some(Arrest { target: c, slaver: false });
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
            if p.captures && p.arrest.is_none() && let Some(c) = p.carrying {
                self.folk[s].arrest = Some(Arrest { target: c, slaver: true });
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
            let Some(f) = self.folk[g].faction.clone() else { continue };
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
        // prisoners: kept in their cage; the sentence served, released; picking their way out
        for id in 0..self.folk.len() {
            let Some(pr) = self.folk[id].prison.clone() else { continue };
            if self.folk[id].dead {
                self.folk[id].prison = None;
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
                self.leave_cage(id, pr.cage);
                self.folk[id].prison = None;
                self.folk[id].bounties.retain(|b| b.faction != pr.faction);
                self.messages.push(format!("{} has been released from prison.", self.display_name(id)));
                continue;
            }
            if !self.folk[id].picking || self.folk[id].down {
                continue;
            }
            let skill = self.arena.f[id].level(StatsEnumerated::StatLockpicking);
            let speed = 0.55 + (2.0 - 0.55) * (skill * 0.01).clamp(0.0, 1.0);
            let level = if self.folk[id].shackled { self.shackles_lock(id).unwrap_or(SHACKLES_LOCK) } else { pr.lock.level };
            let mut lock = pr.lock.clone();
            lock.progress += speed * dt;
            self.arena.f[id].skills.train(StatsEnumerated::StatLockpicking, dt * 0.05, 1.0);
            if lock.progress >= lock.next {
                lock.progress = 0.0;
                lock.next = 2.0 + self.rng.uniform() * 2.0;
                let roll = self.rng.uniform();
                let chance = lockpick_chance(level, skill);
                if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                    eprintln!("law: {id} picks a lock of {level:.0} at skill {skill:.0}: chance {chance:.2}, roll {roll:.2}");
                }
                self.pick_events.push((id, roll < chance));
                if roll < chance {
                    if self.folk[id].shackled {
                        self.remove_shackles(id);
                        self.messages.push(format!("{}: Shackles broken!", self.display_name(id)));
                    } else {
                        // out: escaping is a crime
                        self.leave_cage(id, pr.cage);
                        self.folk[id].prison = None;
                        self.folk[id].picking = false;
                        self.folk[id].escaped = Some((pr.faction.clone(), now + 24.0));
                        let f = pr.faction.clone();
                        self.commit_crime(id, CrimeEnum::CrimeEscapePrison, Some(&f));
                        self.messages.push(format!("{} has escaped from the cage.", self.display_name(id)));
                        continue;
                    }
                }
            }
            if let Some(p) = self.folk[id].prison.as_mut() {
                p.lock = lock;
            }
        }
        // those picking a cage's lock from outside: at its door, the lock's attempts; open, the
        // prisoner walks free and the picker has freed a prisoner (the crime of freeing slaves)
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
            let skill = self.arena.f[id].level(StatsEnumerated::StatLockpicking);
            lock.progress += (0.55 + (2.0 - 0.55) * (skill * 0.01).clamp(0.0, 1.0)) * dt;
            self.arena.f[id].skills.train(StatsEnumerated::StatLockpicking, dt * 0.05, 1.0);
            if lock.progress >= lock.next {
                lock.progress = 0.0;
                lock.next = 2.0 + self.rng.uniform() * 2.0;
                let ok = self.rng.uniform() < lockpick_chance(pr.lock.level, skill);
                self.pick_events.push((id, ok));
                if ok {
                    self.folk[id].freeing = None;
                    self.leave_cage(c, pr.cage);
                    self.folk[c].prison = None;
                    self.folk[c].shackled = false;
                    if self.folk[id].player {
                        self.commit_crime(id, CrimeEnum::CrimeSlaveFreeing, Some(&pr.faction));
                    }
                    self.remember(c, id, crate::dialogue::LT_FREED_ME);
                    self.messages.push(format!("{} has been freed.", self.display_name(c)));
                    continue;
                }
            }
            self.folk[id].freeing = Some((c, lock));
        }
        // escaped prisoners' status lapses
        for p in self.folk.iter_mut() {
            if p.escaped.as_ref().is_some_and(|e| now >= e.1) {
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
