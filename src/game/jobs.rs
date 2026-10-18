//! The player's jobs on the AI planner (port_map `ai`): the job list as the task system's goals,
//! the conditions, scores and target finders of the jobs' tasks, and the executors of their
//! runnable steps.
//!
//! A job is a goal on the AI's goals list (AITaskSytem +0x88: F036821 adds one in its job form,
//! `orders::job_form`), ranked with the character's other goals in list order (F036915: the
//! first job the highest key) and planned (`kenshi_ai::planner`) into runnable steps carried
//! out here. The JOBS toggle is the task system's free will (+0x35, F036819): without it only
//! the urgent goals are weighed (F036879: an Attacking enemies job still runs).
//! - Engineer (125, a generic task): its finder (F050256, BuildingFinderForEngineers F050345 /
//!   F050347) picks the building to build or repair; its target condition, isBuildingComplete
//!   (2, F038523), is solved by Building (2, Task_Build, F031256), which needs the building's
//!   materials put in ahead of the work (51, F038690 / F037723), solved by Adding materials
//!   (71, Task_AddMaterialsToBuilding) with materials carried (50), solved by Finding building
//!   mats (115, a generic task; its finder F038676: a storage holding one of the materials
//!   still needed), solved by Collecting resource (247, Task_EmptyMachine_BuildMats, scored by
//!   F038610) at the storage. A built building damaged (71, F038575) is repaired by Building
//!   too (Repairing, 95, the right-click order, is its other form).
//! - Operating machine (87, Task_OperateMachine) on a building of the player's: its free
//!   operator slot (73, F038577), electricity (93); worked by crate::production while it runs.
//! - Training (97, Task_UseTrainingDummy) on training equipment: a town's (furniture) or the
//!   player's (a building of the player's operated, crate::production's training).
//! - Attacking enemies (16): the AI's own (`sim`).
//!
//! The right-click orders Build, Repair, Operate and Training are the same tasks on the orders
//! list (+0x70), planned and carried out alike.
//!
//! The AI options' idle habits are the built-in goals the goal ranking adds below the jobs
//! (F036915, `kenshi_ai::task_system::IdleGoals`), planned as the jobs are:
//! - Ditch items: Ditching resources (242, a generic task; target condition "It's empty." false
//!   on the storage it finds) -> Filling machine (93) at the storage: what it keeps ("limit
//!   inventory") of what I carry put in; done when nothing I carry is kept by one
//!   (DITCHED_ALL_RESOURCES, 147). (stand-in) Its finder (F038713 -> F038708's storage search)
//!   is the nearest built storage of the player's keeping what I carry, at home.
//! - Sleep when injured: Going to bed (153; F038650: 0.1 for one of the player's at their
//!   outpost needing no first aid (F040406) and hurt, the body's health at most 0.9) -> Sleeping
//!   (98) in the bed; Getting out of bed (257, Task_GetOutOfBed; F038584: the trouble score,
//!   F038651, and 1 more once over 0.9).
//! - Sit when idle: Sitting around (275; finder F047548: of the building I am in, a dummy,
//!   chair or throne no one uses) -> Operating machine (pretend, 221) on it.
//!
//! (stand-in) The player's own beds and chairs are buildings here, not furniture: at home (within
//! `HOME_RANGE` of a building of the player's) the finders fall back on the nearest free bed or
//! chair of the zone's furniture within that range, as the routines did. (assumption) The
//! health share the bed scores read (AI +0x318 +0xb0) is the body's: its parts' health over
//! their most.
//!
//! (not ported) The machines' input and output conditions (62, 63, 89, 169) and the hauling
//! goals they lead to (Collecting resource 92, Try emptying machine 113, Filling machine 93): a
//! full machine is emptied by its operator's errand (crate::production's haul_tick), as is the
//! machine's want of input.
use crate::base::{BUILDING_OWNER, BuildWork};
use crate::loot::BOX_OWNER;
use crate::orders::Job;
use crate::sim::{Sim, Target};
use kenshi_ai::task_def::{Score, TaskDef};
use kenshi_ai::task_system::{DITCHING_RESOURCES, GETTING_OUT_OF_BED_INJURED, IdleGoals, Task};
use kenshi_world::furniture::{BF_BED, BF_CHAIR, BF_THRONE};
use std::collections::HashMap;

pub const BUILDING: u16 = 2;
pub const ADDING_MATERIALS: u16 = 71;
pub const OPERATING_MACHINE: u16 = 87;
pub const REPAIRING: u16 = 95;
pub const TRAINING: u16 = 97;
pub const FINDING_BUILD_MATS: u16 = 115;
pub const ENGINEER: u16 = 125;
pub const COLLECTING_BUILD_MATS: u16 = 247;

pub const FILLING_MACHINE: u16 = 93;
pub const SITTING_AROUND: u16 = kenshi_ai::task_system::SITTING_AROUND;
pub const GOING_TO_BED: u16 = kenshi_ai::task_system::GOING_TO_BED;

/// The runnable steps carried out here (`jobs_tick`).
pub const JOB_RUNNABLE: [u16; 6] = [BUILDING, REPAIRING, ADDING_MATERIALS, TRAINING, COLLECTING_BUILD_MATS, GETTING_OUT_OF_BED_INJURED];
/// The tasks whose target finders are here.
pub const JOB_FINDERS: [u16; 3] = [ENGINEER, FINDING_BUILD_MATS, DITCHING_RESOURCES];
/// Generic tasks (the factory's case 107) among the steps: they carry nothing out.
const GENERIC: [u16; 5] = [ENGINEER, FINDING_BUILD_MATS, DITCHING_RESOURCES, SITTING_AROUND, GOING_TO_BED];
/// (stand-in) How near a building of the player's counts as at home, for the idle habits.
pub const HOME_RANGE: f32 = 2500.0;
/// F038650 / F038584: the health share at or under which the hurt go to bed, over which they
/// get up (0x14168a25c), and Going to bed's score then (0x14167fad0).
const HURT: f32 = 0.9;
const BED_SCORE: f32 = 0.1;
/// Collecting resource's score (AI vtable slot 23, F038610).
const COLLECT_SCORE: Score = Score::Method(23);

/// (assumption) How near a building's middle counts as at it: its radius and a builder's reach
/// (crate::base), or a storage's fetching reach.
const SITE_REACH: f32 = 15.0;
const STORE_REACH: f32 = 40.0;
/// How near a building an operator works it (crate::production's `at_building`).
const OPERATE_REACH: f32 = 40.0;
/// F050347: the falloff of a building's score with its squared distance (81000000), x 2; at
/// none or less, 0.0001.
const ENGINEER_FALLOFF_SQ: f32 = 81_000_000.0;
const ENGINEER_NEAR: f32 = 2.0;
const ENGINEER_FLOOR: f32 = 0.0001;

/// Whether a step is carried out here.
pub fn runs_here(step: &Task<Target>) -> bool {
    JOB_RUNNABLE.contains(&step.task)
        || GENERIC.contains(&step.task)
        || (step.task == OPERATING_MACHINE && matches!(step.target, Target::Building(_)))
        || (step.task == FILLING_MACHINE && matches!(step.target, Target::Store(o) if (BUILDING_OWNER..BOX_OWNER).contains(&o)))
}

/// A job's target as the planner's: furniture, a door, a container, a building of the
/// player's or a person (the job's target as the right click names it).
pub fn job_target(t: usize) -> Target {
    if let Some(f) = t.checked_sub(crate::production::FURN_TARGET) {
        Target::Furniture(f)
    } else if let Some(d) = t.checked_sub(crate::doors::DOOR_TARGET) {
        Target::Door(d)
    } else if t >= BOX_OWNER {
        Target::Store(t)
    } else if let Some(b) = t.checked_sub(BUILDING_OWNER) {
        Target::Building(b)
    } else {
        Target::Person(t)
    }
}

/// @fn F036821
///
/// The goals list of one of the player's people: its jobs in order, each a task at
/// classification 3, priority 1, hours 0-100.
pub fn goals_of(jobs: &[Job], _at: [f32; 3]) -> Vec<Task<Target>> {
    jobs.iter()
        .filter(|j| kenshi_ai::task_def::def(j.task).is_some())
        .map(|j| Task {
            task: j.task,
            classification: 3,
            target: job_target(j.target),
            target2: Target::None,
            priority: 1.0,
            pos: [0.0; 3],
            hours: (0, 100),
        })
        .collect()
}

/// What the jobs read of a building of the player's.
#[derive(Debug, Clone, Default)]
pub struct Site {
    pub at: [f32; 3],
    pub radius: f32,
    pub confirmed: bool,
    /// isBuildingComplete (2, F038523).
    pub done: bool,
    /// isBuildingDamaged (71, F038575): built, its construction or a door short.
    pub damaged: bool,
    /// DOOR_IS_BROKEN (56): (assumption) one of its doors broken in.
    pub door_broken: bool,
    /// F037723: not built and the work caught up with the materials put in (condition 51
    /// fails).
    pub wants_materials: bool,
    /// The materials it still needs, in its order, with how many.
    pub needed: Vec<(String, u32)>,
    pub outpost: Option<usize>,
    /// Its operator slots ("max operators") and those working it.
    pub slots: u32,
    pub operators: Vec<usize>,
    /// It works for want of no power (93).
    pub powered: bool,
    /// Its storage's items in order, with how many.
    pub stock: Vec<(String, u32)>,
}

/// What the jobs read of one of the player's people.
#[derive(Debug, Clone, Default)]
pub struct Carrier {
    /// What it carries (its main grid and worn backpack) by item, and the items one more of
    /// which would fit.
    pub carried: HashMap<String, u32>,
    pub fits: Vec<String>,
    /// The outpost it is in (its town, Character +0x88).
    pub outpost: Option<usize>,
    /// In a town (Character +0x88), or at home (`HOME_RANGE`).
    pub in_town: bool,
    pub home: bool,
    /// The built-in goals it may have (F036915).
    pub idle: IdleGoals,
    /// Its first aid need (F040406) and its body's health share; in bed.
    pub aid: f32,
    pub health: f32,
    pub in_bed: bool,
    /// (stand-in) The storage it would ditch into (the old routine's).
    pub ditch_to: Option<usize>,
    /// An arm it can work with (F050241).
    pub arms: bool,
}

/// What the AI's view (crate::sim View) reads of the jobs each tick.
#[derive(Debug, Clone, Default)]
pub struct JobsView {
    pub sites: Vec<Site>,
    pub people: HashMap<usize, Carrier>,
    /// Each building's claims: the people whose plan works on it (F028562's reservations).
    pub claims_on: Vec<Vec<usize>>,
    /// (stand-in) The zone's free beds and chairs (furniture) near the player's buildings.
    pub beds: Vec<(usize, [f32; 3])>,
    pub chairs: Vec<(usize, [f32; 3])>,
    /// What the storages of the player's keep ("limit inventory"), by building.
    pub keeps: Vec<Vec<String>>,
}

/// The jobs' state: the view, and the step each one runs on a building or equipment.
#[derive(Debug, Clone, Default)]
pub struct JobsState {
    pub view: JobsView,
    using: HashMap<usize, (u16, Target)>,
}

fn near(a: [f32; 3], b: [f32; 3], r: f32) -> bool {
    (a[0] - b[0]).hypot(a[2] - b[2]) <= r
}

/// Whether one more of `item` would go in (as `Inventory::add` puts it: onto a stack of its
/// kind with room, else the main grid's first free cell, else the worn backpack).
fn one_fits(inv: &kenshi_char::inventory::Inventory, item: &kenshi_char::inventory::Item) -> bool {
    inv.sections.iter().any(|s| s.items.iter().any(|p| p.item.record == item.record && p.item.stack > 1 && p.item.quantity < p.item.stack))
        || inv.section("main").is_some_and(|i| inv.sections[i].first_fit(item).is_some())
        || inv.bag().is_some_and(|b| one_fits(b, item))
}

/// The view is made afresh every this many ticks (a tenth of a second).
const VIEW_TICKS: u64 = 6;

impl JobsView {
    /// The edge of building `b` on the side of `from` (units): where a builder works from.
    pub fn approach(&self, b: usize, from: [f32; 3]) -> Option<[f32; 3]> {
        let s = self.sites.get(b)?;
        let (dx, dz) = (from[0] - s.at[0], from[2] - s.at[2]);
        let d = dx.hypot(dz).max(1e-3);
        Some([s.at[0] + dx / d * s.radius, s.at[1], s.at[2] + dz / d * s.radius])
    }

    /// The conditions answered here, ahead of the view's own: those of a building of the
    /// player's, and, for the player's people, being at and taking from a storage of the
    /// player's.
    pub fn claims(&self, me: usize, c: u16, t: &Target) -> bool {
        let mine = self.people.contains_key(&me);
        match t {
            _ if mine && matches!(c, 38 | 86 | 147) => true,
            Target::Building(_) => matches!(c, 1 | 2 | 50 | 51 | 56 | 71 | 73 | 93),
            Target::Store(o) => (BUILDING_OWNER..BOX_OWNER).contains(o) && mine && matches!(c, 1 | 2 | 67 | 82 | 150),
            _ => false,
        }
    }

    pub fn check(&self, me: usize, pos: [f32; 3], c: u16, t: &Target) -> Option<bool> {
        let carrier = self.people.get(&me);
        let carries = |items: &[(String, u32)]| carrier.is_some_and(|k| items.iter().any(|(r, _)| k.carried.get(r).is_some_and(|&n| n > 0)));
        match (c, carrier) {
            // "My arms are messed up..." (F050241): an arm that works
            (38, Some(k)) => return Some(k.arms),
            // atATown (F036791: Character +0x88): (stand-in) the player's at home count
            (86, Some(k)) => return Some(k.in_town || k.home),
            // DITCHED_ALL_RESOURCES: (stand-in, the ditch finder's) nothing carried a storage
            // keeps
            (147, Some(k)) => return Some(k.ditch_to.is_none()),
            _ => {}
        }
        match *t {
            Target::Building(b) => {
                let Some(s) = self.sites.get(b) else { return Some(false) };
                Some(match c {
                    // isAtLocation: (assumption) within its radius and a builder's reach
                    1 => near(pos, s.at, s.radius + SITE_REACH),
                    // isBuildingComplete (F038523)
                    2 => s.done,
                    // buildingHasBuildingMaterials (F038690: not F037723)
                    51 => !s.wants_materials,
                    // isBuildingDamaged (F038575, the building's vtable +0x328)
                    71 => s.damaged,
                    // DOOR_IS_BROKEN (F038691)
                    56 => s.door_broken,
                    // "I don't have any materials" (0x14060e5b0: the AI +0x300 object's +0x70
                    // flag, not identified): (assumption) carrying one the building still needs
                    50 => carries(&s.needed),
                    // machineHasFreeOperatorSlot (F038577): a slot not taken by another
                    73 => (s.operators.iter().filter(|&&o| o != me).count() as u32) < s.slots,
                    // "It needs electricity."
                    93 => s.powered,
                    _ => return None,
                })
            }
            Target::Store(o) => {
                let s = self.sites.get(o - BUILDING_OWNER)?;
                Some(match c {
                    // (assumption) within its radius and a fetcher's reach
                    1 => near(pos, s.at, s.radius + STORE_REACH),
                    // isBuildingComplete (F038523)
                    2 => s.done,
                    // "It's out of resources." (Filling machine): carrying what it keeps
                    67 => carrier.is_some_and(|k| self.keeps.get(o - BUILDING_OWNER).is_some_and(|keep| keep.iter().any(|r| k.carried.get(r).is_some_and(|&n| n > 0)))),
                    // HAVE_SOME_BUILD_MATS (F038535): (assumption) carrying some of what it holds
                    150 => carries(&s.stock),
                    // "I don't have space to pick that up!" (F038628): room for its first item
                    82 => s.stock.first().is_some_and(|(r, _)| carrier.is_some_and(|k| k.fits.contains(r))),
                    _ => return None,
                })
            }
            _ => None,
        }
    }

    /// Whether this view scores the definition.
    pub fn scores(d: &TaskDef) -> bool {
        d.score == COLLECT_SCORE
    }

    /// @fn F038610
    ///
    /// Collecting resource's score: 1 when a container's first item would fit what I carry,
    /// else none (none for an empty one).
    pub fn score(&self, me: usize, d: &TaskDef, t: &Target) -> f32 {
        if d.score != COLLECT_SCORE {
            return 0.0;
        }
        let Target::Store(o) = *t else { return 0.0 };
        let Some(s) = o.checked_sub(BUILDING_OWNER).and_then(|k| self.sites.get(k)) else { return 0.0 };
        match (s.stock.first(), self.people.get(&me)) {
            (Some((r, _)), Some(k)) if k.fits.contains(r) => 1.0,
            _ => 0.0,
        }
    }

    /// @fn F038650
    ///
    /// Going to bed's and Sleeping's score for one of the player's (the view answers carrying
    /// someone and trouble about first): 0.1 at their outpost ((assumption) the squad's town,
    /// ActivePlatoon +0x88, taken as the outpost it is in, or at home) needing no first aid
    /// (F040406) with the body's health at most 0.9, else none.
    pub fn bed_score(&self, me: usize) -> f32 {
        match self.people.get(&me) {
            Some(k) if (k.outpost.is_some() || k.home) && k.aid <= 0.0 && k.health <= HURT => BED_SCORE,
            _ => 0.0,
        }
    }

    /// F038584's more for one of the player's in bed: 1 with the body's health over 0.9.
    pub fn healed(&self, me: usize) -> f32 {
        match self.people.get(&me) {
            Some(k) if k.health > HURT => 1.0,
            _ => 0.0,
        }
    }

    /// The nearest of `list` to `pos` within `HOME_RANGE`, for one of the player's at home.
    fn nearest_home(&self, me: usize, pos: [f32; 3], list: &[(usize, [f32; 3])]) -> Option<usize> {
        if !self.people.get(&me).is_some_and(|k| k.home) {
            return None;
        }
        list.iter()
            .map(|&(k, q)| (k, (q[0] - pos[0]).hypot(q[2] - pos[2])))
            .filter(|&(_, d)| d < HOME_RANGE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// (stand-in) Going to bed's bed for one of the player's at home (the home bed, F047587,
    /// being none): the nearest free bed of the zone.
    pub fn bed_for(&self, me: usize, pos: [f32; 3]) -> Option<usize> {
        self.nearest_home(me, pos, &self.beds)
    }

    /// (stand-in) Sitting around's seat for one of the player's at home (none in the building it
    /// is in, F047548): the nearest free chair or throne of the zone.
    pub fn chair_for(&self, me: usize, pos: [f32; 3]) -> Option<usize> {
        self.nearest_home(me, pos, &self.chairs)
    }

    /// The target finders (the +0x160 hooks).
    pub fn find_target(&self, me: usize, pos: [f32; 3], task: u16, t: &Target) -> Option<Target> {
        match task {
            // (stand-in) Ditching resources' storage (F038713)
            DITCHING_RESOURCES => self.people.get(&me).and_then(|k| k.ditch_to).map(|k| Target::Store(BUILDING_OWNER + k)),
            ENGINEER => self.engineer_site(me, pos).map(Target::Building),
            FINDING_BUILD_MATS => match *t {
                Target::Building(b) => self.materials_store(b, pos).map(|k| Target::Store(BUILDING_OWNER + k)),
                _ => None,
            },
            _ => None,
        }
    }

    /// @fn F038676
    ///
    /// Finding building mats' finder: of the materials building `b` still needs, in order, the
    /// first some storage holds: that storage ((assumption) the item search F015168 taken as
    /// the nearest built storage of the player's holding it).
    pub fn materials_store(&self, b: usize, pos: [f32; 3]) -> Option<usize> {
        let site = self.sites.get(b)?;
        for (m, _) in &site.needed {
            let store = (0..self.sites.len())
                .filter(|&k| k != b && self.sites[k].done && self.sites[k].stock.iter().any(|(r, n)| r == m && *n > 0))
                .min_by(|&x, &y| {
                    let d = |k: usize| (self.sites[k].at[0] - pos[0]).hypot(self.sites[k].at[2] - pos[2]);
                    d(x).total_cmp(&d(y))
                });
            if store.is_some() {
                return store;
            }
        }
        None
    }

    /// @fn F050256
    ///
    /// Engineer's finder: BuildingFinderForEngineers over the town I am in (Character +0x88:
    /// the outpost; out of one, (assumption) every building of the player's, F02588d's search).
    /// Its test (F050345): one of my faction's, not complete or damaged (the building's vtable
    /// +0x328; a damaged door counts as its building's). Its pick (F050347): the best score,
    /// (1 - d^2 / 9000^2) x 2 (0.0001 at none or less), for one wanting materials of which I
    /// carry none (F037723, F038626) only with a storage holding some (Finding building mats'
    /// finder, F038676: (assumption) its score taken as 1), divided by one plus the others
    /// whose plans work on it (F028562). (not ported) F050345's state tests (+1, +2, +8 of the
    /// building's state), the 0.5 for a building whose vtable +0x2c8 answers, and the use test
    /// F038529 (the player's own buildings pass it).
    pub fn engineer_site(&self, me: usize, pos: [f32; 3]) -> Option<usize> {
        let carrier = self.people.get(&me)?;
        let mut best: Option<(usize, f32)> = None;
        for (b, s) in self.sites.iter().enumerate() {
            if !s.confirmed || (carrier.outpost.is_some() && s.outpost != carrier.outpost) {
                continue;
            }
            if s.done && !s.damaged {
                continue;
            }
            let d2 = (s.at[0] - pos[0]).powi(2) + (s.at[1] - pos[1]).powi(2) + (s.at[2] - pos[2]).powi(2);
            let mut score = (d2 - ENGINEER_FALLOFF_SQ) / (0.0 - ENGINEER_FALLOFF_SQ) * ENGINEER_NEAR;
            if score <= 0.0 {
                score = ENGINEER_FLOOR;
            }
            if s.wants_materials && !s.needed.iter().any(|(r, _)| carrier.carried.get(r).is_some_and(|&n| n > 0)) && self.materials_store(b, pos).is_none() {
                continue;
            }
            let others = self.claims_on.get(b).map_or(0, |c| c.iter().filter(|&&o| o != me).count());
            if others > 0 {
                score /= (others + 1) as f32;
            }
            if best.is_none_or(|(_, x)| score > x) {
                best = Some((b, score));
            }
        }
        best.map(|(b, _)| b)
    }
}

impl Sim {
    /// The jobs' view of the player's buildings and people (`JobsView`).
    fn jobs_view(&self) -> JobsView {
        let data = &self.data;
        let count = |inv: &kenshi_char::inventory::Inventory| {
            let mut v: Vec<(String, u32)> = Vec::new();
            for p in inv.sections.iter().flat_map(|s| s.items.iter()) {
                match v.iter_mut().find(|(r, _)| *r == p.item.record) {
                    Some(e) => e.1 += p.item.quantity,
                    None => v.push((p.item.record.clone(), p.item.quantity)),
                }
            }
            v
        };
        let outpost_r = kenshi_world::town::radius(1, 1.0);
        let mut sites = Vec::with_capacity(self.buildings.len());
        for (b, pb) in self.buildings.iter().enumerate() {
            let rec = data.get(&pb.record);
            let needed: Vec<(String, u32)> = if pb.done {
                vec![]
            } else {
                let mut seen: Vec<(String, u32)> = Vec::new();
                for (m, _) in &pb.materials {
                    let n = self.still_needed(b, m);
                    if n > 0 && !seen.iter().any(|(r, _)| r == m) {
                        seen.push((m.clone(), n));
                    }
                }
                seen
            };
            let slots = rec.and_then(|r| crate::production::function_of(data, r).map(|(f, _)| crate::production::max_operators(r, f))).unwrap_or(1);
            let operators = (0..self.folk.len())
                .filter(|&i| self.folk[i].operate == Some(b) && !self.folk[i].down && near(self.folk[i].mover.position_units(), pb.at, pb.radius + OPERATE_REACH))
                .collect();
            sites.push(Site {
                at: pb.at,
                radius: pb.radius,
                confirmed: pb.confirmed,
                done: pb.done,
                damaged: self.needs_repair(b),
                door_broken: self.door_states.iter().any(|d| d.player_building == Some(b) && !d.gone && d.broken),
                wants_materials: !pb.done && pb.total() > 0 && pb.progress >= pb.used as f32,
                needed,
                outpost: pb.outpost,
                slots,
                operators,
                powered: self.has_power(b),
                stock: if pb.done { pb.storage.as_ref().map_or(vec![], count) } else { vec![] },
            });
        }
        // the items whose room is asked about: the materials wanted and the storages' first
        let mut asked: Vec<String> = Vec::new();
        for s in &sites {
            for (r, _) in s.needed.iter().chain(s.stock.first()) {
                if !asked.contains(r) {
                    asked.push(r.clone());
                }
            }
        }
        let opts = &self.ai_options;
        let at_home = |at: [f32; 3]| self.buildings.iter().any(|b| b.confirmed && near(b.at, at, HOME_RANGE));
        let mut people = HashMap::new();
        for p in self.folk.iter().filter(|p| p.player && !p.dead) {
            let mut carried: HashMap<String, u32> = HashMap::new();
            for s in p.inventory.sections.iter().filter(|s| !s.is_equipment()).chain(p.inventory.bag().into_iter().flat_map(|b| b.sections.iter())) {
                for q in &s.items {
                    *carried.entry(q.item.record.clone()).or_default() += q.item.quantity;
                }
            }
            let fits = asked
                .iter()
                .filter(|r| {
                    let Some(rec) = data.get(r) else { return false };
                    one_fits(&p.inventory, &kenshi_char::inventory::Item::from_record(rec, 1))
                })
                .cloned()
                .collect();
            let at = p.mover.position_units();
            let outpost = (0..self.outposts.len())
                .filter(|&i| near(at, self.outposts[i], outpost_r))
                .min_by(|&x, &y| {
                    let d = |i: usize| (self.outposts[i][0] - at[0]).hypot(self.outposts[i][2] - at[2]);
                    d(x).total_cmp(&d(y))
                });
            // F036915's built-in goals: the player's faction, not sneaking (F039177: not in a
            // fight), the AI options; the bed and the seat neither on hold (F029593) nor carrying
            let mine = p.faction.as_deref() == Some(crate::sim::PLAYER_FACTION) && !(p.sneaking && !self.arena.f[p.id].active);
            let settled = !p.stance.hold && p.carrying.is_none();
            let idle = IdleGoals { ditch: mine && opts.ditch_items, bed: mine && settled && opts.sleep_injured, sit: mine && settled && opts.sit_idle };
            let m = &self.arena.f[p.id].medical;
            let (h, most) = m.parts.iter().fold((0.0f32, 0.0f32), |(h, x), q| (h + q.health().max(0.0), x + q.max_health()));
            let home = at_home(at);
            let ditch_to = if home && opts.ditch_items { self.ditch_target(p.id).map(|(k, _)| k) } else { None };
            people.insert(
                p.id,
                Carrier {
                    carried,
                    fits,
                    outpost,
                    in_town: p.town.is_some(),
                    home,
                    idle,
                    aid: m.aid_need(false).flesh,
                    health: if most > 0.0 { h / most } else { 1.0 },
                    in_bed: p.bed.is_some(),
                    ditch_to,
                    // F050241: alive and up (Character +0x5bc, +0x5b9) with an arm working (+0x5bd,
                    // +0x5be: (assumption) an arm not lost with health above none)
                    // (an animal's, the character's vtable +0x248, always)
                    arms: p.race.is_some() || (!p.dead && !p.down && m.parts.iter().any(|q| q.kind == 2 && q.slot != 1 && q.health() > 0.0)),
                },
            );
        }
        let mut claims_on = vec![Vec::new(); self.buildings.len()];
        for p in self.folk.iter().filter(|p| !p.dead) {
            for st in &p.ai.queue {
                if let Target::Building(b) = st.target
                    && matches!(st.task, BUILDING | REPAIRING | ADDING_MATERIALS)
                    && let Some(c) = claims_on.get_mut(b)
                    && !c.contains(&p.id)
                {
                    c.push(p.id);
                }
            }
        }
        // (stand-in) the zone's free beds and seats near the player's buildings
        let free = |k: usize| self.operators.get(k).is_none_or(|o| o.is_none()) && self.bed_taken.get(k).is_none_or(|o| o.is_none());
        let homely = |k: usize, functions: &[i32]| functions.contains(&self.furn[k].function) && free(k) && at_home(self.furn[k].position);
        let beds = (0..self.furn.len()).filter(|&k| homely(k, &[BF_BED])).map(|k| (k, self.furniture_approach[k])).collect();
        let chairs = (0..self.furn.len()).filter(|&k| homely(k, &[BF_CHAIR, BF_THRONE])).map(|k| (k, self.furniture_approach[k])).collect();
        let keeps = self
            .buildings
            .iter()
            .map(|b| if b.done && b.storage.is_some() { data.get(&b.record).map_or(vec![], |r| r.refs("limit inventory").iter().map(|(s, _)| s.clone()).collect()) } else { vec![] })
            .collect();
        JobsView { sites, people, claims_on, beds, chairs, keeps }
    }

    /// Lets go of the building or equipment `me`'s ended step worked.
    fn release(&mut self, me: usize, t: Target) {
        match t {
            Target::Building(b) => {
                if self.folk[me].operate == Some(b) {
                    self.folk[me].operate = None;
                }
                if self.folk[me].build == Some(b) {
                    self.folk[me].build = None;
                }
            }
            Target::Furniture(f) => {
                self.trainees.retain(|(o, ..)| *o != me);
                if self.operators.get(f).copied().flatten() == Some(me) {
                    self.operators[f] = None;
                }
                if self.folk[me].operating == Some(f) {
                    self.folk[me].operating = None;
                }
            }
            _ => {}
        }
    }

    /// Building `b` gone, those after it moved down one (`upkeep::remove_building`): the jobs
    /// on it dropped (a kind kept once whatever its target, definition +0x08 clear, stays), the
    /// orders and plans on it given up, the others' targets moved down.
    pub(crate) fn jobs_building_gone(&mut self, b: usize) {
        let shift = |t: &mut Target| -> bool {
            match t {
                Target::Building(k) if *k == b => return false,
                Target::Building(k) if *k > b => *k -= 1,
                Target::Store(o) if *o == BUILDING_OWNER + b => return false,
                Target::Store(o) if (BUILDING_OWNER + b + 1..BOX_OWNER).contains(o) => *o -= 1,
                _ => {}
            }
            true
        };
        for p in self.folk.iter_mut() {
            p.jobs.retain_mut(|j| {
                let Some(k) = j.target.checked_sub(BUILDING_OWNER).filter(|_| j.target < BOX_OWNER) else { return true };
                if k > b {
                    j.target -= 1;
                }
                k != b || kenshi_ai::task_def::def(j.task).is_some_and(|d| d.f08 == 0)
            });
            p.ai.orders.retain_mut(|t| shift(&mut t.target));
            let mut whole = true;
            for t in p.ai.queue.iter_mut().chain(p.ai.current.iter_mut()) {
                whole &= shift(&mut t.target);
                whole &= shift(&mut t.target2);
            }
            if !whole {
                p.ai.queue.clear();
                p.ai.current = None;
                p.started = false;
                p.rethink = true;
            }
        }
        let using = std::mem::take(&mut self.jobs_state.using);
        for (me, (task, mut t)) in using {
            if shift(&mut t) {
                self.jobs_state.using.insert(me, (task, t));
            }
        }
        self.jobs_state.view = self.jobs_view();
    }

    /// The jobs' steps each tick (see the module notes), then the view for the next think.
    pub fn jobs_tick(&mut self, dt: f32) {
        // jobs from before their tasks were kept: their tasks from their targets
        for id in 0..self.folk.len() {
            if self.folk[id].player && self.folk[id].jobs.iter().any(|j| j.task == 0) {
                let mut jobs = std::mem::take(&mut self.folk[id].jobs);
                for j in jobs.iter_mut().filter(|j| j.task == 0) {
                    j.task = self.order_task_of(j.target).and_then(crate::orders::job_form).map_or(0, |f| f.0);
                }
                jobs.retain(|j| j.task != 0);
                self.folk[id].jobs = jobs;
            }
        }
        // buildings and equipment whose step has ended let go
        let using: Vec<(usize, (u16, Target))> = self.jobs_state.using.iter().map(|(&k, &v)| (k, v)).collect();
        for (me, (task, t)) in using {
            let still = self.folk.get(me).is_some_and(|p| !p.down && !p.dead && p.ai.queue.first().is_some_and(|s| s.task == task && s.target == t));
            if !still {
                self.jobs_state.using.remove(&me);
                self.release(me, t);
            }
        }
        let ids: Vec<usize> = self.folk.iter().filter(|p| !p.dead && !p.down && !p.away && p.ai.queue.first().is_some_and(runs_here)).map(|p| p.id).collect();
        for me in ids {
            let Some(step) = self.folk[me].ai.queue.first().cloned() else { continue };
            let pos = self.folk[me].mover.position_units();
            let done = |sim: &mut Sim| {
                sim.folk[me].ai.queue.remove(0);
                sim.folk[me].started = false;
                sim.folk[me].rethink = true;
            };
            let give_up = |sim: &mut Sim| {
                if std::env::var("KENSHI_JOBS_DEBUG").is_ok() {
                    eprintln!("jobs: {me} gives up {} on {:?}", sim.folk[me].ai.queue.first().map_or(0, |s| s.task), sim.folk[me].ai.queue.first().map(|s| s.target));
                }
                sim.folk[me].ai.queue.clear();
                sim.folk[me].started = false;
                sim.folk[me].rethink = true;
            };
            if std::env::var("KENSHI_JOBS_DEBUG").is_ok() && !self.folk[me].started {
                eprintln!("jobs: {me} starts {} on {:?}", step.task, step.target);
            }
            self.folk[me].started = true;
            match (step.task, step.target) {
                // the generic tasks carry nothing out
                (t, _) if GENERIC.contains(&t) => {
                    self.folk[me].ai.queue.remove(0);
                    self.folk[me].started = false;
                }
                // Task_Build (F031256): by the building, its update (`build_work`) until it is
                // built or whole, or wants materials (the work caught up with them: 51 fails)
                (BUILDING | REPAIRING, Target::Building(b)) => {
                    let Some(s) = self.jobs_state.view.sites.get(b) else { give_up(self); continue };
                    if b >= self.buildings.len() || !near(pos, s.at, s.radius + SITE_REACH) {
                        give_up(self);
                        continue;
                    }
                    self.folk[me].build = Some(b);
                    self.jobs_state.using.insert(me, (step.task, step.target));
                    match self.build_work(me, b, dt) {
                        BuildWork::Working => {}
                        BuildWork::NoMaterials(_) | BuildWork::Finished => done(self),
                    }
                }
                // Task_AddMaterialsToBuilding: by the building, what it still needs of what I
                // carry put in (crate::base `add_materials`, each material in order)
                (ADDING_MATERIALS, Target::Building(b)) => {
                    let Some(s) = self.jobs_state.view.sites.get(b) else { give_up(self); continue };
                    if b >= self.buildings.len() || !near(pos, s.at, s.radius + SITE_REACH) {
                        give_up(self);
                        continue;
                    }
                    let materials: Vec<String> = self.buildings[b].materials.iter().map(|m| m.0.clone()).collect();
                    for m in materials {
                        self.add_materials(me, b, &m);
                    }
                    done(self);
                }
                // Task_EmptyMachine_BuildMats at a storage: of the materials the plan's building
                // still needs (the next building step's), what it holds, as far as they fit.
                // (assumption) All at once; the amounts are the building's wants less what I
                // carry.
                (COLLECTING_BUILD_MATS, Target::Store(o)) => {
                    let Some(k) = o.checked_sub(BUILDING_OWNER).filter(|&k| k < self.buildings.len()) else { give_up(self); continue };
                    let (at, r) = (self.buildings[k].at, self.buildings[k].radius);
                    if !near(pos, at, r + STORE_REACH) {
                        give_up(self);
                        continue;
                    }
                    let site = self.folk[me].ai.queue.iter().skip(1).find_map(|s| match s.target {
                        Target::Building(b) if matches!(s.task, FINDING_BUILD_MATS | ADDING_MATERIALS | BUILDING | REPAIRING) => Some(b),
                        _ => None,
                    });
                    let Some(b) = site.filter(|&b| b < self.buildings.len()) else { done(self); continue };
                    let wants: Vec<(String, u32)> = self.jobs_state.view.sites.get(b).map_or(vec![], |s| s.needed.clone());
                    for (m, n) in wants {
                        let have = self.jobs_state.view.people.get(&me).and_then(|c| c.carried.get(&m).copied()).unwrap_or(0);
                        let mut got = 0;
                        while got + have < n {
                            let Some(inv) = self.buildings[k].storage.as_mut() else { break };
                            let Some(one) = crate::production::take_item(inv, &m) else { break };
                            let p = &mut self.folk[me];
                            if !p.inventory.add(one.clone(), false) && !p.inventory.bag_mut().is_some_and(|bag| bag.add(one.clone(), false)) {
                                if let Some(inv) = self.buildings[k].storage.as_mut() {
                                    inv.add(one, false);
                                }
                                break;
                            }
                            got += 1;
                        }
                        if std::env::var("KENSHI_JOBS_DEBUG").is_ok() {
                            eprintln!("jobs: {me} takes {got} of {m} from storage {k} for {b}");
                        }
                    }
                    done(self);
                }
                // Task_OperateMachine on a building of the player's: worked by crate::production
                // while the step runs (its operator), the step's equipment let go as it ends
                (OPERATING_MACHINE, Target::Building(b)) => {
                    if b >= self.buildings.len() || !self.is_operated(b) {
                        give_up(self);
                        continue;
                    }
                    self.folk[me].operate = Some(b);
                    self.jobs_state.using.insert(me, (step.task, step.target));
                }
                // Task_UseTrainingDummy: on a town's equipment, trained on by crate::production's
                // `training_tick` while the step runs (its operator slot taken); on the player's,
                // as an operator
                (TRAINING, Target::Furniture(f)) => {
                    if self.furn.get(f).is_none_or(|k| k.function != crate::production::FUNCTION_TRAINING) {
                        give_up(self);
                        continue;
                    }
                    if !self.trainees.iter().any(|(o, k, _)| *o == me && *k == f) {
                        self.trainees.retain(|(o, ..)| *o != me);
                        self.trainees.push((me, f, 0.0));
                    }
                    if let Some(op) = self.operators.get_mut(f)
                        && op.is_none()
                    {
                        *op = Some(me);
                    }
                    self.jobs_state.using.insert(me, (step.task, step.target));
                }
                // Filling machine at a storage of the player's: what it keeps of what I carry (its
                // main grid and worn backpack) put in, as far as it fits
                (FILLING_MACHINE, Target::Store(o)) => {
                    let k = o - BUILDING_OWNER;
                    if k >= self.buildings.len() || !near(pos, self.buildings[k].at, self.buildings[k].radius + STORE_REACH) {
                        give_up(self);
                        continue;
                    }
                    let keep = self.jobs_state.view.keeps.get(k).cloned().unwrap_or_default();
                    let mut put = 0;
                    for record in keep {
                        loop {
                            let inv = &mut self.folk[me].inventory;
                            let item = match inv.sections.iter().enumerate().filter(|(_, sec)| !sec.is_equipment()).find_map(|(s, sec)| sec.items.iter().position(|q| q.item.record == record).map(|i| (s, i))) {
                                Some((s, i)) => inv.take(s, i),
                                None => inv.bag_mut().and_then(|bag| {
                                    let i = bag.sections.first()?.items.iter().position(|q| q.item.record == record)?;
                                    bag.take(0, i)
                                }),
                            };
                            let Some(item) = item else { break };
                            if let Some(store) = self.buildings[k].storage.as_mut()
                                && store.add(item.clone(), false)
                            {
                                put += 1;
                                continue;
                            }
                            let _ = self.folk[me].inventory.add(item, false);
                            break;
                        }
                    }
                    if std::env::var("KENSHI_JOBS_DEBUG").is_ok() {
                        eprintln!("jobs: {me} ditched {put} things into {k}");
                    }
                    done(self);
                }
                // Task_GetOutOfBed: out of the bed, by it
                (GETTING_OUT_OF_BED_INJURED, _) => {
                    if let Some(b) = self.folk[me].bed.take() {
                        if self.operators.get(b).copied().flatten() == Some(me) {
                            self.operators[b] = None;
                        }
                        if self.bed_taken.get(b).copied().flatten() == Some(me) {
                            self.bed_taken[b] = None;
                        }
                        let at = self.furniture_approach[b].map(|v| v * kenshi_world::navmesh::METRES);
                        self.folk[me].mover.position = at;
                        self.arena.f[me].mover.position = at;
                        self.folk[me].woke_at = self.t as f64 / self.hour_secs;
                    }
                    done(self);
                }
                (TRAINING, Target::Building(b)) => {
                    if self.training(b).is_none() {
                        give_up(self);
                        continue;
                    }
                    self.folk[me].operate = Some(b);
                    self.jobs_state.using.insert(me, (step.task, step.target));
                }
                _ => give_up(self),
            }
        }
        if self.tick.is_multiple_of(VIEW_TICKS) || self.jobs_state.view.sites.len() != self.buildings.len() {
            self.jobs_state.view = self.jobs_view();
        }
    }
}
