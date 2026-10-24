//! The world beyond the squad (port_map `world`): the zones Kenshi keeps loaded round the
//! camera and the player's people (ZoneManager), squads travelling the whole map while their
//! zone is not loaded (Kenshi's UnloadedPlatoon / AbstractMovementBase), the factions'
//! campaigns on the player (FactionWarMgr), the unique NPCs' deaths and the world states they
//! make (WORLD_EVENT_STATE), and the towns those turn into (TOWN "override town": destroyed,
//! taken over).
//!
//! Specs:
//! - WORLD_EVENT_STATE (F050361): "NPC is" each unique in the state its ref's value gives
//!   (alive 1, dead 0, imprisoned 2), "NPC is NOT" none in it (with "player involvement" a dead
//!   or imprisoned one only by the player's hand), "player ally" / "player enemy" each faction
//!   an ally / enemy of the player's (value 1) or not (0); a unique never met counts as alive
//!   (F039415). "town okay" is not read (no data uses it).
//! - TOWN "override town" (F051436, as the town's zone loads): of the town's current record's
//!   overrides, those whose "world state" refs all hold (a ref's value 1: the state must hold;
//!   else must not), the one with the most world states replaces it (F051428); once a game
//!   load, and not while a save is loading. With "residents override", a building's residents
//!   are the override's "residents" of the building's designation (the ref's second value).
//! - FactionWarMgr (F050818): scheduled campaigns (a line's "trigger campaign", a campaign's
//!   "victory trigger" / "loss trigger" / "trigger player ally", F050814) set out when due
//!   (`campaigns_due`); every 6 game hours a faction with none on its way nor scheduled may set
//!   out one territorial campaign (`campaign_check`, chances F050802); its forces are more than
//!   its "num forces" people: a special leader's squad, the faction's squads free for it, and
//!   squads of its "squads to use" made at its homes nearest the target (F050815, F050801).
//! - FactionUniqueSquadManager (F030231 / F030223 / F030219, `SpecialSquads`): each faction's
//!   "special squads" kept with its prosperity and their replacement timers, as Kenshi keeps
//!   them; Kenshi sends none of them out (Lord Commander Epson's army never takes the field).
//!
//! Kenshi's factions campaign only against the player (FACTION_CAMPAIGN targets the player's
//! bases or characters); no faction raids another's town and nothing fights off-screen, so
//! towns change hands only by the world states.
//!
//! Away squads walk their long-distance route (`kenshi_world::longpath`: the roads over 10000
//! units when they pay at the faction's "road preference", F040644) at 60 units a second, the
//! cap 90 while over 25000 from the target (F045434; campaigns x their "travel speed
//! unloaded"), never over the mover's own 70 (F040640). One standing in a loaded zone, at
//! least 178.5 units in from each side whose neighbour is not loaded, is made real (F045434,
//! F051564 / F051565: its people kept for when it comes back) and walks its route on the
//! navmesh; one whose zone unloads goes away again. A town's "roaming squads" are no world
//! squads: they join the wandering squads spawned round the player (F048143, `spawn_homeless`).
//! A campaign's forces set out from the faction's homes nearest the target and march on it;
//! there, its phase (F050809, `campaign_phases`) decides: the assault (fighting the player's
//! people) or leaving, won after the assault's hours or with the defenders quiet and the town
//! open, lost with its leading squad out or under a fifth of its squads able.

use crate::sim::Sim;
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_world::longpath::WorldGrid;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Away squads' walking speed (units a second): an unloaded platoon walking to its target
/// (F045434, UnloadedPlatoon's tick, mode 6) caps its mover at 60, or 90 while the target is
/// over 25000 units off; the mover goes at the lower of that and its own 70
/// (AbstractMovementBase +0xb4, F040640). The base speed class 25.
const SPEED: f32 = 60.0;
const SPEED_FAR: f32 = 90.0;
const FAR_TARGET: f32 = 25000.0;
const OWN_SPEED: f32 = 70.0;
/// How far in from a side whose neighbour zone is not loaded an unloaded platoon must stand to
/// be loaded (F045434's margin, 0x141718a0c), and how far past its zone's square it may stand
/// (F051564: the square grown by 10 a side).
const LOAD_MARGIN: f32 = 178.5;
const ZONE_GROW: f32 = 10.0;
/// Kenshi's zone activation (ZoneManager): the seconds a zone stays loaded after it was last
/// wanted (F051606's timers, table 0x141748740, by request mode): 4 for the camera's zones, 6
/// for the player's people's; the box round each of those (units, F051614), and how far the
/// camera's focus may go from its current zone's centre before it is chosen again (F051615).
const KEEP_CAMERA: f32 = 4.0;
const KEEP_PEOPLE: f32 = 6.0;
const PEOPLE_BOX: f32 = 340.0;
const RECENTRE: f32 = 4147.2;

/// What a world squad is doing.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Goal {
    /// (an older save's: towns' patrols abroad, which Kenshi has not; such a squad is gone)
    Patrol { home: usize, to: usize, wait: f32 },
    /// (an older save's town raid, likewise gone)
    Raid { town: usize },
    /// A campaign on the player: its record, where it marches, how long it has been there.
    Campaign { campaign: String, target: [f32; 3], there: f32, hostile: bool },
    /// Going back (then gone).
    Home { to: [f32; 3] },
}

/// A squad of the world.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorldSquad {
    pub template: String,
    pub faction: Option<String>,
    /// World position (units) and the way left to go.
    pub pos: [f32; 3],
    pub route: Vec<[f32; 3]>,
    pub goal: Goal,
    pub speed: f32,
    pub strength: f32,
    /// Its people once made real (kept while away), and whether real now.
    pub members: Vec<usize>,
    pub active: bool,
    pub alive: bool,
    /// Seconds to its next order while real.
    pub order_timer: f32,
    /// Its campaign won: what it does now (`raid`), and the player's people it carries off.
    #[serde(default)]
    pub won: Option<crate::raid::Victory>,
    #[serde(default)]
    pub captives: Vec<usize>,
    /// Its campaign's "dialog announcement" has been made (the assembly package's "Diplomat
    /// mission").
    #[serde(default)]
    pub announced: bool,
    /// The faction whose war manager set its campaign out.
    #[serde(default)]
    pub side: Option<String>,
    /// Its campaign's arrival has been told ("{1} has arrived at {2}").
    #[serde(default)]
    pub arrived: bool,
    /// The town it calls home (the platoon's home, F044526 +0x30; Platoon +0xb8 for
    /// Task_GoHomeTown, F031066): a patrol's home town, the town a campaign's squad set out
    /// from, a special leader's own town.
    #[serde(default)]
    pub home: Option<usize>,
    /// The nest it calls home (a campaign's squad set out from one of its faction's nests).
    #[serde(default)]
    pub nest_home: Option<usize>,
    /// F044612's last answer: whether it can still fight (worked out again while it is here,
    /// kept while away).
    #[serde(default = "yes")]
    pub able: bool,
    /// The campaign it was taken on for (its war manager's platoon map, FactionWarMgr +0x28:
    /// platoon -> instance, emptied as the instance goes, F050875).
    #[serde(default)]
    pub campaign: Option<String>,
}

fn yes() -> bool {
    true
}

/// A town a squad may stand in or call home (Kenshi's TownBase): a town of the level, a nest
/// (a TOWN of its own, crate::nests) or one of the player's outposts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Place {
    Town(usize),
    Nest(usize),
    Outpost(usize),
}

/// A campaign's phase (CampaignInstance +0xc0) and its two clocks (+0x18 the phase's start,
/// +0x20 the assault's quiet clock, F050809), game hours; its leading squad, whether that has
/// been in the target town, and its target.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Phase {
    pub phase: u8,
    pub since: f64,
    pub quiet: f64,
    /// +0x80: its leading squad (F050792: the first squad added, or a special leader's squad
    /// already about), a world squad.
    #[serde(default)]
    pub lead: Option<usize>,
    /// +0x3c: its leading squad has been in the target town (F050809's head: the squad's town,
    /// Platoon vtable 0x78, the target).
    #[serde(default)]
    pub entered: bool,
    /// +0x40: what it marches on.
    #[serde(default)]
    pub target: CampaignTarget,
    /// +0xa0: the town it set out from.
    #[serde(default)]
    pub home: Option<Place>,
    /// +0x3d: its faction has scheduled a follow-up while it ran (F050814).
    #[serde(default)]
    pub flagged: bool,
    /// +0xf0: when it last looked for another target (F050950), game hours; 0 not yet.
    #[serde(default)]
    pub redirect_at: f64,
}

impl Phase {
    /// The phase `phase` from `now` (both clocks restarted, as every phase change does), the
    /// rest as `was` had it.
    pub fn at(phase: u8, now: f64, was: Option<Phase>) -> Phase {
        let w = was.unwrap_or(Phase { phase, since: now, quiet: now, lead: None, entered: false, target: CampaignTarget::None, home: None, flagged: false, redirect_at: 0.0 });
        Phase { phase, since: now, quiet: now, ..w }
    }
}

/// What a campaign marches on (CampaignInstance +0x40, its +0x6c set for a character): one of
/// the player's outposts (`Sim::outposts`, the player's TOWN of type 1), a town of the level
/// (a line's campaign on an NPC's home town, F040912) or a character.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum CampaignTarget {
    #[default]
    None,
    Outpost(usize),
    Person(usize),
    Town(usize),
}

/// The town a campaign target is (none for a character).
fn target_place(t: CampaignTarget) -> Option<Place> {
    match t {
        CampaignTarget::Outpost(o) => Some(Place::Outpost(o)),
        CampaignTarget::Town(n) => Some(Place::Town(n)),
        _ => None,
    }
}

/// A campaign a war manager has scheduled (FactionWarMgr::CampaignRequest, F050814): when it is
/// due (+0x00, game hours), the campaign (+0x08), its target (+0x10, and whether that was a
/// town, its hand's type +0x18) and home (+0x30), the target's faction (+0x50, always the
/// player's here) and its tries (+0x58); `side` the war manager's faction.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Request {
    pub campaign: String,
    pub side: String,
    pub due: f64,
    pub target: CampaignTarget,
    pub town: bool,
    pub home: Option<Place>,
    pub tries: u32,
}

/// Campaign phases: on the way, the assault (its "pt2 AI leader"), over (retreating or gone
/// home), victory.
pub const PHASE_APPROACH: u8 = 0;
pub const PHASE_ASSAULT: u8 = 1;
pub const PHASE_OVER: u8 = 2;
pub const PHASE_VICTORY: u8 = 3;
/// F050809: a campaign is there once its leading squad is within 400 units of a character it
/// targets (0x14171f330, 400 squared), within 0.8 of a town's radius of a town (0x14168a178,
/// F048764); a peaceful one that finds the town held by hostile occupiers gives up within 1.8
/// of its radius (0x1417442f0).
const ARRIVED_AT_CHARACTER: f32 = 400.0;
const ARRIVED_AT_TOWN: f32 = 0.8;
const OCCUPIED_TOWN: f32 = 1.8;
/// F050809's hours: an ASSAULT_TOWN's assault 4 (0x1416f0170), its quiet spell 0.2
/// (0x1416ae0c8), a DEFEND_TOWN 12 (0x1416adc00), a TRADER_VISIT 1 (0x14167bd00); a campaign over
/// is let go after 3 (0x141693f20); under 0.2 of its squads able it has lost (0x1416885d8). The
/// assault holds while over half (0x141684440) of more than 2 of the player's people in the
/// town stand.
const ASSAULT_HOURS: f64 = 4.0;
const QUIET_HOURS: f64 = 0.2;
const DEFEND_HOURS: f64 = 12.0;
const VISIT_HOURS: f64 = 1.0;
const OVER_HOURS: f64 = 3.0;
/// F050950: a campaign looks for another target every 0.2 hours (0x14168a4a8).
const REDIRECT_HOURS: f64 = 0.2;
const ABLE_SHARE: f32 = 0.2;
const DEFENDERS_HALF: f64 = 0.5;
/// F050821: a beaten hostile campaign tries again 40 to 96 hours on (0x141685344, 0x1416ad828).
const RETRY_FROM: f64 = 40.0;
const RETRY_TO: f64 = 96.0;
/// CampaignData "key" (CampaignType): ASSAULT_TOWN, DEFEND_TOWN, TRADER_VISIT.
const ASSAULT_TOWN: i32 = 0;
const DEFEND_TOWN: i32 = 2;
const TRADER_VISIT: i32 = 3;

/// A faction's special squads (FactionUniqueSquadManager, Faction +0x70, made for every
/// faction, F030231, and set up from its record, F030223): its prosperity (+0x08 of +0x10,
/// both the FACTION's "max prosperity" to start, F030249) and a row (UniqueSpawnData, 0x28
/// bytes) for each of its "special squads" that is a UNIQUE_SQUAD_TEMPLATE (type 77): the
/// template, the number to keep (the ref's first value), the replacement timer (+0x0c,
/// seconds) and the squads of it about (+0x10).
///
/// Kenshi keeps these but makes no squad from them. Its tick (F030219) only counts the timers
/// down, stopping at the first row due with too few squads; the getter that would hand one out
/// returns none (F030218, `xor eax, eax`) and the faction's spawn factor is 1 (F030216); no
/// caller gives the platoon factory (F038255, the only maker of a UniquePlatoon, F050368) a
/// unique template, and a saved platoon is "special" only as a UniquePlatoon is. So Lord
/// Commander Epson's "General Epson Army" (United Cities) and the Dust Bandits' "UNIQUE bandit
/// test" never take the field, nor their "missions" (DIPLOMATIC_ASSAULTS, read by the
/// UniquePlatoon alone: its update, F050354, picks an aggressive SINGLE_DIPLOMATIC_ASSAULT whose
/// "conditions" hold of its target, F050351 / F030224); the Dust King lives in Dust King Tower.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpecialSquads {
    pub faction: String,
    pub prosperity: f64,
    pub max_prosperity: f32,
    pub rows: Vec<SpecialRow>,
}

/// One special squad a faction keeps (FactionUniqueSquadManager::UniqueSpawnData).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpecialRow {
    pub template: String,
    pub number: i32,
    pub timer: f32,
    pub squads: Vec<usize>,
}

/// F030219: the prosperity grows by a tenth of the time while under its most (0x14168a318).
const PROSPERITY_RATE: f64 = 0.1;

impl SpecialSquads {
    /// @fn F030231
    /// @fn F030223
    /// @fn F030249
    ///
    /// The manager of faction `f`: its prosperity full ("max prosperity"), a row for each of
    /// its "special squads" refs that is a UNIQUE_SQUAD_TEMPLATE, its timer 0.
    pub fn of(data: &GameDataContainer, f: &GameData) -> SpecialSquads {
        let max = num(f, "max prosperity");
        let rows = f
            .refs("special squads")
            .iter()
            .filter(|(t, _)| data.get(t).is_some_and(|t| t.item_type() == Some(ItemType::UniqueSquadTemplate)))
            .map(|(t, v)| SpecialRow { template: t.clone(), number: v.first().copied().unwrap_or(0), timer: 0.0, squads: Vec::new() })
            .collect();
        SpecialSquads { faction: f.string_id.clone(), prosperity: max as f64, max_prosperity: max, rows }
    }

    /// @fn F030219
    ///
    /// The manager's tick (from the faction's, F041585, with the time since its last, Faction
    /// +0x264): the prosperity up by 0.1 a second while under its most; each row's timer down
    /// while above 0, the rows gone through until one is due with fewer squads than its number.
    pub fn tick(&mut self, dt: f32) {
        if self.prosperity < self.max_prosperity as f64 {
            self.prosperity += dt as f64 * PROSPERITY_RATE;
        }
        for r in &mut self.rows {
            if r.timer > 0.0 {
                r.timer -= dt;
            } else if (r.squads.len() as f32) < r.number as f32 {
                return;
            }
        }
    }

    /// @fn F030225
    ///
    /// Special squad `squad` gone (UniquePlatoon's vtable +0xe8, F050363, unless F045424 keeps
    /// it; its read of the template's "persistent" there goes unused): out of its row, whose
    /// timer is set to the template's "replacement time" in game hours (a game day's seconds,
    /// F040787, x the hours / 24).
    pub fn squad_gone(&mut self, data: &GameDataContainer, squad: usize, hour_secs: f64) {
        for r in &mut self.rows {
            if let Some(k) = r.squads.iter().position(|&s| s == squad) {
                r.squads.remove(k);
                let hours = data.get(&r.template).map_or(0.0, |t| num(t, "replacement time"));
                r.timer = (hour_secs * 24.0) as f32 * hours / 24.0;
                return;
            }
        }
    }
}

/// A town as the world keeps it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TownState {
    /// The TOWN record it is now (the base or an override).
    pub record: String,
    /// The town's level id (`Town::id`; empty in older saves).
    #[serde(default)]
    pub id: String,
}

#[derive(Default)]
pub struct World {
    pub grid: Option<Arc<WorldGrid>>,
    pub squads: Vec<WorldSquad>,
    pub towns: Vec<TownState>,
    /// Unique characters (CHARACTER records) dead.
    pub dead_uniques: HashSet<String>,
    /// Each running campaign's phase (CampaignInstance +0xc0, F050809), by campaign.
    pub phases: HashMap<String, Phase>,
    /// Uniques whose death or captivity the player had a hand in (the unique table's flag,
    /// +0x34: set as it dies or is taken, cleared as it is free again, F039250).
    pub involved: HashSet<String>,
    /// Campaigns triggered (record -> times), and when each last set out (game hours).
    pub campaigns_run: HashMap<String, u32>,
    pub campaign_last: HashMap<String, f64>,
    pub timer: f32,
    /// Game hours of the next 6-hourly check.
    pub next_check: f64,
    pub started: bool,
    /// Characters (records) died this tick, for the world states.
    pub pending_deaths: Vec<String>,
    /// Towns (their level ids, `Town::id`) the player has found: the map shows only these; and those it has been at
    /// (within 400 units: the map names them, not by their "unexplored name").
    pub discovered: HashSet<String>,
    pub explored: HashSet<String>,
    /// The factions the player's faction knows (PlayerFactionRelations +0x68, F045530: those
    /// the faction tab lists): those in its record's "relations" at the start (F041513), each
    /// faction a player character senses (CharSensor::perceive, F046546) and each town's
    /// faction as it is explored (F048883), all through F045557. Saved as "known" (F041511).
    pub known_factions: HashSet<String>,
    /// The campaigns the war managers have scheduled (FactionWarMgr +0xc0, F050814), in the
    /// order scheduled.
    pub requests: Vec<Request>,
    /// An older save's scheduled campaigns (no target kept): made requests as it loads.
    pub triggered: Vec<(String, f64)>,
    /// An older save's: the faction each of its scheduled campaigns was set out by.
    pub trigger_side: HashMap<String, String>,
    /// Campaigns a line's "lock campaign" took off territorial triggering ("unlock campaign"
    /// puts back).
    pub campaigns_locked: HashSet<String>,
    /// The options' "Town raid events: frequency" (settings.cfg "raidFrequencyMult", global
    /// 0x1421334e4, F032960): the territorial chances' multiplier.
    pub raid_frequency: f32,
    /// The options' "Town raid events: size" (settings.cfg "raidSizeMult", F050803): a
    /// campaign's "num forces" multiplier; 0 (not set yet) counts as 1.
    pub raid_size: f32,
    /// The towns whose residents are being handed out (F048906's done flag, Town +0x110): the
    /// TOWN record they were made for and the rows left (Town +0x300: template, number left,
    /// priority), by town.
    pub resident_rows: HashMap<usize, (String, Vec<(String, i32, i32)>)>,
    /// Every faction's special squads (`SpecialSquads`), in the factions' order.
    pub special: Vec<SpecialSquads>,
    /// The towns that joined a faction's war manager list after the level's (TownBase::setFaction,
    /// F048805: a town whose record override names another faction, F051428; a nest as it is
    /// made), in that order (`war_towns`); made up again after a load (`war_ready`).
    pub(crate) war_log: Vec<Place>,
    pub(crate) war_ready: bool,
}

fn num(r: &GameData, k: &str) -> f32 {
    r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0)
}

/// A campaign's field through its "inherits from" chain (its own first).
fn campaign_num(data: &GameDataContainer, c: &GameData, k: &str) -> f32 {
    let mut seen = 0;
    let mut cur = Some(c);
    while let Some(r) = cur {
        if r.floats.contains_key(k) || r.ints.contains_key(k) {
            let v = num(r, k);
            // an inheriting campaign's 0 is "not set" for the fields its bases give
            if v != 0.0 || r.refs("inherits from").is_empty() {
                return v;
            }
        }
        seen += 1;
        cur = if seen > 8 { None } else { r.refs("inherits from").first().and_then(|(s, _)| data.get(s)) };
    }
    0.0
}

/// A campaign's flag through its "inherits from" chain (its own first).
fn campaign_bool(data: &GameDataContainer, c: &GameData, k: &str) -> bool {
    let mut cur = Some(c);
    for _ in 0..8 {
        let Some(r) = cur else { break };
        if let Some(&b) = r.bools.get(k) {
            return b;
        }
        cur = r.refs("inherits from").first().and_then(|(s, _)| data.get(s));
    }
    false
}

/// A campaign's refs of a category through its inheritance: its own and its bases' ("inherits
/// from: copies from this base", fcs.def), its own values first where both list one. A child's
/// world states add to its base's: "HN Assault (Phoenix kidnap)" lists the inquisitors alive,
/// its base "Player kidnapped Phoenix".
pub(crate) fn campaign_refs(data: &GameDataContainer, c: &GameData, cat: &str) -> Vec<(String, Vec<i32>)> {
    fn walk(data: &GameDataContainer, c: &GameData, cat: &str, out: &mut Vec<(String, Vec<i32>)>, depth: u32) {
        for (s, v) in c.refs(cat) {
            if !out.iter().any(|(o, _)| o == s) {
                out.push((s.clone(), v.to_vec()));
            }
        }
        if depth > 8 {
            return;
        }
        for (b, _) in c.refs("inherits from") {
            if let Some(base) = data.get(b)
                && base.string_id != c.string_id
            {
                walk(data, base, cat, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    walk(data, c, cat, &mut out, 0);
    out
}

/// @fn F038257
///
/// The town each placement belongs to, as Kenshi gives a building its town as the zone file
/// makes it (`kenshi_world::town::building_town`): the town it stands in, else for one with an
/// "interior" (or inside another) the nearest town within 10000 units; the towns of the
/// player's faction left out. The zone state's town handle is not read (Kenshi reads it only
/// for the player's towns).
pub(crate) fn placement_towns(data: &GameDataContainer, towns: &[kenshi_world::town::Town], placed: &[kenshi_world::Placement], at: &[[f32; 3]]) -> Vec<Option<usize>> {
    let player = |t: usize| towns[t].faction.as_deref() == Some(crate::sim::PLAYER_FACTION);
    placed
        .iter()
        .zip(at)
        .map(|(pl, &p)| {
            let interior = data.get(&pl.building).is_some_and(|b| !b.refs("interior").is_empty());
            kenshi_world::town::building_town(towns, p, interior, pl.inside, player)
        })
        .collect()
}

/// A squad template's people: (character record, count).
fn template_people(t: &GameData) -> Vec<(String, i32)> {
    let mut out = Vec::new();
    for cat in ["leader", "squad"] {
        for (c, v) in t.refs(cat) {
            out.push((c.clone(), v.first().copied().unwrap_or(1).max(1)));
        }
    }
    out
}

/// A person's fighting strength from its character's stats: 1 + (attack + defence) / 40.
fn person_strength(data: &GameDataContainer, character: &str) -> f32 {
    let Some(c) = data.get(character) else { return 1.0 };
    let stats = c.refs("stats").first().and_then(|(s, _)| data.get(s));
    stats.map_or(1.0, |s| 1.0 + (num(s, "attack") + num(s, "defence")) / 40.0)
}

/// A squad template's strength.
pub fn template_strength(data: &GameDataContainer, template: &str) -> f32 {
    let Some(t) = data.get(template) else { return 1.0 };
    template_people(t).iter().map(|(c, n)| person_strength(data, c) * *n as f32).sum::<f32>().max(1.0)
}

impl Sim {
    /// @fn F049338
    /// @fn F037790
    ///
    /// Town `t`'s gates as Kenshi keeps them (Town +0x390, a hash map of hands, F049338): each
    /// wall gate (a GatewayBuilding, "is wall gate") of the town adds its handle as it is made
    /// (Building vtable 0x450, F037790; a building that is no gate is added by F037785 only with
    /// "is gateway", and F048824 passes it over), so in the map's order (`hand_map_order`) of
    /// the gates in the order they were made (their zones' load order, each zone's own order);
    /// each gate's first door (its door list, +0x1c8). A gate the player built has no zone
    /// handle: its placement number stands for the serial Kenshi gives it.
    pub(crate) fn town_gates(&self, t: usize) -> Vec<usize> {
        let gates: Vec<usize> = (0..self.placed.len())
            .filter(|&k| self.placed_town.get(k).copied().flatten() == Some(t))
            .filter(|&k| self.data.get(&self.placed[k].building).is_some_and(|r| r.bools.get("is wall gate").copied().unwrap_or(false)))
            .collect();
        let keys: Vec<[i32; 5]> = gates.iter().map(|&k| self.placed[k].handle.unwrap_or([0, 0, 0, 0, k as i32])).collect();
        kenshi_world::town::hand_map_order(&keys)
            .into_iter()
            .filter_map(|i| (0..self.door_states.len()).find(|&d| self.door_states[d].door.placement == gates[i] && !self.door_states[d].gone))
            .collect()
    }

    /// The towns of the placements added since the last call (`placement_towns`).
    pub(crate) fn update_placed_towns(&mut self) {
        let from = self.placed_town.len().min(self.placed.len());
        self.placed_town.truncate(from);
        let more = placement_towns(&self.data, &self.towns, &self.placed[from..], &self.placed_at[from..self.placed.len().min(self.placed_at.len())]);
        self.placed_town.extend(more);
    }

    /// A unique's state in the world's table (F039415: dead 0, imprisoned 2, else alive 1, a
    /// unique never met counting alive) and whether the player had a hand in it (F050474,
    /// `World::involved`). Imprisoned (F039250, Character +0x3d4): caged, or (assumption)
    /// carried off by one of the player's.
    pub fn unique_state(&self, c: &str) -> (i32, bool) {
        if self.world.dead_uniques.contains(c) {
            return (0, self.world.involved.contains(c));
        }
        for p in self.folk.iter().filter(|p| p.character.as_deref() == Some(c) && !p.dead) {
            if p.prison.is_some() || p.carried_by.is_some_and(|k| self.folk[k].player) {
                return (2, self.world.involved.contains(c) || self.held_by_player(p.id));
            }
        }
        (1, false)
    }

    /// Whether person `id` is in the player's hands: carried by one of the player's, or in a
    /// cage of the player's faction.
    pub(crate) fn held_by_player(&self, id: usize) -> bool {
        let p = &self.folk[id];
        let mine = |f: &str| f == crate::sim::PLAYER_FACTION || self.folk.iter().any(|q| q.player && q.faction.as_deref() == Some(f));
        p.carried_by.is_some_and(|k| self.folk[k].player) || p.prison.as_ref().is_some_and(|pr| mine(&pr.faction))
    }

    /// @fn F039250
    ///
    /// Every second, the uniques' captivity in the world's table: one taken by the player (in
    /// its hands, `held_by_player`) has the player's hand in it, kept as it is handed on (a
    /// bounty turned in to the police: "Ghost bounty caught"); one free again has it cleared.
    pub(crate) fn unique_captivity_tick(&mut self) {
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            if p.dead || p.player {
                continue;
            }
            let Some(c) = p.character.clone() else { continue };
            if !self.data.get(&c).is_some_and(|r| r.bools.get("unique").copied().unwrap_or(false)) {
                continue;
            }
            let captive = p.prison.is_some() || p.carried_by.is_some();
            if captive && self.held_by_player(id) {
                self.world.involved.insert(c);
            } else if !captive && !self.world.dead_uniques.contains(&c) {
                self.world.involved.remove(&c);
            }
        }
    }

    /// @fn F050361
    ///
    /// Whether WORLD_EVENT_STATE `state` holds now: each "NPC is" unique in the state its value
    /// gives and each "NPC is NOT" not in it (with "player involvement", a dead or imprisoned
    /// one only as the player was involved), each "player ally" / "player enemy" faction one
    /// toward the player's (F041495 / F041494 on the faction's own relations) as its value
    /// asks. "town okay" is not read (no data uses it).
    pub fn world_state(&self, state: &str) -> bool {
        let Some(s) = self.data.get(state) else { return false };
        let involvement = s.bools.get("player involvement").copied().unwrap_or(false);
        let toward_player = |f: &str| match self.relations.get(f) {
            Some(r) => (r.is_ally(Some(crate::sim::PLAYER_FACTION)), r.is_enemy(Some(crate::sim::PLAYER_FACTION))),
            None => self.data.get(f).map_or((false, false), |d| {
                let r = kenshi_ai::faction::FactionRelations::from_data(d, &self.data);
                (r.is_ally(Some(crate::sim::PLAYER_FACTION)), r.is_enemy(Some(crate::sim::PLAYER_FACTION)))
            }),
        };
        s.refs("NPC is").iter().all(|(c, v)| {
            let (st, inv) = self.unique_state(c);
            st == v[0] && !(involvement && v[0] != 1 && !inv)
        }) && s.refs("NPC is NOT").iter().all(|(c, v)| {
            let (st, inv) = self.unique_state(c);
            st != v[0] && !(involvement && v[0] == 1 && !inv)
        }) && s.refs("player ally").iter().all(|(f, v)| toward_player(f).0 == (v[0] != 0))
            && s.refs("player enemy").iter().all(|(f, v)| toward_player(f).1 == (v[0] != 0))
    }

    /// The TOWN record town `t` is now: its override (Town +0x338, "replacementTown" in a save)
    /// else the level's (Town::getGameData, F049328).
    pub fn town_record(&self, t: usize) -> String {
        self.world.towns.get(t).map(|s| s.record.clone()).filter(|r| !r.is_empty()).unwrap_or_else(|| self.towns[t].record.clone())
    }

    /// @fn F051436
    ///
    /// The override town `t` takes as its zone loads: of its current record's "override town"
    /// refs, each whose "world state" refs (F050371: by state, its value 1 or not) all hold
    /// (F050361) and that is not the current one, weighed by its number of world states (none:
    /// not taken, F038149); the heaviest (the first of a weight).
    pub(crate) fn town_override(&self, t: usize) -> Option<String> {
        let now = self.town_record(t);
        let rec = self.data.get(&now)?;
        let mut best: Option<(String, usize)> = None;
        for (o, _) in rec.refs("override town") {
            let Some(od) = self.data.get(o) else { continue };
            let mut states: Vec<(&String, bool)> = Vec::new();
            for (st, v) in od.refs("world state") {
                if self.data.get(st).is_none() {
                    continue;
                }
                let want = v.first().copied().unwrap_or(0) == 1;
                match states.iter_mut().find(|(k, _)| *k == st) {
                    Some(e) => e.1 = want,
                    None => states.push((st, want)),
                }
            }
            if states.iter().any(|(st, want)| self.world_state(st) != *want) || *o == now {
                continue;
            }
            let w = states.len();
            if w > 0 && best.as_ref().is_none_or(|(_, b)| w > *b) {
                best = Some((o.clone(), w));
            }
        }
        best.map(|(o, _)| o)
    }

    /// @fn F051428
    ///
    /// Town `t` becomes `record`: its record (+0x338), "no-foliage range", "type", "is public"
    /// and, when it names one, "faction".
    pub(crate) fn set_town_record(&mut self, t: usize, record: &str) {
        let Some(r) = self.data.get(record) else { return };
        let get = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
        let town = &mut self.towns[t];
        town.no_foliage = get("no-foliage range");
        town.kind = r.ints.get("type").copied().unwrap_or(0);
        town.public = r.bools.get("is public").copied().unwrap_or(false);
        let mut moved = false;
        if let Some((f, _)) = r.refs("faction").first() {
            moved = town.faction.as_deref() != Some(f.as_str());
            town.faction = Some(f.clone());
        }
        // F048805: a town set to another faction joins that one's war manager list
        if moved {
            self.war_town_moved(Place::Town(t));
        }
        if let Some(s) = self.world.towns.get_mut(t) {
            s.record = record.to_string();
        }
    }

    /// Every town's state from the level (its record not overridden yet), made once.
    pub(crate) fn town_states(&mut self) {
        if self.world.towns.len() == self.towns.len() {
            return;
        }
        self.world.towns = self.towns.iter().map(|t| TownState { record: t.record.clone(), id: t.id.clone() }).collect();
    }

    /// The towns of zone `z` as its content loads (ZoneMapContent's load, F051437 -> F051436):
    /// each one centred in it (TownManager's zone lists, F048847), not the player's, not
    /// started this game load (ZoneManager +0x168148: set up by F051608 or overridden here)
    /// takes its override unless a save is loading (ZoneManager +0x08, set by the save's load
    /// F031584, cleared by F051615 once it is done); then counts as started. A town overridden
    /// starts its zone afresh (F051436's caller: the zone's saved buildings of the town dropped,
    /// F051426, and its items lying about, `loot`): the people of its buildings leave and the
    /// override's move in. Returns the towns overridden.
    pub(crate) fn zone_loaded(&mut self, z: (i32, i32)) -> Vec<usize> {
        self.town_states();
        let mut changed = Vec::new();
        for t in 0..self.towns.len() {
            let c = self.towns[t].position;
            if kenshi_world::zone_of(c[0], c[2]) != z || self.towns_started.contains(&t) {
                continue;
            }
            let mine = self.towns[t].faction.as_deref().is_some_and(|f| f == crate::sim::PLAYER_FACTION || self.folk.iter().any(|p| p.player && p.faction.as_deref() == Some(f)));
            if mine {
                continue;
            }
            if !self.loading
                && let Some(o) = self.town_override(t)
            {
                if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                    eprintln!("world: {} ({}) is now {o}", self.towns[t].name, self.towns[t].id);
                }
                self.set_town_record(t, &o);
                changed.push(t);
            }
            self.towns_started.insert(t);
        }
        for &t in &changed {
            self.overridden.insert(t);
            let here = |sim: &Sim, h: usize| sim.placed_town.get(h).copied().flatten() == Some(t) && sim.placed_at.get(h).is_some_and(|a| kenshi_world::zone_of(a[0], a[2]) == z);
            if self.folk.iter().any(|p| p.home.is_some_and(|h| here(self, h)) && !p.player && !p.dead && !p.away) {
                self.reseat_town(t, z);
            }
        }
        changed
    }

    /// Zone activation (ZoneManager, F051606's timers): the camera's current zone (F051615:
    /// under its focus, chosen again once the focus is over 4147.2 units from its centre in x
    /// or z) and its eight neighbours (F051613 -> F051611, radius 1, mode 0: 4 s); every player
    /// character's zones its box of +-340 units overlaps (F039110 -> F051614 -> F051611, mode
    /// 1: 6 s), with the option "Fast zone hopping" their neighbours too. A zone stays loaded
    /// while its timer runs; each tick's zones newly loaded and those unloaded are returned.
    /// (harness) Run headless, with no camera, the first player character stands for its focus.
    pub fn zones_tick(&mut self, dt: f32) -> (Vec<(i32, i32)>, Vec<(i32, i32)>) {
        let tiles = kenshi_world::navmesh::TILES;
        let ok = |z: (i32, i32)| (0..tiles).contains(&z.0) && (0..tiles).contains(&z.1);
        for t in self.live_zones.values_mut() {
            *t -= dt;
        }
        let mut want: Vec<((i32, i32), f32)> = Vec::new();
        let hop = i32::from(self.fast_zone_hopping);
        for p in self.folk.iter().filter(|p| p.player && !p.dead) {
            let [x, _, z] = p.mover.position_units();
            let lo = kenshi_world::zone_of(x - PEOPLE_BOX, z - PEOPLE_BOX);
            let hi = kenshi_world::zone_of(x + PEOPLE_BOX, z + PEOPLE_BOX);
            for zx in lo.0 - hop..=hi.0 + hop {
                for zy in lo.1 - hop..=hi.1 + hop {
                    want.push(((zx, zy), KEEP_PEOPLE));
                }
            }
        }
        let focus = self.camera_focus.or_else(|| self.folk.iter().find(|p| p.player && !p.dead).map(|p| p.mover.position_units()));
        if let Some(f) = focus {
            let centre = |z: (i32, i32)| {
                let c = |k: i32| (k as f32 + 0.5) * kenshi_world::ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
                (c(z.0), c(z.1))
            };
            let current = match self.camera_zone {
                Some(z) if {
                    let (cx, cz) = centre(z);
                    (f[0] - cx).abs() <= RECENTRE && (f[2] - cz).abs() <= RECENTRE
                } => z,
                _ => kenshi_world::zone_of(f[0], f[2]),
            };
            self.camera_zone = Some(current);
            for zx in current.0 - 1..=current.0 + 1 {
                for zy in current.1 - 1..=current.1 + 1 {
                    want.push(((zx, zy), KEEP_CAMERA));
                }
            }
        }
        let mut came = Vec::new();
        for (z, keep) in want.into_iter().filter(|(z, _)| ok(*z)) {
            let t = self.live_zones.entry(z).or_insert_with(|| {
                came.push(z);
                keep
            });
            *t = t.max(keep);
        }
        let gone: Vec<(i32, i32)> = self.live_zones.iter().filter(|(_, t)| **t <= 0.0).map(|(k, _)| *k).collect();
        for z in &gone {
            self.live_zones.remove(z);
        }
        came.sort();
        came.dedup();
        (came, gone)
    }

    /// Whether a world squad at `p` stands where an unloaded platoon is loaded (F045434): in a
    /// loaded zone, its square grown by 10 a side and moved in by 178.5 on each side whose
    /// neighbour is not loaded (F051564 / F051565).
    fn loads_here(&self, p: [f32; 3]) -> bool {
        let z = kenshi_world::zone_of(p[0], p[2]);
        if !self.live_zones.contains_key(&z) {
            return false;
        }
        let lo = |k: i32| k as f32 * kenshi_world::ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
        let (mut x0, mut z0, mut x1, mut z1) = (lo(z.0) - ZONE_GROW, lo(z.1) - ZONE_GROW, lo(z.0 + 1) + ZONE_GROW, lo(z.1 + 1) + ZONE_GROW);
        let tiles = kenshi_world::navmesh::TILES;
        let cut = |n: (i32, i32)| (0..tiles).contains(&n.0) && (0..tiles).contains(&n.1) && !self.live_zones.contains_key(&n);
        if cut((z.0 - 1, z.1)) {
            x0 += LOAD_MARGIN;
        }
        if cut((z.0 + 1, z.1)) {
            x1 -= LOAD_MARGIN;
        }
        if cut((z.0, z.1 - 1)) {
            z0 += LOAD_MARGIN;
        }
        if cut((z.0, z.1 + 1)) {
            z1 -= LOAD_MARGIN;
        }
        x0 <= p[0] && p[0] <= x1 && z0 <= p[2] && p[2] <= z1
    }

    /// Town `t` overridden with people in its buildings of zone `z` (a game loaded: the zone's
    /// saved buildings of the town dropped, F051426): they leave (set away) and its record's own
    /// move in (an override's "residents" of each building's designation with "residents
    /// override", else the building's residents template). Bar squads are left.
    fn reseat_town(&mut self, t: usize, z: (i32, i32)) {
        let data = self.data.clone();
        let now = self.town_record(t);
        let override_rec = (now != self.towns[t].record).then(|| data.get(&now)).flatten().filter(|r| r.bools.get("residents override").copied().unwrap_or(false));
        self.update_placed_towns();
        let homes: Vec<usize> = (0..self.placed.len())
            .filter(|&k| self.placed[k].squad.is_some() && self.placed_town[k] == Some(t) && kenshi_world::zone_of(self.placed_at[k][0], self.placed_at[k][2]) == z)
            .collect();
        let (mut gone, mut came) = (0, 0);
        for k in homes {
            for p in self.folk.iter_mut().filter(|p| p.home == Some(k) && !p.player && !p.dead && !p.away && p.prison.is_none()) {
                p.away = true;
                gone += 1;
            }
            let Some(base) = self.placed[k].squad.as_deref().and_then(|s| data.get(s)) else { continue };
            let squad = match override_rec {
                Some(o) => {
                    let d = base.ints.get("building designation").copied().unwrap_or(0);
                    match o.refs("residents").iter().find(|(_, v)| v.get(1).copied().unwrap_or(0) == d).and_then(|(s, _)| data.get(s)) {
                        Some(sq) => sq,
                        None => continue,
                    }
                }
                None => base,
            };
            let pl = &self.placed[k];
            let at = [pl.position[0], pl.world_y.unwrap_or(pl.position[1]), pl.position[2]];
            came += self.spawn_resident_squad(squad, at, Some(k)).len();
        }
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: {} re-peopled as {now}: {gone} left, {came} moved in", self.towns[t].name);
        }
    }

    /// A unique character died: the world states and towns follow.
    pub fn unique_died(&mut self, character: &str) {
        if self.data.get(character).is_some_and(|r| r.bools.get("unique").copied().unwrap_or(false)) && self.world.dead_uniques.insert(character.to_string()) {
            let name = self.data.get(character).map_or(character.to_string(), |r| r.name.clone());
            self.messages.push(format!("{name} is dead."));
        }
    }

    /// The world's first tick: the walking grid, the towns' states, the patrols.
    fn world_start(&mut self) {
        let mut prof = kenshi_render::prof::Marks::start();
        self.world.started = true;
        // F041513: the player's faction knows those its record has relations with (F045557)
        if let Some(r) = self.data.get(crate::sim::PLAYER_FACTION) {
            let listed: Vec<String> = r.refs("relations").iter().map(|(f, _)| f.clone()).collect();
            self.world.known_factions.extend(listed);
        }
        match WorldGrid::build(&mut self.terrain) {
            Ok(g) => self.world.grid = Some(Arc::new(g)),
            Err(e) => eprintln!("world: no walking grid: {e}"),
        }
        prof.mark("world start: walking grid");
        // each town as the level has it (an override is taken as its zone loads, `zone_loaded`)
        self.town_states();
        prof.mark("world start: town states");
        // every town faction's relations (the arena adds them as factions turn up)
        let factions: Vec<String> = self.towns.iter().filter_map(|t| t.faction.clone()).collect();
        let data = self.data.clone();
        let all: Vec<&GameData> = data.of_type(ItemType::Faction).collect();
        for f in factions {
            if !self.relations.contains_key(&f)
                && let Some(r) = self.data.get(&f)
            {
                let rel = kenshi_ai::faction::FactionRelations::from_data_among(r, &self.data, &all);
                self.relations.insert(f, rel);
            }
        }
        prof.mark("world start: towns, relations");
        self.world.next_check = self.hours_gone() + if std::env::var("KENSHI_CAMPAIGN").is_ok() { 0.1 } else { 6.0 };
        self.world.raid_frequency = kenshi_io::cfg::load(&self.install, "settings.cfg").get("raidFrequencyMult").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0);
        self.world.raid_size = kenshi_io::cfg::load(&self.install, "settings.cfg").get("raidSizeMult").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0);
        let data = self.data.clone();
        // every faction's special squads (F030231 / F030223, from the faction's setup)
        let mut factions: Vec<&GameData> = data.of_type(ItemType::Faction).collect();
        factions.sort_by(|a, b| a.string_id.cmp(&b.string_id));
        self.world.special = factions.into_iter().map(|f| SpecialSquads::of(&data, f)).collect();
        prof.mark("world start: settings, special squads");
        eprintln!("world: {} squads abroad, {} towns", self.world.squads.len(), self.towns.len());
    }

    /// Add a squad of `template` to the world at `at` heading for its goal.
    pub fn add_world_squad(&mut self, template: &str, at: [f32; 3], goal: Goal, speed: f32) -> usize {
        let faction = self.data.get(template).and_then(|t| t.refs("faction").first().map(|(f, _)| f.clone()));
        let strength = template_strength(&self.data, template);
        // its home: the town it is made at (a campaign's origin)
        let home = (0..self.towns.len()).find(|&t| self.towns[t].position == at);
        let nest_home = if home.is_none() { (0..self.nests.len()).find(|&n| self.nests[n].at == at) } else { None };
        let mut s = WorldSquad { template: template.to_string(), faction, pos: at, route: Vec::new(), goal, speed, strength, members: Vec::new(), active: false, alive: true, order_timer: 0.0, won: None, captives: Vec::new(), announced: false, side: None, arrived: false, home, nest_home, able: true, campaign: None };
        s.route = self.route_for(&s);
        self.world.squads.push(s);
        self.world.squads.len() - 1
    }

    /// Where a squad's goal is.
    fn goal_point(&self, g: &Goal) -> [f32; 3] {
        match g {
            Goal::Patrol { to, .. } => self.towns.get(*to).map_or([0.0; 3], |t| t.position),
            Goal::Raid { town } => self.towns.get(*town).map_or([0.0; 3], |t| t.position),
            Goal::Campaign { target, .. } => *target,
            Goal::Home { to } => *to,
        }
    }

    /// A squad's long-distance route to its goal (straight when there is no grid or way).
    /// (F040645 / F040644, `kenshi_world::roads`: over 10000 units away, the roads when they pay
    /// at its faction's "road preference"; none, no roads, as "No Faction".)
    fn route_for(&self, s: &WorldSquad) -> Vec<[f32; 3]> {
        let to = self.goal_point(&s.goal);
        let preference = s.faction.as_deref().and_then(|f| self.data.get(f)).and_then(|f| f.floats.get("road preference").copied()).unwrap_or(0.0);
        self.world.grid.as_ref().map(|g| g.route_with(s.pos, to, preference)).unwrap_or_else(|| vec![to])
    }

    /// The player's base: the middle of their confirmed buildings.
    pub fn player_base(&self) -> Option<[f32; 3]> {
        let b: Vec<[f32; 3]> = self.buildings.iter().filter(|b| b.confirmed).map(|b| b.at).collect();
        if b.is_empty() {
            return None;
        }
        let n = b.len() as f32;
        Some([b.iter().map(|p| p[0]).sum::<f32>() / n, b.iter().map(|p| p[1]).sum::<f32>() / n, b.iter().map(|p| p[2]).sum::<f32>() / n])
    }

    /// @fn F048883
    ///
    /// The towns about the player's characters: within 400 units explored (and found); else found
    /// within 17000 units of a town (TOWN_TOWN), 8500 of anything else, half that for a nest or
    /// ruins, with "Discovered {1}" for all but nests; nest markers and secret towns ("is
    /// secret") are not found this way.
    fn discover_towns(&mut self) {
        let at: Vec<[f32; 3]> = self.folk.iter().filter(|p| p.player && !p.dead && !p.away).map(|p| p.mover.position_units()).collect();
        let mut found = Vec::new();
        for t in &self.towns {
            if t.kind == 8 || self.data.get(&t.record).is_some_and(|r| r.bools.get("is secret").copied().unwrap_or(false)) {
                continue;
            }
            let near = |r: f32| at.iter().any(|p| (p[0] - t.position[0]).powi(2) + (p[2] - t.position[2]).powi(2) < r * r);
            if !self.world.explored.contains(&t.id) && near(400.0) {
                self.world.explored.insert(t.id.clone());
                self.world.discovered.insert(t.id.clone());
                // its faction known (F045557)
                if let Some(f) = &t.faction {
                    self.world.known_factions.insert(f.clone());
                }
            }
            if self.world.discovered.contains(&t.id) {
                continue;
            }
            let mut r = if t.kind == 2 { 17000.0 } else { 8500.0 };
            if t.kind == 0 || t.kind == 4 {
                r *= 0.5;
            }
            if near(r) {
                self.world.discovered.insert(t.id.clone());
                if t.kind != 0 {
                    found.push(t.name.clone());
                }
            }
        }
        for name in found {
            self.messages.push(format!("Discovered {name}"));
        }
    }

    pub fn world_tick(&mut self, dt: f32) {
        if !self.world.started {
            self.world_start();
            // a loaded world's squads, towns and relations as they were saved
            if let Some(w) = self.pending_world.take() {
                self.apply_saved_world(w);
            }
        }
        for c in std::mem::take(&mut self.world.pending_deaths) {
            self.unique_died(&c);
        }
        self.world.timer += dt;
        if self.world.timer < 1.0 {
            return;
        }
        let dt = std::mem::take(&mut self.world.timer);
        // the factions' special squads (F030219, each faction's tick, F041585)
        for s in &mut self.world.special {
            s.tick(dt);
        }
        self.discover_towns();
        self.campaigns_due();
        self.campaign_phases();
        let now = self.hours_gone();
        // the 6-hourly checks: the factions' territorial campaigns
        if now >= self.world.next_check {
            self.world.next_check = now + 6.0;
            self.campaign_check();
            // (testing) a forced campaign is tried again until it starts
            if std::env::var("KENSHI_CAMPAIGN").is_ok() && self.world.campaigns_run.is_empty() {
                self.world.next_check = now + 0.1;
            }
        }
        for k in 0..self.world.squads.len() {
            if !self.world.squads[k].alive {
                continue;
            }
            // (an older save's patrols and raids: gone)
            if matches!(self.world.squads[k].goal, Goal::Patrol { .. } | Goal::Raid { .. }) {
                self.legacy_gone(k);
                continue;
            }
            if self.world.squads[k].active {
                self.real_squad(k, dt);
                // its zone unloaded: it goes away (its zone's content saved, an unloaded
                // platoon walking on)
                let s = &self.world.squads[k];
                if s.alive && s.active && !self.live_zones.contains_key(&kenshi_world::zone_of(s.pos[0], s.pos[2])) {
                    self.send_away(k);
                }
            } else {
                self.away_squad(k, dt);
                let s = &self.world.squads[k];
                // (the navmesh there in: the zone's content loaded)
                let loaded = self.nav.closest_point(s.pos.map(|v| v * kenshi_world::navmesh::METRES), 200.0 * kenshi_world::navmesh::METRES).is_some();
                if s.alive && self.loads_here(s.pos) && loaded {
                    self.make_real(k);
                }
            }
        }
    }

    /// A squad of an older save's goal Kenshi has not (a town's patrol abroad, a town raid):
    /// gone, its people with it.
    fn legacy_gone(&mut self, k: usize) {
        self.world.squads[k].alive = false;
        self.world.squads[k].active = false;
        for m in self.world.squads[k].members.clone() {
            let p = &mut self.folk[m];
            p.away = true;
            p.world_squad = None;
        }
    }

    /// An away squad walks its route; arriving, its goal.
    fn away_squad(&mut self, k: usize, dt: f32) {
        let s = &mut self.world.squads[k];
        // F045434: the cap 90 while the target is over 25000 off, else the squad's own speed;
        // the mover the lower of that and its own 70 (F040640)
        let goal_at = s.route.last().copied().unwrap_or(s.pos);
        let far = (goal_at[0] - s.pos[0]).hypot(goal_at[2] - s.pos[2]) > FAR_TARGET;
        let cap = if far { s.speed.max(SPEED_FAR * s.speed / SPEED) } else { s.speed };
        let mut step = cap.min(OWN_SPEED) * dt;
        while step > 0.0 {
            let Some(&w) = s.route.first() else { break };
            let (dx, dz) = (w[0] - s.pos[0], w[2] - s.pos[2]);
            let d = dx.hypot(dz);
            if d <= step {
                s.pos = w;
                s.route.remove(0);
                step -= d;
            } else {
                s.pos = [s.pos[0] + dx / d * step, w[1], s.pos[2] + dz / d * step];
                step = 0.0;
            }
        }
        if self.world.squads[k].route.is_empty() {
            self.arrive(k, dt);
        }
    }

    /// A squad at its goal.
    fn arrive(&mut self, k: usize, dt: f32) {
        let goal = self.world.squads[k].goal.clone();
        match goal {
            // (an older save's)
            Goal::Patrol { .. } | Goal::Raid { .. } => self.legacy_gone(k),
            // how long it stays and what ends it are its phase's (`campaign_phases`)
            Goal::Campaign { campaign, target, there, hostile } => {
                let there = there + dt / self.hour_secs as f32;
                self.world.squads[k].goal = Goal::Campaign { campaign, target, there, hostile };
            }
            Goal::Home { to } => {
                if !self.world.squads[k].active {
                    self.captives_home(k, to);
                    self.world.squads[k].alive = false;
                    // a town's own people (a special leader's squad) back in their homes
                    for m in self.world.squads[k].members.clone() {
                        if let Some(h) = self.folk[m].home.filter(|_| !self.folk[m].dead) {
                            let at = self.placed_at[h];
                            let p = &mut self.folk[m];
                            p.away = false;
                            p.world_squad = None;
                            p.mover.position = at.map(|v| v * 0.1);
                            self.arena.f[m].mover.position = self.folk[m].mover.position;
                        }
                    }
                } else if self.world.squads[k].route.is_empty() {
                    // home on foot: a catch with no cage of theirs to go to is left here
                    self.captives_home(k, to);
                }
            }
        }
    }

    /// @fn F031330
    ///
    /// Task_MakeAnnouncement, for campaign `cid`'s first squad `k` once there (or on its way,
    /// "can talk before arrival"): with a town target, "{1} has arrived at {2}" (the campaign's
    /// own "display name", F050926; the town), once; then, with the player's people within 2500 units (F031319),
    /// its `leader` (at `at`) makes its announcement (F040927, `Sim::announce_run`: its
    /// package's EV_ANNOUNCEMENT dialogues with the campaign's "dialog announcement", F050808,
    /// said to no one, line by line; the player answers a demand by talking to it, the line's
    /// "interrupt").
    fn announce(&mut self, k: usize, cid: &str, leader: usize, at: [f32; 3]) {
        let Some(c) = self.data.get(cid).cloned() else { return };
        let early = campaign_bool(&self.data, &c, "can talk before arrival");
        let there = self.campaign_there(k);
        if !there && !early {
            return;
        }
        let first = |s: &WorldSquad| matches!(&s.goal, Goal::Campaign { campaign, .. } if campaign == cid);
        if there && !self.world.squads.iter().any(|s| first(s) && s.arrived) {
            for s in self.world.squads.iter_mut().filter(|s| first(s)) {
                s.arrived = true;
            }
            if let Some(CampaignTarget::Outpost(_)) = self.world.phases.get(cid).map(|p| p.target) {
                let shown = c.strings.get("display name").cloned().unwrap_or_default();
                self.messages.push(format!("{shown} has arrived at {}", self.outpost_name));
            }
        }
        let near = self.folk.iter().filter(|p| p.player && !p.down && !p.dead).map(|p| {
            let q = p.mover.position_units();
            (p.id, (q[0] - at[0]).hypot(q[2] - at[2]))
        });
        let Some((player, d)) = near.min_by(|a, b| a.1.total_cmp(&b.1)) else { return };
        if d > 2500.0 {
            return;
        }
        // the campaign's other squads say nothing more
        for s in self.world.squads.iter_mut().filter(|s| first(s)) {
            s.announced = true;
        }
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: squad {k}'s {leader} makes {cid}'s announcement to {player}");
        }
        // F040927: EV_ANNOUNCEMENT to no one, the campaign's "dialog announcement" among its
        // dialogues (crate::conversation)
        let dialog = campaign_refs(&self.data, &c, "dialog announcement").into_iter().next().map(|(d, _)| d);
        self.announce_run(leader, dialog.as_deref());
    }

    /// "liberate captives" (276): squad `k`'s faction's people caged by the player, each let out
    /// by one of `standing` not fighting nor already at it (`order_free`).
    fn liberate(&mut self, k: usize, standing: &[usize]) {
        let Some(faction) = self.world.squads[k].faction.clone() else { return };
        let mine = self.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone()).unwrap_or_else(|| crate::sim::PLAYER_FACTION.to_string());
        let caged: Vec<usize> = self
            .folk
            .iter()
            .filter(|p| !p.dead && p.faction.as_deref() == Some(faction.as_str()) && p.prison.as_ref().is_some_and(|c| c.faction == mine))
            .map(|p| p.id)
            .filter(|&c| !self.folk.iter().any(|q| q.freeing.as_ref().is_some_and(|f| f.0 == c)))
            .collect();
        for c in caged {
            let free = standing.iter().copied().find(|&i| !self.arena.f[i].active && self.folk[i].freeing.is_none() && self.folk[i].carrying.is_none());
            let Some(i) = free else { break };
            if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                eprintln!("world: squad {k}'s {i} goes to free {c}");
            }
            self.order_free(i, c);
        }
    }

    /// A squad going home picks up its knocked out: each one down and not carried taken up by
    /// one standing who carries no one and is not already going for someone (`order_pickup`).
    fn carry_downed(&mut self, members: &[usize], standing: &[usize]) {
        let downed: Vec<usize> = members
            .iter()
            .copied()
            .filter(|&i| self.folk[i].down && !self.folk[i].dead && self.folk[i].carried_by.is_none())
            .filter(|&i| !self.folk.iter().any(|q| q.pickup == Some(i)))
            .collect();
        for d in downed {
            let Some(c) = standing.iter().copied().find(|&i| self.folk[i].carrying.is_none() && self.folk[i].pickup.is_none() && !self.arena.f[i].active) else { break };
            if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                eprintln!("world: {c} picks up the downed {d} to carry home");
            }
            self.order_pickup(&[c], d);
        }
    }

    /// Send a squad home (its origin town or where it came from).
    pub(crate) fn go_home(&mut self, k: usize) {
        // a campaign's squad on its retreat package (crate::raid_ai)
        self.retreat_package(k);
        // slavers shipping their catch: to their depot (crate::raid)
        let depot = self.slave_depot(k);
        if let Some(t) = depot
            && std::env::var("KENSHI_WORLD_DEBUG").is_ok()
        {
            eprintln!("world: squad {k} ships its catch to {} ({:?})", self.towns[t].name, self.towns[t].faction);
        }
        let s = &self.world.squads[k];
        let home = || {
            let f = s.faction.as_deref()?;
            (0..self.towns.len()).filter(|&t| self.towns[t].faction.as_deref() == Some(f)).min_by(|&a, &b| {
                let d = |t: usize| (self.towns[t].position[0] - s.pos[0]).hypot(self.towns[t].position[2] - s.pos[2]);
                d(a).total_cmp(&d(b))
            })
        };
        // (its own home first: the town it set out from, F044526 +0x30, as its people's
        // Retreating takes them, crate::raid_ai)
        let to = depot.or(s.home).or_else(home).map_or([s.pos[0] + 15000.0, s.pos[1], s.pos[2]], |t| self.towns[t].position);
        self.world.squads[k].goal = Goal::Home { to };
        self.world.squads[k].route = self.route_for(&self.world.squads[k]);
    }

    /// Make an away squad real: its people where it is (made the first time), walking on.
    fn make_real(&mut self, k: usize) {
        let pos = self.world.squads[k].pos;
        let at = self.nav.closest_point(pos.map(|v| v * kenshi_world::navmesh::METRES), 200.0 * kenshi_world::navmesh::METRES).map_or(pos, |(_, q)| q.map(|v| v / kenshi_world::navmesh::METRES));
        if self.world.squads[k].members.is_empty() {
            let t = self.world.squads[k].template.clone();
            let Some(m) = self.spawn_template(&t, at) else {
                self.world.squads[k].alive = false;
                return;
            };
            for &i in &m {
                self.folk[i].world_squad = Some(k);
            }
            self.world.squads[k].members = m;
        } else {
            let members = self.world.squads[k].members.clone();
            for (j, &i) in members.iter().enumerate() {
                let p = &mut self.folk[i];
                p.away = false;
                let spot = [at[0] + (j as f32 % 3.0) * 15.0, at[1], at[2] + (j as f32 / 3.0).floor() * 15.0];
                p.mover.position = spot.map(|v| v * 0.1);
                self.arena.f[i].mover.position = p.mover.position;
            }
        }
        self.world.squads[k].active = true;
        self.world.squads[k].order_timer = 0.0;
        self.captives_back(k);
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: squad {k} ({}) arrives near the player at {at:?}", self.world.squads[k].template);
        }
    }

    /// A real squad goes away: its people hidden, its place their leader's.
    fn send_away(&mut self, k: usize) {
        let members = self.world.squads[k].members.clone();
        let standing: Vec<usize> = members.iter().copied().filter(|&i| !self.folk[i].dead && !self.folk[i].down).collect();
        if let Some(&l) = standing.first() {
            self.world.squads[k].pos = self.folk[l].mover.position_units();
        }
        for &i in &members {
            self.folk[i].away = true;
        }
        self.captives_away(k);
        self.world.squads[k].active = false;
        self.world.squads[k].route = self.route_for(&self.world.squads[k]);
    }

    /// A real squad: its strength from its people standing, its next waypoint ordered, its goal
    /// played out (a campaign's attack).
    fn real_squad(&mut self, k: usize, dt: f32) {
        let members = self.world.squads[k].members.clone();
        let standing: Vec<usize> = members.iter().copied().filter(|&i| !self.folk[i].dead && !self.folk[i].down).collect();
        if standing.is_empty() {
            // beaten: the rest lie where they fell, the squad no longer able (F044612); what
            // becomes of its campaign is the campaign's (`campaign_phases`)
            self.world.squads[k].alive = false;
            self.world.squads[k].active = false;
            self.world.squads[k].able = false;
            for &i in &members {
                self.folk[i].world_squad = None;
            }
            return;
        }
        let base = template_strength(&self.data, &self.world.squads[k].template);
        self.world.squads[k].strength = base * standing.len() as f32 / members.len().max(1) as f32;
        let leader = standing[0];
        let at = self.folk[leader].mover.position_units();
        self.world.squads[k].pos = at;
        let fighting = standing.iter().any(|&i| self.arena.f[i].active);
        // a campaign at its target: (stand-in: needs the campaign's "pt2 AI" package run on the
        // planner, see crate::raid) one whose assault does not fight is won on arriving
        // (F050820); a won one (but an occupation) plays out its win (`raid`)
        if let Goal::Campaign { campaign, .. } = self.world.squads[k].goal.clone()
            && self.campaign_there(k)
        {
            let won = self.world.squads[k].won;
            if won.is_none() && self.assaulting(&campaign) && self.data.get(&campaign).is_some_and(|c| crate::raid::loots_without_fighting(&self.data, c)) {
                self.campaign_won(&campaign);
                return;
            }
            if won.is_some_and(|v| v != crate::raid::Victory::Occupy) {
                self.world.squads[k].order_timer -= dt;
                self.victory_step(k, &standing, at);
                self.arrive(k, dt);
                return;
            }
        }
        // the campaign's announcement (its assembly package's "Diplomat mission", 1525): its
        // leader's "dialog announcement" to the player's nearest, once, on arriving (as it comes
        // within 1500 units when it "can talk before arrival")
        if let Goal::Campaign { campaign, .. } = self.world.squads[k].goal.clone()
            && !self.world.squads[k].announced
            && !fighting
        {
            self.announce(k, &campaign, leader, at);
        }
        // a campaign at its target in its assault (an ASSAULT_TOWN in phase 1): attack the
        // player's people near
        if let Goal::Campaign { campaign, .. } = self.world.squads[k].goal.clone()
            && self.campaign_there(k)
            && self.assaulting(&campaign)
        {
            // the assault's "liberate captives" (276, "Military Assault Town"): its faction's
            // people the player holds in a cage are let out, one freer each, by those not fighting
            self.liberate(k, &standing);
            let foe = self.folk.iter().filter(|p| p.player && !p.down && !p.dead).min_by(|a, b| {
                let d = |p: &crate::sim::Person| {
                    let q = p.mover.position_units();
                    (q[0] - at[0]).hypot(q[2] - at[2])
                };
                d(a).total_cmp(&d(b))
            });
            self.world.squads[k].order_timer -= dt;
            match foe {
                Some(f) if !fighting && self.world.squads[k].order_timer <= 0.0 => {
                    self.world.squads[k].order_timer = 10.0;
                    let f = f.id;
                    if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                        eprintln!("world: squad {k} attacks the player's {f}");
                    }
                    self.order_attack(&standing, f);
                }
                // none of the player's left standing: once won, its victory (`raid`); the win
                // itself is the phase's (`campaign_phases`)
                None if self.world.squads[k].won.is_some() => self.victory_step(k, &standing, at),
                _ => {}
            }
            self.arrive(k, dt);
            return;
        }
        if fighting {
            return;
        }
        // going home ("Retreat and refresh": "Pickup downed leader" 142, "Put cairried dude in
        // bed" 106; the cannibals' retreat carries its catch): its knocked out are carried
        if matches!(self.world.squads[k].goal, Goal::Home { .. }) {
            self.carry_downed(&members, &standing);
        }
        self.world.squads[k].order_timer -= dt;
        // the next waypoint reached
        if let Some(&w) = self.world.squads[k].route.first()
            && (w[0] - at[0]).hypot(w[2] - at[2]) < 300.0
        {
            self.world.squads[k].route.remove(0);
            self.world.squads[k].order_timer = 0.0;
        }
        if self.world.squads[k].route.is_empty() {
            self.arrive(k, dt);
            return;
        }
        // (those walking home on their Retreating goal go their own way, crate::raid_ai)
        if self.world.squads[k].order_timer <= 0.0 && !self.retreats_by_plan(k) {
            self.world.squads[k].order_timer = 5.0;
            // (assumption) toward the waypoint, at most 1500 units a leg on the loaded navmesh
            let w = self.world.squads[k].route[0];
            let (dx, dz) = (w[0] - at[0], w[2] - at[2]);
            let d = dx.hypot(dz).max(1.0);
            let leg = d.min(1500.0);
            let to = [at[0] + dx / d * leg, w[1], at[2] + dz / d * leg];
            // (those taking a catch to a cage near go their own way)
            let walkers: Vec<usize> = standing
                .iter()
                .copied()
                .filter(|&i| self.folk[i].pickup.is_none() && self.folk[i].freeing.is_none())
                .filter(|&i| {
                    let p = &self.folk[i];
                    let Some(a) = &p.arrest else { return true };
                    let f = a.to.clone().or_else(|| p.faction.clone()).unwrap_or_default();
                    self.free_cage(&f, p.mover.position_units(), a.slaver).is_none()
                })
                .collect();
            if !walkers.is_empty() {
                self.order_move_squad(&walkers, to);
            }
        }
    }

    /// @fn F050818
    ///
    /// Every 6 hours, each faction's war manager with no campaign on its way nor scheduled
    /// rolls its territorial ones (its FACTION "campaigns" with "territorial triggers", not
    /// locked by a line's "lock campaign"): each whose chance now (`campaign_chance`, F050802)
    /// beats a roll of 0..1 is a candidate, and one of the candidates is set out, picked at
    /// random by their chances, at its target finder's target from its home (`campaign_way`,
    /// F050805; one with none is no candidate).
    fn campaign_check(&mut self) {
        let data = self.data.clone();
        let test = std::env::var("KENSHI_CAMPAIGN").ok();
        for faction in data.of_type(ItemType::Faction) {
            let fid = faction.string_id.clone();
            let listed: Vec<String> = faction.refs("campaigns").iter().map(|(c, _)| c.clone()).collect();
            if listed.is_empty() {
                continue;
            }
            // (testing) KENSHI_CAMPAIGN=<name>: that one set out once, whatever its chance
            if let Some(name) = test.as_deref()
                && let Some(cid) = listed.iter().find(|c| data.get(c).is_some_and(|r| r.name == name))
            {
                if !self.world.campaigns_run.contains_key(cid) {
                    let side = data.get(cid).and_then(|c| c.refs("faction override").first().map(|(f, _)| f.clone())).unwrap_or(fid.clone());
                    // (testing) KENSHI_CAMPAIGN_AT_SQUAD: at the player's first standing
                    let way = match std::env::var("KENSHI_CAMPAIGN_AT_SQUAD") {
                        Ok(_) => self.folk.iter().find(|p| p.player && !p.dead && !p.down).map(|p| (CampaignTarget::Person(p.id), None)),
                        Err(_) => data.get(cid).and_then(|c| self.campaign_way(c, &side)),
                    };
                    match way {
                        Some((target, home)) => {
                            self.launch_campaign(cid, &side, target, home);
                        }
                        None if std::env::var("KENSHI_WORLD_DEBUG").is_ok() => {
                            let homes: Vec<(Place, [f32; 3])> = self.campaign_homes(&side).into_iter().map(|h| (h, self.place_pos(h))).collect();
                            eprintln!("world: {name}: no target (homes {homes:?}, outposts {:?})", self.outposts);
                        }
                        None => {}
                    }
                }
                continue;
            }
            let busy = self.world.squads.iter().any(|s| s.alive && matches!(s.goal, Goal::Campaign { .. }) && s.side.as_deref() == Some(fid.as_str()))
                || self.world.requests.iter().any(|r| r.side == fid);
            if busy {
                continue;
            }
            let mut candidates: Vec<(String, String, f32)> = Vec::new();
            for cid in &listed {
                let Some(c) = data.get(cid) else { continue };
                if !campaign_bool(&data, c, "territorial triggers") || self.world.campaigns_locked.contains(cid) {
                    continue;
                }
                let side = c.refs("faction override").first().map(|(f, _)| f.clone()).unwrap_or(fid.clone());
                let Some((target, _)) = self.campaign_way(c, &side) else { continue };
                let chance = self.campaign_chance(c, &side, target);
                if chance > 0.0 && self.rng.uniform() < chance {
                    candidates.push((cid.clone(), side, chance));
                }
            }
            let pick = match candidates.len() {
                0 => continue,
                1 => candidates.pop(),
                _ => {
                    let total: f32 = candidates.iter().map(|c| c.2).sum();
                    let r = self.rng.uniform() * total;
                    let mut sum = 0.0;
                    candidates.into_iter().find(|c| {
                        sum += c.2;
                        sum > r
                    })
                }
            };
            if let Some((cid, side, _)) = pick {
                if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                    eprintln!("world: -CAMPAIGN TRIGGERED: {}", data.get(&cid).map_or(cid.as_str(), |c| c.name.as_str()));
                }
                if let Some((target, home)) = data.get(&cid).and_then(|c| self.campaign_way(c, &side)) {
                    self.launch_campaign(&cid, &side, target, home);
                }
            }
        }
    }

    /// @fn F050802
    ///
    /// Territorial campaign `c`'s chance (0..1) of setting out from faction `side` at this
    /// 6-hourly check on `target`: none unless "territorial triggers" (and not locked); none
    /// while the player's people are fewer than "target population min" (F041596: all of the
    /// player's faction), nor on a town target below "target tech level >=" (its size level,
    /// Town +0x388); none within its "repeat limit" (days) of its last; none unless it may
    /// start (`campaign_can_start`); with a "range far", none without a home of the faction
    /// (`nearest_home`, F050780), none when that is farther than it from the target, and scaled
    /// down linearly from 1 at "range near" (F050480; the ranges x 10 as loaded, F050812);
    /// times the options' raid frequency unless it "ignores chance mults"; then "chance per
    /// day" x 0.25 x that.
    pub(crate) fn campaign_chance(&mut self, c: &GameData, side: &str, target: CampaignTarget) -> f32 {
        let data = &self.data.clone();
        let Some(at) = self.target_at(target) else { return 0.0 };
        if !campaign_bool(data, c, "territorial triggers") || self.world.campaigns_locked.contains(&c.string_id) {
            return 0.0;
        }
        let pop = self.folk.iter().filter(|p| p.player && !p.dead).count() as f32;
        if pop < campaign_num(data, c, "target population min") {
            return 0.0;
        }
        if matches!(target, CampaignTarget::Outpost(_)) && (self.outpost_level(at) as f32) < campaign_num(data, c, "target tech level >=") {
            return 0.0;
        }
        let now = self.hours_gone();
        let limit = campaign_num(data, c, "repeat limit") as f64 * 24.0;
        if self.world.campaign_last.get(&c.string_id).is_some_and(|&at| now <= at + limit) {
            return 0.0;
        }
        if !self.campaign_can_start(c) {
            return 0.0;
        }
        let mut scale = 1.0;
        let (near, far) = (campaign_num(data, c, "range near") * 10.0, campaign_num(data, c, "range far") * 10.0);
        if far > 0.0 {
            let Some(home) = self.nearest_home(side, at) else { return 0.0 };
            let home = self.place_pos(home);
            let d = (home[0] - at[0]).hypot(home[2] - at[2]);
            if d > far {
                return 0.0;
            }
            if d > near {
                scale = (d - far) / (near - far);
            }
        }
        if !campaign_bool(data, c, "ignores chance mults") {
            scale *= self.world.raid_frequency;
        }
        campaign_num(data, c, "chance per day") * 0.25 * scale
    }

    /// @fn F050800
    ///
    /// Whether campaign `c` may start: one of its "special leader"s alive and free (any of them:
    /// the uniques' state 1; none listed, yes) and every "world state" as its value asks (1
    /// holds, 0 does not; "AND combination", fcs.def).
    pub(crate) fn campaign_can_start(&self, c: &GameData) -> bool {
        let leaders = campaign_refs(&self.data, c, "special leader");
        let alive = |l: &str| !self.world.dead_uniques.contains(l) && !self.folk.iter().any(|p| p.character.as_deref() == Some(l) && (p.dead || p.prison.is_some()));
        (leaders.is_empty() || leaders.iter().any(|(l, _)| alive(l))) && campaign_refs(&self.data, c, "world state").iter().all(|(s, v)| self.world_state(s) == (v.first().copied().unwrap_or(1) != 0))
    }

    /// The faction whose war manager runs campaign `cid`: its squads', else its "faction
    /// override", else a faction listing it.
    fn campaign_side(&self, cid: &str) -> Option<String> {
        if let Some(s) = self.campaign_squads(cid).iter().find_map(|&k| self.world.squads[k].side.clone()) {
            return Some(s);
        }
        let c = self.data.get(cid)?;
        c.refs("faction override")
            .first()
            .map(|(f, _)| f.clone())
            .or_else(|| self.data.of_type(ItemType::Faction).find(|f| f.refs("campaigns").iter().any(|(x, _)| x == cid)).map(|f| f.string_id.clone()))
    }

    /// @fn F050876
    ///
    /// One of a list of follow-up campaigns ("trigger campaign", "victory trigger", "loss
    /// trigger": each ref's values the hours from, to, and its % chance): a single roll of
    /// 0..100 against their chances added up in turn, the first whose running total is over
    /// it; none when the roll is over them all ("absolute chance", fcs.def). The campaign and
    /// its hours.
    pub(crate) fn pick_campaign(&mut self, list: &[(String, Vec<i32>)]) -> Option<(String, f64, f64)> {
        if list.is_empty() {
            return None;
        }
        let r = self.rng.uniform() * 100.0;
        let mut sum = 0.0;
        for (c, v) in list {
            sum += v.get(2).copied().unwrap_or(100) as f32;
            if sum > r {
                return Some((c.clone(), v.first().copied().unwrap_or(0) as f64, v.get(1).copied().unwrap_or(0) as f64));
            }
        }
        None
    }

    /// @fn F050814
    ///
    /// Campaign `cid` scheduled by faction `side`'s war manager on `target` from `home`, to set
    /// out `lo` to `hi` game hours from now: none without a target. Its "faction override"'s
    /// war manager takes it instead (as it is not `again`, from no home, by `side`). Without a
    /// home (or one destroyed), the faction's nearest the target (`nearest_home`); none, not
    /// at all. Not when that war manager has it scheduled on the same target already, nor
    /// (unless `again`, param_7: the campaigns' own follow-ups and a line's) running on it; and
    /// when `by` is that war manager's own faction, not while one of its campaigns running has
    /// had a follow-up so scheduled (+0x3d), each of them now marked. "Campaign message sent".
    #[allow(clippy::too_many_arguments)]
    pub fn schedule_campaign(&mut self, cid: &str, side: &str, lo: f64, hi: f64, again: bool, target: CampaignTarget, home: Option<Place>, by: Option<&str>) {
        let Some(at) = self.target_at(target) else { return };
        let Some(c) = self.data.get(cid).cloned() else { return };
        let (mut side, mut again, mut home, mut by) = (side.to_string(), again, home, by.map(|b| b.to_string()));
        if let Some((f, _)) = c.refs("faction override").first()
            && *f != side
        {
            by = Some(side.clone());
            side = f.clone();
            again = false;
            home = None;
        }
        let home = match home.filter(|&h| !self.place_destroyed(h)) {
            Some(h) => h,
            None => match self.nearest_home(&side, at) {
                Some(h) => h,
                None => return,
            },
        };
        if self.world.requests.iter().any(|r| r.side == side && r.campaign == cid && r.target == target) {
            return;
        }
        let mine = |s: &Self, k: &String| s.campaign_side(k).as_deref() == Some(side.as_str());
        if !again && self.world.phases.iter().any(|(k, p)| k == cid && p.target == target && mine(self, k)) {
            return;
        }
        if by.as_deref() == Some(side.as_str()) {
            let mut ids: Vec<String> = self.world.phases.keys().filter(|k| mine(self, k)).cloned().collect();
            ids.sort_unstable();
            for k in ids {
                let Some(p) = self.world.phases.get_mut(&k) else { continue };
                if p.flagged {
                    return;
                }
                p.flagged = true;
            }
        }
        let due = self.hours_gone() + lo + (hi - lo) * self.rng.uniform() as f64;
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: Campaign message sent: {} (due at {due:.1} h, by {side}, on {target:?})", c.name);
        }
        let town = !matches!(target, CampaignTarget::Person(_) | CampaignTarget::None);
        self.world.requests.push(Request { campaign: cid.to_string(), side, due, target, town, home: Some(home), tries: 0 });
    }

    /// Whether a request's target stands as F050818 asks: one not the player's always (a town
    /// of the level, a character not the player's); one of the player's outposts while manned
    /// (Town vtable 0x230, F048831); one of the player's characters (F039149: standing in a
    /// WallBuilding, Character +0x640 +0x338, (not ported: needs the building a character
    /// stands on) taken as not).
    fn target_holds(&self, t: CampaignTarget) -> bool {
        match t {
            CampaignTarget::Outpost(o) => o < self.outposts.len() && self.outpost_manned(o),
            CampaignTarget::Town(n) => n < self.towns.len(),
            CampaignTarget::Person(p) => p < self.folk.len() && !self.folk[p].player,
            CampaignTarget::None => false,
        }
    }

    /// @fn F050818
    /// @fn F050797
    /// @fn F044455
    ///
    /// The scheduled campaigns now due (past their hour), each in turn. One whose target is
    /// gone, or is the player's and (a town held by hostile occupiers and the campaign peaceful,
    /// or no longer manned, Town vtable 0x230) is redirected (F050797: a town target gone, or
    /// one not manned: its target finder's new target when it has one other, "redirected to
    /// {name}", tried again next time); not redirected, it is put off 5 to 10 hours more and
    /// dropped after 10 tries. Else one that may not start (`campaign_can_start`: "campaign
    /// start failed due to world states or dead leader") is dropped, and the rest set out
    /// (F050815), dropped once they did or after 100 tries.
    fn campaigns_due(&mut self) {
        // an older save's: scheduled with no target kept, as a town target gone
        for (cid, due) in std::mem::take(&mut self.world.triggered) {
            if let Some(side) = self.world.trigger_side.get(&cid).cloned().or_else(|| self.campaign_side(&cid)) {
                self.world.requests.push(Request { campaign: cid, side, due, target: CampaignTarget::None, town: true, home: None, tries: 0 });
            }
        }
        let now = self.hours_gone();
        let data = self.data.clone();
        let mut i = 0;
        while i < self.world.requests.len() {
            let r = self.world.requests[i].clone();
            if now <= r.due {
                i += 1;
                continue;
            }
            let Some(c) = data.get(&r.campaign) else {
                self.world.requests.remove(i);
                continue;
            };
            let hostile = campaign_bool(&data, c, "is hostile");
            let gone = self.target_at(r.target).is_none();
            let occupied = matches!(r.target, CampaignTarget::Outpost(o) if self.outpost_occupied(o));
            if gone || (occupied && !hostile) || !self.target_holds(r.target) {
                // F050797
                let redirect = if gone { r.town } else { !self.target_holds(r.target) };
                let new = if redirect { self.campaign_way(c, &r.side).map(|(t, _)| t).filter(|&t| t != r.target) } else { None };
                if let Some(t) = new {
                    // ("redirected to {name}": the war manager's log, F042957, a no-op)
                    if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                        eprintln!("world: {} redirected to {}", c.name, self.target_name(t));
                    }
                    self.world.requests[i].target = t;
                    i += 1;
                    continue;
                }
                self.world.requests[i].due += 5.0 + 5.0 * self.rng.uniform() as f64;
                self.world.requests[i].tries += 1;
                if self.world.requests[i].tries > 10 {
                    self.world.requests.remove(i);
                    continue;
                }
                i += 1;
                continue;
            }
            let start = self.campaign_can_start(c);
            let mut launched = false;
            if !start {
                // (the war manager's log, F042957, a no-op)
                if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                    eprintln!("world: campaign start failed due to world states or dead leader: {}", c.name);
                }
            } else {
                launched = self.launch_campaign(&r.campaign, &r.side, r.target, r.home);
            }
            self.world.requests[i].tries += 1;
            if !start || launched || self.world.requests[i].tries > 100 {
                self.world.requests.remove(i);
                continue;
            }
            i += 1;
        }
    }

    /// @fn F048824
    ///
    /// A town's gate point `dist` out (TownBase vtable 0x290): a town of the level, out from the
    /// door of the first of its gates (`town_gates`, the gate map's order) whose building's
    /// gate code is 0, along the door's way out, on the ground; none, its position. A nest (or
    /// outpost): its position (F049285). (not ported: needs a town's gateways that are no gate,
    /// "is gateway" walls, in its gate map; `town_gates` holds the wall gates) Kenshi's way out
    /// through such a wall from the town's centre.
    fn gate_point(&mut self, p: Place, dist: f32) -> [f32; 3] {
        let Place::Town(t) = p else { return self.place_pos(p) };
        let gates = self.town_gates(t);
        if let Some(&g) = gates.iter().find(|&&g| self.building_codes.get(self.door_states[g].door.placement).copied() == Some(0)) {
            let (q, i) = (self.door_states[g].door.position, self.door_states[g].inside);
            let (dx, dz) = (q[0] - i[0], q[2] - i[2]);
            let l = dx.hypot(dz).max(1e-6);
            let (x, z) = (q[0] + dx / l * dist, q[2] + dz / l * dist);
            let y = self.terrain.height_at(x, z).unwrap_or(q[1]);
            return [x, y, z];
        }
        self.towns[t].position
    }

    /// @fn F044491
    ///
    /// The people of world squad `k` counted for its campaign's forces (Platoon +0xa0): here,
    /// those not dead; away, those it has; not yet made, its template's.
    fn squad_count(&self, k: usize) -> u32 {
        let s = &self.world.squads[k];
        if s.active {
            return s.members.iter().filter(|&&m| !self.folk[m].dead).count() as u32;
        }
        if !s.members.is_empty() {
            return s.members.len() as u32;
        }
        self.data.get(&s.template).map_or(1, |t| template_people(t).iter().map(|(_, n)| *n as u32).sum::<u32>().max(1))
    }

    /// Whether world squad `k` is free to be taken on for a campaign (its war manager's map
    /// entry empty: its last campaign let go, F050875).
    fn squad_free(&self, k: usize) -> bool {
        let s = &self.world.squads[k];
        s.alive && s.campaign.as_ref().is_none_or(|c| !self.world.phases.contains_key(c))
    }

    /// @fn F050792
    ///
    /// World squad `k` taken on for campaign `cid` (CampaignInstance::addPlatoon): on its way
    /// to the target at the campaign's speed, its faction's; its people counted in.
    fn take_on(&mut self, k: usize, cid: &str, side: &str, goal: &Goal, speed: f32) -> u32 {
        let s = &mut self.world.squads[k];
        s.goal = goal.clone();
        s.speed = speed;
        s.side = Some(side.to_string());
        s.campaign = Some(cid.to_string());
        s.won = None;
        s.announced = false;
        s.arrived = false;
        s.order_timer = 0.0;
        let route = self.route_for(&self.world.squads[k]);
        self.world.squads[k].route = route;
        // the first taken on leads (+0x80)
        if let Some(p) = self.world.phases.get_mut(cid)
            && p.lead.is_none()
        {
            p.lead = Some(k);
        }
        self.squad_count(k)
    }

    /// A new squad of `template` for campaign `cid` at `at`, homed at `home` (the platoon
    /// factory, F038271 / F038259, as an unloaded platoon of the faction there).
    #[allow(clippy::too_many_arguments)]
    fn new_campaign_squad(&mut self, template: &str, at: [f32; 3], home: Option<Place>, cid: &str, side: &str, goal: &Goal, speed: f32) -> (usize, u32) {
        let k = self.add_world_squad(template, at, goal.clone(), speed);
        self.world.squads[k].home = match home {
            Some(Place::Town(t)) => Some(t),
            _ => None,
        };
        self.world.squads[k].nest_home = match home {
            Some(Place::Nest(n)) => Some(n),
            _ => None,
        };
        let n = self.take_on(k, cid, side, goal, speed);
        (k, n)
    }

    /// @fn F050801
    /// @fn F050785
    /// @fn F049017
    /// @fn F038418
    ///
    /// Campaign `cid`'s forces gathered (CampaignInstance's platoons), `needed` the people it
    /// wants (F050803). First its special leaders in turn: one about (the unique table's,
    /// F050473: here, a character not dead) brings its own squad, which leads; one not about and
    /// free to be made (F038418) whose SQUAD_TEMPLATE it leads ("leader"; several: "ERROR:
    /// unique NPC with more than 1 squad referencing it", the first) has that made at the gate
    /// point 700 out (`gate_point`) of the first of the faction's four homes nearest the target
    /// (F049017 over its war manager's towns, nearest first), none: at a spawn point of the
    /// target's zone (F048102: 1.3, 0.001); the first that does, and no more. Then while it has
    /// no more than it needs: the faction's squads with no home that are free, able (F044612)
    /// and (when it lists "squads to use") of a template it lists; then for each of those four
    /// homes, its squads free, able and listed whose home is that town or none, and squads made
    /// there (F050785, at most 100 tries: a template by its weight, the ref's value; made where
    /// its leading squad is, else at a spawn point of the zone of the home's gate point 700 out,
    /// F048102: 0.3, 0.2). The people it has (F044491). (not ported: needs the factions'
    /// platoons as Kenshi keeps them, loaded and unloaded, Faction +0x210 and the war manager's
    /// map) Here the faction's squads are its world squads: the people of towns and the
    /// wanderers about the player are none of them.
    #[allow(clippy::too_many_arguments)]
    fn gather_forces(&mut self, c: &GameData, cid: &str, side: &str, target: [f32; 3], goal: &Goal, speed: f32, needed: u32) -> u32 {
        let data = self.data.clone();
        // F049017: the four homes nearest the target, nearest first (the first of equals)
        let mut near: Vec<Place> = self.war_towns(side);
        let d2 = |me: &Self, p: Place| {
            let q = me.place_pos(p);
            (q[0] - target[0]).powi(2) + (q[1] - target[1]).powi(2) + (q[2] - target[2]).powi(2)
        };
        let mut towns4: Vec<Place> = Vec::new();
        while towns4.len() < 4 && !near.is_empty() {
            let mut best = 0;
            for j in 1..near.len() {
                if d2(self, near[j]) < d2(self, near[best]) {
                    best = j;
                }
            }
            towns4.push(near[best]);
            let last = near.len() - 1;
            near.swap(best, last);
            near.pop();
        }
        let listed: Vec<(String, f32)> = campaign_refs(&data, c, "squads to use").into_iter().map(|(t, v)| (t, v.first().copied().unwrap_or(0) as f32)).collect();
        let fits = |me: &Self, k: usize| listed.is_empty() || listed.iter().any(|(t, _)| *t == me.world.squads[k].template);
        let mut forces = 0u32;
        // the special leaders: the first that comes
        let listing = |l: &str| {
            let all: Vec<String> = data.of_type(ItemType::SquadTemplate).filter(|t| t.refs("leader").iter().any(|(x, _)| x == l)).map(|t| t.string_id.clone()).collect();
            if all.len() > 1 {
                eprintln!("ERROR: unique NPC with more than 1 squad referencing it - {l}");
            }
            all.into_iter().next()
        };
        for (l, _) in campaign_refs(&data, c, "special leader") {
            if self.world.dead_uniques.contains(&l) {
                continue;
            }
            let here = self.folk.iter().find(|p| p.character.as_deref() == Some(l.as_str()) && !p.dead && !p.player).map(|p| p.id);
            if let Some(u) = here {
                let members = self.squad_of(u);
                if let Some(old) = self.folk[u].world_squad.and_then(|k| self.world.squads.get_mut(k)) {
                    old.members.retain(|m| !members.contains(m));
                }
                let at = self.folk[u].mover.position_units();
                let k = self.add_world_squad(&listing(&l).unwrap_or_default(), at, goal.clone(), speed);
                for &m in &members {
                    self.folk[m].world_squad = Some(k);
                }
                let away = self.folk[u].away;
                let home = self.home_town(u);
                let s = &mut self.world.squads[k];
                s.members = members.clone();
                s.active = !away;
                s.faction = self.folk[u].faction.clone();
                s.home = home;
                forces += self.take_on(k, cid, side, goal, speed);
                break;
            }
            let Some(t) = listing(&l) else { continue };
            let at = match towns4.first() {
                Some(&h) => self.gate_point(h, 700.0),
                None => {
                    let z = kenshi_world::zone_of(target[0], target[2]);
                    self.zone_spawn_point(z, true, Some(side), 1.3, 0.001).unwrap_or(target)
                }
            };
            forces += self.new_campaign_squad(&t, at, towns4.first().copied(), cid, side, goal, speed).1;
            break;
        }
        if forces > needed {
            return forces;
        }
        // the faction's squads with no home
        for k in 0..self.world.squads.len() {
            let s = &self.world.squads[k];
            if s.side.as_deref().or(s.faction.as_deref()) != Some(side) || s.home.is_some() || s.nest_home.is_some() || !self.squad_free(k) || !fits(self, k) || !self.squad_able(k) {
                continue;
            }
            forces += self.take_on(k, cid, side, goal, speed);
            if forces > needed {
                return forces;
            }
        }
        // the four homes': their squads, then squads made there
        let total: f32 = listed.iter().map(|(_, w)| *w).sum();
        for &h in &towns4 {
            for k in 0..self.world.squads.len() {
                let s = &self.world.squads[k];
                let home = s.home.map(Place::Town).or(s.nest_home.map(Place::Nest));
                if s.side.as_deref().or(s.faction.as_deref()) != Some(side) || !(home.is_none() || home == Some(h)) || !self.squad_free(k) || !fits(self, k) || !self.squad_able(k) {
                    continue;
                }
                forces += self.take_on(k, cid, side, goal, speed);
                if forces > needed {
                    return forces;
                }
            }
            if listed.is_empty() {
                continue;
            }
            for _ in 0..100 {
                let t = if listed.len() == 1 {
                    listed[0].0.clone()
                } else {
                    // F050502: a random number of 0 .. the weights' total
                    let r = self.rng.uniform() * total;
                    let mut sum = 0.0;
                    listed.iter().find(|(_, w)| {
                        sum += *w;
                        sum > r
                    }).unwrap_or(&listed[listed.len() - 1]).0.clone()
                };
                let mut at = self.gate_point(h, 700.0);
                match self.world.phases.get(cid).and_then(|p| p.lead) {
                    Some(l) => at = self.world.squads[l].pos,
                    None => {
                        let z = kenshi_world::zone_of(at[0], at[2]);
                        if let Some(p) = self.zone_spawn_point(z, true, Some(side), 0.3, 0.2) {
                            at = p;
                        }
                    }
                }
                forces += self.new_campaign_squad(&t, at, Some(h), cid, side, goal, speed).1;
                if forces > needed {
                    return forces;
                }
            }
        }
        forces
    }

    /// @fn F048805
    /// @fn F048897
    /// @fn F031580
    ///
    /// Faction `f`'s war manager's towns (FactionWarMgr +0x10), in the order they joined it:
    /// each town joins its faction's list as it is set to it (TownBase::setFaction, F048805:
    /// not a faction "not real", Faction +0x1d0; leaving its old one's), the level's towns as
    /// TownManager makes them (in the level's order, with their own records), a town whose
    /// record override names another faction as it takes it (F051428; on a load as its state is
    /// read, Town::loadState F031580, in the town list's order), a nest as it is made (on a
    /// load, after the level's towns, as the town list has them).
    pub(crate) fn war_towns(&mut self, f: &str) -> Vec<Place> {
        if self.data.get(f).is_some_and(|r| r.bools.get("not real").copied().unwrap_or(false)) {
            return Vec::new();
        }
        if !self.world.war_ready {
            self.world.war_ready = true;
            self.world.war_log.clear();
            for t in 0..self.towns.len() {
                if self.town_faction(t) != self.base_faction(t) {
                    self.world.war_log.push(Place::Town(t));
                }
            }
            for n in 0..self.nests.len() {
                self.world.war_log.push(Place::Nest(n));
            }
        }
        let mut out: Vec<Place> = (0..self.towns.len()).filter(|&t| self.base_faction(t).as_deref() == Some(f) && !self.world.war_log.contains(&Place::Town(t))).map(Place::Town).collect();
        for &p in &self.world.war_log {
            let of = match p {
                Place::Town(t) => self.town_faction(t),
                Place::Nest(n) => self.nest_faction(n),
                Place::Outpost(_) => None,
            };
            if of.as_deref() == Some(f) {
                out.push(p);
            }
        }
        out
    }

    /// A town (or nest) joins a war manager's list anew (F048805), once the lists are made.
    pub(crate) fn war_town_moved(&mut self, p: Place) {
        if self.world.war_ready {
            self.world.war_log.retain(|&q| q != p);
            self.world.war_log.push(p);
        }
    }

    /// Town `t`'s faction as the level made it (its own record's) and as it is now.
    fn base_faction(&self, t: usize) -> Option<String> {
        self.data.get(&self.towns[t].record).and_then(|r| r.refs("faction").first().map(|(f, _)| f.clone()))
    }

    fn town_faction(&self, t: usize) -> Option<String> {
        self.towns[t].faction.clone()
    }

    /// Where a town (or nest, or outpost) stands.
    fn place_pos(&self, p: Place) -> [f32; 3] {
        match p {
            Place::Town(t) => self.towns[t].position,
            Place::Nest(n) => self.nests[n].at,
            Place::Outpost(o) => self.outposts[o],
        }
    }

    /// A town's type (TOWN "type", +0xd8): a nest's record's.
    fn place_kind(&self, p: Place) -> i32 {
        match p {
            Place::Town(t) => self.towns[t].kind,
            Place::Nest(n) => self.data.get(&self.nests[n].record).and_then(|r| r.ints.get("type").copied()).unwrap_or(0),
            Place::Outpost(_) => 1,
        }
    }

    /// Town vtable 0x288: destroyed (its population manager dead, F048779: a nest wiped out).
    fn place_destroyed(&self, p: Place) -> bool {
        matches!(p, Place::Nest(n) if self.nests[n].wiped)
    }

    /// @fn F050780
    /// @fn F050849
    ///
    /// Faction `f`'s home nearest `at` (its war manager's towns, `war_towns`, the first of the
    /// nearest; one destroyed is passed over).
    pub(crate) fn nearest_home(&mut self, f: &str, at: [f32; 3]) -> Option<Place> {
        let homes = self.war_towns(f);
        let mut best = None;
        let mut d = f32::MAX;
        for p in homes {
            if self.place_destroyed(p) {
                continue;
            }
            let q = self.place_pos(p);
            let e = (q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2) + (q[2] - at[2]).powi(2);
            if e < d {
                d = e;
                best = Some(p);
            }
        }
        best
    }

    /// @fn F050781
    ///
    /// The homes faction `f` sets its campaigns out from: its war manager's towns (`war_towns`)
    /// but nest markers (type 8), nor, for a faction of "fundamental type" 1 or that "offers
    /// bounties" (Faction +0x40), villages (3), slave camps (5), prisons (7) or points of
    /// interest (9); none destroyed.
    fn campaign_homes(&mut self, f: &str) -> Vec<Place> {
        let lawful = self.data.get(f).is_some_and(|r| r.ints.get("fundamental type").copied() == Some(1) || r.bools.get("offers bounties").copied().unwrap_or(false));
        self.war_towns(f)
            .into_iter()
            .filter(|&p| {
                let k = self.place_kind(p);
                k != 8 && (!lawful || ![3, 9, 7, 5].contains(&k)) && !self.place_destroyed(p)
            })
            .collect()
    }

    /// @fn F048831
    ///
    /// Town vtable 0x230 of one of the player's outposts: one of the player's people in it (a
    /// player's town with its zones loaded asks whether any of the player's squads' people
    /// stand in it; the player's people's zones are always loaded).
    fn outpost_manned(&self, o: usize) -> bool {
        self.folk.iter().any(|p| p.player && !p.dead && !p.away && self.place_at(p.mover.position_units()) == Some(Place::Outpost(o)))
    }

    /// @fn F050805
    /// @fn F049017
    ///
    /// Where campaign `c` of faction `side` marches, and from where (its target finder, data
    /// vtable 0): none while the faction's war manager has no towns. One that "target
    /// characters" and is not territorial: the first of its "specific target NPC"s about (the
    /// unique's state not dead, its place known: one here), its home left to `nearest_home`.
    /// Else of the player's outposts those in the faction's territory (its "biomes"), else all
    /// of them; those without one of the player's people in them (`outpost_manned`) left out,
    /// one held by hostile occupiers left to hostile campaigns, and unless it "ignores nogo
    /// zones", those in its "no-go zones"; then for each of its homes in turn
    /// (`campaign_homes`), the nearest of the outposts left (of the five nearest, the first not
    /// destroyed) within its "range far" (x 10 as loaded, F050812) is the target, from that
    /// home. (not ported: needs the world's unloaded characters, UnloadedPlatoon) A specific
    /// target NPC away in an unloaded zone, which Kenshi still finds.
    fn campaign_way(&mut self, c: &GameData, side: &str) -> Option<(CampaignTarget, Option<Place>)> {
        let data = self.data.clone();
        if self.war_towns(side).is_empty() {
            return None;
        }
        if campaign_bool(&data, c, "target characters") && !campaign_bool(&data, c, "territorial triggers") {
            for (npc, _) in campaign_refs(&data, c, "specific target NPC") {
                if self.world.dead_uniques.contains(&npc) {
                    continue;
                }
                if let Some(p) = self.folk.iter().find(|p| p.character.as_deref() == Some(npc.as_str()) && !p.dead) {
                    return Some((CampaignTarget::Person(p.id), None));
                }
            }
            return None;
        }
        let faction = data.get(side);
        let zone_in = |me: &Self, cat: &str, p: [f32; 3]| {
            let b = me.biome_at(p[0], p[2]).map(|b| b.string_id.as_str());
            faction.is_some_and(|f| f.refs(cat).iter().any(|(x, _)| Some(x.as_str()) == b))
        };
        let mut outposts: Vec<usize> = (0..self.outposts.len()).filter(|&o| zone_in(self, "biomes", self.outposts[o])).collect();
        if outposts.is_empty() {
            outposts = (0..self.outposts.len()).collect();
        }
        let hostile = campaign_bool(&data, c, "is hostile");
        let nogo = !campaign_bool(&data, c, "ignores nogo zones");
        outposts.retain(|&o| self.outpost_manned(o) && (hostile || !self.outpost_occupied(o)) && !(nogo && zone_in(self, "no-go zones", self.outposts[o])));
        if outposts.is_empty() {
            return None;
        }
        let far = campaign_num(&data, c, "range far") * 10.0;
        for h in self.campaign_homes(side) {
            let at = self.place_pos(h);
            let d2 = |q: [f32; 3]| (q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2) + (q[2] - at[2]).powi(2);
            // F049017: the nearest first (the first of equals)
            let mut best: Option<usize> = None;
            for &o in &outposts {
                if best.is_none_or(|b| d2(self.outposts[o]) < d2(self.outposts[b])) {
                    best = Some(o);
                }
            }
            if let Some(o) = best {
                if d2(self.outposts[o]) <= far * far {
                    return Some((CampaignTarget::Outpost(o), Some(h)));
                }
            }
        }
        None
    }

    /// @fn F048905
    /// @fn F048906
    /// @fn F048907
    ///
    /// Town::setupResidents / distributeResidents for the buildings of the zone just loaded
    /// (placements from `from` on): a town whose buildings here have no resident squad of its
    /// own in the zone state (Kenshi hands its residents out as it sets the town up, its rows at Town
    /// +0x300) gets them now. Its rows (F048906, made once for the town and kept,
    /// `World::resident_rows`: a town over several zones hands out what is left of them in
    /// each, and one whose zone state holds its residents has none left): its faction's
    /// "residents" then the town's (as it is now; only the town's with "residents override"),
    /// each once, its number (val0) and priority (val1). By priority, 5 down to 0 (a higher one
    /// never: as the game), each row takes one building at a time (`resident_building`) until
    /// its number is filled or no building is left for it; the buildings left the "default
    /// resident" (the town's, else, without the override, its faction's). Each building's
    /// squad is spawned at it.
    /// The town's buildings: the BUILDING placements of this zone whose town it is (F038257,
    /// `placement_towns`: the Spider Factory's Ancient Factory stands outside its radius, in
    /// the next zone), (assumption) not destroyed, nor a feature, foliage, node, furniture,
    /// sign or gate; a resident whose leader is a unique already about (or dead) is not made
    /// again (a town over two zones).
    pub(crate) fn setup_residents(&mut self, from: usize) {
        if self.quiet_zones || from >= self.placed.len() {
            return;
        }
        let data = self.data.clone();
        if self.layouts.is_none() {
            self.layouts = Some(kenshi_world::nodes::Layouts::load(&self.install.clone()));
        }
        self.update_placed_towns();
        let mut towns: Vec<usize> = Vec::new();
        for i in from..self.placed.len() {
            if let Some(t) = self.placed_town[i]
                && !towns.contains(&t)
            {
                towns.push(t);
            }
        }
        for t in towns {
            let homes: Vec<usize> = (from..self.placed.len()).filter(|&i| self.placed_town[i] == Some(t)).collect();
            if homes.is_empty() {
                continue;
            }
            let Some(rec) = data.get(&self.town_record(t)) else { continue };
            // (assumption) the zone state's squads count only when they are the town's own (its
            // or its faction's residents, default resident or bar squads, the base town's or
            // the override's): one left from another town's (the Ark's factory with Brink's
            // whelps) has the town set up again
            let own: HashSet<String> = [Some(rec), data.get(&self.towns[t].record), self.towns[t].faction.as_deref().and_then(|f| data.get(f))]
                .into_iter()
                .flatten()
                .flat_map(|r| ["residents", "default resident", "bar squads"].into_iter().flat_map(move |k| r.refs(k).iter().map(|(s, _)| s.clone())))
                .collect();
            let set = |i: usize| [&self.placed[i].squad, &self.placed[i].residents].into_iter().flatten().any(|s| own.contains(s));
            if homes.iter().any(|&i| set(i)) {
                // (assumption) its residents handed out already: none left for its other zones
                if self.world.resident_rows.get(&t).is_none_or(|r| r.0 != rec.string_id) {
                    self.world.resident_rows.insert(t, (rec.string_id.clone(), Vec::new()));
                }
                continue;
            }
            let overrides = rec.bools.get("residents override").copied().unwrap_or(false);
            let faction = self.towns[t].faction.as_deref().and_then(|f| data.get(f)).filter(|_| !overrides);
            // F048906: the rows made once for the town (its done flag, Town +0x110), their
            // numbers going down as they are handed out over the zones it spans; made again
            // for an override town
            let mut rows: Vec<(String, i32, i32)> = match self.world.resident_rows.get(&t) {
                Some((r, left)) if *r == rec.string_id => left.clone(),
                _ => {
                    let mut rows = Vec::new();
                    for r in faction.into_iter().chain(std::iter::once(rec)) {
                        for (sq, v) in r.refs("residents") {
                            if !rows.iter().any(|x: &(String, i32, i32)| x.0 == *sq) {
                                rows.push((sq.clone(), v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0)));
                            }
                        }
                    }
                    rows
                }
            };
            let filler = rec.refs("default resident").first().or_else(|| faction.and_then(|f| f.refs("default resident").first())).map(|(s, _)| s.clone());
            let building = |i: usize| {
                data.get(&self.placed[i].building).is_some_and(|b| {
                    b.item_type() == Some(ItemType::Building)
                        && ["is feature", "is foliage", "is node", "is exterior furniture", "is interior furniture", "is sign", "is wall gate", "is gateway"].iter().all(|k| !b.bools.get(*k).copied().unwrap_or(false))
                })
            };
            let mut free: Vec<usize> = homes.iter().copied().filter(|&i| building(i) && !self.placed[i].destroyed).collect();
            if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                eprintln!("world: {} sets up its residents: {} buildings of {} placements, rows {rows:?}, filler {filler:?}", self.towns[t].name, free.len(), homes.len());
            }
            let mut given: Vec<(usize, String)> = Vec::new();
            let mut level = 5;
            while level >= 0 {
                let mut again = false;
                for r in rows.iter_mut().filter(|r| r.1 > 0 && r.2 == level) {
                    let Some(tpl) = data.get(&r.0) else { continue };
                    let Some(b) = self.resident_building(tpl, &free) else { continue };
                    free.retain(|&x| x != b);
                    given.push((b, r.0.clone()));
                    r.1 -= 1;
                    again |= r.1 != 0;
                }
                if !again {
                    level -= 1;
                }
            }
            self.world.resident_rows.insert(t, (rec.string_id.clone(), rows));
            // F048907's last pass: the default resident to each building left with no resident
            // template of its own (+0xf0 empty: none from the zone state either)
            if let Some(f) = filler {
                given.extend(free.iter().filter(|&&b| self.placed[b].squad.is_none() && self.placed[b].residents.is_none()).map(|&b| (b, f.clone())));
            }
            for (b, sq) in given {
                let Some(tpl) = data.get(&sq) else { continue };
                let unique_about = tpl.refs("leader").iter().any(|(c, _)| {
                    data.get(c).is_some_and(|c| c.bools.get("unique").copied().unwrap_or(false))
                        && (self.world.dead_uniques.contains(c) || self.folk.iter().any(|p| p.character.as_deref() == Some(c.as_str())))
                });
                if unique_about {
                    continue;
                }
                self.placed[b].squad = Some(sq.clone());
                self.placed[b].residents = Some(sq.clone());
                let pl = &self.placed[b];
                let at = [pl.position[0], pl.world_y.unwrap_or(pl.position[1]), pl.position[2]];
                if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                    eprintln!("world: {} gives {} the {}", self.towns[t].name, data.get(&pl.building).map_or("?", |r| r.name.as_str()), tpl.name);
                }
                self.spawn_resident_squad(tpl, at, Some(b));
            }
        }
    }

    /// @fn F048877
    ///
    /// The building of `free` (placements) a town gives resident squad `tpl`: none its "layout
    /// interior" does not fit (the building has no such layout, `Layouts::for_building`);
    /// with a "building" and no "layout interior", only those; each weighs 1, 0.4 one its
    /// "layout exterior" does not fit, x10 one of its "building", x0.05 one of its "building
    /// dislike"; one picked at random by weight. (not ported) A building already furnished
    /// (taken only by a resident with a "building").
    fn resident_building(&mut self, tpl: &GameData, free: &[usize]) -> Option<usize> {
        let liked: Vec<&String> = tpl.refs("building").iter().map(|(b, _)| b).collect();
        let disliked: Vec<&String> = tpl.refs("building dislike").iter().map(|(b, _)| b).collect();
        let layout = |k: &str| tpl.strings.get(k).filter(|l| !l.is_empty()).cloned();
        let (inner, outer) = (layout("layout interior"), layout("layout exterior"));
        let only_liked = !liked.is_empty() && inner.is_none();
        let data = self.data.clone();
        let fits = |b: &str, l: &Option<String>| l.as_deref().is_none_or(|l| self.layouts.as_ref().is_some_and(|ls| !ls.for_building(&data, b, l).is_empty()));
        let weights: Vec<(usize, f32)> = free
            .iter()
            .filter_map(|&b| {
                let rec = &self.placed[b].building;
                if (only_liked && !liked.contains(&rec)) || !fits(rec, &inner) {
                    return None;
                }
                let mut w = if fits(rec, &outer) { 1.0 } else { 0.4 };
                if liked.contains(&rec) {
                    w *= 10.0;
                }
                if disliked.contains(&rec) {
                    w *= 0.05;
                }
                Some((b, w))
            })
            .collect();
        let total: f32 = weights.iter().map(|x| x.1).sum();
        if total <= 0.0 {
            return None;
        }
        let r = self.rng.uniform() * total;
        let mut sum = 0.0;
        weights.iter().find(|x| {
            sum += x.1;
            sum > r
        }).or(weights.last()).map(|x| x.0)
    }

    /// Whether campaign squad `k` stands at its target for its own doings (its attack, its
    /// leader's announcement): its way walked, or within F050809's distances of the target
    /// (`near_target`). The campaign's phase is its leading squad's (`campaign_phases`).
    pub(crate) fn campaign_there(&self, k: usize) -> bool {
        let s = &self.world.squads[k];
        let Goal::Campaign { campaign, target, .. } = &s.goal else { return s.route.is_empty() };
        let t = self.world.phases.get(campaign).map(|p| p.target).unwrap_or_default();
        s.route.is_empty() || self.near_target(s.pos, t, Some(*target), ARRIVED_AT_TOWN)
    }

    /// Where a campaign's target is: its outpost's centre, its character's place.
    fn target_at(&self, t: CampaignTarget) -> Option<[f32; 3]> {
        match t {
            CampaignTarget::Outpost(o) => self.outposts.get(o).copied(),
            CampaignTarget::Person(p) => self.folk.get(p).map(|q| q.mover.position_units()),
            CampaignTarget::Town(t) => self.towns.get(t).map(|t| t.position),
            CampaignTarget::None => None,
        }
    }

    /// F050809's arrival test of a squad at `at` (its position, Platoon vtable 0x40): within
    /// 400 units of a character target (the squared distance, F050929 the target's place), within
    /// `mult` of a town target's radius (Town::contains, F048764: the player's town of type 1,
    /// 1250 units; `fallback` stands for a target not kept, an older save's).
    fn near_target(&self, at: [f32; 3], t: CampaignTarget, fallback: Option<[f32; 3]>, mult: f32) -> bool {
        let d2 = |q: [f32; 3]| (q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2) + (q[2] - at[2]).powi(2);
        match t {
            CampaignTarget::Person(_) => self.target_at(t).is_some_and(|q| d2(q) <= ARRIVED_AT_CHARACTER * ARRIVED_AT_CHARACTER),
            CampaignTarget::Outpost(_) => self.target_at(t).is_some_and(|q| (q[0] - at[0]).hypot(q[2] - at[2]) < kenshi_world::town::radius(1, 1.0) * mult),
            CampaignTarget::Town(n) => self.towns.get(n).is_some_and(|town| town.contains(at, mult)),
            CampaignTarget::None => fallback.is_some_and(|q| (q[0] - at[0]).hypot(q[2] - at[2]) < kenshi_world::town::radius(1, 1.0) * mult),
        }
    }

    /// Whether campaign `cid` is assaulting: an ASSAULT_TOWN campaign (its "key" 0) in its
    /// assault phase.
    fn assaulting(&self, cid: &str) -> bool {
        let key = self.data.get(cid).map_or(0.0, |c| campaign_num(&self.data, c, "key"));
        key == 0.0 && self.world.phases.get(cid).is_some_and(|p| p.phase == PHASE_ASSAULT)
    }

    /// Campaign `cid` to phase `phase` (a line's DA_ASSAULT_PHASE / DA_RETREAT_PHASE /
    /// DA_VICTORY_PHASE, F040912 cases 0x25..0x27, and the arrival): the assault (1) when the
    /// campaign has one (its "pt2 AI leader", CampaignData +0x110), else over (2); both clocks
    /// restart. Over, its squads go home; victory (3), its squads play out their win (`raid`).
    pub fn set_campaign_phase(&mut self, cid: &str, phase: u8) {
        let Some(c) = self.data.get(cid).cloned() else { return };
        let phase = if phase == PHASE_ASSAULT && campaign_refs(&self.data, &c, "pt2 AI leader").is_empty() { PHASE_OVER } else { phase };
        let now = self.hours_gone();
        let was = self.world.phases.get(cid).copied();
        self.world.phases.insert(cid.to_string(), Phase::at(phase, now, was));
        let ks: Vec<usize> = (0..self.world.squads.len()).filter(|&k| self.world.squads[k].alive && matches!(&self.world.squads[k].goal, Goal::Campaign { campaign, .. } if campaign == cid)).collect();
        match phase {
            PHASE_OVER => {
                for k in ks {
                    self.go_home(k);
                }
            }
            PHASE_VICTORY => {
                let victory = crate::raid::victory_of(&self.data, &c);
                for k in ks {
                    self.begin_victory(k, victory);
                }
            }
            _ => {}
        }
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: campaign {} phase {phase}", c.name);
        }
    }

    /// The campaign squad `k` of person `id` is on, if any.
    pub(crate) fn campaign_of(&self, id: usize) -> Option<String> {
        let k = self.folk[id].world_squad?;
        match &self.world.squads.get(k)?.goal {
            Goal::Campaign { campaign, .. } => Some(campaign.clone()),
            _ => None,
        }
    }

    /// The squads of campaign `cid` (CampaignInstance +0xc8: every squad it took on, beaten or
    /// not).
    fn campaign_squads(&self, cid: &str) -> Vec<usize> {
        (0..self.world.squads.len()).filter(|&k| matches!(&self.world.squads[k].goal, Goal::Campaign { campaign, .. } if campaign == cid)).collect()
    }

    /// @fn F044565
    /// @fn F044487
    ///
    /// Squad `k`'s leader: while it is here (its active platoon), its leader while up and free,
    /// else the first of it up and free; away, its first (the unloaded platoon's).
    fn squad_leader(&self, k: usize) -> Option<usize> {
        let s = self.world.squads.get(k)?;
        if !s.active {
            return s.members.first().copied();
        }
        let free = |m: usize| {
            let q = &self.folk[m];
            !q.dead && !q.down && !q.away && q.prison.is_none() && q.bed.is_none() && q.slave.is_none()
        };
        s.members.iter().copied().filter(|&m| free(m)).find(|&m| self.folk[m].role == 5).or_else(|| s.members.iter().copied().find(|&m| free(m)))
    }

    /// @fn F050250
    /// @fn F039191
    ///
    /// Whether squad `k` is all out of action (Platoon vtable 0x30): here (its active platoon)
    /// with people, each of them dead, down or imprisoned (Character vtable 0x30: +0x5bc,
    /// +0x3d4).
    fn squad_out(&self, k: usize) -> bool {
        let s = &self.world.squads[k];
        // (a beaten squad was here as it fell)
        (s.active || !s.alive) && !s.members.is_empty() && s.members.iter().all(|&m| self.folk[m].dead || self.folk[m].down || self.folk[m].prison.is_some())
    }

    /// @fn F044612
    /// @fn F044564
    ///
    /// Whether squad `k` can still fight: worked out again while it is here, its last answer
    /// while away (`WorldSquad::able`).
    fn squad_able(&mut self, k: usize) -> bool {
        if self.world.squads[k].active {
            let members = self.world.squads[k].members.clone();
            let able = self.platoon_able(&members);
            self.world.squads[k].able = able;
        }
        self.world.squads[k].able
    }

    /// The town a squad at `at` stands in (Platoon vtable 0x78): one of the player's outposts
    /// (its radius), a nest (`NEST_RADIUS`) or a town (`kenshi_world::town::current_town`).
    fn place_at(&self, at: [f32; 3]) -> Option<Place> {
        let r = kenshi_world::town::radius(1, 1.0);
        if let Some(o) = (0..self.outposts.len()).find(|&o| (self.outposts[o][0] - at[0]).hypot(self.outposts[o][2] - at[2]) < r) {
            return Some(Place::Outpost(o));
        }
        if let Some(n) = (0..self.nests.len()).find(|&n| !self.nests[n].wiped && (self.nests[n].at[0] - at[0]).hypot(self.nests[n].at[2] - at[2]) < crate::nests::NEST_RADIUS) {
            return Some(Place::Nest(n));
        }
        kenshi_world::town::current_town(&self.towns, at, None).map(Place::Town)
    }

    /// The town person `id` stands in (Character +0x1a0 +0x88) as a campaign target: one of the
    /// player's outposts it is in, else its town of the level (`Person::town`).
    pub(crate) fn speaker_town(&self, id: usize) -> Option<CampaignTarget> {
        match self.place_at(self.folk[id].mover.position_units()) {
            Some(Place::Outpost(o)) => Some(CampaignTarget::Outpost(o)),
            _ => self.folk[id].town.map(CampaignTarget::Town),
        }
    }

    /// Squad `k`'s home town (Platoon +0x148 +0x30): the town or nest it set out from.
    fn squad_home(&self, k: usize) -> Option<Place> {
        let s = &self.world.squads[k];
        s.home.map(Place::Town).or(s.nest_home.map(Place::Nest))
    }

    /// Town +0x128: whether one of the player's outposts holds hostile occupiers (its "Hostile
    /// Occupiers" set, Town +0x118, filled as Package_OccupyTown starts and emptied as it ends,
    /// F029245): a won campaign's squad occupying it.
    fn outpost_occupied(&self, o: usize) -> bool {
        self.world.squads.iter().any(|s| {
            s.alive && s.won == Some(crate::raid::Victory::Occupy) && matches!(&s.goal, Goal::Campaign { campaign, .. } if self.world.phases.get(campaign).is_some_and(|p| p.target == CampaignTarget::Outpost(o)))
        })
    }

    /// @fn F050774
    ///
    /// Whether campaign `cid`'s forces are at peace (CampaignInstance vtable 0x58): none of
    /// its squads here has one up and not knocked over (Character vtable 0x30, 0x360: +0x5b8)
    /// who is outside the target town (a character target: anywhere) and perceives a standing
    /// enemy (its CharSensor's +0x94).
    fn forces_quiet(&self, cid: &str, t: CampaignTarget) -> bool {
        for k in self.campaign_squads(cid) {
            let s = &self.world.squads[k];
            if !s.active {
                continue;
            }
            for &m in &s.members {
                let q = &self.folk[m];
                if q.dead || q.down || q.prison.is_some() || self.arena.f[m].targeting.knocked {
                    continue;
                }
                let in_town = target_place(t).is_some_and(|tp| self.place_at(q.mover.position_units()) == Some(tp));
                if !in_town && q.sensor_enemies > 0 {
                    return false;
                }
            }
        }
        true
    }

    /// @fn F047563
    /// @fn F048825
    ///
    /// Whether a town lies open to its assault: no door of its buildings with interiors shut
    /// (Town vtable 0x2e0: each one's door, DoorStuff +0x380 0 closed), and not walled up (vtable
    /// 0x300: it has gates, each with its door, none open; a gate's door missing or open (1)
    /// breaks it). One of the player's outposts: its buildings' doors and its wall gates; a town
    /// of the level: its placements' doors and its gates (`town_gates`).
    fn town_open(&self, tp: Place) -> bool {
        let is_gate = |d: &crate::sim::DoorState| match d.player_building {
            Some(b) => self.buildings.get(b).and_then(|b| self.data.get(&b.record)),
            None => self.placed.get(d.door.placement).and_then(|p| self.data.get(&p.building)),
        }
        .is_some_and(|r| r.bools.get("is wall gate").copied().unwrap_or(false));
        let of_town = |d: &crate::sim::DoorState| match (tp, d.player_building) {
            (Place::Outpost(o), Some(b)) => self.buildings.get(b).is_some_and(|b| b.outpost == Some(o)),
            (Place::Town(t), None) => self.placed_town.get(d.door.placement).copied().flatten() == Some(t),
            _ => false,
        };
        let doors_open = self.door_states.iter().filter(|d| !d.gone && of_town(d) && !is_gate(d)).all(|d| d.phase != crate::sim::DOOR_CLOSED);
        let gates: Vec<&crate::sim::DoorState> = match tp {
            Place::Town(t) => self.town_gates(t).into_iter().map(|g| &self.door_states[g]).collect(),
            _ => self.door_states.iter().filter(|d| !d.gone && of_town(d) && is_gate(d)).collect(),
        };
        let walled = !gates.is_empty() && gates.iter().all(|d| d.phase != crate::sim::DOOR_OPEN);
        doors_open && !walled
    }

    /// @fn F045499
    /// @fn F044497
    ///
    /// Whether a hostile campaign is under way against the player (any faction's war manager
    /// running one whose target is the player's).
    fn hostile_campaign_running(&self) -> bool {
        self.world.phases.keys().any(|c| self.data.get(c).is_some_and(|c| campaign_bool(&self.data, c, "is hostile")))
    }

    /// @fn F050809
    /// @fn F050808
    /// @fn F040852
    /// @fn F040846
    ///
    /// Every second, each running campaign (CampaignInstance::update):
    /// - its leading squad (+0x80) first in the target town: noted (+0x3c, F050821 reads it);
    /// - not over and its leading squad all out of action (`squad_out`): lost (F050821), over;
    /// - on the way (0): its leading squad there (`near_target`: a character target within 400
    ///   units, a town within 0.8 of its radius; a target gone, at once). A peaceful campaign
    ///   (not "is hostile", not DEFEND_TOWN) whose town is held by hostile occupiers
    ///   (`outpost_occupied`) gives up instead once its leading squad is here and within 1.8 of
    ///   the radius: over and lost. There, one with a "dialog announcement" (F050808: its own
    ///   record's first, a DIALOGUE) waits until its leader (`squad_leader`) has delivered
    ///   EV_ANNOUNCEMENT and is out of its conversation (isDialogDelivered, F040852); one without
    ///   is left to its leader's own EV_ANNOUNCEMENT lines while it has some (F040846: their
    ///   DA_ASSAULT_PHASE .. DA_VICTORY_PHASE set the phase); else a town target hears "{1} has
    ///   arrived at {2}" ({1} the campaign's own "display name", F050926; {2} the town). Then
    ///   the assault (1) when it has one ("pt2 AI leader"), else over (2);
    /// - the assault (1): its leading squad in its home town (both none counting alike), won
    ///   (F050820). Else by its "key": ASSAULT_TOWN won after 4 hours; before, while more than
    ///   2 of the player's people are in a town target not sneaking (F039177) and over half of
    ///   them stand or lie in bed, it holds; else its clock (+0x20) starts the first time its
    ///   forces are not at peace (F050774), and once they are again, or none of those people
    ///   stand, 0.2 hours past it a town target open to it (`town_open`) is won. DEFEND_TOWN
    ///   after 12 hours is won unless a hostile campaign is on the player (F045499);
    ///   TRADER_VISIT after 1 hour unless an inventory window is open (ForgottenGUI +0xa8,
    ///   F042066: `Sim::inventory_open`). Not won, it is lost once under 0.2 of its squads can
    ///   fight (F044612; none, lost);
    /// - over (2): let go after 3 hours (F050875); victory (3, DA_VICTORY_PHASE) stays.
    fn campaign_phases(&mut self) {
        let now = self.hours_gone();
        // (a campaign on its way with no phase kept, from an older save: on the way)
        let running: Vec<String> = self
            .world
            .squads
            .iter()
            .filter(|s| s.alive && s.won.is_none())
            .filter_map(|s| match &s.goal {
                Goal::Campaign { campaign, .. } => Some(campaign.clone()),
                _ => None,
            })
            .collect();
        for c in running {
            self.world.phases.entry(c).or_insert(Phase::at(PHASE_APPROACH, now, None));
        }
        // (in a fixed order: the map's own is the hasher's)
        let mut ids: Vec<String> = self.world.phases.keys().cloned().collect();
        ids.sort_unstable();
        let data = self.data.clone();
        for cid in ids {
            let Some(mut ph) = self.world.phases.get(&cid).copied() else { continue };
            let Some(c) = data.get(&cid) else {
                self.world.phases.remove(&cid);
                continue;
            };
            let squads = self.campaign_squads(&cid);
            // (an older save's: its first squad leads)
            let lead = ph.lead.filter(|&k| k < self.world.squads.len()).or_else(|| squads.first().copied());
            ph.lead = lead;
            if !ph.entered
                && let Some(l) = lead
                && let Some(t) = target_place(ph.target)
                && self.place_at(self.world.squads[l].pos) == Some(t)
            {
                ph.entered = true;
            }
            // F050950: a target gone, or the player's and no longer holding (Town vtable 0x230:
            // an outpost no longer manned; one of the player's characters, `target_holds`), is
            // looked for again every 0.2 hours by its target finder; another found, "{1} has
            // redirected to {2}"
            if self.target_at(ph.target).is_none() || !self.target_holds(ph.target) {
                if ph.redirect_at == 0.0 {
                    ph.redirect_at = now;
                }
                if now - ph.redirect_at >= REDIRECT_HOURS {
                    if let Some(side) = self.campaign_side(&cid)
                        && let Some((t, _)) = self.campaign_way(c, &side)
                        && t != ph.target
                    {
                        ph.target = t;
                        let shown = c.strings.get("display name").cloned().unwrap_or_default();
                        self.messages.push(format!("{shown} has redirected to {}", self.target_name(t)));
                        if let Some(at) = self.target_at(t) {
                            for &k in &squads {
                                if let Goal::Campaign { target, .. } = &mut self.world.squads[k].goal {
                                    *target = at;
                                }
                                let route = self.route_for(&self.world.squads[k]);
                                self.world.squads[k].route = route;
                            }
                        }
                    }
                    ph.redirect_at = now;
                }
            }
            self.world.phases.insert(cid.clone(), ph);
            // a character target is followed where it goes (the instance's target is the
            // character itself, F050929): its squads' way to it made again once it has moved
            // past the arrival distance from where they head
            if let CampaignTarget::Person(p) = ph.target
                && p < self.folk.len()
            {
                let at = self.folk[p].mover.position_units();
                for &k in &squads {
                    let s = &mut self.world.squads[k];
                    if !s.alive {
                        continue;
                    }
                    if let Goal::Campaign { target, .. } = &mut s.goal
                        && (target[0] - at[0]).powi(2) + (target[1] - at[1]).powi(2) + (target[2] - at[2]).powi(2) > ARRIVED_AT_CHARACTER * ARRIVED_AT_CHARACTER
                    {
                        *target = at;
                        let route = self.route_for(&self.world.squads[k]);
                        self.world.squads[k].route = route;
                    }
                }
            }
            if ph.phase != PHASE_OVER && lead.is_some_and(|l| self.squad_out(l)) {
                self.campaign_lost(&cid);
                continue;
            }
            let hostile = campaign_bool(&data, c, "is hostile");
            let key = campaign_num(&data, c, "key") as i32;
            let hours = now - ph.since;
            match ph.phase {
                PHASE_APPROACH => {
                    let Some(l) = lead else { continue };
                    let at = self.world.squads[l].pos;
                    let mut town = None;
                    match ph.target {
                        CampaignTarget::Outpost(o) if o < self.outposts.len() => {
                            if self.world.squads[l].active && self.outpost_occupied(o) && !hostile && key != DEFEND_TOWN && self.near_target(at, ph.target, None, OCCUPIED_TOWN) {
                                self.campaign_lost(&cid);
                                continue;
                            }
                            if !self.near_target(at, ph.target, None, ARRIVED_AT_TOWN) {
                                continue;
                            }
                            town = Some(o);
                        }
                        CampaignTarget::Town(t) if t < self.towns.len() => {
                            if !self.near_target(at, ph.target, None, ARRIVED_AT_TOWN) {
                                continue;
                            }
                        }
                        CampaignTarget::Person(p) if p < self.folk.len() => {
                            if !self.near_target(at, ph.target, None, ARRIVED_AT_TOWN) {
                                continue;
                            }
                        }
                        // its target gone: there at once
                        _ => {}
                    }
                    let leader = self.squad_leader(l);
                    let announcement = c.refs("dialog announcement").first().is_some_and(|(d, _)| data.get(d).is_some_and(|d| d.item_type() == Some(ItemType::Dialogue)));
                    if announcement {
                        if leader.is_some_and(|ch| self.dialog_memory.delivered.contains(&(ch, crate::dialogue::EV_ANNOUNCEMENT)) && !self.in_conversation(ch)) {
                            self.set_campaign_phase(&cid, PHASE_ASSAULT);
                        }
                        continue;
                    }
                    if leader.is_some_and(|ch| self.has_event(ch, crate::dialogue::EV_ANNOUNCEMENT)) {
                        continue;
                    }
                    if town.is_some() || matches!(ph.target, CampaignTarget::Town(_)) {
                        let shown = c.strings.get("display name").cloned().unwrap_or_default();
                        self.messages.push(format!("{shown} has arrived at {}", self.target_name(ph.target)));
                    }
                    self.set_campaign_phase(&cid, PHASE_ASSAULT);
                }
                PHASE_ASSAULT => {
                    if let Some(l) = lead
                        && self.place_at(self.world.squads[l].pos) == self.squad_home(l)
                    {
                        self.campaign_won(&cid);
                        continue;
                    }
                    let won = match key {
                        ASSAULT_TOWN if hours > ASSAULT_HOURS => true,
                        ASSAULT_TOWN => {
                            // the player's people in a town target: there not sneaking, and
                            // standing (Character vtable 0x368 0) or in bed (+0x2f8 1)
                            let mut up = 0;
                            if let Some(tp) = target_place(ph.target) {
                                let mut present = 0;
                                for p in self.folk.iter().filter(|p| p.player && !p.dead && !p.away) {
                                    if self.place_at(p.mover.position_units()) != Some(tp) {
                                        continue;
                                    }
                                    if !(p.sneaking && !self.arena.f[p.id].active) {
                                        present += 1;
                                    }
                                    if !p.down || p.bed.is_some() {
                                        up += 1;
                                    }
                                }
                                if present > 2 && present as f64 * DEFENDERS_HALF < up as f64 {
                                    continue;
                                }
                            }
                            let quiet = self.forces_quiet(&cid, ph.target);
                            if ph.quiet == ph.since {
                                if !quiet {
                                    ph.quiet = now;
                                    self.world.phases.insert(cid.clone(), ph);
                                }
                                false
                            } else {
                                (quiet || up == 0) && now - ph.quiet > QUIET_HOURS && target_place(ph.target).is_some_and(|tp| self.town_open(tp))
                            }
                        }
                        DEFEND_TOWN => hours > DEFEND_HOURS && !self.hostile_campaign_running(),
                        TRADER_VISIT => hours > VISIT_HOURS && !self.inventory_open,
                        _ => false,
                    };
                    if won {
                        self.campaign_won(&cid);
                        continue;
                    }
                    let able = squads.iter().filter(|&&k| self.squad_able(k)).count();
                    if !(ABLE_SHARE <= (1.0 / squads.len() as f32) * able as f32) {
                        self.campaign_lost(&cid);
                    }
                }
                PHASE_OVER if hours > OVER_HOURS => {
                    self.world.phases.remove(&cid);
                }
                _ => {}
            }
        }
    }

    /// @fn F050815
    ///
    /// A campaign sets out on `target` from `home` (none, or one destroyed: the faction's home
    /// nearest the target, `nearest_home`; none at all, it does not): "num forces" people of
    /// its "squads to use", at its "travel speed unloaded"; its last time kept (its repeat
    /// limit); the target being the player's, "Warning: {1} moving towards {2}" for a hostile
    /// one, "{1} on the way to {2}" for another ({1} its own "display name", F050926; {2} the
    /// target's name); then each of its "trigger player ally" campaigns whose faction ("faction
    /// override") is the player's ally scheduled at once (F050814). Whether it did.
    fn launch_campaign(&mut self, cid: &str, side: &str, aim: CampaignTarget, home: Option<Place>) -> bool {
        let data = self.data.clone();
        let Some(c) = data.get(cid) else { return false };
        let Some(target) = self.target_at(aim) else { return false };
        let home = match home.filter(|&h| !self.place_destroyed(h)) {
            Some(h) => Some(h),
            None => self.nearest_home(side, target),
        };
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: {} sets out on {aim:?} from {home:?} (outposts {:?})", c.name, self.outposts);
        }
        let Some(home) = home else { return false };
        let origin = self.place_pos(home);
        // (testing) KENSHI_CAMPAIGN_NEAR: from 5000 units off
        let origin = if std::env::var("KENSHI_CAMPAIGN_NEAR").is_ok() { [target[0] + 5000.0, target[1], target[2]] } else { origin };
        let hostile = campaign_bool(&data, c, "is hostile");
        // F050803: "num forces" x the options' raid size (as a whole number)
        let size = if self.world.raid_size > 0.0 { self.world.raid_size } else { 1.0 };
        let needed = (campaign_num(&data, c, "num forces") * size) as u32;
        let speed = SPEED * campaign_num(&data, c, "travel speed unloaded").max(1.0);
        let goal = Goal::Campaign { campaign: cid.to_string(), target, there: 0.0, hostile };
        // F050792: its first squad leads (+0x80, `take_on`); the phase is there as its squads
        // gather
        let now = self.hours_gone();
        self.world.phases.insert(cid.to_string(), Phase { phase: PHASE_APPROACH, since: now, quiet: now, lead: None, entered: false, target: aim, home: Some(home), flagged: false, redirect_at: 0.0 });
        let first = self.world.squads.len();
        let forces = self.gather_forces(c, cid, side, target, &goal, speed, needed);
        // (testing) KENSHI_CAMPAIGN_NEAR: the squads it made set out from 5000 units off
        if std::env::var("KENSHI_CAMPAIGN_NEAR").is_ok() {
            for k in first..self.world.squads.len() {
                if !self.world.squads[k].active {
                    self.world.squads[k].pos = origin;
                    let route = self.route_for(&self.world.squads[k]);
                    self.world.squads[k].route = route;
                }
            }
        }
        // not more than it needs: "Assault to {1} canceled due to faction weakness" (the war
        // manager's log, F042957, a no-op), and it ends (F050875)
        if forces <= needed {
            if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                eprintln!("world: Assault to {} canceled due to faction weakness ({forces} of {needed})", self.target_name(aim));
            }
            self.world.phases.remove(cid);
            let ks: Vec<usize> = (0..self.world.squads.len()).filter(|&k| self.world.squads[k].campaign.as_deref() == Some(cid)).collect();
            for k in ks {
                self.world.squads[k].campaign = None;
                self.go_home(k);
            }
            return false;
        }
        *self.world.campaigns_run.entry(cid.to_string()).or_insert(0) += 1;
        self.world.campaign_last.insert(cid.to_string(), now);
        let shown = c.strings.get("display name").cloned().unwrap_or_default();
        let to = self.target_name(aim);
        self.messages.push(if hostile { format!("Warning: {shown} moving towards {to}") } else { format!("{shown} on the way to {to}") });
        eprintln!("world: campaign {} triggered from {origin:?} to {target:?}", c.name);
        // its allies' campaigns ("trigger player ally"): those whose faction is the player's ally
        let player_faction = self.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
        for (ally, _) in campaign_refs(&data, c, "trigger player ally") {
            let Some(a) = data.get(&ally) else { continue };
            let Some((f, _)) = a.refs("faction override").first() else {
                eprintln!("error: campaign with [trigger player ally] has no override faction: {}", a.name);
                continue;
            };
            if player_faction.as_ref().and_then(|p| self.relations.get(p)).is_some_and(|r| r.is_ally(Some(f))) {
                let f = f.clone();
                self.schedule_campaign(&ally, &f, 0.0, 0.0, false, aim, None, Some(side));
            }
        }
        true
    }

    /// The name of campaign `cid`'s target (its vtable 8), the player's faction's when it has
    /// none (PlayerFaction +0x1a8).
    fn target_name(&self, t: CampaignTarget) -> String {
        match t {
            CampaignTarget::Outpost(o) if o < self.outposts.len() => self.outpost_name.clone(),
            CampaignTarget::Person(p) if p < self.folk.len() => self.display_name(p),
            CampaignTarget::Town(t) if t < self.towns.len() => self.towns[t].name.clone(),
            _ => {
                if self.faction_name.is_empty() {
                    self.data.get(crate::sim::PLAYER_FACTION).map_or(String::new(), |f| f.name.clone())
                } else {
                    self.faction_name.clone()
                }
            }
        }
    }

    /// Campaign `cid` over (F050809's phase 2, both clocks now): its squads go home.
    fn campaign_ends(&mut self, cid: &str) {
        let now = self.hours_gone();
        let was = self.world.phases.get(cid).copied();
        self.world.phases.insert(cid.to_string(), Phase::at(PHASE_OVER, now, was));
    }

    /// @fn F050820
    ///
    /// Campaign `cid` won (CampaignInstance vtable 0x10): a hostile one says "{1} against {2}
    /// has ended" ({1} its own "display name", F050926; {2} its target's name); one of its
    /// "victory trigger" campaigns (`pick_campaign`: a single roll over their chances) is
    /// scheduled by its faction's war manager between the ref's first and second values of
    /// hours, at its target (F050814: none without one). Then over (F050809). (stand-in: Kenshi
    /// puts its squads on its "retreat AI" packages, F050791, and has no victory package, the
    /// "victory AI leader" never being read; here its squads play out the port's win, `raid`)
    pub(crate) fn campaign_won(&mut self, cid: &str) {
        let data = self.data.clone();
        let Some(c) = data.get(cid) else { return };
        let ph = self.world.phases.get(cid).copied();
        let target = ph.map(|p| p.target).unwrap_or_default();
        if campaign_bool(&data, c, "is hostile") {
            let shown = c.strings.get("display name").cloned().unwrap_or_default();
            self.messages.push(format!("{shown} against {} has ended", self.target_name(target)));
        }
        let side = self.campaign_side(cid);
        let list = campaign_refs(&data, c, "victory trigger");
        if let (Some((next, lo, hi)), Some(side)) = (self.pick_campaign(&list), side) {
            let home = ph.and_then(|p| p.home);
            self.schedule_campaign(&next, &side, lo, hi, true, target, home, Some(&side));
        }
        self.campaign_ends(cid);
        let victory = crate::raid::victory_of(&data, c);
        let ks: Vec<usize> = self.campaign_squads(cid).into_iter().filter(|&k| self.world.squads[k].alive).collect();
        for k in ks {
            self.begin_victory(k, victory);
        }
        eprintln!("world: campaign {} won", c.name);
    }

    /// @fn F050821
    /// @fn F040881
    ///
    /// Campaign `cid` lost (CampaignInstance vtable 0x18), its target there (else nothing). It
    /// was beaten by the player when its leading squad is here and its leader's squad memory
    /// tags any of the player's squads' leaders (F044487) an aggressor or a temporary enemy
    /// (short-term tags 2, 4: mask 0x14, F040881). A peaceful one says "{1} was defeated on
    /// their way to {2}", a hostile one beaten by the player "{1} against {2} has been
    /// defeated", else "{1} to {2} has failed". A hostile one not beaten by the player that had
    /// not yet been in its target town (or whose town holds hostile occupiers) tries again,
    /// itself, 40 to 96 hours on; else a hostile one that had been in it, or any beaten by the
    /// player, schedules one of its "loss trigger" campaigns (the ref's first and second
    /// values of hours). Either is at its target town (F050814: none for a character target,
    /// CampaignInstance vtable 0xa0 giving it none). Then over (F050809): its squads go home.
    pub(crate) fn campaign_lost(&mut self, cid: &str) {
        let data = self.data.clone();
        let Some(c) = data.get(cid) else { return };
        let ph = self.world.phases.get(cid).copied().unwrap_or(Phase::at(PHASE_OVER, self.hours_gone(), None));
        let ks: Vec<usize> = self.campaign_squads(cid).into_iter().filter(|&k| self.world.squads[k].alive).collect();
        let side = self.campaign_side(cid);
        if self.target_at(ph.target).is_some() {
            // CampaignInstance vtable 0xa0: its target town (none for a character)
            let town = match ph.target {
                CampaignTarget::Person(_) | CampaignTarget::None => None,
                t => Some(t),
            };
            let beaten = ph.lead.is_some_and(|l| {
                let Some(holder) = self.squad_leader(l).filter(|_| self.world.squads[l].active) else { return false };
                self.player_squads.iter().filter_map(|q| self.player_squad_leader(q.platoon)).any(|pl| self.tagged_short_mask(holder, pl, 0x14))
            });
            let hostile = campaign_bool(&data, c, "is hostile");
            let shown = c.strings.get("display name").cloned().unwrap_or_default();
            let to = self.target_name(ph.target);
            self.messages.push(if !hostile {
                format!("{shown} was defeated on their way to {to}")
            } else if beaten {
                format!("{shown} against {to} has been defeated")
            } else {
                format!("{shown} to {to} has failed")
            });
            let occupied = matches!(town, Some(CampaignTarget::Outpost(o)) if self.outpost_occupied(o));
            if let Some(side) = side {
                if (!ph.entered || occupied) && hostile && !beaten {
                    if let Some(t) = town {
                        self.schedule_campaign(cid, &side, RETRY_FROM, RETRY_TO, true, t, ph.home, Some(&side));
                    }
                } else if (hostile && ph.entered) || beaten {
                    let list = campaign_refs(&data, c, "loss trigger");
                    if let Some((next, lo, hi)) = self.pick_campaign(&list)
                        && let Some(t) = town
                    {
                        self.schedule_campaign(&next, &side, lo, hi, true, t, ph.home, Some(&side));
                    }
                }
            }
        }
        self.campaign_ends(cid);
        for k in ks {
            self.go_home(k);
        }
        eprintln!("world: campaign {} lost", c.name);
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_squads_only_count_their_timers_down() {
        let row = |timer: f32, number: i32| SpecialRow { template: String::new(), number, timer, squads: Vec::new() };
        let mut s = SpecialSquads { faction: String::new(), prosperity: 990.0, max_prosperity: 1000.0, rows: vec![row(5.0, 1), row(0.0, 1), row(3.0, 1)] };
        s.tick(2.0);
        // the prosperity grows a tenth of the time while under its most
        assert_eq!(s.prosperity, 990.2);
        // the second row is due with too few squads: the third is not reached (F030219)
        assert_eq!(s.rows.iter().map(|r| r.timer).collect::<Vec<_>>(), vec![3.0, 0.0, 3.0]);
        // and nothing is made of it
        assert!(s.rows.iter().all(|r| r.squads.is_empty()));
        s.prosperity = 1000.0;
        s.tick(1.0);
        assert_eq!(s.prosperity, 1000.0);
    }
}
