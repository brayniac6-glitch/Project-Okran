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
    /// The seconds (Task_CloseDoor) or the lock's turn (Task_LockDoor) of each one at a door
    /// task (crate::intruders `door_tasks_tick`).
    pub door_time: std::collections::HashMap<usize, f32>,
    /// Each town's alarms (TownBase vtable +0x2e8, the AlarmManager: by town), when the last
    /// "Alarm raised" was told (sim seconds), who has an alarm in its vicinity now (condition
    /// 130) and the seconds left of each one looking at an alarm (Task_InvestigateAlarms +0x7c).
    pub alarms: std::collections::HashMap<usize, Vec<Alarm>>,
    pub alarm_message_at: Option<f32>,
    pub alarm_near: Vec<bool>,
    pub investigating: std::collections::HashMap<usize, f32>,
    /// What the AI's view reads of the law (crate::sim View `law`) and the seconds each one
    /// has spent at its law task in hand.
    pub view: LawView,
    pub task_time: std::collections::HashMap<usize, f32>,
    /// The weather on each one (WeatherAffecting and strength, F039251: its region's outdoors,
    /// none indoors, for an animal or a race immune to it), for its noise (`Noise::weather`).
    pub weather: Vec<(i32, f32)>,
    /// Kidnappings ordered and not yet begun (kidnapper, victim): Task_Kidnapping::start
    /// (`kidnap_start`) runs as the victim is lifted.
    pub kidnaps: Vec<(usize, usize)>,
    /// Those being kidnapped (the state object's +0xe9, "kidn": set by Task_Kidnapping F031325,
    /// cleared by F046493; `kidnapped_tick`).
    pub kidnapped: std::collections::HashSet<usize>,
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

/// What the sensor reads of a character for its sight and its noise (F046539 / F046538 /
/// F046476): its stealth mode (CharMovement +0x24), whether it is in a fight (F033833), its
/// measured and its own speed (CharMovement +0xb8 / +0xb4, units / s), its stealth level
/// (F047035, 0 .. 1), the building it is in, the weather on it (its noise's +0x198 type and
/// +0x1a0 strength, set by F039251: WeatherAffecting and strength), whether it is the
/// player's (F046535), whether the sight test casts a line to it (`marked`, F046539's list)
/// and whether it is held in a cage (Character +0x2f8).
#[derive(Debug, Clone, Copy, Default)]
pub struct Noise {
    pub sneaking: bool,
    pub fighting: bool,
    pub speed: f32,
    pub own_speed: f32,
    pub stealth: f32,
    pub inside: Option<usize>,
    pub weather: (i32, f32),
    pub player: bool,
    pub marked: bool,
    pub caged: bool,
}

/// WeatherAffecting WA_RAIN (crate::hazards has 1 .. 4).
pub const WA_RAIN: i32 = 5;

/// The buildings' solids a sight line is cast against (`kenshi_combat::arena::Arena::obstacles`,
/// world frame less the origin given).
pub type Obstacles = Option<(std::sync::Arc<kenshi_physics::world::StaticWorld>, [f32; 3])>;

/// F050479.
fn lerp(t: f32, a: f32, b: f32) -> f32 {
    (b - a) * t + a
}

impl Noise {
    /// F039177: sneaking, out of a fight. (Its stance 1 / 2, F040664: none crouch or crawl
    /// here.)
    pub fn sneaks(&self) -> bool {
        self.sneaking && !self.fighting
    }

    /// @fn F046476
    ///
    /// The radius it is heard within (units): none unless in stealth mode or fighting. Its
    /// weather muffles it: m = lerp(the weather's strength, 1, 0.25 in a dust storm, 0.75 in
    /// rain or acid rain, else 1). Sneaking: lerp(its stealth level, 450 m, 80); else
    /// lerp(its speed / its own speed, 350 m, 600 m).
    pub fn radius(&self) -> f32 {
        if !self.sneaking && !self.fighting {
            return 0.0;
        }
        let w = match self.weather.0 {
            crate::hazards::WA_DUST => 0.25,
            WA_RAIN | crate::hazards::WA_ACID => 0.75,
            _ => 1.0,
        };
        let m = lerp(self.weather.1, 1.0, w);
        if self.sneaks() {
            lerp(self.stealth, 450.0 * m, 80.0)
        } else {
            let frac = if self.own_speed > 0.0 { self.speed / self.own_speed } else { 0.0 };
            lerp(frac, 350.0 * m, 600.0 * m)
        }
    }
}

/// What the sensor reads of person `p` (its fighter `f`) under weather `weather`
/// (`LawState::weather`).
/// (assumption) Chained (Character +0x320): a slave; down (F039148's state not 0): knocked
/// out; disguised (state +0x100 above 0): wearing a disguise. (not ported) Character +0x1a9.
pub fn noise(p: &crate::sim::Person, f: &kenshi_combat::arena::Fighter, weather: (i32, f32)) -> Noise {
    let sneaks = p.sneaking && !f.active;
    Noise {
        sneaking: p.sneaking,
        fighting: f.active,
        speed: p.speed,
        own_speed: p.own_speed,
        stealth: stealth_level(p, f),
        inside: p.inside,
        weather,
        player: p.player,
        marked: p.down || sneaks || p.disguised.is_some() || p.escaped.is_some() || p.slave.as_ref().is_some_and(|s| s.state == crate::slavery::IS_SLAVE),
        caged: p.prison.as_ref().is_some_and(|c| !c.out),
    }
}

/// F047035 (with its buffs): the stealth level x 0.75 carrying someone (Character +0x348),
/// x 0.01, within 0 .. 1 (x 2 crawling: none crawl here).
/// (not ported) Its load, medical +0xb4, CharStats +0x3c and activity-0x10 fitness factors.
pub fn stealth_level(p: &crate::sim::Person, f: &kenshi_combat::arena::Fighter) -> f32 {
    (f.level(StatsEnumerated::StatStealth) * if p.carrying.is_some() { 0.75 } else { 1.0 } * 0.01).clamp(0.0, 1.0)
}

/// @fn F046538
///
/// Whether observer `seer` (`o`) hears `seen` (`n`) at squared distance `d2` (units), at sim
/// time `t` (seconds); `open` is F037712 (whether a building is open). Two of the player's
/// hear each other within 1000 units. Else one neither in stealth mode nor fighting is not
/// heard; one in another building only while fighting with one of the two outdoors. The
/// radius (F046476) is halved for an awake observer, and in another building halved again,
/// and again when that building (the target's, else the observer's) is shut. The chance is
/// (1 - d2 / radius^2) x k, k = 0.05 x (1 - its stealth level) while it sneaks, else 0.75;
/// doubled for one noticed on the last roll; one fighting in the same building is heard at
/// once. The roll is fresh every 2 s for each pair (the sensor's per-target timer: game hours
/// x 3600 / 33 x 0.5).
/// (assumption) "Noticed" (the sensor record's +0x3c above 0): heard on the last roll.
/// (not ported) The floors (Character vtable +0x60: another floor only while fighting, at most
/// one apart, not in a building with +0xc2 set): one floor here. A sleeping observer (the
/// whole radius, k 0.1, "almost woke up", EV 47, for a sneaker heard): sleepers hear nothing
/// here. The stealth name tag's report (F00578b).
pub fn hear_roll(seer: usize, seen: usize, t: f32, o: &Noise, d2: f32, n: &Noise, open: impl Fn(usize) -> bool) -> bool {
    if seer == seen {
        return false;
    }
    if o.player && n.player {
        return d2 < 1_000_000.0;
    }
    if !n.sneaking && !n.fighting {
        return false;
    }
    let apart = o.inside != n.inside;
    if apart && (!n.fighting || (o.inside.is_some() && n.inside.is_some())) {
        return false;
    }
    let mut r = n.radius() * 0.5;
    if apart {
        r *= 0.5;
        if let Some(b) = n.inside.or(o.inside)
            && !open(b)
        {
            r *= 0.5;
        }
    }
    let k = if n.sneaks() { 0.05 * (1.0 - n.stealth) } else { 0.75 };
    let near = (d2 - r * r) / (0.0 - r * r);
    let chance = near * k;
    if !(chance > 0.0) {
        return false;
    }
    if n.fighting && !apart {
        return true;
    }
    let window = (t / 2.0).max(0.0) as u64;
    let roll = |w: u64| {
        // splitmix64 of the pair and the window: the same roll for the whole window
        let mut z = (seer as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (seen as u64).rotate_left(29) ^ w.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    };
    let mut chance = chance;
    if window > 0 && roll(window - 1) < chance {
        chance *= 2.0;
    }
    roll(window) < chance
}

/// The eye's height over a character's position (F046499's offset, units); the line ends at
/// half of it on the target, and a hit within the last 2 units of the two's distance does not
/// block it.
const EYE_HEIGHT: f32 = 18.0;
const SIGHT_SLACK: f32 = 2.0;
/// (assumption) A hit this near a caged target (units, across) is its cage.
const CAGE_REACH: f32 = 25.0;

/// @fn F046499
///
/// The line of sight from a character at `from` to one at `at` (positions, world units): a ray
/// from the eye (18 units up) toward the target's middle (9 units up) against the buildings'
/// solids (the physics query, mask 0x87f9e04 outdoors / 0x87fde44 inside); blocked by a hit
/// nearer than the two's distance less 2, unless what it hits is the cage the target is held
/// in (Character +0x2f8 / +0x300). Clear with no solids loaded (headless).
/// (assumption) The cage: a hit within 25 units across of a caged target.
/// (not ported) An observer with its Character +0x258 handle set (+0x260 not NULL_ITEM): none
/// within 40 units seen, the eye along its CharMovement +0xd0 x 40.
pub fn sight_line(obstacles: &Obstacles, from: [f32; 3], at: [f32; 3], caged: bool) -> bool {
    let eye = [from[0], from[1] + EYE_HEIGHT, from[2]];
    let to = [at[0], at[1] + EYE_HEIGHT / SIGHT_SLACK, at[2]];
    let d = [to[0] - eye[0], to[1] - eye[1], to[2] - eye[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let reach = ((at[0] - from[0]).powi(2) + (at[1] - from[1]).powi(2) + (at[2] - from[2]).powi(2)).sqrt() - SIGHT_SLACK;
    if len < 1e-3 || reach <= 0.0 {
        return true;
    }
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    let Some(hit) = kenshi_combat::arena::wall_hit(obstacles, eye, dir, reach) else {
        return true;
    };
    caged && (eye[0] + dir[0] * hit - at[0]).hypot(eye[2] + dir[2] * hit - at[2]) < CAGE_REACH
}

/// F046539's last test: whether the sight test casts the line of sight (F046499) from `o` to
/// `n`: never between two of the player's; else for a target of the player's, an observer of
/// the player's (F046535), or a `marked` target. The others are seen without it.
/// (not ported) Its exception before (F044452 / F028552 of each, unidentified).
pub fn sight_cast(o: &Noise, n: &Noise) -> bool {
    !(o.player && n.player) && (n.player || o.player || n.marked)
}

/// Whether the sight test's line from `o` at `from` to `n` at `at` is clear (F046539's tail:
/// `sight_cast`, then `sight_line`).
pub fn in_sight(obstacles: &Obstacles, o: &Noise, from: [f32; 3], n: &Noise, at: [f32; 3]) -> bool {
    !sight_cast(o, n) || sight_line(obstacles, from, at, n.caged)
}

/// F037712 over `doors`: whether building `b` is open (no doors, or one broken, open or
/// opening).
pub fn building_open_in(doors: &[crate::sim::DoorState], b: usize) -> bool {
    let mut doors = doors.iter().filter(|d| d.door.placement == b && !d.gone).peekable();
    doors.peek().is_none() || doors.any(|d| d.broken || d.phase == crate::sim::DOOR_OPEN || d.phase == crate::sim::DOOR_OPENING)
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
    /// stealth, and the line of sight for those it is cast to, `in_sight`), or hears it.
    pub fn can_see(&self, seer: usize, seen: usize) -> bool {
        (self.visibility(seer, seen) > kenshi_ai::perception::SEEN_THRESHOLD
            && in_sight(&self.arena.obstacles, &self.noise_of(seer), self.folk[seer].mover.position_units(), &self.noise_of(seen), self.folk[seen].mover.position_units()))
            || self.heard(seer, seen)
    }

    /// @fn F046500
    ///
    /// `visibility` (F046498) when at least 0.01 and the line of sight is clear (F046499),
    /// else 0.
    pub fn sight(&self, seer: usize, seen: usize) -> f32 {
        let v = self.visibility(seer, seen);
        if v >= 0.01 && self.line_of_sight(seer, seen) { v } else { 0.0 }
    }

    /// F046499 from `seer` to `seen` (`sight_line`).
    pub fn line_of_sight(&self, seer: usize, seen: usize) -> bool {
        let caged = self.folk[seen].prison.as_ref().is_some_and(|c| !c.out);
        sight_line(&self.arena.obstacles, self.folk[seer].mover.position_units(), self.folk[seen].mover.position_units(), caged)
    }

    /// The weather on each one (`LawState::weather`), once a second.
    fn weather_tick(&mut self) {
        let n = self.folk.len();
        if !self.tick.is_multiple_of(60) && self.law_state.weather.len() == n {
            return;
        }
        let weather = (0..n)
            .map(|id| {
                let p = &self.folk[id];
                if p.dead || p.away || p.race.is_some() || p.inside.is_some() {
                    return (0, 0.0);
                }
                let q = p.mover.position_units();
                let c = self.climate.get(&kenshi_world::zone_of(q[0], q[2])).copied().unwrap_or_default();
                if c.affect == 0 || self.immune(id, c.affect) { (0, 0.0) } else { (c.affect, c.strength) }
            })
            .collect();
        self.law_state.weather = weather;
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

    /// Whether `seer` hears `seen` now (`hear_roll`, F046538): awake, neither down nor away.
    pub fn heard(&self, seer: usize, seen: usize) -> bool {
        let (p, q) = (&self.folk[seer], &self.folk[seen]);
        if seer == seen || p.down || p.dead || p.bed.is_some() || q.dead || p.away || q.away {
            return false;
        }
        let (a, b) = (p.mover.position_units(), q.mover.position_units());
        let d2 = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
        hear_roll(seer, seen, self.t, &self.noise_of(seer), d2, &self.noise_of(seen), |b| self.building_open(b))
    }

    /// What the sensor reads of person `id` (`noise`).
    pub fn noise_of(&self, id: usize) -> Noise {
        noise(&self.folk[id], &self.arena.f[id], self.law_state.weather.get(id).copied().unwrap_or_default())
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
    /// slave's faction). One carried who is being kidnapped (F044460: no bounty, no bandit, no
    /// slave, carried, its state +0xe9 set by Task_Kidnapping), `w` not its enemy: unless `w`
    /// is its friend, only in a town, of a real faction, with no bounty with `w`'s faction;
    /// then kidnapping it (F046495, its time 20 s, `c`'s disguise blown). Anyone else carried
    /// (picked up, a squad mate carried off) is no crime.
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
        if !self.being_kidnapped(k) || self.takes_for_enemy(w, k, true) {
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

    /// @fn F044460
    ///
    /// Whether `k` is being kidnapped: no bounty with anyone, no bandit (NPC class 8), no slave,
    /// carried, and its state's +0xe9 set (Task_Kidnapping).
    pub fn being_kidnapped(&self, k: usize) -> bool {
        let p = &self.folk[k];
        !p.bounties.iter().any(|b| b.amount > 0) && self.npc_class(k) != 8 && self.slave_state(k) == 0 && p.carried_by.is_some() && self.law_state.kidnapped.contains(&k)
    }

    /// @fn F046493
    ///
    /// Each second, the kidnapped flags: one carried by someone it takes for a friend (F044470)
    /// or that its squad remembers as its lifesaver or freer (F038591: long-term tags 2 / 3) is
    /// no longer being kidnapped; carried by anyone else it tags the carrier a temporary enemy
    /// (F040863). One put down is no longer once in a bed (Character +0x2f8 1) in its home town.
    /// (assumption) The dead are let go.
    fn kidnapped_tick(&mut self) {
        if !self.tick.is_multiple_of(60) || self.law_state.kidnapped.is_empty() {
            return;
        }
        let list: Vec<usize> = self.law_state.kidnapped.iter().copied().collect();
        for k in list {
            let Some(p) = self.folk.get(k) else {
                self.law_state.kidnapped.remove(&k);
                continue;
            };
            if p.dead {
                self.law_state.kidnapped.remove(&k);
                continue;
            }
            match p.carried_by {
                Some(c) => {
                    let saved = self.dialog_memory.tags.get(&(k, c)).is_some_and(|t| t & ((1 << crate::dialogue::LT_MY_LIFESAVER) | (1 << crate::dialogue::LT_FREED_ME)) != 0);
                    if self.takes_for_friend(k, c, true) || saved {
                        self.law_state.kidnapped.remove(&k);
                    } else {
                        self.tag_short(k, c, kenshi_ai::PerceptionShortTerm::StTemporaryEnemy);
                    }
                }
                None => {
                    if p.bed.is_some() && p.town.is_some() && p.town == self.home_town(k) {
                        self.law_state.kidnapped.remove(&k);
                    }
                }
            }
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
        self.weather_tick();
        self.kidnaps_tick();
        self.alarms_tick(dt);
        self.investigate_tick(dt);
        self.law_tasks_tick(dt);
        self.law_view_tick();
        self.door_tasks_tick(dt);
        self.locked_up_tick();
        self.kidnapped_tick();
        self.witness_tick();
        // (the guards' goals, Law enforcer 34 and Bounty hunting 204, run on the planner:
        // `LawView`, `law_tasks_tick`)
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
                // Task_StealthKO (F031279): the target says EV_ASSASSINATION_FAILED at the assassin
                self.assassination_failed(t, id);
                self.messages.push(format!("{} failed to knock {} out!", self.display_name(id), self.display_name(t)));
            }
        }
    }
}

/// An alarm a town keeps (AlarmManager::Alarm, 0x68 bytes): the crime (+0x20), its owner key
/// (+0x28: the crime's faction), where (+0x30), the building it is in (+0x40) and the floor
/// (+0x60), the seconds it stays (+0x24, 8 at first: refreshing it leaves them) and the
/// one it is about (+0x8, a handle).
#[derive(Debug, Clone)]
pub struct Alarm {
    pub crime: CrimeEnum,
    pub owner: Option<String>,
    pub position: [f32; 3],
    pub building: Option<usize>,
    pub floor: i32,
    pub left: f32,
    pub criminal: Option<usize>,
}

/// How long an alarm stays (seconds; F048892's +0x24, F048855).
const ALARM_SECONDS: f32 = 8.0;
/// How near an alarm is heard (units; 0x1416c1590, F038582 / F031313 / F038651).
pub const ALARM_RANGE: f32 = 375.0;
/// How near one investigating stands to look at the alarm (units; F031313: 10000 squared) and
/// how far short of it it stops on its way (30).
const INVESTIGATE_REACH: f32 = 100.0;
const INVESTIGATE_SHORT: f32 = 30.0;
/// How long one investigating looks before it calls the alarm off (seconds; F031313: 10).
const INVESTIGATE_SECONDS: f32 = 10.0;
/// The real seconds between two "Alarm raised" messages (F048892: 30).
const ALARM_MESSAGE_GAP: f32 = 30.0;
/// "Investigate alarms", "Investigate alarms- allies only" (Task_InvestigateAlarms).
pub const INVESTIGATING: u16 = 209;
pub const INVESTIGATING_ALLIES: u16 = 210;

impl Sim {
    /// F037712: whether building `b` is open (no doors, or one broken, open or opening).
    pub(crate) fn building_open(&self, b: usize) -> bool {
        building_open_in(&self.door_states, b)
    }

    /// @fn F048892
    ///
    /// Town `town`'s alarm for `crime` at `at` (AlarmManager::raise): an alarm of that crime with
    /// the same owner key (`owner`, the crime's faction) still about is moved there (the
    /// building and floor of the one raising it, else of the criminal), its time left as it
    /// is; else a new one, for 8 s, about `criminal`, in its building (else the raiser's). A
    /// new one about one of the player's people says so ("Alarm raised", once in 30 s).
    /// (assumption) The floors (Character vtable +0x60) are all 0 here.
    pub fn raise_alarm(&mut self, town: usize, crime: CrimeEnum, at: [f32; 3], criminal: Option<usize>, owner: Option<String>, raiser: usize) {
        if crime == CrimeEnum::CrimeNone {
            return;
        }
        let building = criminal.and_then(|c| self.folk[c].inside).or(self.folk[raiser].inside);
        let alarms = self.law_state.alarms.entry(town).or_default();
        if let Some(a) = alarms.iter_mut().find(|a| a.crime == crime && a.owner == owner && a.criminal.is_none_or(|c| c < self.folk.len() && !self.folk[c].dead)) {
            a.position = at;
            a.building = building;
            return;
        }
        alarms.push(Alarm { crime, owner, position: at, building, floor: 0, left: ALARM_SECONDS, criminal });
        if criminal.is_some_and(|c| self.folk[c].player) && self.law_state.alarm_message_at.is_none_or(|t| ALARM_MESSAGE_GAP < self.t - t) {
            self.law_state.alarm_message_at = Some(self.t);
            self.messages.push("Alarm raised".to_string());
        }
        if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
            eprintln!("law: alarm in town {town} for {crime:?} at {:?} (by {raiser})", at.map(|v| v as i32));
        }
    }

    /// @fn F048840
    ///
    /// The nearest alarm of town `town` to `id` within `range` (squared distance, all three
    /// axes), of the crimes in `mask` (bits by CrimeEnum; 0 any) and of owner key `owner` (none:
    /// any). One in a building is heard at half the range, and in a building shut up only by
    /// those inside it, at a quarter.
    /// (not ported) The floors' test (another floor not heard when the building has its +0xc2
    /// flag, or two or more floors apart).
    pub fn nearest_alarm(&self, town: usize, id: usize, mask: u32, range: f32, owner: Option<&str>) -> Option<usize> {
        let alarms = self.law_state.alarms.get(&town)?;
        let at = self.folk[id].mover.position_units();
        let mut best: Option<(usize, f32)> = None;
        for (k, a) in alarms.iter().enumerate() {
            if mask != 0 && mask & (1 << (a.crime as u32)) == 0 {
                continue;
            }
            if owner.is_some_and(|o| a.owner.as_deref() != Some(o)) {
                continue;
            }
            let mut r = range;
            if let Some(b) = a.building {
                r *= 0.5;
                if !self.building_open(b) {
                    if self.folk[id].inside != Some(b) {
                        continue;
                    }
                    r *= 0.5;
                }
            }
            let d2 = (a.position[0] - at[0]).powi(2) + (a.position[1] - at[1]).powi(2) + (a.position[2] - at[2]).powi(2);
            if d2 < r * r && best.is_none_or(|(_, b)| d2 < b) {
                best = Some((k, d2));
            }
        }
        best.map(|(k, _)| k)
    }

    /// @fn F038582
    ///
    /// ALARMS_IN_THE_VICINITY (condition 130): an alarm of the town `id` is in within 375 units.
    pub fn alarm_in_vicinity(&self, id: usize) -> bool {
        self.folk[id].town.is_some_and(|t| self.nearest_alarm(t, id, 0, ALARM_RANGE, None).is_some())
    }

    /// @fn F048855
    ///
    /// Each tick, the towns' alarms run down and go (AlarmManager::update), and who has one in
    /// its vicinity now (condition 130, for the AI's view).
    pub fn alarms_tick(&mut self, dt: f32) {
        for alarms in self.law_state.alarms.values_mut() {
            alarms.iter_mut().for_each(|a| a.left -= dt);
            alarms.retain(|a| a.left >= 0.0);
        }
        self.law_state.alarms.retain(|_, a| !a.is_empty());
        let near: Vec<bool> = if self.law_state.alarms.is_empty() { vec![false; self.folk.len()] } else { (0..self.folk.len()).map(|id| self.alarm_in_vicinity(id)).collect() };
        self.law_state.alarm_near = near;
    }

    /// @fn F040912
    ///
    /// DA_CRIME_ALARM (F040912 case 0x21), the crime alarm `npc` raises about `target`: its crime
    /// seen (F046479) and, in a town with alarms whose faction `npc`'s is not at war with, an
    /// alarm of the target's crime at the target (F048892; the owner key the crime's faction):
    /// one with no crime inside `npc`'s private home is trespassing. Whether an alarm went up.
    /// (not ported) F046543 on `npc`'s AI and the run its AI's +0x70 flag sends it on.
    pub fn crime_alarm(&mut self, npc: usize, target: usize) -> bool {
        self.crime_seen(target);
        let Some(town) = self.folk[npc].town else { return false };
        let theirs = self.towns.get(town).and_then(|t| t.faction.clone());
        if let (Some(mine), Some(t)) = (self.folk[npc].faction.as_deref(), theirs.as_deref())
            && self.at_war(mine, t)
        {
            return false;
        }
        if self.folk[target].crime.is_none()
            && let Some(home) = self.folk[npc].home
            && self.folk[target].inside == Some(home)
            && !self.building_public(home)
        {
            let f = self.folk[npc].faction.clone();
            self.folk[target].crime = Some(Crime { kind: CrimeEnum::CrimeTresspassing, faction: f.unwrap_or_default(), left: 3.0, caught: false, victim: Some(npc) });
        }
        let (kind, owner) = self.folk[target].crime.as_ref().map_or((CrimeEnum::CrimeNone, None), |c| (c.kind, Some(c.faction.clone()).filter(|f| !f.is_empty())));
        let at = self.folk[target].mover.position_units();
        self.raise_alarm(town, kind, at, Some(target), owner, npc);
        kind != CrimeEnum::CrimeNone
    }

    /// The kidnappings under way: each begins (`kidnap_start`) once its kidnapper carries its
    /// victim; one whose kidnapper has turned to something else is dropped.
    fn kidnaps_tick(&mut self) {
        let mut k = 0;
        while k < self.law_state.kidnaps.len() {
            let (a, v) = self.law_state.kidnaps[k];
            let p = &self.folk[a];
            if p.carrying == Some(v) {
                self.law_state.kidnaps.remove(k);
                self.kidnap_start(a, v);
            } else if p.pickup != Some(v) || p.dead || p.down {
                self.law_state.kidnaps.remove(k);
            } else {
                k += 1;
            }
        }
    }

    /// @fn F031292
    /// @fn F047051
    ///
    /// Task_Kidnapping::start, `kidnapper` lifting `victim`: the victim tags it a temporary
    /// enemy (F040863), the crime of kidnapping against the victim's faction (F046495, on the
    /// victim), then the victim's notice: the kidnap's stealth chance (F047051: 100 on one
    /// knocked out, else the stealth knockout chance F047050 x 100) under a roll of 0 .. 100 is
    /// noticed: the victim's sensor told of the kidnapper (F046543) and `kidnap_alarm`.
    /// (not ported) Character +0x278 (no sure 100 on the knocked out while set), the noticing
    /// victim's own reactions (task 0x74 while its +0x2f8 is 1, F0319d0, F02ca98) and the
    /// kidnapper's assassination training (F009769).
    pub fn kidnap_start(&mut self, kidnapper: usize, victim: usize) {
        // Task_Kidnapping (F031325): the victim is being kidnapped (+0xe9)
        self.law_state.kidnapped.insert(victim);
        self.tag_short(victim, kidnapper, kenshi_ai::PerceptionShortTerm::StTemporaryEnemy);
        let faction = self.faction_id(victim);
        self.commit_crime_on(kidnapper, CrimeEnum::CrimeKidnapping, faction.as_deref(), Some(victim));
        let chance = if self.folk[victim].down {
            100.0
        } else {
            stealth_ko_chance(self.arena.f[kidnapper].level(StatsEnumerated::StatAssassination), self.ko_guard(victim)) * 100.0
        };
        let roll = self.rng.uniform() * 100.0;
        if chance < roll {
            // F046543: the victim's sensor knows the kidnapper, where it is now
            let (at, now) = (self.folk[kidnapper].mover.position_units(), self.t);
            match self.folk[victim].sensed.iter_mut().find(|e| e.0 == kidnapper) {
                Some(e) => {
                    e.1 = at;
                    e.2 = now;
                }
                None => self.folk[victim].sensed.push((kidnapper, at, now)),
            }
            self.kidnap_alarm(kidnapper, victim);
        }
    }

    /// F031292's alarm (Task_Kidnapping::start): the victim's town is alarmed of a kidnapping
    /// at the kidnapper (the owner key the victim's faction), the kidnapper's disguise blown.
    pub fn kidnap_alarm(&mut self, kidnapper: usize, victim: usize) {
        self.folk[kidnapper].disguise_blown = crate::disguise::BLOWN_SECONDS;
        self.folk[kidnapper].disguised = None;
        let Some(town) = self.folk[victim].town else { return };
        let at = self.folk[kidnapper].mover.position_units();
        let owner = self.faction_id(victim);
        self.raise_alarm(town, CrimeEnum::CrimeKidnapping, at, Some(kidnapper), owner, victim);
    }

    /// @fn F031313
    ///
    /// Task_InvestigateAlarms (209; 210 "allies only": the alarms its faction owns), for whoever
    /// has it in hand: done out of town or with no alarm within 375 units; else running to 30
    /// units short of the nearest, sizing up the criminals about (one with a crime against an
    /// ally of its faction tagged a temporary enemy and attacked); there (within 100 units) it
    /// faces the alarm for 10 s, then calls it off (the alarm gone) and is done.
    pub fn investigate_tick(&mut self, dt: f32) {
        let ids: Vec<usize> = self.folk.iter().filter(|p| !p.dead && !p.down && p.ai.queue.first().is_some_and(|s| s.task == INVESTIGATING || s.task == INVESTIGATING_ALLIES)).map(|p| p.id).collect();
        for id in ids {
            let allies = self.folk[id].ai.queue.first().is_some_and(|s| s.task == INVESTIGATING_ALLIES);
            let done = |sim: &mut Sim| {
                sim.folk[id].ai.queue.remove(0);
                sim.folk[id].started = false;
                sim.law_state.investigating.remove(&id);
            };
            let Some(town) = self.folk[id].town else {
                done(self);
                continue;
            };
            let owner = if allies { self.folk[id].faction.clone() } else { None };
            let Some(k) = self.nearest_alarm(town, id, 0, ALARM_RANGE, owner.as_deref()) else {
                done(self);
                continue;
            };
            let a = self.law_state.alarms[&town][k].clone();
            let at = self.folk[id].mover.position_units();
            let d2 = (a.position[0] - at[0]).powi(2) + (a.position[1] - at[1]).powi(2) + (a.position[2] - at[2]).powi(2);
            if d2 <= INVESTIGATE_REACH * INVESTIGATE_REACH {
                let (dx, dz) = (a.position[0] - at[0], a.position[2] - at[2]);
                let l = dx.hypot(dz);
                if l > 0.1 {
                    self.folk[id].facing = [dx / l, 0.0, dz / l];
                }
                let left = self.law_state.investigating.get(&id).copied().unwrap_or(INVESTIGATE_SECONDS) - dt;
                if left < 0.0 {
                    if let Some(list) = self.law_state.alarms.get_mut(&town) {
                        list.remove(k);
                    }
                    done(self);
                } else {
                    self.law_state.investigating.insert(id, left);
                }
                continue;
            }
            self.law_state.investigating.insert(id, INVESTIGATE_SECONDS);
            // on its way: 30 units short of the alarm, at a run
            let l = d2.sqrt().max(1e-3);
            let goal = [a.position[0] + (at[0] - a.position[0]) / l * INVESTIGATE_SHORT, a.position[1], a.position[2] + (at[2] - a.position[2]) / l * INVESTIGATE_SHORT];
            if self.tick.is_multiple_of(60) || !self.folk[id].started {
                self.folk[id].started = true;
                let Sim { folk, manager, .. } = self;
                let p = &mut folk[id];
                p.mover.set_max_speed(kenshi_char::movement::speed_limit(p.own_speed, kenshi_char::movement::MoveSpeed::Run.cap(15.0).unwrap(), None));
                p.pathing.go_to(p.id, &p.mover, goal, false, manager);
            }
            // the criminals about of a crime against its faction's allies
            if self.tick.is_multiple_of(60)
                && let Some(mine) = self.folk[id].faction.clone()
            {
                let c = (0..self.folk.len()).find(|&c| {
                    let q = &self.folk[c];
                    let r = q.mover.position_units();
                    c != id && !q.dead && (r[0] - at[0]).hypot(r[2] - at[2]) < 1500.0 && q.crime.as_ref().is_some_and(|cr| !cr.faction.is_empty() && self.allied(&mine, &cr.faction)) && self.can_see(id, c)
                });
                if let Some(c) = c {
                    self.tag_short(id, c, kenshi_ai::PerceptionShortTerm::StTemporaryEnemy);
                    self.order_attack(&[id], c);
                }
            }
        }
    }
}

/// The law's and the slavers' AI tasks (AI_TASK enums) the planner runs with crate::law's
/// conditions, scores, finders and handlers.
pub const LAW_ENFORCER: u16 = 34;
pub const BOUNTY_HUNTING: u16 = 204;
pub const WATCHING_SLAVES: u16 = 183;
pub const FIND_CAGE_FOR_CARRIED: u16 = 112;
pub const PUT_CARRIED_DEAD_IN_CAGES: u16 = 281;
pub const STRIPPING_WEAPONS: u16 = 181;
pub const LIFTING: u16 = 68;
pub const PUTTING_DOWN: u16 = 69;
pub const PUT_IN_PRISON: u16 = 108;
pub const RESTRAINING: u16 = 166;
pub const PROCESSING_SLAVE: u16 = 182;
pub const STAND_STILL_DAMMIT: u16 = 64;
pub const LIFTING_SNATCHING: u16 = 213;
pub const CAPTURE_TARGET: u16 = 240;
/// The runnable ones (their handlers are `law_tasks_tick`'s).
/// "Remove Home Intruder" (223, the residents' "pickup intruder": lift the knocked out found
/// in the home) and "Intruder out" (224, Task_TakeIntruderOutside: carry them out).
pub const REMOVE_HOME_INTRUDER: u16 = 223;
pub const INTRUDER_OUT: u16 = 224;
/// "Medic" (58, the packages' goal) and "First aid" (25, Task_FirstAid): the medic's patient by
/// F050304, treated by `Sim::first_aid_tick`.
pub const MEDIC: u16 = 58;
pub const FIRST_AID: u16 = 25;
pub const LAW_RUNNABLE: [u16; 11] = [STRIPPING_WEAPONS, LIFTING, PUTTING_DOWN, PUT_IN_PRISON, RESTRAINING, PROCESSING_SLAVE, STAND_STILL_DAMMIT, LIFTING_SNATCHING, CAPTURE_TARGET, INTRUDER_OUT, FIRST_AID];
/// Task_TakeIntruderOutside: the way out runs to the home's door's outside point and 1.1 of
/// the door's depth beyond it (F030841 -> F029254); the task gives up (the carried put down)
/// after 5 game minutes (F030843).
const INTRUDER_OUT_BEYOND: f32 = 1.1;
const INTRUDER_OUT_MINUTES: f64 = 5.0;
/// Task_StripWeapons's time (F031285: 0.8 s) and Task_CuffTarget's (F031266: 0.99 s).
const STRIP_SECONDS: f32 = 0.8;
const CUFF_SECONDS: f32 = 0.99;

/// What the AI's view (crate::sim View) reads of the law each tick: each one's flags for the
/// law's conditions and each NPC's targets the law's finders found (the planner's hooks).
#[derive(Debug, Clone, Default)]
pub struct LawView {
    /// Each one's first aid need (F040427, flesh and robotic) and each medic's patient
    /// (F050304), once a second.
    pub aid_need: Vec<f32>,
    pub medic: std::collections::HashMap<usize, usize>,
    pub caged: Vec<bool>,
    pub dead: Vec<bool>,
    pub carried_by: Vec<Option<usize>>,
    pub carrying: Vec<Option<usize>>,
    pub armed: Vec<bool>,
    pub chained: Vec<bool>,
    pub shackled: Vec<bool>,
    pub escaping: Vec<bool>,
    pub looks_slave: Vec<bool>,
    pub standing_still: Vec<bool>,
    pub crouching: Vec<bool>,
    pub in_town: Vec<bool>,
    pub has_home_town: Vec<bool>,
    pub at_home_town: Vec<bool>,
    pub arms_lost: Vec<bool>,
    /// Inside a building; whom each carrier carries and takes for a friend (F044470); each
    /// one's INTRUDER_IS_OUTSIDE of whom it carries (F038580).
    pub in_building: Vec<bool>,
    pub friends: std::collections::HashSet<(usize, usize)>,
    pub intruder_out: Vec<bool>,
    pub faction: Vec<Option<String>>,
    /// (person, finder's task) -> the character found.
    pub targets: std::collections::HashMap<(usize, u16), usize>,
    /// person -> the cage F047589 found (furniture).
    pub cages: std::collections::HashMap<usize, usize>,
}

impl LawView {
    fn person(t: &crate::sim::Target) -> Option<usize> {
        match t {
            crate::sim::Target::Person(x) => Some(*x),
            _ => None,
        }
    }

    /// The law's conditions (F039867's checks) for `me` on `t`; `None` for the others.
    pub fn check(&self, me: usize, c: u16, t: &crate::sim::Target) -> Option<bool> {
        let x = Self::person(t);
        let at = |v: &Vec<bool>, i: usize| v.get(i).copied().unwrap_or(false);
        Some(match c {
            // TARGET_IS_ARRESTED (F038613): caged, or carried by one of another faction; a
            // target that is no character: whether I carry it
            129 => match x {
                Some(x) => at(&self.caged, x) || self.carried_by.get(x).copied().flatten().is_some_and(|k| self.faction.get(k) != self.faction.get(x)),
                None => false,
            },
            // IS_CARRYING_TARGET (F038612: Character +0x348 and the carried handle valid, never
            // compared with the target; crate::sim answers it first), IS_CARRYING_SOMETHING
            // (F038574, Character +0x348)
            48 | 47 => self.carrying.get(me).copied().flatten().is_some(),
            // TARGET_IS_UNARMED (F038666): no weapon in hand or worn; anything not a character is
            118 => x.is_none_or(|x| !at(&self.armed, x)),
            // targetAtATown (F036823), isAtHomeTown (F050236)
            113 => x.is_some_and(|x| at(&self.in_town, x)),
            95 => at(&self.at_home_town, me),
            // "My arm is messed up too badly to lift anything." (F039107, the medical +0x166; the
            // name is what the player is told when it fails): the arms fit to lift,
            // (assumption) not both arms lost
            80 => !at(&self.arms_lost, me),
            // isCarriedDudeDead (F038657)
            99 => self.carrying.get(me).copied().flatten().is_some_and(|k| at(&self.dead, k)),
            // FINAL_FOUND_AND_PUT_CARRIED_IN_A_CAGE (Character +0x380's handle empty),
            // CARRIED_DUDE_NOW_IN_CAGE (F039855): (assumption) carrying no one
            154 | 81 => self.carrying.get(me).copied().flatten().is_none(),
            // TARGET_LOOKS_LIKE_A_SLAVE (F038711), isChained (F038662, Character +0x320),
            // targetStandingStill (F044567), isTargetEscapee (F038664),
            // isWearingShackles_locked (F038663)
            120 => x.is_some_and(|x| at(&self.looks_slave, x)),
            134 => x.is_some_and(|x| at(&self.chained, x)),
            42 => x.is_some_and(|x| at(&self.standing_still, x)),
            124 => x.is_some_and(|x| at(&self.escaping, x)),
            114 => x.is_some_and(|x| at(&self.shackled, x)),
            // INTRUDER_IS_OUTSIDE (F038580): the target, else the one carried: none, a friend,
            // or out of every building and no longer carried by me
            // "I need a medkit." (F050288): the target needs first aid (F03ded3 over 0.01).
            // (not ported) Its Character +0x2e0 +0x7c test
            22 => x.is_some_and(|x| 0.01 < self.aid_need.get(x).copied().unwrap_or(0.0)),
            137 => match x {
                Some(x) => !at(&self.in_building, x) && self.carrying.get(me).copied().flatten() != Some(x) || self.friends.contains(&(me, x)),
                None => at(&self.intruder_out, me),
            },
            _ => return None,
        })
    }

    /// The law's scores (AI methods by vtable slot, functions by address).
    pub fn score(&self, me: usize, d: &kenshi_ai::task_def::TaskDef, t: &crate::sim::Target) -> f32 {
        use kenshi_ai::task_def::Score;
        let carrying = self.carrying.get(me).copied().flatten();
        match d.score {
            // Arresting (F038566): none out of town for a squad with a home town
            Score::Method(32) => {
                if !self.in_town.get(me).copied().unwrap_or(false) && self.has_home_town.get(me).copied().unwrap_or(false) { 0.0 } else { 1.0 }
            }
            // Bounty hunting (F038567): 1 while a cage is to be had (F047589)
            Score::Method(33) => {
                if self.cages.contains_key(&me) { 1.0 } else { 0.0 }
            }
            // Lifting (F038637): none carrying; 1 for a character not carried by me, 0.5 for
            // anything else
            Score::Method(59) => match (carrying, Self::person(t)) {
                (Some(_), _) => 0.0,
                (None, Some(x)) => {
                    if self.carried_by.get(x).copied().flatten() == Some(me) { 0.0 } else { 1.0 }
                }
                (None, None) => 0.5,
            },
            // Watching slaves, Capturing slave (F038543): with no target 1 while carrying no one
            // and standing; (assumption) 1 with one (the AI's +0x1d8 score not ported)
            Score::Method(61) => match t {
                crate::sim::Target::None => {
                    if carrying.is_none() && !self.crouching.get(me).copied().unwrap_or(false) { 1.0 } else { 0.0 }
                }
                _ => 1.0,
            },
            // Find and put in cage (F038654): none carrying no one, 0.1 out of town, else 1
            // ((not ported) its tests of the town's cages held by the AI's +0x48)
            // First aid (F050293): (assumption) the target's need (F050304 weighs it by
            // nearness and relation in finding the patient)
            Score::Function(0x14099e0a0) => Self::person(t).map_or(0.0, |x| self.aid_need.get(x).copied().unwrap_or(0.0)),
            // Intruder out (F038656): 2 carrying someone it takes for no friend, else none
            Score::Function(0x1405a13a0) => match carrying {
                Some(c) if !self.friends.contains(&(me, c)) => 2.0,
                _ => 0.0,
            },
            Score::Function(0x1405a1140) => match carrying {
                None => 0.0,
                Some(_) => {
                    if self.in_town.get(me).copied().unwrap_or(false) { 1.0 } else { 0.1 }
                }
            },
            _ => 0.0,
        }
    }

    /// The law's target finders (the +0x160 hooks).
    pub fn find_target(&self, me: usize, task: u16) -> Option<crate::sim::Target> {
        match task {
            LAW_ENFORCER | BOUNTY_HUNTING | WATCHING_SLAVES | REMOVE_HOME_INTRUDER => self.targets.get(&(me, task)).map(|&x| crate::sim::Target::Person(x)),
            MEDIC => self.medic.get(&me).map(|&x| crate::sim::Target::Person(x)),
            FIND_CAGE_FOR_CARRIED | PUT_CARRIED_DEAD_IN_CAGES => self.cages.get(&me).map(|&k| crate::sim::Target::Furniture(k)),
            _ => None,
        }
    }
}

impl Sim {
    /// Whether `id` has goal `task` among its jobs.
    fn has_goal(&self, id: usize, task: u16) -> bool {
        self.folk[id].ai.jobs.iter().flatten().any(|g| g.task == task)
    }

    /// @fn F050281
    ///
    /// Bounty hunting's target finder: the first one `me` senses that is not carried, of a real
    /// faction, not an animal, not held in a cage, with a bounty `me` sees (F046512), or tagged
    /// ST_CRIMINAL by its squad, or that it takes for an enemy (F044572).
    /// (assumption) The sensed: within 1500 units and seen (`can_see`).
    fn bounty_hunter_target(&self, me: usize) -> Option<usize> {
        let at = self.folk[me].mover.position_units();
        (0..self.folk.len()).find(|&c| {
            let p = &self.folk[c];
            if c == me || p.dead || p.carried_by.is_some() || p.race.is_some() || p.prison.as_ref().is_some_and(|pr| !pr.out) || self.faction_id(c).is_some_and(|f| self.faction_not_real(&f)) {
                return false;
            }
            let q = p.mover.position_units();
            (q[0] - at[0]).hypot(q[2] - at[2]) < 1500.0
                && (0 < self.bounty_seen_by(c, me) || self.has_short(me, c, kenshi_ai::PerceptionShortTerm::StCriminal) || self.takes_for_enemy(me, c, true))
                && (p.down || self.can_see(me, c))
        })
    }

    /// @fn F050280
    ///
    /// Law enforcer's target finder (Attacking trouble makers, 34): in its town, over what its
    /// sensor knows (its knowledge entries, F046505) of that town and standing (not flagged 8,
    /// down): the enemies (flag 4) making trouble (the state object +0xac above 0, or tagged
    /// ST_AGGRESSOR by its squad, F040885 bit 2) and the neutrals (flag 2) `troublemaker`
    /// takes (none of law enforcement, traders or diplomats; military only from another town
    /// and of a faction the town does not keep, its +0xf0 map); the best of them its squad's
    /// target scoring picks (F038638).
    /// (assumption) The state object +0xac above 0: a crime under way. The town's +0xf0 map:
    /// the town's own faction. F038638's pick: the nearest. F038606 holds for one of its town.
    fn law_enforcer_target(&self, me: usize) -> Option<usize> {
        let town = self.folk[me].town?;
        let at = self.folk[me].mover.position_units();
        let town_faction = self.towns.get(town).and_then(|t| t.faction.clone());
        let mut list: Vec<usize> = Vec::new();
        for &(c, _, _) in &self.folk[me].sensed {
            let Some(q) = self.folk.get(c) else { continue };
            if c == me || q.dead || q.down || q.away || q.town != Some(town) {
                continue;
            }
            if self.takes_for_enemy(me, c, true) {
                if q.crime.is_some() || self.has_short(me, c, kenshi_ai::PerceptionShortTerm::StAggressor) {
                    list.push(c);
                }
                continue;
            }
            if self.takes_for_friend(me, c, true) {
                continue;
            }
            match self.npc_class(c) {
                1 | 3 | 5 => continue,
                2 if self.home_town(c) == Some(town) || self.faction_id(c).is_some_and(|f| town_faction.as_deref() == Some(f.as_str())) => continue,
                _ => {}
            }
            if self.troublemaker(me, c) {
                list.push(c);
            }
        }
        list.into_iter().min_by(|&a, &b| {
            let d = |k: usize| {
                let q = self.folk[k].mover.position_units();
                (q[0] - at[0]).powi(2) + (q[2] - at[2]).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
    }

    /// @fn F050279
    ///
    /// Whether law enforcer `me` takes neutral `c` for a troublemaker: never a slaver (NPC
    /// class 7); one with no trouble under way (the state object +0xac) and not tagged
    /// ST_AGGRESSOR only with a prison sentence (+0xa0, F046518) and as a bandit (class 8);
    /// then the one it wrongs (the state object's +0x68 handle) must be a character it goes for
    /// (F039200): one `me` takes for a friend (F044470), or, that one no slaver, no bandit,
    /// with no bounty with `me`'s faction (F046513) and no crime of its own: when `c`'s faction
    /// is "not real" or of "fundamental type" 8, or that one takes `me` for no enemy (F044572)
    /// and is no slave.
    /// (assumption) +0xac above 0: a crime under way; +0x68: its crime's victim; F039200: in a
    /// fight with it; a prison sentence: held or an escaped prisoner. Character vtable +0x248
    /// (F039356) is none for people.
    fn troublemaker(&self, me: usize, c: usize) -> bool {
        if self.npc_class(c) == 7 {
            return false;
        }
        let q = &self.folk[c];
        if q.crime.is_none() && !self.has_short(me, c, kenshi_ai::PerceptionShortTerm::StAggressor) {
            let sentenced = q.prison.is_some() || q.escaped.is_some();
            if !sentenced || self.npc_class(c) != 8 {
                return false;
            }
        }
        let Some(v) = q.crime.as_ref().and_then(|cr| cr.victim).filter(|&v| v < self.folk.len() && v != c) else { return false };
        let f = &self.arena.f[c];
        if !(f.active && f.focus.or(f.combat.target) == Some(v)) {
            return false;
        }
        if self.takes_for_friend(me, v, true) {
            return true;
        }
        let vc = self.npc_class(v);
        if vc == 7 {
            return false;
        }
        let mine = self.faction_id(me);
        if mine.as_deref().is_some_and(|f| 0 < self.bounty_with(v, f)) || vc == 8 || self.folk[v].crime.is_some() {
            return false;
        }
        let cf = self.faction_id(c);
        if cf.as_deref().is_some_and(|f| self.faction_not_real(f) || self.fundamental_type(f) == 8) {
            return true;
        }
        !self.takes_for_enemy(v, me, true) && vc != 6
    }

    /// @fn F050304
    ///
    /// Medic's target finder: each person in reach not an enemy scores its first aid need
    /// (F040427) x how near (F050251: 1 - distance^2 / 3397390, so 1843 units' reach) x, outside
    /// the medic's squad, 0.8 for an ally else the relation / 100, at least 0.05 (F050239); x
    /// 0.05 when someone else treats them already. Its own need, unscaled, wins when greater.
    /// (assumption) A stranger needs a score of 0.1 to be treated (F050304's threshold
    /// constant is not read; its "already treated" test is its squad's job list, here
    /// anyone's). One of the player's treats the player's people, and allies' only with the AI
    /// option Heal Allies (crate::routines' reading of it).
    fn medic_patient(&self, id: usize, need: &[f32]) -> Option<usize> {
        const REACH2: f32 = 3_397_390.0;
        let p = &self.folk[id];
        if p.dead || p.down || p.carrying.is_some() || p.prison.is_some() || p.carried_by.is_some() || p.race.is_some() {
            return None;
        }
        let me = p.mover.position_units();
        let rel = p.faction.as_ref().and_then(|f| self.relations.get(f));
        let mut best: Option<(usize, f32)> = None;
        for (k, q) in self.folk.iter().enumerate() {
            if k == id || q.dead || q.carried_by.is_some() || q.race.is_some() {
                continue;
            }
            let at = q.mover.position_units();
            let d2 = (at[0] - me[0]).powi(2) + (at[2] - me[2]).powi(2);
            if REACH2 <= d2 || rel.is_some_and(|r| r.is_enemy(q.faction.as_deref())) {
                continue;
            }
            if p.player && !q.player && !(self.ai_options.heal_allies && rel.is_some_and(|r| r.is_ally(q.faction.as_deref()))) {
                continue;
            }
            let mut score = need.get(k).copied().unwrap_or(0.0);
            if score <= 0.0 {
                continue;
            }
            let same_squad = p.platoon.is_some() && p.platoon == q.platoon;
            if !same_squad {
                let f = match (rel, q.faction.as_deref()) {
                    (Some(r), Some(o)) if r.is_ally(Some(o)) => 0.8,
                    (Some(r), Some(o)) => r.relation(o) / 100.0,
                    _ => 0.0,
                };
                score *= if f < 0.0 { 0.05 } else { f.max(0.05) };
            }
            score *= (1.0 - d2 / REACH2).clamp(0.0, 1.0);
            if self.folk.iter().any(|m| m.id != id && m.first_aid == Some(k)) {
                score *= 0.05;
            }
            if !same_squad && score < 0.1 {
                continue;
            }
            if best.is_none_or(|(_, b)| b < score) {
                best = Some((k, score));
            }
        }
        let own = need.get(id).copied().unwrap_or(0.0);
        match best {
            Some((_, b)) if b < own => Some(id),
            Some((k, _)) => Some(k),
            None if 0.0 < own => Some(id),
            None => None,
        }
    }

    /// @fn F050282
    ///
    /// Remove Home Intruder's target finder: of those its sensor knows as neutral, enemy, down
    /// or lying low (flags 1 / 2 / 3 / 14, none a friend or caged, flags 0 / 9), the first not
    /// carried, inside `me`'s home and down, in no bed or cage (Character +0x2f8 0).
    fn intruder_target(&self, me: usize) -> Option<usize> {
        let home = self.folk[me].home?;
        self.folk[me].sensed.iter().map(|e| e.0).find(|&c| {
            let Some(q) = self.folk.get(c) else { return false };
            c != me
                && !q.dead
                && q.down
                && q.carried_by.is_none()
                && q.inside == Some(home)
                && q.bed.is_none()
                && q.prison.as_ref().is_none_or(|pr| pr.out)
                && !self.takes_for_friend(me, c, true)
        })
    }

    /// @fn F047571
    /// @fn F047535
    ///
    /// Watching slaves' target finder: none while `me` is held; the first slave (any state) it
    /// senses still to be processed (F047535: with an owner of a faction allied to `me`'s, not
    /// carried, not an animal; not one already shaved and a slave (state 1)); else one held in a
    /// cage in the building `me` is in, within 180 units, still to be processed.
    /// (assumption) The sensed: within 1500 units and seen; (not ported) F047535's
    /// reservation count (F052a3b) and F03a355.
    fn slave_watch_target(&self, me: usize) -> Option<usize> {
        if self.folk[me].prison.is_some() {
            return None;
        }
        let mine = self.folk[me].faction.clone()?;
        let at = self.folk[me].mover.position_units();
        let to_process = |c: usize| {
            let p = &self.folk[c];
            let Some(s) = p.slave.as_ref() else { return false };
            if p.carried_by.is_some() || p.race.is_some() || p.dead || !self.allied(&mine, &s.faction) {
                return false;
            }
            !(s.state == crate::slavery::IS_SLAVE && p.shaved)
        };
        let sensed = (0..self.folk.len()).find(|&c| {
            let q = self.folk[c].mover.position_units();
            c != me && self.folk[c].slave.is_some() && (q[0] - at[0]).hypot(q[2] - at[2]) < 1500.0 && to_process(c) && self.can_see(me, c)
        });
        sensed.or_else(|| {
            let inside = self.folk[me].inside?;
            (0..self.folk.len()).find(|&c| {
                let p = &self.folk[c];
                let q = p.mover.position_units();
                c != me && p.prison.as_ref().is_some_and(|pr| !pr.out) && p.inside == Some(inside) && (q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2) + (q[2] - at[2]).powi(2) <= 32400.0 && to_process(c)
            })
        })
    }

    /// F038711: whether `x` looks like a slave to `me`'s faction: no animal, no diplomat or law
    /// enforcer; unless the faction "allow slaves weapons" no weapon in hand or worn; its body
    /// armour and trousers none or slave things ("stigma" 6) and locked shackles worn.
    /// (assumption) The rest of F038711's tests (the faction's "slave clothing" list
    /// otherwise) are not ported.
    fn looks_slave_to(&self, me: usize, x: usize) -> bool {
        let p = &self.folk[x];
        if p.race.is_some() || matches!(self.npc_class(x), 1 | 5) {
            return false;
        }
        let allowed = self.folk[me].faction.as_deref().and_then(|f| self.data.get(f)).is_some_and(|f| f.bools.get("allow slaves weapons").copied().unwrap_or(false));
        if !allowed && self.armed(x) {
            return false;
        }
        let worn = |slot: i32| p.inventory.sections.iter().find(|s| s.slot == slot && s.is_equipment()).and_then(|s| s.items.first()).map(|pl| pl.item.record.clone());
        let slave_thing = |r: &String| self.data.get(r).and_then(|d| d.ints.get("stigma").copied()) == Some(6);
        worn(5).is_none_or(|r| slave_thing(&r)) && worn(6).is_none_or(|r| slave_thing(&r)) && self.shackles_lock(x).is_some()
    }

    /// Whether `id` has a weapon in hand or worn (F038666's test).
    fn armed(&self, id: usize) -> bool {
        self.folk[id].inventory.equipped().any(|i| matches!(i.kind, Some(ItemType::Weapon) | Some(ItemType::Crossbow)))
    }

    /// The law's view for the AI (see `LawView`): every tick the flags, once a second the
    /// finders' targets for those with the law's goals.
    pub fn law_view_tick(&mut self) {
        let n = self.folk.len();
        let mut v = std::mem::take(&mut self.law_state.view);
        v.caged = self.folk.iter().map(|p| p.prison.as_ref().is_some_and(|pr| !pr.out)).collect();
        v.dead = self.folk.iter().map(|p| p.dead).collect();
        v.carried_by = self.folk.iter().map(|p| p.carried_by).collect();
        v.carrying = self.folk.iter().map(|p| p.carrying).collect();
        v.armed = (0..n).map(|i| self.armed(i)).collect();
        v.chained = (0..n).map(|i| self.slave_state(i) == crate::slavery::IS_SLAVE).collect();
        v.shackled = (0..n).map(|i| self.shackles_lock(i).is_some()).collect();
        v.escaping = (0..n).map(|i| self.slave_state(i) == crate::slavery::ESCAPING_SLAVE).collect();
        v.standing_still = self.folk.iter().map(|p| p.speed < 1.0).collect();
        v.crouching = self.folk.iter().map(|p| p.down || p.sneaking).collect();
        v.in_town = self.folk.iter().map(|p| p.town.is_some()).collect();
        v.has_home_town = (0..n).map(|i| self.home_town(i).is_some()).collect();
        v.at_home_town = (0..n).map(|i| self.folk[i].town.is_some() && self.home_town(i) == self.folk[i].town).collect();
        v.arms_lost = (0..n).map(|i| self.limb_lost(i, 0) && self.limb_lost(i, 1)).collect();
        v.faction = (0..n).map(|i| self.faction_id(i)).collect();
        v.in_building = self.folk.iter().map(|p| p.inside.is_some()).collect();
        // the carriers' take on whom they carry (F044470), and F038580 of it
        v.friends.clear();
        v.intruder_out = vec![true; n];
        for me in 0..n {
            if let Some(c) = self.folk[me].carrying {
                let friend = self.takes_for_friend(me, c, true);
                if friend {
                    v.friends.insert((me, c));
                }
                v.intruder_out[me] = friend;
            }
        }
        if self.tick.is_multiple_of(60) || v.looks_slave.len() != n {
            v.targets.clear();
            v.cages.clear();
            v.medic.clear();
            v.aid_need = (0..n).map(|i| {
                let a = self.arena.f[i].medical.aid_need(false);
                if self.folk[i].dead { 0.0 } else { a.flesh + a.robotic }
            }).collect();
            v.looks_slave = vec![false; n];
            for me in 0..n {
                let p = &self.folk[me];
                // the player's people's Medic job (the Medic button; crate::jobs' goals list)
                if p.player && !p.dead && !p.down && !p.away && p.jobs.iter().any(|j| j.task == MEDIC) && !self.arena.f[me].active && let Some(t) = self.medic_patient(me, &v.aid_need) {
                    v.medic.insert(me, t);
                }
                if p.player || p.dead || p.down || p.away {
                    continue;
                }
                if self.has_goal(me, BOUNTY_HUNTING) && let Some(t) = self.bounty_hunter_target(me) {
                    v.targets.insert((me, BOUNTY_HUNTING), t);
                }
                if self.has_goal(me, LAW_ENFORCER) && let Some(t) = self.law_enforcer_target(me) {
                    v.targets.insert((me, LAW_ENFORCER), t);
                }
                if self.has_goal(me, MEDIC) && !self.arena.f[me].active && let Some(t) = self.medic_patient(me, &v.aid_need) {
                    v.medic.insert(me, t);
                }
                if self.has_goal(me, REMOVE_HOME_INTRUDER) && let Some(t) = self.intruder_target(me) {
                    v.targets.insert((me, REMOVE_HOME_INTRUDER), t);
                }
                if self.has_goal(me, WATCHING_SLAVES) && let Some(t) = self.slave_watch_target(me) {
                    v.targets.insert((me, WATCHING_SLAVES), t);
                    v.looks_slave[t] = self.looks_slave_to(me, t);
                }
                let cages_wanted = self.has_goal(me, BOUNTY_HUNTING) || self.has_goal(me, FIND_CAGE_FOR_CARRIED) || self.has_goal(me, PUT_CARRIED_DEAD_IN_CAGES);
                if cages_wanted && let Some(f) = p.faction.clone() {
                    let at = p.mover.position_units();
                    if let Some(k) = self.free_cage(&f, at, false) {
                        v.cages.insert(me, k);
                    }
                }
            }
        }
        if std::env::var("KENSHI_LAW_DEBUG").is_ok() && self.tick.is_multiple_of(600) {
            let with = |t: u16| (0..n).filter(|&i| self.has_goal(i, t)).count();
            eprintln!("law view: goals 34 {} / 204 {} / 183 {} / 281 {}; targets {:?}; cages {}", with(LAW_ENFORCER), with(BOUNTY_HUNTING), with(WATCHING_SLAVES), with(PUT_CARRIED_DEAD_IN_CAGES), v.targets, v.cages.len());
        }
        self.law_state.view = v;
    }

    /// The clip a law or slavery task of `id`'s plays while it is at it (the game's task clips
    /// ask this): "medic" for Task_StripWeapons (181, F031285: every update until done),
    /// Task_CuffTarget (166, F031266: under 0.99 s) and Task_ProcessNewSlave (182, F031348: its
    /// first 3 s, after which it stops "squat T" and "medic").
    pub fn law_task_clip(&self, id: usize) -> Option<&'static str> {
        if self.law_state.processing.iter().any(|e| e.taker == id) {
            return Some("medic");
        }
        let time = self.law_state.task_time.get(&id).copied()?;
        match self.folk.get(id)?.ai.queue.first()?.task {
            STRIPPING_WEAPONS => Some("medic"),
            RESTRAINING if time < CUFF_SECONDS => Some("medic"),
            _ => None,
        }
    }

    /// The law's runnable tasks for whoever has one in hand (the planner's steps; crate::sim
    /// leaves them to this):
    /// - Stripping weapons (181, Task_StripWeapons F031285): after 0.8 s the target's weapons
    ///   taken into one's own inventory (left on the ground when there is no room), the target
    ///   tagged ST_HAS_BEEN_LOOTED;
    /// - Lifting (68, 213, 240, Task_LiftSomething): the target picked up; done once carried;
    /// - Putting down (69, Task_PutDownObject): the carried put down;
    /// - Put in prison (108, Task_PutInSomething F031342): the carried put in the cage: a slave
    ///   of one's faction (or by one who takes captives) as a slave, else a prisoner;
    /// - Restraining (166, Task_CuffTarget F031266): after 0.99 s the target enslaved by one's
    ///   squad (F030712, the crime of enslaving where it is one, F031266);
    /// - Processing slave (182, Task_ProcessNewSlave): `begin_processing`, done once processed;
    /// - Stand still dammit! (64, Task_MakeTargetStandStill): the target stops.
    ///
    /// A step whose target is gone (dead, or carried by another) is given up.
    pub fn law_tasks_tick(&mut self, dt: f32) {
        use crate::sim::Target;
        let ids: Vec<usize> = self.folk.iter().filter(|p| !p.dead && !p.down && p.ai.queue.first().is_some_and(|s| LAW_RUNNABLE.contains(&s.task))).map(|p| p.id).collect();
        for me in ids {
            let Some(step) = self.folk[me].ai.queue.first().cloned() else { continue };
            if std::env::var("KENSHI_LAW_DEBUG").is_ok() && !self.law_state.task_time.contains_key(&me) {
                eprintln!("law: {me} takes up task {} on {:?}", step.task, step.target);
            }
            let done = |sim: &mut Sim| {
                // the step may already be gone (the plan cleared while it ran)
                if !sim.folk[me].ai.queue.is_empty() {
                    sim.folk[me].ai.queue.remove(0);
                }
                sim.folk[me].started = false;
                sim.law_state.task_time.remove(&me);
            };
            let give_up = |sim: &mut Sim| {
                sim.folk[me].ai.queue.clear();
                sim.folk[me].started = false;
                sim.law_state.task_time.remove(&me);
            };
            let target = match step.target {
                Target::Person(x) => Some(x),
                _ => None,
            };
            let time = self.law_state.task_time.get(&me).copied().unwrap_or(0.0) + dt;
            self.law_state.task_time.insert(me, time);
            match step.task {
                STRIPPING_WEAPONS => {
                    let Some(t) = target.filter(|&t| !self.folk[t].dead) else {
                        give_up(self);
                        continue;
                    };
                    if time <= STRIP_SECONDS {
                        continue;
                    }
                    let mut taken = Vec::new();
                    for s in self.folk[t].inventory.sections.iter_mut().filter(|s| s.is_equipment()) {
                        let mut k = 0;
                        while k < s.items.len() {
                            if matches!(s.items[k].item.kind, Some(ItemType::Weapon) | Some(ItemType::Crossbow)) {
                                taken.push(s.items.remove(k).item);
                            } else {
                                k += 1;
                            }
                        }
                    }
                    for item in taken {
                        if !self.folk[me].inventory.add(item.clone(), false) {
                            self.drop_item(t, item);
                        }
                    }
                    self.tag_short(me, t, kenshi_ai::PerceptionShortTerm::StHasBeenLooted);
                    done(self);
                }
                LIFTING | LIFTING_SNATCHING | CAPTURE_TARGET => {
                    // (Task_LiftSomething lifts the dead too: Find corpses' bodies, crate::sim)
                    let Some(t) = target.filter(|&t| (step.task == LIFTING || !self.folk[t].dead) && self.folk[t].carried_by.is_none_or(|c| c == me)) else {
                        give_up(self);
                        continue;
                    };
                    if self.folk[me].carrying == Some(t) {
                        done(self);
                    } else if self.folk[me].pickup != Some(t) && self.folk[t].down {
                        // (at it: the lift within reach is crate::sim's)
                        self.folk[me].pickup = Some(t);
                    }
                }
                PUTTING_DOWN => {
                    if self.folk[me].carrying.is_some() {
                        self.order_drop(&[me]);
                    }
                    done(self);
                }
                PUT_IN_PRISON => {
                    let (Target::Furniture(cage), Some(c)) = (step.target, self.folk[me].carrying) else {
                        give_up(self);
                        continue;
                    };
                    let Some(f) = self.folk[me].faction.clone() else {
                        give_up(self);
                        continue;
                    };
                    self.folk[me].carrying = None;
                    self.folk[c].carried_by = None;
                    let theirs = self.folk[c].slave.as_ref().is_some_and(|s| s.faction == f);
                    if theirs || self.folk[me].captures {
                        let owner = self.folk[me].platoon;
                        if self.folk[c].slave.is_none() {
                            self.make_slave(c, owner, &f);
                        }
                        self.imprison(c, cage, &f, true);
                    } else {
                        self.imprison(c, cage, &f, false);
                    }
                    self.remember(c, me, crate::dialogue::LT_MY_CAPTOR);
                    done(self);
                }
                RESTRAINING => {
                    let Some(t) = target.filter(|&t| !self.folk[t].dead) else {
                        give_up(self);
                        continue;
                    };
                    if time <= CUFF_SECONDS {
                        continue;
                    }
                    self.cuffing_crime(me, t);
                    let Some(f) = self.folk[me].faction.clone() else {
                        give_up(self);
                        continue;
                    };
                    if self.folk[t].slave.is_none() {
                        let owner = self.folk[me].platoon;
                        self.folk[t].slave = Some(crate::slavery::Slavery::new(owner, &f));
                    } else {
                        self.set_slave_state(t, crate::slavery::IS_SLAVE);
                    }
                    self.put_on_shackles(t);
                    done(self);
                }
                PROCESSING_SLAVE => {
                    let Some(t) = target.filter(|&t| !self.folk[t].dead) else {
                        give_up(self);
                        continue;
                    };
                    let queued = self.law_state.processing.iter().any(|e| e.slave == t);
                    if !self.folk[me].started {
                        self.folk[me].started = true;
                        if !queued {
                            let f = self.folk[t].slave.as_ref().map(|s| s.faction.clone()).or_else(|| self.folk[me].faction.clone()).unwrap_or_default();
                            self.begin_processing(t, &f, me);
                        }
                    } else if !queued {
                        done(self);
                    }
                }
                // Task_TakeIntruderOutside (F030842 start, F030843 update, F031060 arrival): out
                // of its home's door (the door nearest) to beyond its outside point; there, or
                // already out of every building, the carried is put down and ignored for 20 s
                // (F044546); stuck, or after 5 game minutes, put down where it is
                INTRUDER_OUT => {
                    let Some(c) = self.folk[me].carrying else {
                        give_up(self);
                        continue;
                    };
                    let Some(home) = self.folk[me].home else {
                        give_up(self);
                        continue;
                    };
                    let put_down = |sim: &mut Sim, ignore: bool| {
                        sim.order_drop(&[me]);
                        if ignore {
                            let p = &mut sim.folk[me];
                            match p.ignored.iter_mut().find(|i| i.0 == c) {
                                Some(i) => i.1 = crate::sim::IGNORE_SECONDS,
                                None => p.ignored.push((c, crate::sim::IGNORE_SECONDS)),
                            }
                        }
                    };
                    if self.folk[me].inside.is_none() && !self.folk[me].started {
                        put_down(self, true);
                        done(self);
                        continue;
                    }
                    let at = self.folk[me].mover.position_units();
                    let door = self.door_states.iter().filter(|d| d.door.placement == home && !d.gone).min_by(|x, y| {
                        let l = |p: [f32; 3]| (p[0] - at[0]).hypot(p[2] - at[2]);
                        l(x.door.position).total_cmp(&l(y.door.position))
                    });
                    let Some(door) = door else {
                        give_up(self);
                        continue;
                    };
                    let (p0, q0) = (door.door.position, door.inside);
                    let out = [p0[0] + (p0[0] - q0[0]) * INTRUDER_OUT_BEYOND, p0[1], p0[2] + (p0[2] - q0[2]) * INTRUDER_OUT_BEYOND];
                    let minutes = time as f64 / self.hour_secs * 60.0;
                    if !self.folk[me].started {
                        self.folk[me].started = true;
                        let Sim { folk, manager, .. } = self;
                        let p = &mut folk[me];
                        p.pathing.go_to(p.id, &p.mover, out, false, manager);
                        continue;
                    }
                    let arrived = (at[0] - out[0]).hypot(at[2] - out[2]) < 20.0;
                    let p = &self.folk[me];
                    let stuck = !arrived && !p.pathing.pending && p.pathing.follow.is_none();
                    if arrived || (stuck && p.inside.is_none()) {
                        put_down(self, true);
                        done(self);
                    } else if stuck || (!p.sneaking && INTRUDER_OUT_MINUTES <= minutes) {
                        put_down(self, false);
                        done(self);
                    }
                }
                // Task_FirstAid (25): the patient treated (`Sim::first_aid_tick` walks there and
                // treats until done)
                FIRST_AID => {
                    if let Some(t) = target.filter(|&t| !self.folk[t].dead) {
                        self.folk[me].first_aid = Some(t);
                    }
                    done(self);
                }
                STAND_STILL_DAMMIT => {
                    if let Some(t) = target {
                        self.order_stop(&[t]);
                    }
                    done(self);
                }
                _ => {}
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
        let n = |speed: f32, stealth: f32| Noise { sneaking: true, speed, own_speed: 30.0, stealth, ..Default::default() };
        let me = Noise::default();
        let heard = |n: &Noise, d: f32| (0..2000).filter(|&w| hear_roll(1, 2, w as f32 * 2.0, &me, d * d, n, |_| true)).count();
        // sneaking: lerp(stealth, 450, 80), halved for the awake: 225 at no stealth
        assert_eq!(n(0.0, 0.0).radius(), 450.0);
        assert!(heard(&n(0.0, 0.0), 200.0) > 0);
        assert_eq!(heard(&n(0.0, 0.0), 230.0), 0);
        // the stealthier, the less
        assert!(heard(&n(30.0, 0.8), 20.0) < heard(&n(30.0, 0.0), 20.0));
        assert_eq!(heard(&n(30.0, 1.0), 20.0), 0);
        // a dust storm at full strength: a quarter
        assert_eq!(Noise { weather: (crate::hazards::WA_DUST, 1.0), ..n(0.0, 0.0) }.radius(), 112.5);
        // fighting: lerp(speed / own speed, 350, 600), heard at once in the same building
        let f = Noise { fighting: true, ..n(15.0, 0.0) };
        assert_eq!(f.radius(), 475.0);
        assert_eq!(heard(&f, 230.0), 2000);
        assert_eq!(heard(&f, 240.0), 0);
        // in another building: only fighting, one of the two outdoors, a quarter (shut: an eighth)
        let inside = Noise { inside: Some(3), ..f };
        let roll = |shut: bool, d: f32| (0..200).filter(|&w| hear_roll(1, 2, w as f32 * 2.0, &me, d * d, &inside, |_| !shut)).count();
        assert!(roll(false, 100.0) > 0);
        assert_eq!(roll(false, 120.0), 0);
        assert_eq!(roll(true, 60.0), 0);
        assert_eq!(hear_roll(1, 2, 0.0, &Noise { inside: Some(4), ..me }, 1.0, &inside, |_| true), false);
        // neither sneaking nor fighting: not by noise; two of the player's: within 1000
        assert_eq!(heard(&Noise { sneaking: false, ..n(30.0, 0.0) }, 20.0), 0);
        let pl = Noise { player: true, ..Default::default() };
        assert!(hear_roll(1, 2, 0.0, &pl, 999.0 * 999.0, &pl, |_| true));
        // the line of sight: clear with no solids
        assert!(sight_line(&None, [0.0; 3], [100.0, 0.0, 0.0], false));
        assert!(!sight_cast(&me, &Noise::default()));
        assert!(sight_cast(&me, &pl));
    }

    #[test]
    fn sight_lines() {
        use kenshi_io::nxu::{Pose, Shape, ShapeKind};
        // a wall 4 units thick at x 50 (origin 1000 off)
        let s = kenshi_physics::world::Solids::new(&[Shape { pose: Pose { rows: Pose::IDENTITY.rows, pos: [50.0, 0.0, 0.0] }, kind: ShapeKind::Box([2.0, 100.0, 100.0]) }]);
        let w = kenshi_physics::world::StaticWorld::new([(bevy::math::Affine3A::IDENTITY, &s)]);
        let o = [1000.0, 0.0, 0.0];
        let obs: Obstacles = Some((std::sync::Arc::new(w), o));
        let at = |x: f32| [x + 1000.0, 0.0, 0.0];
        assert!(sight_line(&obs, at(0.0), at(40.0), false));
        assert!(!sight_line(&obs, at(0.0), at(100.0), false));
        // the hit within 2 of the target does not block; nor the cage of a caged one
        assert!(sight_line(&obs, at(0.0), at(49.0), false));
        assert!(sight_line(&obs, at(0.0), at(60.0), true));
        assert!(!sight_line(&obs, at(0.0), at(100.0), true));
    }
}
