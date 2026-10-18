//! The world beyond the squad (port_map `world`): squads travelling the whole map while no one
//! is near (Kenshi's UnloadedPlatoon / AbstractMovementBase), towns' patrols, the factions'
//! campaigns on the player (FactionWarMgr), towns' garrisons, the unique
//! NPCs' deaths and the world states they make (WORLD_EVENT_STATE), and the towns those turn
//! into (TOWN "override town": destroyed, taken over).
//!
//! Specs:
//! - WORLD_EVENT_STATE (fcs.def): "NPC is" each unique in the state its ref's value gives
//!   (alive 1, dead 0, imprisoned 2), "NPC is NOT" none in it, "player ally" / "player enemy"
//!   each faction an ally / enemy of the player's (value 1) or not (0), "town okay" each town
//!   standing (1) or destroyed (0). (assumption) "player involvement" is not read; a unique not
//!   yet met counts as alive.
//! - TOWN "override town" (in order): the first whose "world state" refs all hold (each ref's
//!   first value 1: the state must hold; 0: must not) replaces the town, its own overrides tried
//!   in turn. With "residents override", a building's residents are the override's "residents"
//!   of the building's designation (the ref's second value).
//! - FactionWarMgr (F050818): scheduled campaigns (a line's "trigger campaign", a campaign's
//!   "victory trigger" / "loss trigger" / "trigger player ally", F050814) set out when due
//!   (`campaigns_due`); every 6 game hours a faction with none on its way nor scheduled may set
//!   out one territorial campaign (`campaign_check`, chances F050802); its forces are "num
//!   forces" people from its "squads to use" (F050815).
//!
//! Kenshi's factions campaign only against the player (FACTION_CAMPAIGN targets the player's
//! bases or characters); no faction raids another's town and nothing fights off-screen, so
//! towns change hands only by the world states.
//!
//! Away squads walk their long-distance route (`kenshi_world::longpath`: the roads over 10000
//! units when they pay at the faction's "road preference", F040644) at 60 units a second, the
//! cap 90 while over 25000 from the target (F045434; campaigns x their "travel speed
//! unloaded"), never over the mover's own 70 (F040640). (assumption) A squad within 4000
//! units of a player character on loaded ground is made real (its people kept for when it comes
//! back) and walks its route on the navmesh; one beyond 6000 goes away again. Each town keeps
//! its "roaming squads" (one each) patrolling to other towns of its faction or open to it,
//! waiting 2 to 6 hours at each. A campaign's forces set out from the faction's town or camp
//! nearest the target (or 15000 units off) and march on it; there, its phase (F050809,
//! `campaign_phases`) decides: the assault (fighting the player's people) or leaving, won after
//! the assault's hours or with the defenders down, lost with its forces under a fifth.

use crate::sim::Sim;
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_world::longpath::WorldGrid;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Away squads' walking speed (units a second): an unloaded platoon walking to its target
/// (F045434, UnloadedPlatoon's tick, mode 6) caps its mover at 60, or 90 while the target is
/// over 25000 units off; the mover goes at the lower of that and its own 70
/// (AbstractMovementBase +0xb4, F040640). The base speed class 25.
/// (assumption) Patrols, raids and campaigns take mode 6 (their AI package's mode is not read).
const SPEED: f32 = 60.0;
const SPEED_FAR: f32 = 90.0;
const FAR_TARGET: f32 = 25000.0;
const OWN_SPEED: f32 = 70.0;
/// Made real within this of a player character; away again beyond the second.
const NEAR: f32 = 4000.0;
const FAR: f32 = 6000.0;

/// What a world squad is doing.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Goal {
    /// Walking to town `to` of the patrol from `home`, then waiting there `wait` hours.
    Patrol { home: usize, to: usize, wait: f32 },
    /// Raiding `town` for `faction`.
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
}

/// A campaign's phase (CampaignInstance +0xc0) and its two clocks (+0x18 the phase's start,
/// +0x20 when the target's defenders were last seen standing), game hours.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Phase {
    pub phase: u8,
    pub since: f64,
    pub quiet: f64,
}

/// Campaign phases: on the way, the assault (its "pt2 AI leader"), over (retreating or gone
/// home), victory.
pub const PHASE_APPROACH: u8 = 0;
pub const PHASE_ASSAULT: u8 = 1;
pub const PHASE_OVER: u8 = 2;
pub const PHASE_VICTORY: u8 = 3;
/// F050809: a campaign is there within 400 units of a character it targets (0x14171f330, 400
/// squared), within 0.8 of a town's radius of a town (0x14168a178, F048764).
const ARRIVED_AT_CHARACTER: f32 = 400.0;
const ARRIVED_AT_TOWN: f32 = 0.8;

/// A town as the world keeps it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TownState {
    /// The TOWN record it is now (the base or an override).
    pub record: String,
    pub garrison: f32,
    pub full: f32,
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
    /// Game hours of the next 6-hourly check, and of the last garrison recovery.
    pub next_check: f64,
    pub started: bool,
    /// Characters (records) died this tick, for the world states.
    pub pending_deaths: Vec<String>,
    /// Towns (records) the player has found: the map shows only these; and those it has been at
    /// (within 400 units: the map names them, not by their "unexplored name").
    pub discovered: HashSet<String>,
    pub explored: HashSet<String>,
    /// Campaigns a dialogue line triggered ("trigger campaign"), each from the game hour it is
    /// due (crate::dialogue).
    pub triggered: Vec<(String, f64)>,
    /// The faction a dialogue's campaign is set out by (the speaker's, its war manager), for
    /// one no faction lists.
    pub trigger_side: HashMap<String, String>,
    /// Campaigns a line's "lock campaign" took off territorial triggering ("unlock campaign"
    /// puts back).
    pub campaigns_locked: HashSet<String>,
    /// How many times each scheduled campaign has found no target (F050818: put off 5 to 10
    /// hours each time, dropped after 10).
    pub trigger_tries: HashMap<String, u32>,
    /// The options' "Town raid events: frequency" (settings.cfg "raidFrequencyMult", global
    /// 0x1421334e4, F032960): the territorial chances' multiplier.
    pub raid_frequency: f32,
    /// The options' "Town raid events: size" (settings.cfg "raidSizeMult", F050803): a
    /// campaign's "num forces" multiplier; 0 (not set yet) counts as 1.
    pub raid_size: f32,
    /// Towns changed hands whose loaded people are still the old ones: re-peopled once no
    /// player character is near (`reseat_tick`).
    pub reseat: Vec<usize>,
}

/// (assumption) How far every player character must be from a changed town before its people
/// change (world units: past where its zones would be kept loaded).
const RESEAT_RANGE: f32 = 8000.0;

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

/// A campaign's text field through its "inherits from" chain (its own first): the first
/// not empty.
fn campaign_str(data: &GameDataContainer, c: &GameData, k: &str) -> Option<String> {
    let mut cur = Some(c);
    for _ in 0..8 {
        let r = cur?;
        if let Some(v) = r.strings.get(k).filter(|v| !v.is_empty()) {
            return Some(v.clone());
        }
        cur = r.refs("inherits from").first().and_then(|(s, _)| data.get(s));
    }
    None
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
        let mut changed = false;
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
                changed |= self.world.involved.insert(c);
            } else if !captive && !self.world.dead_uniques.contains(&c) {
                changed |= self.world.involved.remove(&c);
            }
        }
        if changed {
            self.update_towns();
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

    /// The TOWN record town `t` is now: its overrides whose world states all hold, in turn.
    pub fn town_record(&self, t: usize) -> String {
        let mut cur = self.towns[t].record.clone();
        for _ in 0..8 {
            let Some(r) = self.data.get(&cur) else { break };
            let next = r.refs("override town").iter().find(|(o, _)| {
                self.data.get(o).is_some_and(|o| o.refs("world state").iter().all(|(s, v)| self.world_state(s) == (v.first().copied().unwrap_or(1) != 0)))
            });
            match next {
                Some((o, _)) => cur = o.clone(),
                None => break,
            }
        }
        cur
    }

    /// A garrison's full strength: its residents' people.
    fn garrison_of(&self, record: &str) -> f32 {
        let Some(t) = self.data.get(record) else { return 0.0 };
        t.refs("residents").iter().map(|(sq, v)| template_strength(&self.data, sq) * v.first().copied().unwrap_or(1).max(1) as f32).sum()
    }

    /// The towns' states again after world states changed: each town takes its override, with
    /// a message when one changes hands or falls.
    pub(crate) fn update_towns(&mut self) {
        for t in 0..self.towns.len() {
            let now = self.town_record(t);
            if self.world.towns.get(t).is_some_and(|s| s.record != now) {
                let rec = self.data.get(&now);
                let faction = rec.and_then(|r| r.refs("faction").first().map(|(f, _)| f.clone()));
                let fname = faction.as_deref().and_then(|f| self.data.get(f)).map_or("nobody".to_string(), |f| f.name.clone());
                let ruined = rec.is_some_and(|r| r.refs("residents").iter().any(|(s, _)| self.data.get(s).is_some_and(|s| s.name.contains("ruined"))));
                self.messages.push(if ruined {
                    format!("{} has been destroyed.", self.towns[t].name)
                } else {
                    format!("{} is now held by {fname}.", self.towns[t].name)
                });
                let full = self.garrison_of(&now);
                self.world.towns[t] = TownState { record: now, garrison: full, full };
                self.towns[t].faction = faction;
                if !self.world.reseat.contains(&t) {
                    self.world.reseat.push(t);
                }
            }
        }
    }

    /// Towns changed hands re-peopled out of sight: Kenshi builds a town's people when its area
    /// loads, so a change shows the next time the player comes. (assumption) Here, once no
    /// player character is within `RESEAT_RANGE` of it: the loaded residents of its buildings
    /// retire (set away) and the town record's own (an override's "residents" of each building's
    /// designation, else the building's residents template) move in. Bar squads are left.
    fn reseat_tick(&mut self) {
        if self.world.reseat.is_empty() {
            return;
        }
        let players: Vec<[f32; 3]> = self.folk.iter().filter(|p| p.player && !p.dead).map(|p| p.mover.position_units()).collect();
        let pending = std::mem::take(&mut self.world.reseat);
        for t in pending {
            let c = self.towns[t].position;
            if players.iter().any(|q| (q[0] - c[0]).hypot(q[2] - c[2]) < RESEAT_RANGE) {
                self.world.reseat.push(t);
                continue;
            }
            self.reseat_town(t);
        }
    }

    fn reseat_town(&mut self, t: usize) {
        let data = self.data.clone();
        let now = self.town_record(t);
        let override_rec = (now != self.towns[t].record).then(|| data.get(&now)).flatten().filter(|r| r.bools.get("residents override").copied().unwrap_or(false));
        let homes: Vec<usize> = (0..self.placed.len()).filter(|&k| self.placed[k].squad.is_some() && self.towns[t].contains(self.placed_at[k], 1.0)).collect();
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
            self.update_towns();
        }
    }

    /// The world's first tick: the walking grid, the towns' states, the patrols.
    fn world_start(&mut self) {
        self.world.started = true;
        match WorldGrid::build(&mut self.terrain) {
            Ok(g) => self.world.grid = Some(Arc::new(g)),
            Err(e) => eprintln!("world: no walking grid: {e}"),
        }
        // each town as the world states have it now (no messages for how it starts)
        self.world.towns = (0..self.towns.len())
            .map(|t| {
                let record = self.town_record(t);
                let full = self.garrison_of(&record);
                TownState { record, garrison: full, full }
            })
            .collect();
        for t in 0..self.towns.len() {
            let f = self.data.get(&self.world.towns[t].record).and_then(|r| r.refs("faction").first().map(|(f, _)| f.clone()));
            if f.is_some() {
                self.towns[t].faction = f;
            }
        }
        // every town faction's relations (the arena adds them as factions turn up)
        let factions: Vec<String> = self.towns.iter().filter_map(|t| t.faction.clone()).collect();
        for f in factions {
            if !self.relations.contains_key(&f)
                && let Some(r) = self.data.get(&f)
            {
                let rel = kenshi_ai::faction::FactionRelations::from_data(r, &self.data);
                self.relations.insert(f, rel);
            }
        }
        self.world.next_check = self.hours_gone() + if std::env::var("KENSHI_CAMPAIGN").is_ok() { 0.1 } else { 6.0 };
        self.world.raid_frequency = kenshi_io::cfg::load(&self.install, "settings.cfg").get("raidFrequencyMult").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0);
        self.world.raid_size = kenshi_io::cfg::load(&self.install, "settings.cfg").get("raidSizeMult").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0);
        // each town's roaming squads, one each
        let data = self.data.clone();
        for t in 0..self.towns.len() {
            let Some(rec) = data.get(&self.world.towns[t].record) else { continue };
            for (sq, _) in rec.refs("roaming squads") {
                let at = self.towns[t].position;
                if let Some(to) = self.patrol_target(t, t) {
                    self.add_world_squad(sq, at, Goal::Patrol { home: t, to, wait: 0.0 }, SPEED);
                }
            }
        }
        eprintln!("world: {} squads abroad, {} towns", self.world.squads.len(), self.towns.len());
    }

    /// A town for a patrol from `home` (at `from`) to go to: another of its faction, or a public
    /// town of a faction not its enemy, within 60000 units.
    fn patrol_target(&mut self, home: usize, from: usize) -> Option<usize> {
        let faction = self.towns[home].faction.clone();
        let at = self.towns[from].position;
        let rel = faction.as_ref().and_then(|f| self.relations.get(f));
        let options: Vec<usize> = (0..self.towns.len())
            .filter(|&k| k != from && self.towns[k].kind != 8)
            .filter(|&k| {
                let p = self.towns[k].position;
                (p[0] - at[0]).hypot(p[2] - at[2]) < 60000.0
            })
            .filter(|&k| self.towns[k].faction == faction || (self.towns[k].public && !rel.is_some_and(|r| r.is_enemy(self.towns[k].faction.as_deref()))))
            .collect();
        if options.is_empty() {
            return (from != home).then_some(home);
        }
        Some(options[(self.rng.uniform() * options.len() as f32) as usize % options.len()])
    }

    /// Add a squad of `template` to the world at `at` heading for its goal.
    pub fn add_world_squad(&mut self, template: &str, at: [f32; 3], goal: Goal, speed: f32) -> usize {
        let faction = self.data.get(template).and_then(|t| t.refs("faction").first().map(|(f, _)| f.clone()));
        let strength = template_strength(&self.data, template);
        let mut s = WorldSquad { template: template.to_string(), faction, pos: at, route: Vec::new(), goal, speed, strength, members: Vec::new(), active: false, alive: true, order_timer: 0.0, won: None, captives: Vec::new(), announced: false, side: None, arrived: false };
        s.route = self.route_for(&s);
        self.world.squads.push(s);
        self.world.squads.len() - 1
    }

    /// Where a squad's goal is.
    fn goal_point(&self, g: &Goal) -> [f32; 3] {
        match g {
            Goal::Patrol { to, .. } => self.towns[*to].position,
            Goal::Raid { town } => self.towns[*town].position,
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

    /// The player's characters' positions (world units).
    fn player_spots(&self) -> Vec<[f32; 3]> {
        self.folk.iter().filter(|p| p.player && !p.dead).map(|p| p.mover.position_units()).collect()
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
            if !self.world.explored.contains(&t.record) && near(400.0) {
                self.world.explored.insert(t.record.clone());
                self.world.discovered.insert(t.record.clone());
            }
            if self.world.discovered.contains(&t.record) {
                continue;
            }
            let mut r = if t.kind == 2 { 17000.0 } else { 8500.0 };
            if t.kind == 0 || t.kind == 4 {
                r *= 0.5;
            }
            if near(r) {
                self.world.discovered.insert(t.record.clone());
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
        self.discover_towns();
        self.reseat_tick();
        self.campaigns_due();
        self.campaign_phases();
        let now = self.hours_gone();
        // the 6-hourly checks: raids, campaigns, garrisons recovering
        if now >= self.world.next_check {
            self.world.next_check = now + 6.0;
            for t in &mut self.world.towns {
                t.garrison = (t.garrison + t.full * 0.1 * 0.25).min(t.full);
            }
            self.campaign_check();
            // (testing) a forced campaign is tried again until it starts
            if std::env::var("KENSHI_CAMPAIGN").is_ok() && self.world.campaigns_run.is_empty() {
                self.world.next_check = now + 0.1;
            }
        }
        let players = self.player_spots();
        let near = |p: [f32; 3], d: f32| players.iter().any(|q| (q[0] - p[0]).hypot(q[2] - p[2]) < d);
        for k in 0..self.world.squads.len() {
            if !self.world.squads[k].alive {
                continue;
            }
            if self.world.squads[k].active {
                self.real_squad(k, dt);
                let s = &self.world.squads[k];
                if s.alive && s.active && !near(s.pos, FAR) {
                    self.send_away(k);
                }
            } else {
                self.away_squad(k, dt);
                let s = &self.world.squads[k];
                let loaded = self.nav.closest_point(s.pos.map(|v| v * kenshi_world::navmesh::METRES), 200.0 * kenshi_world::navmesh::METRES).is_some();
                if s.alive && near(s.pos, NEAR) && loaded {
                    self.make_real(k);
                }
            }
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
            Goal::Patrol { home, to, wait } => {
                if wait <= 0.0 && self.world.squads[k].route.is_empty() {
                    // (assumption) 2 to 6 hours in town
                    let w = 2.0 + self.rng.uniform() * 4.0;
                    self.world.squads[k].goal = Goal::Patrol { home, to, wait: w };
                } else if wait > 0.0 {
                    let left = wait - dt / self.hour_secs as f32;
                    if left > 0.0 {
                        self.world.squads[k].goal = Goal::Patrol { home, to, wait: left };
                    } else if let Some(next) = self.patrol_target(home, to) {
                        self.world.squads[k].goal = Goal::Patrol { home, to: next, wait: 0.0 };
                        self.world.squads[k].route = self.route_for(&self.world.squads[k]);
                    }
                }
            }
            // (an older save's raid) home
            Goal::Raid { .. } => self.go_home(k),
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
    /// "can talk before arrival"): "{1} has arrived at {2}" (its "display name", the player's
    /// outpost or person), once; then, with the player's people within 2500 units (F031319),
    /// its `leader` (at `at`) announces itself to the nearest (`announce_to`: its package's
    /// EV_ANNOUNCEMENT dialogues with the campaign's "dialog announcement", F050808 / F040927):
    /// a line with replies (the bandits' demands) a conversation, one without said aloud.
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
            let characters = campaign_bool(&self.data, &c, "target characters");
            let shown = campaign_str(&self.data, &c, "display name").unwrap_or_else(|| c.name.clone());
            let to = if characters {
                self.folk.iter().find(|p| p.player && !p.dead && !p.down).map(|p| p.id).map_or("you".to_string(), |p| self.display_name(p))
            } else {
                self.outpost_name.clone()
            };
            self.messages.push(format!("{shown} has arrived at {to}"));
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
        let dialog = campaign_refs(&self.data, &c, "dialog announcement").into_iter().next().map(|(d, _)| d);
        let said = match dialog {
            Some(d) => self.announce_to(leader, player, &d),
            None => self.talk_on(leader, player, crate::dialogue::EV_ANNOUNCEMENT, false),
        };
        if let Some((mut conv, effects)) = said {
            if conv.choices.is_empty() {
                let text = std::mem::take(&mut conv.npc_text);
                self.say(leader, text);
                if effects.is_empty() {
                    return;
                }
            }
            self.npc_talks.push((conv, effects));
        }
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
        let to = depot.or_else(home).map_or([s.pos[0] + 15000.0, s.pos[1], s.pos[2]], |t| self.towns[t].position);
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
            // beaten: the rest lie where they fell; a campaign whose squads are all beaten has
            // lost
            self.world.squads[k].alive = false;
            self.world.squads[k].active = false;
            if let Goal::Campaign { campaign, .. } = self.world.squads[k].goal.clone()
                && !self.world.squads.iter().any(|s| s.alive && matches!(&s.goal, Goal::Campaign { campaign: c, .. } if *c == campaign))
            {
                self.campaign_over(&campaign, false);
            }
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
        // a campaign at its target: one whose assault does not fight is won on arriving; a won
        // one (but an occupation) plays out its victory (`raid`)
        if let Goal::Campaign { campaign, .. } = self.world.squads[k].goal.clone()
            && self.campaign_there(k)
        {
            let won = self.world.squads[k].won;
            if won.is_none() && self.assaulting(&campaign) && self.data.get(&campaign).is_some_and(|c| crate::raid::loots_without_fighting(&self.data, c)) {
                self.campaign_over(&campaign, true);
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
    /// random by their chances. (assumption) A hostile one is not set out by a faction allied
    /// to the player. Its target: its target finder's (`campaign_way`, F050805); none, none.
    fn campaign_check(&mut self) {
        let data = self.data.clone();
        let player_faction = self.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
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
                    self.launch_campaign(cid, &side, true);
                }
                continue;
            }
            let busy = self.world.squads.iter().any(|s| s.alive && matches!(s.goal, Goal::Campaign { .. }) && s.side.as_deref() == Some(fid.as_str()))
                || self.world.triggered.iter().any(|(c, _)| self.scheduled_side(c).as_deref() == Some(fid.as_str()));
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
                let characters = campaign_bool(&data, c, "target characters");
                let Some((target, _)) = self.campaign_way(c, &side) else { continue };
                if campaign_bool(&data, c, "is hostile") && player_faction.as_ref().and_then(|p| self.relations.get(p)).is_some_and(|r| r.is_ally(Some(&side))) {
                    continue;
                }
                let chance = self.campaign_chance(c, &side, target, characters);
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
                self.launch_campaign(&cid, &side, true);
            }
        }
    }

    /// @fn F050802
    ///
    /// Territorial campaign `c`'s chance (0..1) of setting out from faction `side` at this
    /// 6-hourly check on `target`: none unless "territorial triggers" (and not locked); none
    /// while the player's people are fewer than "target population min" (F041596: all of the
    /// player's faction), nor on a base below "target tech level >=" (its size level); none
    /// within its "repeat limit" (days) of its last; none unless it may start
    /// (`campaign_can_start`); with a "range far", none without a town of the faction, none
    /// when its nearest town is farther than that from the target, and scaled down linearly
    /// from 1 at "range near" (F050480; the ranges x 10 as loaded, F050812); times the options'
    /// raid frequency unless it "ignores chance mults"; then "chance per day" x 0.25 x that.
    pub(crate) fn campaign_chance(&self, c: &GameData, side: &str, target: [f32; 3], characters: bool) -> f32 {
        let data = &self.data;
        if !campaign_bool(data, c, "territorial triggers") || self.world.campaigns_locked.contains(&c.string_id) {
            return 0.0;
        }
        let pop = self.folk.iter().filter(|p| p.player && !p.dead).count() as f32;
        if pop < campaign_num(data, c, "target population min") {
            return 0.0;
        }
        if !characters && (self.outpost_level(target) as f32) < campaign_num(data, c, "target tech level >=") {
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
            let Some(home) = self.campaign_home(side, target, 0.0) else { return 0.0 };
            let d = (home[0] - target[0]).hypot(home[2] - target[2]);
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

    /// The faction a scheduled campaign is set out by: its "faction override", else the one
    /// that scheduled it, else a faction listing it.
    fn scheduled_side(&self, cid: &str) -> Option<String> {
        let c = self.data.get(cid)?;
        c.refs("faction override")
            .first()
            .map(|(f, _)| f.clone())
            .or_else(|| self.world.trigger_side.get(cid).cloned())
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
    /// Campaign `cid` scheduled by faction `side`'s war manager (its "faction override"'s when
    /// it has one) to set out `lo` to `hi` game hours from now: not when it is already
    /// scheduled or on its way ("Campaign message sent").
    pub fn schedule_campaign(&mut self, cid: &str, side: &str, lo: f64, hi: f64) {
        let Some(c) = self.data.get(cid) else { return };
        let side = c.refs("faction override").first().map(|(f, _)| f.clone()).unwrap_or(side.to_string());
        // (on its way: its squads up and not yet playing out a victory)
        let running = self.world.squads.iter().any(|s| s.alive && s.won.is_none() && matches!(&s.goal, Goal::Campaign { campaign, .. } if campaign == cid));
        if running || self.world.triggered.iter().any(|(t, _)| t == cid) {
            return;
        }
        let at = self.hours_gone() + lo + (hi - lo) * self.rng.uniform() as f64;
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: Campaign message sent: {} (due at {at:.1} h, by {side})", c.name);
        }
        self.world.trigger_side.insert(cid.to_string(), side);
        self.world.trigger_tries.remove(cid);
        self.world.triggered.push((cid.to_string(), at));
    }

    /// @fn F050818
    ///
    /// The scheduled campaigns now due: one with no target yet put off 5 to 10 hours (dropped
    /// after 10 tries); one that may not start (`campaign_can_start`: "campaign start failed
    /// due to world states or dead leader") dropped; the rest set out.
    fn campaigns_due(&mut self) {
        let now = self.hours_gone();
        let due: Vec<String> = self.world.triggered.iter().filter(|(_, at)| *at <= now).map(|(c, _)| c.clone()).collect();
        if due.is_empty() {
            return;
        }
        let data = self.data.clone();
        for cid in due {
            let Some(c) = data.get(&cid) else {
                self.world.triggered.retain(|(t, _)| *t != cid);
                continue;
            };
            let characters = campaign_bool(&data, c, "target characters") || std::env::var("KENSHI_CAMPAIGN_AT_SQUAD").is_ok();
            if self.campaign_target(characters).is_none() {
                let tries = self.world.trigger_tries.entry(cid.clone()).or_default();
                *tries += 1;
                if *tries > 10 {
                    self.world.triggered.retain(|(t, _)| *t != cid);
                } else {
                    let later = now + 5.0 + 5.0 * self.rng.uniform() as f64;
                    for t in self.world.triggered.iter_mut().filter(|(t, _)| *t == cid) {
                        t.1 = later;
                    }
                }
                continue;
            }
            self.world.triggered.retain(|(t, _)| *t != cid);
            self.world.trigger_tries.remove(&cid);
            if !self.campaign_can_start(c) {
                if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                    eprintln!("world: campaign start failed due to world states or dead leader: {}", c.name);
                }
                continue;
            }
            // one that could not set out is tried again (100 times)
            let side = self.scheduled_side(&cid);
            if !side.is_some_and(|side| self.launch_campaign(&cid, &side, false)) {
                let tries = self.world.trigger_tries.entry(cid.clone()).or_default();
                *tries += 1;
                if *tries <= 100 {
                    self.world.triggered.push((cid.clone(), now));
                }
            }
        }
    }

    /// @fn F050801
    ///
    /// A campaign's leading squad: its first "special leader" alive and free leads it, with its
    /// own squad as it is when it is about (its people the world squad's, setting out from
    /// where they are), else a squad made from the SQUAD_TEMPLATE it leads, set out with the
    /// rest (none: " has no squads!"). The people it adds. (assumption) One not about is made from its template (the
    /// game finds the character wherever it is in the world, F03889d).
    fn special_leader_squad(&mut self, c: &GameData, side: &str, origin: [f32; 3], goal: Goal, speed: f32) -> usize {
        let data = self.data.clone();
        // the SQUAD_TEMPLATE whose "leader" it is (F025b71; "ERROR: unique NPC with more than 1
        // squad referencing it" when several: the first)
        let listing = |l: &str| data.of_type(ItemType::SquadTemplate).find(|t| t.refs("leader").iter().any(|(x, _)| x == l)).map(|t| t.string_id.clone());
        for (l, _) in campaign_refs(&data, c, "special leader") {
            if self.world.dead_uniques.contains(&l) {
                continue;
            }
            let here = self.folk.iter().find(|p| p.character.as_deref() == Some(l.as_str()) && !p.dead && !p.player).map(|p| p.id);
            if let Some(u) = here {
                if self.folk[u].prison.is_some() {
                    continue;
                }
                let members = self.squad_of(u);
                if let Some(old) = self.folk[u].world_squad.and_then(|k| self.world.squads.get_mut(k)) {
                    old.members.retain(|m| !members.contains(m));
                }
                let at = self.folk[u].mover.position_units();
                let k = self.add_world_squad(&listing(&l).unwrap_or_default(), at, goal, speed);
                for &m in &members {
                    self.folk[m].world_squad = Some(k);
                }
                let away = self.folk[u].away;
                let s = &mut self.world.squads[k];
                s.members = members.clone();
                s.active = !away;
                s.faction = self.folk[u].faction.clone();
                s.side = Some(side.to_string());
                return members.len();
            }
            let Some(t) = listing(&l) else { continue };
            let k = self.add_world_squad(&t, origin, goal, speed);
            self.world.squads[k].side = Some(side.to_string());
            return data.get(&t).map_or(1, |t| template_people(t).iter().map(|(_, n)| *n as usize).sum::<usize>().max(1));
        }
        0
    }

    /// Where a campaign marches: the player's base (the middle of their buildings), or for one
    /// that targets characters their first standing character.
    fn campaign_target(&self, characters: bool) -> Option<[f32; 3]> {
        if characters {
            return self.folk.iter().find(|p| p.player && !p.dead && !p.down).map(|p| p.mover.position_units());
        }
        self.player_base()
    }

    /// @fn F050805
    ///
    /// Where territorial campaign `c` of faction `side` marches, and from where (its target
    /// finder): none when the faction has no home (its towns, then its camps: `campaign_home`'s);
    /// the player's outposts (`Sim::outposts`) in the faction's territory (its "biomes"), else
    /// all of them; unless it "ignores nogo zones", none in its "no-go zones"; then the first of
    /// its homes whose nearest outpost is within its "range far" (x 10 as loaded, F050812):
    /// that outpost, from that home. One that "target characters": the player's first standing
    /// character, from wherever `campaign_home` finds. (not ported) The outposts' +0x128 that
    /// keeps one off the list unless the campaign's +0x12 (unknown), the faction's own home list
    /// (F041fe2: here its towns in the world's order), and the character branch's "specific
    /// target NPC" list (+0x78).
    fn campaign_way(&self, c: &GameData, side: &str) -> Option<([f32; 3], Option<[f32; 3]>)> {
        let data = &self.data;
        if campaign_bool(data, c, "target characters") {
            return self.campaign_target(true).map(|t| (t, None));
        }
        let camps = self
            .nests
            .iter()
            .filter(|n| !n.wiped && !n.record.is_empty())
            .filter(|n| data.get(&n.record).is_some_and(|r| r.refs("faction").first().is_some_and(|(f, _)| f == side)))
            .map(|n| n.at);
        let homes: Vec<[f32; 3]> = self.towns.iter().filter(|t| t.faction.as_deref() == Some(side)).map(|t| t.position).chain(camps).collect();
        if homes.is_empty() {
            return None;
        }
        let faction = data.get(side);
        let zone_in = |cat: &str, p: [f32; 3]| {
            let b = self.biome_at(p[0], p[2]).map(|b| b.string_id.as_str());
            faction.is_some_and(|f| f.refs(cat).iter().any(|(x, _)| Some(x.as_str()) == b))
        };
        let mut outposts: Vec<[f32; 3]> = self.outposts.iter().copied().filter(|&o| zone_in("biomes", o)).collect();
        if outposts.is_empty() {
            outposts = self.outposts.clone();
        }
        if !campaign_bool(data, c, "ignores nogo zones") {
            outposts.retain(|&o| !zone_in("no-go zones", o));
        }
        let far = campaign_num(data, c, "range far") * 10.0;
        let d = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
        homes.iter().find_map(|&h| {
            let o = outposts.iter().copied().min_by(|&a, &b| d(a, h).total_cmp(&d(b, h)))?;
            (d(o, h) <= far).then_some((o, Some(h)))
        })
    }

    /// A faction's town or camp nearest `target` (within `far` when above 0). (assumption) Its
    /// camps (nests not wiped out, of its TOWN "faction") count as its towns: the bandit
    /// factions have no others.
    fn campaign_home(&self, side: &str, target: [f32; 3], far: f32) -> Option<[f32; 3]> {
        let camps: Vec<[f32; 3]> = self
            .nests
            .iter()
            .filter(|n| !n.wiped && !n.record.is_empty())
            .filter(|n| self.data.get(&n.record).is_some_and(|r| r.refs("faction").first().is_some_and(|(f, _)| f == side)))
            .map(|n| n.at)
            .collect();
        (0..self.towns.len())
            .filter(|&t| self.towns[t].faction.as_deref() == Some(side))
            .map(|t| self.towns[t].position)
            .chain(camps)
            .filter(|p| far <= 0.0 || (p[0] - target[0]).hypot(p[2] - target[2]) <= far)
            .min_by(|a, b| (a[0] - target[0]).hypot(a[2] - target[2]).total_cmp(&(b[0] - target[0]).hypot(b[2] - target[2])))
    }

    /// @fn F048905
    /// @fn F048906
    /// @fn F048907
    ///
    /// Town::setupResidents / distributeResidents for the buildings of the zone just loaded
    /// (placements from `from` on): a town whose buildings here have no resident squad in the
    /// zone state (Kenshi hands its residents out as it sets the town up, its rows at Town
    /// +0x300) gets them now. Its rows: its faction's "residents" then the town's (as it is
    /// now; only the town's with "residents override"), each once, its number (val0) and
    /// priority (val1). By priority, 5 down to 0 (a higher one never: as the game), each row
    /// takes one building at a time (`resident_building`) until its number is filled or no
    /// building is left for it; the buildings left the "default resident" (the town's, else,
    /// without the override, its faction's). Each building's squad is spawned at it.
    /// (assumption) The town's buildings: its BUILDING placements in this zone within its
    /// radius, not destroyed, nor a feature, foliage, node, furniture, sign or gate; a resident whose leader is a unique already about (or dead) is
    /// not made again (a town over two zones).
    pub(crate) fn setup_residents(&mut self, from: usize) {
        if self.quiet_zones || from >= self.placed.len() {
            return;
        }
        let data = self.data.clone();
        if self.layouts.is_none() {
            self.layouts = Some(kenshi_world::nodes::Layouts::load(&self.install.clone()));
        }
        let mut towns: Vec<usize> = Vec::new();
        for i in from..self.placed.len() {
            if let Some(t) = kenshi_world::town::current_town(&self.towns, self.placed_at[i], None)
                && !towns.contains(&t)
            {
                towns.push(t);
            }
        }
        for t in towns {
            let homes: Vec<usize> = (from..self.placed.len()).filter(|&i| self.towns[t].contains(self.placed_at[i], 1.0)).collect();
            if homes.is_empty() || homes.iter().any(|&i| self.placed[i].squad.is_some() || self.placed[i].residents.is_some()) {
                continue;
            }
            let Some(rec) = data.get(&self.town_record(t)) else { continue };
            let overrides = rec.bools.get("residents override").copied().unwrap_or(false);
            let faction = self.towns[t].faction.as_deref().and_then(|f| data.get(f)).filter(|_| !overrides);
            let mut rows: Vec<(String, i32, i32)> = Vec::new();
            for r in faction.into_iter().chain(std::iter::once(rec)) {
                for (sq, v) in r.refs("residents") {
                    if !rows.iter().any(|x| x.0 == *sq) {
                        rows.push((sq.clone(), v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0)));
                    }
                }
            }
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
            if let Some(f) = filler {
                given.extend(free.iter().map(|&b| (b, f.clone())));
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

    /// Whether campaign squad `k` is there (F050809): at the end of its way, or within 400 units
    /// of a character its campaign targets (`ARRIVED_AT_CHARACTER`), 0.8 of an outpost's radius
    /// of one (`ARRIVED_AT_TOWN`, F048764). (assumption) The game measures from the player's
    /// people (its target's), here from the squad.
    pub(crate) fn campaign_there(&self, k: usize) -> bool {
        let s = &self.world.squads[k];
        let Goal::Campaign { campaign, target, .. } = &s.goal else { return s.route.is_empty() };
        let characters = self.data.get(campaign).is_some_and(|c| campaign_bool(&self.data, c, "target characters"));
        let near = if characters { ARRIVED_AT_CHARACTER } else { ARRIVED_AT_TOWN * kenshi_world::town::radius(1, 1.0) };
        s.route.is_empty() || (s.pos[0] - target[0]).hypot(s.pos[2] - target[2]) < near
    }

    /// Whether campaign `cid` is assaulting: an ASSAULT_TOWN campaign (its "key" 0) in its
    /// assault phase.
    fn assaulting(&self, cid: &str) -> bool {
        let key = self.data.get(cid).map_or(0.0, |c| campaign_num(&self.data, c, "key"));
        key == 0.0 && self.world.phases.get(cid).is_some_and(|p| p.phase == PHASE_ASSAULT)
    }

    /// Campaign `cid` to phase `phase` (a line's DA_ASSAULT_PHASE / DA_RETREAT_PHASE /
    /// DA_VICTORY_PHASE, F040912 cases 0x25..0x27, and the arrival): the assault (1) when the
    /// campaign has one (its "pt2 AI leader"), else over (2); over, its squads go home (no
    /// follow-ups); victory (3), its squads play out its victory (`raid`, no follow-ups). Both
    /// clocks restart.
    pub fn set_campaign_phase(&mut self, cid: &str, phase: u8) {
        let Some(c) = self.data.get(cid).cloned() else { return };
        let phase = if phase == PHASE_ASSAULT && campaign_refs(&self.data, &c, "pt2 AI leader").is_empty() { PHASE_OVER } else { phase };
        let now = self.hours_gone();
        self.world.phases.insert(cid.to_string(), Phase { phase, since: now, quiet: now });
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

    /// @fn F050809
    ///
    /// Every second, each running campaign's phase (CampaignInstance::update):
    /// - on the way (0): once any of its squads is there (`campaign_there`), with a "dialog
    ///   announcement" when it has been made (its lines may set the phase themselves), else at
    ///   once, the assault (1) when it has one ("pt2 AI leader"), else over (2);
    /// - the assault (1), by its "key": ASSAULT_TOWN won after 4 hours, or once the player's
    ///   people about have been all down for 0.2 hours; DEFEND_TOWN done after 12 hours,
    ///   TRADER_VISIT after 1 (done: won); lost once under a fifth of its people stand;
    /// - over (2): forgotten after 3 hours.
    ///   (not ported) The checks the game makes past the hours (F001a23, F009acf: assumed to
    ///   pass) and the home-town test of a character target. (assumption) The defenders: the
    ///   player's people within 3000 units of the target.
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
            self.world.phases.entry(c).or_insert(Phase { phase: PHASE_APPROACH, since: now, quiet: now });
        }
        let ids: Vec<String> = self.world.phases.keys().cloned().collect();
        for cid in ids {
            let Some(ph) = self.world.phases.get(&cid).copied() else { continue };
            let squads: Vec<usize> = (0..self.world.squads.len()).filter(|&k| matches!(&self.world.squads[k].goal, Goal::Campaign { campaign, .. } if *campaign == cid)).collect();
            let Some(c) = self.data.get(&cid).cloned() else {
                self.world.phases.remove(&cid);
                continue;
            };
            match ph.phase {
                PHASE_APPROACH => {
                    let there = squads.iter().any(|&k| self.world.squads[k].alive && self.campaign_there(k));
                    if !there {
                        continue;
                    }
                    let announcement = !campaign_refs(&self.data, &c, "dialog announcement").is_empty();
                    if announcement && !squads.iter().any(|&k| self.world.squads[k].announced) {
                        continue;
                    }
                    // (the arrival's message, "{1} has arrived at {2}", is `announce`'s)
                    self.set_campaign_phase(&cid, PHASE_ASSAULT);
                }
                PHASE_ASSAULT => {
                    let key = campaign_num(&self.data, &c, "key") as i32;
                    let hours = now - ph.since;
                    let won = match key {
                        0 => {
                            // the defenders about the target: the player's people near it
                            let target = squads.iter().find_map(|&k| match &self.world.squads[k].goal {
                                Goal::Campaign { target, .. } => Some(*target),
                                _ => None,
                            });
                            let up = target.is_some_and(|t| {
                                self.folk.iter().any(|p| {
                                    let q = p.mover.position_units();
                                    p.player && !p.dead && !p.down && !p.away && (q[0] - t[0]).hypot(q[2] - t[2]) < 3000.0
                                })
                            });
                            let mut quiet = ph.quiet;
                            if up {
                                quiet = now;
                                if let Some(p) = self.world.phases.get_mut(&cid) {
                                    p.quiet = now;
                                }
                            }
                            hours > 4.0 || (!up && now - quiet > 0.2)
                        }
                        2 => hours > 12.0,
                        3 => hours > 1.0,
                        _ => false,
                    };
                    if won {
                        self.campaign_over(&cid, true);
                        continue;
                    }
                    // its forces under a fifth standing: lost
                    let (all, standing) = squads
                        .iter()
                        .filter(|&&k| self.world.squads[k].active)
                        .flat_map(|&k| self.world.squads[k].members.iter().copied())
                        .fold((0usize, 0usize), |(a, s), m| (a + 1, s + usize::from(!self.folk[m].dead && !self.folk[m].down)));
                    if all > 0 && (standing as f32) < 0.2 * all as f32 {
                        self.campaign_over(&cid, false);
                    }
                }
                PHASE_OVER if now - ph.since > 3.0 => {
                    self.world.phases.remove(&cid);
                }
                _ => {}
            }
        }
    }

    /// @fn F050815
    ///
    /// A campaign sets out: "num forces" people of its "squads to use" from its faction's home
    /// nearest the target (else 15000 units off), at its "travel speed unloaded"; its last
    /// time kept (its repeat limit); "Warning: {1} moving towards {2}" for a hostile one, "{1}
    /// on the way to {2}" for another ({1} its "display name", {2} the player's outpost or
    /// person); then each of its "trigger player ally" campaigns whose faction ("faction
    /// override") is the player's ally scheduled at once (F050814). Whether it did.
    fn launch_campaign(&mut self, cid: &str, side: &str, territorial: bool) -> bool {
        let data = self.data.clone();
        let Some(c) = data.get(cid) else { return false };
        // (testing) KENSHI_CAMPAIGN_AT_SQUAD: at the player's people even when it wants a base
        let characters = campaign_bool(&data, c, "target characters") || std::env::var("KENSHI_CAMPAIGN_AT_SQUAD").is_ok();
        // a territorial one where its target finder sends it (F050805); (assumption) a
        // scheduled one (a line's, a follow-up: most have no "range far" for the finder) at the
        // player's base or people from the faction's home nearest
        let way = if territorial && !characters { self.campaign_way(c, side) } else { None };
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: {} target finder: {way:?} (outposts {:?})", c.name, self.outposts);
        }
        let Some((target, home)) = way.or_else(|| self.campaign_target(characters).map(|t| (t, None))) else { return false };
        let far = campaign_num(&data, c, "range far") * 10.0;
        let origin = home.or_else(|| self.campaign_home(side, target, if far > 0.0 { far.max(20000.0) } else { 0.0 })).unwrap_or_else(|| {
            let a = self.rng.uniform() * std::f32::consts::TAU;
            [target[0] + a.cos() * 15000.0, target[1], target[2] + a.sin() * 15000.0]
        });
        // (testing) KENSHI_CAMPAIGN_NEAR: from 5000 units off
        let origin = if std::env::var("KENSHI_CAMPAIGN_NEAR").is_ok() { [target[0] + 5000.0, target[1], target[2]] } else { origin };
        let squads: Vec<String> = campaign_refs(&data, c, "squads to use").into_iter().map(|(s, _)| s).collect();
        if squads.is_empty() {
            return false;
        }
        let hostile = campaign_bool(&data, c, "is hostile");
        // F050803: "num forces" x the options' raid size
        let size = if self.world.raid_size > 0.0 { self.world.raid_size } else { 1.0 };
        let forces = (campaign_num(&data, c, "num forces") * size).max(1.0);
        let speed = SPEED * campaign_num(&data, c, "travel speed unloaded").max(1.0);
        let goal = Goal::Campaign { campaign: cid.to_string(), target, there: 0.0, hostile };
        let mut people = self.special_leader_squad(c, side, origin, goal.clone(), speed) as f32;
        let mut j = 0;
        while people < forces && j < 12 {
            let sq = squads[j % squads.len()].clone();
            people += data.get(&sq).map_or(1, |t| template_people(t).iter().map(|(_, n)| *n).sum::<i32>().max(1)) as f32;
            let k = self.add_world_squad(&sq, origin, goal.clone(), speed);
            self.world.squads[k].side = Some(side.to_string());
            j += 1;
        }
        *self.world.campaigns_run.entry(cid.to_string()).or_insert(0) += 1;
        let now = self.hours_gone();
        self.world.campaign_last.insert(cid.to_string(), now);
        self.world.phases.insert(cid.to_string(), Phase { phase: PHASE_APPROACH, since: now, quiet: now });
        let shown = campaign_str(&data, c, "display name").unwrap_or_else(|| data.get(side).map_or(side.to_string(), |r| r.name.clone()));
        let to = if characters {
            self.folk.iter().find(|p| p.player && !p.dead && !p.down).map(|p| p.id).map_or("you".to_string(), |p| self.display_name(p))
        } else {
            self.outpost_name.clone()
        };
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
                self.schedule_campaign(&ally, &f, 0.0, 0.0);
            }
        }
        true
    }

    /// A campaign won (the player's people beaten) or lost (its own): its squads play out
    /// their victory or go home, and one of its "victory trigger" / "loss trigger" campaigns
    /// (`pick_campaign`: a single roll over their chances) is scheduled by the same faction
    /// between the ref's first and second values of hours.
    pub(crate) fn campaign_over(&mut self, cid: &str, won: bool) {
        let ks: Vec<usize> = (0..self.world.squads.len())
            .filter(|&k| self.world.squads[k].alive && matches!(&self.world.squads[k].goal, Goal::Campaign { campaign, .. } if campaign == cid))
            .collect();
        let data = self.data.clone();
        let side = self.world.squads.iter().find(|s| matches!(&s.goal, Goal::Campaign { campaign, .. } if campaign == cid)).and_then(|s| s.side.clone()).or_else(|| self.scheduled_side(cid));
        let victory = data.get(cid).filter(|_| won).map_or(crate::raid::Victory::Leave, |c| crate::raid::victory_of(&data, c));
        for k in ks {
            self.begin_victory(k, victory);
        }
        let now = self.hours_gone();
        if let Some(ph) = self.world.phases.get_mut(cid) {
            *ph = Phase { phase: PHASE_OVER, since: now, quiet: now };
        }
        let Some(c) = data.get(cid) else { return };
        let list = campaign_refs(&data, c, if won { "victory trigger" } else { "loss trigger" });
        if let (Some((next, lo, hi)), Some(side)) = (self.pick_campaign(&list), side) {
            self.schedule_campaign(&next, &side, lo, hi.max(lo));
        }
        eprintln!("world: campaign {} {}", c.name, if won { "won" } else { "lost" });
    }

}
