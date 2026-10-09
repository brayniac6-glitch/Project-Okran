//! The world beyond the squad (port_map `world`): squads travelling the whole map while no one
//! is near (Kenshi's UnloadedPlatoon / AbstractMovementBase), towns' patrols, faction raids on
//! NPC towns and the player's base (FactionWarMgr's campaigns), towns' garrisons, the unique
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
//! - FactionWarMgr (F050818): the campaigns are checked every 6 game hours; a campaign
//!   (FACTION_CAMPAIGN, fields inherited through "inherits from") of a faction's "campaigns"
//!   triggers with its "chance per day" share, its world states holding, within its repeat
//!   limit and its "target population min"; "range near" / "range far" are x 10 (F050812).
//!   Its forces are "num forces" people from its "squads to use".
//!
//! (assumption) The game's abstract movement, its NPC-town raids and its battles off-screen are
//! not decompiled. Here: away squads walk their long-distance route (`kenshi_world::longpath`)
//! at 60 units a second, 90 while over 25000 from the target (F045434; campaigns x their
//! "travel speed unloaded"); a squad within 4000 units of
//! a player character on loaded ground is made real (its people kept for when it comes back)
//! and walks its route on the navmesh; one beyond 6000 goes away again. Each town keeps its
//! "roaming squads" (one each) patrolling to other towns of its faction or open to it, waiting
//! 2 to 6 hours at each. Every 6 hours a faction with towns may raid an enemy town within 40000
//! units with chance 0.02, two of its roaming squads. Off-screen fights: a raid against a
//! town's garrison, or enemy squads within 400 units of each other: the side with the more
//! strength (each person 1 + (attack + defence) / 40 of its character's stats) wins by chance in
//! proportion and loses half the loser's strength; a garrison beaten to nothing lets the town
//! fall: its unique people die (the world states follow), and it recovers a tenth a day. A
//! campaign's forces set out from the faction's town or camp nearest the base (or 15000 units off),
//! march on the centre of the player's buildings, fight when there, and go home when beaten or
//! after 24 hours.

use crate::sim::Sim;
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_world::longpath::WorldGrid;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Away squads' walking speed (units a second): an unloaded platoon walking to its target
/// (F045434, UnloadedPlatoon's tick, mode 6) goes at 60, or 90 while the target is over 25000
/// units off; the base speed class 25.
/// (assumption) Patrols, raids and campaigns take mode 6 (their AI package's mode is not read).
const SPEED: f32 = 60.0;
const SPEED_FAR: f32 = 90.0;
const FAR_TARGET: f32 = 25000.0;
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
}

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
    /// Campaigns triggered (record -> times).
    pub campaigns_run: HashMap<String, u32>,
    pub timer: f32,
    /// Game hours of the next 6-hourly check, and of the last garrison recovery.
    pub next_check: f64,
    pub started: bool,
    /// Characters (records) died this tick, for the world states.
    pub pending_deaths: Vec<String>,
    /// Towns (records) the player has found: the map shows only these.
    pub discovered: HashSet<String>,
    /// Campaigns a dialogue line triggered ("trigger campaign"), each from the game hour it is
    /// due (crate::dialogue).
    pub triggered: Vec<(String, f64)>,
}

/// (assumption) How near a player character comes to a town to find it (world units).
const DISCOVER_RANGE: f32 = 3000.0;

/// Whether a TOWN "type" takes part in raids: towns (2) and forts (1).
fn raidable(kind: i32) -> bool {
    kind == 1 || kind == 2
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

/// A campaign's refs of a category through its inheritance (its own, then its bases').
fn campaign_refs(data: &GameDataContainer, c: &GameData, cat: &str) -> Vec<(String, Vec<i32>)> {
    let mut out: Vec<(String, Vec<i32>)> = c.refs(cat).iter().map(|(s, v)| (s.clone(), v.to_vec())).collect();
    if out.is_empty() {
        for (b, _) in c.refs("inherits from") {
            if let Some(base) = data.get(b)
                && base.string_id != c.string_id
            {
                out.extend(campaign_refs(data, base, cat));
            }
        }
    }
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
    /// Whether WORLD_EVENT_STATE `state` holds now.
    pub fn world_state(&self, state: &str) -> bool {
        let Some(s) = self.data.get(state) else { return false };
        // a unique's state: dead 0, imprisoned 2, else alive 1
        let npc = |c: &str| {
            if self.world.dead_uniques.contains(c) {
                0
            } else if self.folk.iter().any(|p| p.character.as_deref() == Some(c) && p.prison.is_some() && !p.dead) {
                2
            } else {
                1
            }
        };
        let player = self.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
        let rel = player.as_ref().and_then(|f| self.relations.get(f));
        // a town destroyed: its record now (as last worked out) one of ruins
        let okay = |t: &str| {
            let k = self.towns.iter().position(|x| x.record == t);
            let now = k.and_then(|k| self.world.towns.get(k)).map_or(t.to_string(), |s| s.record.clone());
            !self.data.get(&now).is_some_and(|r| r.refs("residents").iter().any(|(s, _)| self.data.get(s).is_some_and(|s| s.name.contains("ruined"))))
        };
        s.refs("NPC is").iter().all(|(c, v)| npc(c) == v[0])
            && s.refs("NPC is NOT").iter().all(|(c, v)| npc(c) != v[0])
            && s.refs("player ally").iter().all(|(f, v)| rel.is_some_and(|r| r.is_ally(Some(f))) == (v[0] != 0))
            && s.refs("player enemy").iter().all(|(f, v)| rel.is_some_and(|r| r.is_enemy(Some(f))) == (v[0] != 0))
            && s.refs("town okay").iter().all(|(t, v)| okay(t) == (v[0] != 0))
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

    /// The unique characters living in a TOWN record's resident squads.
    fn town_uniques(&self, record: &str) -> Vec<String> {
        let Some(t) = self.data.get(record) else { return Vec::new() };
        let mut out = Vec::new();
        for (sq, _) in t.refs("residents") {
            let Some(sq) = self.data.get(sq) else { continue };
            for (c, _) in template_people(sq) {
                if self.data.get(&c).is_some_and(|r| r.bools.get("unique").copied().unwrap_or(false)) {
                    out.push(c);
                }
            }
        }
        out
    }

    /// A garrison's full strength: its residents' people.
    fn garrison_of(&self, record: &str) -> f32 {
        let Some(t) = self.data.get(record) else { return 0.0 };
        t.refs("residents").iter().map(|(sq, v)| template_strength(&self.data, sq) * v.first().copied().unwrap_or(1).max(1) as f32).sum()
    }

    /// The towns' states again after world states changed: each town takes its override, with
    /// a message when one changes hands or falls.
    fn update_towns(&mut self) {
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
            }
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
        let mut s = WorldSquad { template: template.to_string(), faction, pos: at, route: Vec::new(), goal, speed, strength, members: Vec::new(), active: false, alive: true, order_timer: 0.0 };
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
    fn route_for(&self, s: &WorldSquad) -> Vec<[f32; 3]> {
        let to = self.goal_point(&s.goal);
        self.world.grid.as_ref().and_then(|g| g.route(s.pos, to)).unwrap_or_else(|| vec![to])
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

    /// The world each tick (its work once a second).
    /// The towns near the player's characters found (the map shows them from then on).
    fn discover_towns(&mut self) {
        let at: Vec<[f32; 3]> = self.folk.iter().filter(|p| p.player && !p.dead && !p.away).map(|p| p.mover.position_units()).collect();
        for t in &self.towns {
            if self.world.discovered.contains(&t.record) {
                continue;
            }
            let r = DISCOVER_RANGE * t.scale.max(1.0);
            if at.iter().any(|p| (p[0] - t.position[0]).powi(2) + (p[2] - t.position[2]).powi(2) < r * r) {
                self.world.discovered.insert(t.record.clone());
            }
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
        let now = self.hours_gone();
        // the 6-hourly checks: raids, campaigns, garrisons recovering
        if now >= self.world.next_check {
            self.world.next_check = now + 6.0;
            for t in &mut self.world.towns {
                t.garrison = (t.garrison + t.full * 0.1 * 0.25).min(t.full);
            }
            self.npc_raids();
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
        self.encounters();
    }

    /// An away squad walks its route; arriving, its goal.
    fn away_squad(&mut self, k: usize, dt: f32) {
        let s = &mut self.world.squads[k];
        // F045434: 90 while the target is over 25000 off, else the squad's own speed
        let goal_at = s.route.last().copied().unwrap_or(s.pos);
        let far = (goal_at[0] - s.pos[0]).hypot(goal_at[2] - s.pos[2]) > FAR_TARGET;
        let mut step = if far { s.speed.max(SPEED_FAR * s.speed / SPEED) } else { s.speed } * dt;
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
            Goal::Raid { town } => {
                if !self.world.squads[k].active {
                    self.raid_town(k, town);
                }
            }
            Goal::Campaign { campaign, target, there, hostile } => {
                let there = there + dt / self.hour_secs as f32;
                self.world.squads[k].goal = Goal::Campaign { campaign, target, there, hostile };
                // (assumption) a visit (not hostile) stays 4 hours
                if there > if hostile { 24.0 } else { 4.0 } {
                    self.go_home(k);
                }
            }
            Goal::Home { .. } => {
                if !self.world.squads[k].active {
                    self.world.squads[k].alive = false;
                }
            }
        }
    }

    /// Send a squad home (its origin town or where it came from).
    pub(crate) fn go_home(&mut self, k: usize) {
        let s = &self.world.squads[k];
        let to = s
            .faction
            .as_deref()
            .and_then(|f| {
                (0..self.towns.len()).filter(|&t| self.towns[t].faction.as_deref() == Some(f)).min_by(|&a, &b| {
                    let d = |t: usize| (self.towns[t].position[0] - s.pos[0]).hypot(self.towns[t].position[2] - s.pos[2]);
                    d(a).total_cmp(&d(b))
                })
            })
            .map_or([s.pos[0] + 15000.0, s.pos[1], s.pos[2]], |t| self.towns[t].position);
        self.world.squads[k].goal = Goal::Home { to };
        self.world.squads[k].route = self.route_for(&self.world.squads[k]);
    }

    /// Off-screen: a raid on a town's garrison.
    fn raid_town(&mut self, k: usize, town: usize) {
        let a = self.world.squads[k].strength;
        let g = self.world.towns[town].garrison;
        let win = self.rng.uniform() < a / (a + g).max(1e-3);
        let tname = self.towns[town].name.clone();
        let fname = self.world.squads[k].faction.as_deref().and_then(|f| self.data.get(f)).map_or("raiders".to_string(), |r| r.name.clone());
        if win {
            self.world.squads[k].strength = (a - g * 0.5).max(1.0);
            self.world.towns[town].garrison = 0.0;
            self.messages.push(format!("{tname} has fallen to {fname}."));
            // its unique people die
            let rec = self.world.towns[town].record.clone();
            for u in self.town_uniques(&rec) {
                self.unique_died(&u);
            }
        } else {
            self.world.towns[town].garrison = (g - a * 0.5).max(0.0);
            self.world.squads[k].alive = false;
            if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                eprintln!("world: {fname} raid on {tname} beaten off");
            }
            return;
        }
        self.go_home(k);
    }

    /// Off-screen fights between away squads of enemy factions within 400 units.
    fn encounters(&mut self) {
        let n = self.world.squads.len();
        for a in 0..n {
            for b in a + 1..n {
                let (sa, sb) = (&self.world.squads[a], &self.world.squads[b]);
                if !sa.alive || !sb.alive || sa.active || sb.active {
                    continue;
                }
                if (sa.pos[0] - sb.pos[0]).hypot(sa.pos[2] - sb.pos[2]) > 400.0 {
                    continue;
                }
                let enemy = sa.faction.as_ref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(sb.faction.as_deref()));
                if !enemy {
                    continue;
                }
                let (x, y) = (sa.strength, sb.strength);
                let (win, lose) = if self.rng.uniform() < x / (x + y) { (a, b) } else { (b, a) };
                let ls = self.world.squads[lose].strength;
                self.world.squads[win].strength = (self.world.squads[win].strength - ls * 0.5).max(1.0);
                self.world.squads[lose].alive = false;
                if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                    let name = |k: usize| self.data.get(&self.world.squads[k].template).map_or("?", |r| r.name.as_str()).to_string();
                    eprintln!("world: {} beat {} off-screen", name(win), name(lose));
                }
            }
        }
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
        self.world.squads[k].active = false;
        self.world.squads[k].route = self.route_for(&self.world.squads[k]);
    }

    /// A real squad: its strength from its people standing, its next waypoint ordered, its goal
    /// played out (a campaign's attack).
    fn real_squad(&mut self, k: usize, dt: f32) {
        let members = self.world.squads[k].members.clone();
        let standing: Vec<usize> = members.iter().copied().filter(|&i| !self.folk[i].dead && !self.folk[i].down).collect();
        if standing.is_empty() {
            // beaten: the rest lie where they fell
            self.world.squads[k].alive = false;
            self.world.squads[k].active = false;
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
        // a campaign at its target: attack the player's people near
        if let Goal::Campaign { hostile: true, .. } = self.world.squads[k].goal
            && self.world.squads[k].route.is_empty()
        {
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
                None => self.go_home(k),
                _ => {}
            }
            self.arrive(k, dt);
            return;
        }
        if fighting {
            return;
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
        if self.world.squads[k].order_timer <= 0.0 {
            self.world.squads[k].order_timer = 5.0;
            // (assumption) toward the waypoint, at most 1500 units a leg on the loaded navmesh
            let w = self.world.squads[k].route[0];
            let (dx, dz) = (w[0] - at[0], w[2] - at[2]);
            let d = dx.hypot(dz).max(1.0);
            let leg = d.min(1500.0);
            let to = [at[0] + dx / d * leg, w[1], at[2] + dz / d * leg];
            self.order_move_squad(&standing, to);
        }
    }

    /// Every 6 hours: factions raid enemy towns near theirs.
    fn npc_raids(&mut self) {
        let data = self.data.clone();
        let n = self.towns.len();
        let mut pairs = 0;
        for t in 0..n {
            let Some(f) = self.towns[t].faction.clone() else { continue };
            if !raidable(self.towns[t].kind) {
                continue;
            }
            let Some(rel) = self.relations.get(&f) else { continue };
            let Some(rec) = data.get(&self.world.towns[t].record) else { continue };
            // raiders: the faction's own "squads", else the town's roaming squads
            let mut roaming: Vec<String> = data.get(&f).map_or(Vec::new(), |r| r.refs("squads").iter().map(|(s, _)| s.clone()).collect());
            if roaming.is_empty() {
                roaming = rec.refs("roaming squads").iter().map(|(s, _)| s.clone()).collect();
            }
            if roaming.is_empty() {
                continue;
            }
            let at = self.towns[t].position;
            let targets: Vec<usize> = (0..n)
                .filter(|&k| raidable(self.towns[k].kind) && rel.is_enemy(self.towns[k].faction.as_deref()))
                .filter(|&k| {
                    let p = self.towns[k].position;
                    (p[0] - at[0]).hypot(p[2] - at[2]) < 40000.0
                })
                .collect();
            pairs += targets.len();
            for target in targets {
                // (testing) KENSHI_RAID_CHANCE overrides the chance
                let chance = std::env::var("KENSHI_RAID_CHANCE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.01);
                if self.rng.uniform() < chance {
                    let msg = format!(
                        "{} are marching on {}.",
                        data.get(&f).map_or(f.clone(), |r| r.name.clone()),
                        self.towns[target].name
                    );
                    for j in 0..2 {
                        let sq = roaming[j % roaming.len()].clone();
                        let k = self.add_world_squad(&sq, at, Goal::Raid { town: target }, SPEED);
                        self.world.squads[k].faction.get_or_insert(f.clone());
                    }
                    self.messages.push(msg);
                }
            }
        }
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: raid check at {:.0} h: {pairs} town pairs at war in reach", self.hours_gone());
        }
    }

    /// @fn F050818
    ///
    /// Every 6 hours: the factions' campaigns against the player's base (see the module notes).
    fn campaign_check(&mut self) {
        let Some(base) = self.player_base() else { return };
        let data = self.data.clone();
        let players = self.folk.iter().filter(|p| p.player && !p.dead).count() as f32;
        let player_faction = self.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
        for faction in data.of_type(ItemType::Faction) {
            for (cid, _) in faction.refs("campaigns") {
                let Some(c) = data.get(cid) else { continue };
                // (testing) KENSHI_CAMPAIGN=name; or a dialogue's trigger now due
                let now = self.hours_gone();
                let due = self.world.triggered.iter().position(|(t, at)| t == cid && *at <= now);
                if let Some(k) = due {
                    self.world.triggered.remove(k);
                }
                let forced = due.is_some() || std::env::var("KENSHI_CAMPAIGN").is_ok_and(|n| n == c.name);
                let chance = campaign_num(&data, c, "chance per day");
                if chance <= 0.0 && !forced {
                    continue;
                }
                let limit = campaign_num(&data, c, "repeat limit");
                if !forced && limit > 0.0 && self.world.campaigns_run.get(cid).copied().unwrap_or(0) as f32 >= limit {
                    continue;
                }
                if !forced && players < campaign_num(&data, c, "target population min") {
                    continue;
                }
                if !forced && !c.refs("world state").iter().all(|(s, v)| self.world_state(s) == (v.first().copied().unwrap_or(1) != 0)) {
                    continue;
                }
                let side = c.refs("faction override").first().map(|(f, _)| f.clone()).unwrap_or(faction.string_id.clone());
                // a hostile campaign of a faction allied to the player does not come
                let hostile = campaign_num(&data, c, "is hostile") != 0.0 || c.bools.get("is hostile").copied().unwrap_or(false);
                let allied = player_faction.as_ref().and_then(|p| self.relations.get(p)).is_some_and(|r| r.is_ally(Some(&side)));
                if !forced && hostile && allied {
                    continue;
                }
                // the share of a day's chance in 6 hours; (testing) KENSHI_CAMPAIGN=name: that one
                if !forced && self.rng.uniform() >= chance * 6.0 / 24.0 {
                    continue;
                }
                let far = campaign_num(&data, c, "range far") * 10.0;
                // (assumption) a faction's camps (nests not wiped out, of its TOWN "faction") count
                // as its towns: the bandit factions have no others
                let camps: Vec<[f32; 3]> = self
                    .nests
                    .iter()
                    .filter(|n| !n.wiped && !n.record.is_empty())
                    .filter(|n| data.get(&n.record).is_some_and(|r| r.refs("faction").first().is_some_and(|(f, _)| *f == side)))
                    .map(|n| n.at)
                    .collect();
                let origin = (0..self.towns.len())
                    .filter(|&t| self.towns[t].faction.as_deref() == Some(side.as_str()))
                    .map(|t| self.towns[t].position)
                    .chain(camps)
                    .filter(|p| far <= 0.0 || (p[0] - base[0]).hypot(p[2] - base[2]) <= far.max(20000.0))
                    .min_by(|a, b| (a[0] - base[0]).hypot(a[2] - base[2]).total_cmp(&(b[0] - base[0]).hypot(b[2] - base[2])))
                    .unwrap_or_else(|| {
                        let a = self.rng.uniform() * std::f32::consts::TAU;
                        [base[0] + a.cos() * 15000.0, base[1], base[2] + a.sin() * 15000.0]
                    });
                // (testing) KENSHI_CAMPAIGN_NEAR: from 5000 units off
                let origin = if forced && std::env::var("KENSHI_CAMPAIGN_NEAR").is_ok() { [base[0] + 5000.0, base[1], base[2]] } else { origin };
                let squads: Vec<String> = campaign_refs(&data, c, "squads to use").into_iter().map(|(s, _)| s).collect();
                if squads.is_empty() {
                    continue;
                }
                let forces = campaign_num(&data, c, "num forces").max(1.0);
                let speed = SPEED * campaign_num(&data, c, "travel speed unloaded").max(1.0);
                let mut people = 0.0;
                let mut j = 0;
                while people < forces && j < 12 {
                    let sq = squads[j % squads.len()].clone();
                    people += data.get(&sq).map_or(1, |t| template_people(t).iter().map(|(_, n)| *n).sum::<i32>().max(1)) as f32;
                    self.add_world_squad(&sq, origin, Goal::Campaign { campaign: cid.clone(), target: base, there: 0.0, hostile }, speed);
                    j += 1;
                }
                *self.world.campaigns_run.entry(cid.clone()).or_insert(0) += 1;
                let name = data.get(&side).map_or(side.clone(), |r| r.name.clone());
                self.messages.push(if hostile {
                    format!("{name}: {} - they are marching on your base!", c.name)
                } else {
                    format!("{name}: {} - they are coming to your base.", c.name)
                });
                eprintln!("world: campaign {} triggered from {origin:?}", c.name);
            }
        }
    }
}
