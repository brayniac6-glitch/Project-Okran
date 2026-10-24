//! The raid and return goals (port_map `ai`): the planner's hooks (scores, conditions, target
//! finders) and the task handlers of the goals squads go home and raid by.
//!
//! - Retreating (128, "Go Home Town") and Retreating fast (127, "Go Home (Run away)"): the score
//!   F038688 (AI slot 56), Task_GoHomeTown / Task_GoHomeTownFast (F031066: where home is; walking,
//!   running). A campaign's squads going home take its "retreat AI leader" package (its "victory
//!   AI leader" once it has won), and those whose package has Retreating walk home on it
//!   (crate::world leaves them be).
//!
//! As the law's (crate::law), the AI's view (crate::sim View) asks `RaidView` for what it does
//! not answer itself; `RAID_RUNNABLE` are carried out here (`raid_tasks_tick`).
use crate::sim::{AT_LOCATION_SQ, Sim, Target};
use crate::world::{Goal, campaign_refs};
use kenshi_ai::task_def::{Score, TaskDef};
use kenshi_char::movement::{MoveSpeed, speed_limit};
use std::collections::HashMap;

pub const RETREATING_FAST: u16 = 127;
pub const RETREATING: u16 = 128;
/// The runnable ones (their handlers are `raid_tasks_tick`'s).
pub const RAID_RUNNABLE: [u16; 2] = [RETREATING_FAST, RETREATING];
/// The tasks whose target finders are here.
pub const RAID_FINDERS: [u16; 0] = [];
/// Retreating's score (both; AI vtable slot 56, F038688).
const RETREAT_SCORE: Score = Score::Method(56);
/// F038688: a slave not the player's 0.01, anyone else 0.5 (0x141682ad8, 0x14167e8d8).
const RETREAT_SLAVE: f32 = 0.01;
const RETREAT: f32 = 0.5;
/// F031066: with nowhere to go, a random point this far about (0x1416bfcac / 0x1416bfcb0).
const WANDER: f32 = 5000.0;
/// (stand-in: needs the pathfinder's way beyond the loaded navmesh) A world squad's way home on
/// the loaded navmesh: legs of at most this toward its route's next point (as the world's own
/// legs, crate::world).
const HOME_LEG: f32 = 1500.0;

/// What the AI's view (crate::sim View) reads of the raid goals each tick.
#[derive(Debug, Clone, Default)]
pub struct RaidView {
    /// Retreating's score (F038688) of each.
    pub retreat: Vec<f32>,
}

impl RaidView {
    /// The conditions this view answers, ahead of the view's own (none yet).
    pub fn claims(&self, _c: u16, _t: &Target) -> bool {
        false
    }

    pub fn check(&self, _me: usize, _c: u16, _t: &Target) -> Option<bool> {
        None
    }

    /// Whether this view scores the definition.
    pub fn scores(d: &TaskDef) -> bool {
        d.score == RETREAT_SCORE
    }

    pub fn score(&self, me: usize, d: &TaskDef, _t: &Target) -> f32 {
        match d.score {
            RETREAT_SCORE => self.retreat.get(me).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// The target finders (the +0x160 hooks; none yet).
    pub fn find_target(&self, _me: usize, _task: u16) -> Option<Target> {
        None
    }
}

/// The raid goals' state: the view, and where each one's Task_GoHomeTown heads.
#[derive(Debug, Clone, Default)]
pub struct RaidState {
    pub view: RaidView,
    heading: HashMap<usize, [f32; 3]>,
}

impl Sim {
    /// Whether `id` has goal `task` among its jobs.
    pub(crate) fn has_job(&self, id: usize, task: u16) -> bool {
        self.folk[id].ai.jobs.iter().flatten().any(|g| g.task == task)
    }

    /// The town Retreating takes `id` home to (F044526 +0x30, the squad home's town): a world
    /// squad's home (crate::world: a patrol's home town, the town a campaign's squad set out
    /// from), else its home town (crate::intruders).
    fn retreat_home(&self, id: usize) -> Option<usize> {
        match self.folk[id].world_squad.and_then(|k| self.world.squads.get(k)) {
            Some(s) => s.home,
            None => self.home_town(id),
        }
    }

    /// @fn F038670
    ///
    /// Whether `id` may get out of where it is, as asked of `target` (a character: the building
    /// it is in, F044495): `id` in no building (Character vtable 0x1d8) yes; the target in the
    /// same building yes; else the building's doors (+0x1c0 / +0x1c8): none, or one broken
    /// (vtable 0x308), open (DoorStuff +0x380 1) or opening (2), yes; all shut, no. (A target
    /// that is a part of the building (+0x1a0 set, its +0x368 the building) is passed as yes.)
    /// F038688 and F038687 ask it of the character itself (the AI's +0x18 +0x10, its own
    /// handle +0x58), so for them it always answers yes.
    fn can_leave(&self, id: usize, target: usize) -> bool {
        let Some(b) = self.folk[id].inside else { return true };
        if self.folk[target].inside == Some(b) {
            return true;
        }
        let doors: Vec<&crate::sim::DoorState> = self.door_states.iter().filter(|d| d.player_building.is_none() && d.door.placement == b && !d.gone).collect();
        doors.is_empty() || doors.iter().any(|d| d.broken || d.phase == crate::sim::DOOR_OPEN || d.phase == crate::sim::DOOR_OPENING)
    }

    /// @fn F033834
    ///
    /// Aiming a crossbow (the gun class, Character +0x2f0: its +0x36 aiming) at a living target
    /// (its +0x50 handle).
    fn aiming(&self, id: usize) -> bool {
        let f = &self.arena.f[id];
        f.gun.is_some() && f.shooting && f.focus.or(f.combat.target).is_some_and(|t| !self.folk[t].dead)
    }

    /// @fn F038688
    ///
    /// Retreating's score (AI slot 56). One of the player's faction (F044443) scores none while
    /// either arm is of use (F040411, the medical's +0x165 / +0x166), while aiming a crossbow
    /// (F033834), or with any weapon on its hip or back (F043569's "hip" and "back" sections)
    /// that is not two-handed (its "skill category", +0x234: heavy 3, bow 6, polearm 8). Then:
    /// in its home's town (the town it is in, Character +0x1a0 +0x88, being F044526's +0x30)
    /// none; unable to get out (F038670, asked of itself: never); a slave (F039148 state 1) not
    /// the player's (F046535) 0.01; anyone else 0.5; but none in a region locked off from the
    /// ground outside (its gate code, Character vtable 0x118 F044467, above 0 and joined to
    /// code 0 only through a locked door: F030342 answering 3; crate::gate_codes).
    fn retreat_score(&self, id: usize) -> f32 {
        let p = &self.folk[id];
        if p.player {
            let f = &self.arena.f[id];
            let (a, b) = f.medical.arms_usable(f.body.animal, f.ko_point());
            if a || b || self.aiming(id) {
                return 0.0;
            }
            let one_handed = ["hip", "back"].iter().filter_map(|n| p.inventory.section(n)).flat_map(|k| p.inventory.sections[k].items.iter()).any(|i| {
                let cat = self.data.get(&i.item.record).and_then(|r| r.ints.get("skill category").copied()).unwrap_or(-1);
                ![3, 6, 8].contains(&cat)
            });
            if one_handed {
                return 0.0;
            }
        }
        let home = self.retreat_home(id);
        if p.town.is_some() && p.town == home {
            return 0.0;
        }
        if !self.can_leave(id, id) {
            return 0.0;
        }
        let area = self.area(id);
        if area > 0 && self.gate_codes.state(area, 0) == kenshi_world::gates::PAIR_LOCKED as i32 {
            return 0.0;
        }
        let players = |f: &str| f == crate::sim::PLAYER_FACTION || self.folk.iter().any(|q| q.player && q.faction.as_deref() == Some(f));
        if self.slave_state(id) == crate::slavery::IS_SLAVE && !p.slave.as_ref().is_some_and(|s| players(&s.faction)) {
            return RETREAT_SLAVE;
        }
        RETREAT
    }

    /// @fn F048771
    /// @fn F048824
    ///
    /// Town `t`'s point (Town vtable 0x38, kept and worked out again once its timer, +0xc0 /
    /// +0xc8, runs out): a walled town (its gate list, +0x3b0, not empty: vtable 0x2f0) 200
    /// units out from the door of its first gate in the list's order whose code is 0
    /// (`town_gates`: the hash map's, by the gates' handles; F048824: door +0x3bc, out along it from its inside point
    /// +0x3c8, on the ground), else its position; an open town 3 door-depths out of the door of
    /// the building at its centre (F050526: the building found straight down from above its
    /// position), else its position. F048824 takes a gate only when its building's gate code
    /// (vtable 0x118: `building_codes`, F030361) is 0.
    fn town_point(&self, t: usize) -> [f32; 3] {
        let c = self.towns[t].position;
        let gates = self.town_gates(t);
        let out = |d: usize, by: f32| {
            let (q, i) = (self.door_states[d].door.position, self.door_states[d].inside);
            let (dx, dz) = (q[0] - i[0], q[2] - i[2]);
            let l = dx.hypot(dz);
            if l < 1e-3 {
                return q;
            }
            let (ux, uz) = if by > 50.0 { (dx / l * by, dz / l * by) } else { (dx * by, dz * by) };
            [q[0] + ux, q[1], q[2] + uz]
        };
        // (F048824: the first whose gate building's code, vtable 0x118, is 0: outside ground)
        if let Some(&g) = gates.iter().find(|&&g| self.building_codes.get(self.door_states[g].door.placement).copied() == Some(0)) {
            return out(g, 200.0);
        }
        let centre = (0..self.placed_at.len()).find(|&b| {
            self.placed_town.get(b).copied().flatten() == Some(t) && self.inner.iter().any(|v| v.placement == b && v.contains([c[0], (v.lo[1] + v.hi[1]) * 0.5, c[2]]))
        });
        match centre.and_then(|b| (0..self.door_states.len()).find(|&d| self.door_states[d].door.placement == b && !self.door_states[d].gone)) {
            Some(d) => out(d, 3.0),
            None => c,
        }
    }

    /// @fn F031066
    ///
    /// Where Task_GoHomeTown takes `id` (its start, F030956): with a parent squad (Platoon +0xb8,
    /// the squad it was split from: none here, the world's squads are whole) its position; else
    /// its home town's point (F044526 +0x30, Town vtable 0x38: `town_point`); without a home
    /// town, the nearest town of its faction (F050235 -> F03c3a3); else a random point up to
    /// 5000 units off along both axes (F050502). (stand-in: needs the pathfinder's way beyond the
    /// loaded navmesh) A world squad's people take it leg by leg: at most `HOME_LEG` toward the
    /// next point of its route home (crate::world's), the last leg to the town's point.
    fn retreat_to(&mut self, id: usize) -> [f32; 3] {
        let me = self.folk[id].mover.position_units();
        let home = self.retreat_home(id);
        if let Some(s) = self.folk[id].world_squad.and_then(|k| self.world.squads.get(k)) {
            let end = home.map(|t| self.town_point(t));
            let w = match (s.route.len(), end) {
                (0 | 1, Some(e)) => e,
                _ => s.route.first().copied().or(end).unwrap_or(s.pos),
            };
            let (dx, dz) = (w[0] - me[0], w[2] - me[2]);
            let d = dx.hypot(dz);
            if d <= HOME_LEG {
                return w;
            }
            return [me[0] + dx / d * HOME_LEG, w[1], me[2] + dz / d * HOME_LEG];
        }
        if let Some(t) = home {
            return self.town_point(t);
        }
        let faction = self.faction_id(id);
        let near = (0..self.towns.len()).filter(|&t| faction.is_some() && self.towns[t].faction == faction).min_by(|&a, &b| {
            let d = |t: usize| (self.towns[t].position[0] - me[0]).hypot(self.towns[t].position[2] - me[2]);
            d(a).total_cmp(&d(b))
        });
        if let Some(t) = near {
            return self.town_point(t);
        }
        let z = self.rng.next() * 2.0 * WANDER - WANDER;
        let x = self.rng.next() * 2.0 * WANDER - WANDER;
        [me[0] + x, me[1], me[2] + z]
    }

    /// Whether squad `k`'s way home is its people's Retreating: one standing runs the task now
    /// (its plan's step), the world then leaves their walking to it; while none does (all
    /// taken up by other goals, or the way there not found), the world's legs lead them.
    /// (stand-in: needs the pathfinder's way beyond the loaded navmesh, so that the world's legs,
    /// crate::world, are not needed) In Kenshi only the people's own goals move them.
    pub(crate) fn retreats_by_plan(&self, k: usize) -> bool {
        self.world.squads[k].members.iter().any(|&m| {
            let p = &self.folk[m];
            !p.dead && !p.down && !p.away && p.ai.queue.first().is_some_and(|s| RAID_RUNNABLE.contains(&s.task))
        })
    }

    /// Squad `k` sets off home from a campaign: its people take the campaign's "retreat AI
    /// leader" package (its "victory AI leader" once won; either for the other when missing),
    /// the first its leader's list, the rest the squad's (as `raid` gives a victory's); the world's
    /// orders to them dropped.
    pub(crate) fn retreat_package(&mut self, k: usize) {
        let Goal::Campaign { campaign, .. } = &self.world.squads[k].goal else { return };
        let data = self.data.clone();
        let Some(c) = data.get(campaign) else { return };
        let phases = if self.world.squads[k].won.is_some() { ["victory AI leader", "retreat AI leader"] } else { ["retreat AI leader", "victory AI leader"] };
        let Some(pkg) = phases.iter().find_map(|ph| campaign_refs(&data, c, ph).first().map(|r| r.0.clone())) else { return };
        let Some(rec) = data.get(&pkg) else { return };
        let members = self.world.squads[k].members.clone();
        self.take_package(&members, &pkg);
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: squad {k} goes home on package {}", rec.name);
        }
    }

    /// `members` take package `pkg`'s goals in place of their jobs: the first its leader's
    /// list, the rest the squad's; their orders and plans dropped.
    pub fn take_package(&mut self, members: &[usize], pkg: &str) {
        let data = self.data.clone();
        let Some(rec) = data.get(pkg) else { return };
        let leader = members.first().copied();
        for (j, &m) in members.iter().enumerate() {
            let role = if j == 0 { kenshi_ai::package::Role::Leader } else { kenshi_ai::package::Role::Squad };
            let goals = kenshi_ai::package::package_goals(&data, rec, role);
            let p = &mut self.folk[m];
            p.ai.jobs = crate::sim::jobs_of(goals, leader.filter(|&l| l != m), None);
            p.ai.orders.clear();
            p.ai.queue.clear();
            p.started = false;
            p.rethink = true;
            p.package = Some(pkg.to_string());
        }
    }

    /// The raid goals' view for the AI (`RaidView`): each one's figures, every tick.
    fn raid_view_tick(&mut self) {
        let n = self.folk.len();
        let mut v = std::mem::take(&mut self.raid_state.view);
        v.retreat = (0..n)
            .map(|i| {
                let p = &self.folk[i];
                if p.dead || p.away || !(self.has_job(i, RETREATING) || self.has_job(i, RETREATING_FAST)) {
                    0.0
                } else {
                    self.retreat_score(i)
                }
            })
            .collect();
        self.raid_state.view = v;
    }

    /// The raid goals each tick: the view, then the runnable tasks' handlers.
    pub fn raid_ai_tick(&mut self, _dt: f32) {
        self.raid_view_tick();
        let ids: Vec<usize> = self
            .folk
            .iter()
            .filter(|p| !p.dead && !p.down && !p.away && p.getting_up <= 0.0 && p.ai.queue.first().is_some_and(|s| RAID_RUNNABLE.contains(&s.task)))
            .map(|p| p.id)
            .collect();
        for me in ids {
            let Some(step) = self.folk[me].ai.queue.first().cloned() else { continue };
            match step.task {
                RETREATING | RETREATING_FAST => self.go_home_town(me, step.task == RETREATING_FAST),
                _ => {}
            }
        }
        // the headings of those no longer at it
        let folk = &self.folk;
        self.raid_state.heading.retain(|&i, _| folk.get(i).is_some_and(|p| p.ai.queue.first().is_some_and(|s| s.task == RETREATING || s.task == RETREATING_FAST)));
    }

    /// @fn F030956
    /// @fn F030957
    ///
    /// Task_GoHomeTown (128) and Task_GoHomeTownFast (127, vtable +0x28 answering 2): on starting,
    /// where home is (vtable +0x30, `retreat_to`); then each tick the move there, walking (mode 0)
    /// or running (2). The task never ends by itself: arrived, it stands (the goal's score, none
    /// at home, or Retreating fast's completion, isAtHomeTown, ends it). (stand-in: needs the
    /// pathfinder's way beyond the loaded navmesh) A world squad's way home goes leg by leg:
    /// arrived, the next leg.
    fn go_home_town(&mut self, me: usize, fast: bool) {
        let p = &self.folk[me];
        let at = p.mover.position_units();
        let near = |q: [f32; 3]| (q[0] - at[0]).powi(2) + (q[2] - at[2]).powi(2) < AT_LOCATION_SQ;
        let idle = !p.pathing.pending && p.pathing.follow.is_none();
        let heading = self.raid_state.heading.get(&me).copied().filter(|_| p.started);
        let goal = match heading {
            None => self.retreat_to(me),
            // on its way
            Some(_) if !idle => return,
            // no way there (or the path ended short): asked again at the next decision
            Some(h) if !near(h) => {
                let p = &mut self.folk[me];
                p.ai.queue.clear();
                p.started = false;
                self.raid_state.heading.remove(&me);
                return;
            }
            // arrived: it stands there, a world squad's people take the next leg
            Some(_) if p.world_squad.is_none() => return,
            Some(h) => {
                let next = self.retreat_to(me);
                if near(next) || (next[0] - h[0]).hypot(next[2] - h[2]) < 1.0 {
                    return;
                }
                next
            }
        };
        self.raid_state.heading.insert(me, goal);
        let mode = if fast { MoveSpeed::Run } else { MoveSpeed::Walk };
        let Sim { folk, manager, run, .. } = self;
        let p = &mut folk[me];
        let own = if p.own_speed > 0.0 { p.own_speed } else { *run };
        p.mover.set_max_speed(speed_limit(own, mode.cap(15.0).unwrap_or(15.0), None));
        p.pathing.go_to(p.id, &p.mover, goal, false, manager);
        p.started = true;
        if std::env::var("KENSHI_RAID_DEBUG").is_ok() {
            eprintln!("raid: {me} goes home ({}) to {:?}", if fast { "running" } else { "walking" }, goal.map(|v| v as i32));
        }
    }
}
