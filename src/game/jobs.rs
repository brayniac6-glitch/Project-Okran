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
//!   on the building it finds) -> Filling machine (93) there: done when nothing I carry is
//!   taken in by one (DITCHED_ALL_RESOURCES, 147). Its finder (F038713 -> F038708,
//!   `storage_search`, with what I carry); (stand-in) only at home.
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
//! Hauling and feeding (docs/HAULING.md). The buildings' inventory objects (Building +0x2f8) are
//! a machine's (+0x3b0: a ProductionBuilding and those made like it, `crate::base::BuildingClass`)
//! containers (F029392: each ingredient's "in" container and the "out" one, crate::production)
//! and a StorageBuilding's typed containers (F029410: its "limit inventory"); their vtable
//! methods are `JobsView::accepts` (+0x4a0), `input_empty` (+0x4a8, 62), `items_of` (+0x548),
//! `item_kept` (+0x550), `wants` (+0x560), `has_room` (+0x568).
//! - Operating machine (87) wants the machine not full (63, F029267), no input empty (62,
//!   F029332), none of its output carried that a building would take (89, F038680), no input
//!   invalid (169, F038536).
//! - An input empty: Filling machine (93, Task_FillMachine F031179 -> F031147) at the machine
//!   with one of its inputs carried (67, F038703), else Try filling machine (94, a generic task;
//!   its finder F038633: what the machine wants, the goal's building holding it, else the item
//!   search F038632: the nearest StorageBuilding holding it, else machine) -> Collecting
//!   resource (92, Task_EmptyMachine F031048: as many as the machines and storages about have
//!   room for, F031335) at the storage.
//! - A full machine: Collecting resource at the machine takes its output; carrying it, Try
//!   emptying machine (113, a generic task; its finder F038713 -> F038708: the machines of my
//!   goals that take it in, else the nearest building that does, F038621) -> Filling machine
//!   there puts it in.
//! - "Auto-Haul" (F044603): on a machine of no operators, Operating automatic machine (152,
//!   Method 21 F038636: an operator slot for me or the machine full, or one of its inputs
//!   carried or found by F038633) with, as a job, its follow-up Hauling to (124) on it; on a
//!   storage, Hauling to (124, Task_FillMachine) on the storage: what it keeps fetched and put
//!   in, again while there is any.
use crate::base::{BUILDING_OWNER, BuildWork, BuildingClass};
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

pub const COLLECTING_RESOURCE: u16 = 92;
pub const FILLING_MACHINE: u16 = 93;
pub const TRY_FILLING_MACHINE: u16 = 94;
pub const TRY_EMPTYING_MACHINE: u16 = 113;
pub const DITCHING_TO_MAKE_SPACE: u16 = 114;
pub const HAULING_TO: u16 = 124;
pub const OPERATING_AUTOMATIC: u16 = 152;
pub const SITTING_AROUND: u16 = kenshi_ai::task_system::SITTING_AROUND;
pub const GOING_TO_BED: u16 = kenshi_ai::task_system::GOING_TO_BED;

/// The runnable steps carried out here (`jobs_tick`).
pub const JOB_RUNNABLE: [u16; 8] = [BUILDING, REPAIRING, ADDING_MATERIALS, TRAINING, COLLECTING_BUILD_MATS, GETTING_OUT_OF_BED_INJURED, COLLECTING_RESOURCE, HAULING_TO];
/// The tasks whose target finders are here.
pub const JOB_FINDERS: [u16; 7] = [ENGINEER, FINDING_BUILD_MATS, DITCHING_RESOURCES, TRY_FILLING_MACHINE, TRY_EMPTYING_MACHINE, DITCHING_TO_MAKE_SPACE, OPERATING_AUTOMATIC];
/// Generic tasks (the factory's case 107) among the steps: they carry nothing out. ((assumption)
/// The hauling goals 94, 113, 114 and 152 among them: they have no Task class of their own.)
const GENERIC: [u16; 9] = [ENGINEER, FINDING_BUILD_MATS, DITCHING_RESOURCES, SITTING_AROUND, GOING_TO_BED, TRY_FILLING_MACHINE, TRY_EMPTYING_MACHINE, DITCHING_TO_MAKE_SPACE, OPERATING_AUTOMATIC];
/// (stand-in) How near a building of the player's counts as at home, for the idle habits.
pub const HOME_RANGE: f32 = 2500.0;
/// F038650 / F038584: the health share at or under which the hurt go to bed, over which they
/// get up (0x14168a25c), and Going to bed's score then (0x14167fad0).
const HURT: f32 = 0.9;
const BED_SCORE: f32 = 0.1;
/// Collecting resource's score (AI vtable slot 23, F038610).
const COLLECT_SCORE: Score = Score::Method(23);
/// Collecting resource's (92) score (slot 22, F038709) and Operating automatic machine's (slot
/// 21, F038636).
const EMPTY_SCORE: Score = Score::Method(22);
const AUTO_SCORE: Score = Score::Method(21);
/// F038709: a building function 6's score with no storage for its output (0x14167fad0).
const FUNCTION_6_SCORE: f32 = 0.1;

/// (assumption) How near a building's middle counts as at it: its radius and a builder's reach
/// (crate::base), or a storage's fetching reach.
const SITE_REACH: f32 = 15.0;
const STORE_REACH: f32 = 40.0;
/// How near a building an operator works it (crate::production's `at_building`).
const OPERATE_REACH: f32 = 40.0;
/// (assumption) How near a loose item one picks it up: within the planner's isAtLocation
/// (AT_LOCATION_SQ 144, 12 units) and a step's slack.
const PICKUP_REACH: f32 = 25.0;
/// F050347: the falloff of a building's score with its squared distance (81000000), x 2; at
/// none or less, 0.0001.
const ENGINEER_FALLOFF_SQ: f32 = 81_000_000.0;
const ENGINEER_NEAR: f32 = 2.0;
const ENGINEER_FLOOR: f32 = 0.0001;

/// Whether a step is carried out here.
pub fn runs_here(step: &Task<Target>) -> bool {
    let player_store = matches!(step.target, Target::Store(o) if (BUILDING_OWNER..BOX_OWNER).contains(&o));
    JOB_RUNNABLE.contains(&step.task)
        || GENERIC.contains(&step.task)
        || (step.task == OPERATING_MACHINE && matches!(step.target, Target::Building(_)))
        || (matches!(step.task, FILLING_MACHINE | HAULING_TO | COLLECTING_RESOURCE) && (player_store || matches!(step.target, Target::Building(_))))
        || (matches!(step.task, COLLECTING_RESOURCE | COLLECTING_BUILD_MATS) && matches!(step.target, Target::Item(_)))
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

/// A container as the jobs read it: a machine's ingredient's or output's (F029392), a
/// StorageBuilding's typed one (F029410).
#[derive(Debug, Clone, Default)]
pub struct Feed {
    pub item: String,
    /// What its section holds (F018d09) and its limit (the entry's +0x8; 0 or below: none).
    pub held: u32,
    pub most: i32,
    /// Its section takes one more of the item (InventorySection vtable +0x8, F043593).
    pub room: bool,
    /// An ingredient's amount a unit of output (+0x4) and its banked level (+0x0).
    pub per: f32,
    pub level: f32,
}

impl Feed {
    /// @fn F029309
    ///
    /// Full: at its limit, or (none) its section taking no more.
    pub fn full(&self) -> bool {
        if self.most > 0 { self.held >= self.most as u32 } else { !self.room }
    }
}

/// What the jobs read of a building of the player's.
#[derive(Debug, Clone)]
pub struct Site {
    pub at: [f32; 3],
    pub radius: f32,
    pub confirmed: bool,
    /// isBuildingComplete (2, F038523).
    pub done: bool,
    /// Its construction paused (the construction state's +1, F037657).
    pub paused: bool,
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
    /// Its inventory's items by type in order, with how many (F029349's list).
    pub stock: Vec<(String, u32)>,
    /// Its class (the building factory's, F038257) and its type (+0x198).
    pub class: BuildingClass,
    pub kind: i32,
    /// +0x3b0: a machine, built (a ProductionBuilding or one made like it, a crafting bench, a
    /// furnace); +0x2f8: the inventory object the hauling reads (a machine or a
    /// StorageBuilding).
    pub machine: bool,
    pub hauls: bool,
    /// Its building function and its "max operators" as given (0 for an automatic one; F023204
    /// reads +0x3ac).
    pub function: i32,
    pub max_ops: i32,
    /// A machine made with containers (F029392): its ingredients' (+0x480) and its output's
    /// (+0x448).
    pub containers: bool,
    pub feeds: Vec<Feed>,
    pub out: Option<Feed>,
    /// A machine still set up the old way (a crafting bench, a furnace, a farm): its products
    /// ("produces", "item crafts"), its ingredients ("consumes") and its output (its products in
    /// its storage).
    pub products: Vec<String>,
    pub inputs: Vec<String>,
    pub output: Vec<(String, u32)>,
    /// A StorageBuilding's typed containers (F029410: +0x450, the first +0x448), its category
    /// (+0x440, "itemtype limit") and whether it is kept from emptying (+0x444).
    pub entries: Vec<Feed>,
    pub category: i32,
    pub kept: bool,
    /// Of the items asked about, those its inventory has room for one more of (Inventory
    /// vtable +0x20).
    pub room: Vec<String>,
    /// The output's full state (+0x468 == 1, F029267): crate::production's `full`.
    pub full: bool,
    /// A farm's crop (FarmBuilding: its stage +0x518, death +0x51c and "death threshold"
    /// +0x54c, plants taken off +0x528 of its plants +0x4f0).
    pub farm: Option<(f32, f32, f32, u32, u32)>,
}

/// An item lying loose as the jobs read it (`Sim::ground`): its id, record and place, its
/// owners (the town spot's, else the item's own, Item +0x2a8) and whether it lies in a town's
/// building (Item vtable +0x1d8).
#[derive(Debug, Clone, Default)]
pub struct Loose {
    pub id: u64,
    pub item: String,
    pub at: [f32; 3],
    pub owner: Option<String>,
    pub in_building: bool,
}

/// What the jobs read of one of the player's people.
#[derive(Debug, Clone, Default)]
pub struct Carrier {
    /// What its own inventory holds (Character +0x2e8, vtable +0x178 F039214: not the worn
    /// backpack's) and with the backpack's, by item; the items one more of which would fit
    /// (vtable +0x170 F039213: its inventory or its backpack).
    pub carried: HashMap<String, u32>,
    pub all: HashMap<String, u32>,
    pub fits: Vec<String>,
    /// The food it carries (Character vtable +0x2f0, F038531 / F030795: the stacks of "item
    /// function" 15 or 3, its own and its backpack's).
    pub foods: u32,
    /// What record types it carries (F039230's test, Character vtable +0x2f8: its backpack's
    /// and its own).
    pub kinds: Vec<i32>,
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
    /// The storage it would ditch into (Ditching resources' finder, F038713 -> F038708 with
    /// what it carries), at home with the habit on.
    pub ditch_to: Option<Target>,
    /// An arm it can work with (F050241).
    pub arms: bool,
    /// The task system's own target (AITaskSytem +0x10, F038633's "goal building"): (assumption)
    /// the building of the goal being followed (+0x1c0).
    pub goal_building: Option<usize>,
    /// The buildings its goals (the goals list +0x90: its jobs, F036821) are on, F038708's.
    pub goal_targets: Vec<usize>,
    /// The building of an Engineer goal being followed (+0x1c0, F038708 leaves its materials
    /// out).
    pub engineer: Option<usize>,
    /// Getting up (Character +0x278, Task_GetUp's start F031044): F038708 finds no storage.
    pub getting_up: bool,
    /// My squad's town is the player's (ActivePlatoon +0x88 a town of the player's faction,
    /// F050233): (assumption) as F038650 reads it, the outpost I am in or at home.
    pub player_town: bool,
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
    /// The record type (GameData +0x50) of the items asked about, and those that are food
    /// ("item function" 3, F031329).
    pub item_kinds: HashMap<String, i32>,
    pub food: Vec<String>,
    /// F038708's list for a character's target: the "death items" of "3979-gamedata.base".
    pub death_items: Vec<String>,
    /// The items lying loose (the item search's first part, F038632).
    pub ground: Vec<Loose>,
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
/// kind with room, else a section kept to it or the main grid's first free cell, else the worn
/// backpack).
fn one_fits(inv: &kenshi_char::inventory::Inventory, item: &kenshi_char::inventory::Item) -> bool {
    let mut trial = inv.clone();
    trial.add(item.clone(), false)
}

/// The AI's area (AI +0x2bc mode 2, about the character itself: the player's people's): the
/// squared reach of the storage and item searches (F038708, F038621, F038632: 4.24673e7).
const AREA_SQ: f32 = 42_467_328.0;
/// F038628: the item asked about when the target names none ("1515-gamedata.base", Raw Stone).
const NO_ITEM: &str = "1515-gamedata.base";
/// F038708: the record whose "death items" a character's target adds (looked up once, type 0x4c).
const DEATH_ITEMS: &str = "3979-gamedata.base";
/// F038632: how near me the loose items it looks at lie (0x1416f0110).
const GROUND_SEARCH: f32 = 1200.0;
/// F029371 / F029410: a StorageBuilding's item for its category when it keeps none (+0x441c85a30:
/// 2 weapons "477-gamedata.base", 3 armour "2165-gamedata.base").
fn category_item(category: i32) -> Option<&'static str> {
    match category {
        2 => Some("477-gamedata.base"),
        3 => Some("2165-gamedata.base"),
        _ => None,
    }
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
            Target::Building(_) => matches!(c, 1 | 2 | 50 | 51 | 56 | 71 | 73 | 93 | 62 | 63 | 66 | 67 | 82 | 89 | 105 | 169),
            Target::Store(o) => (BUILDING_OWNER..BOX_OWNER).contains(o) && mine && matches!(c, 1 | 2 | 62 | 63 | 66 | 67 | 82 | 89 | 105 | 150 | 169),
            Target::Item(_) => mine && matches!(c, 66 | 82 | 150),
            _ => false,
        }
    }

    /// Where loose item `id` lies.
    pub fn ground_at(&self, id: u64) -> Option<[f32; 3]> {
        self.ground.iter().find(|g| g.id == id).map(|g| g.at)
    }

    /// @fn F050233
    ///
    /// Whether I may take loose item `g`: in my squad's town of the player's, one not in a
    /// building not the player's; else one whose owners are none or my faction's.
    pub fn may_take(&self, me: usize, g: &Loose) -> bool {
        let Some(k) = self.people.get(&me) else { return false };
        if k.player_town && !g.in_building {
            return true;
        }
        g.owner.as_deref().is_none_or(|o| o == crate::sim::PLAYER_FACTION)
    }

    /// The building a planner target names (a building's own, or its storage's).
    pub fn building_of(t: &Target) -> Option<usize> {
        match *t {
            Target::Building(b) => Some(b),
            Target::Store(o) if (BUILDING_OWNER..BOX_OWNER).contains(&o) => Some(o - BUILDING_OWNER),
            _ => None,
        }
    }

    /// Building `k` as the planner's target: a machine by itself, a storage by its inventory.
    pub fn as_target(&self, k: usize) -> Target {
        if self.sites.get(k).is_some_and(|s| s.machine) { Target::Building(k) } else { Target::Store(BUILDING_OWNER + k) }
    }

    fn carries(&self, me: usize, item: &str) -> bool {
        self.people.get(&me).is_some_and(|k| k.carried.get(item).is_some_and(|&n| n > 0))
    }

    fn is_food(&self, item: &str) -> bool {
        self.food.iter().any(|r| r == item)
    }

    fn dist2(&self, k: usize, pos: [f32; 3]) -> f32 {
        let s = &self.sites[k];
        (s.at[0] - pos[0]).powi(2) + (s.at[1] - pos[1]).powi(2) + (s.at[2] - pos[2]).powi(2)
    }

    /// The nearest of `list` to `pos` within the AI's area (F038708, F038621, F038632's pick).
    fn nearest(&self, list: impl Iterator<Item = usize>, pos: [f32; 3]) -> Option<usize> {
        list.map(|k| (k, self.dist2(k, pos))).filter(|&(_, d)| d <= AREA_SQ).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(k, _)| k)
    }

    /// @fn F029329
    ///
    /// Building `k`'s vtable +0x4a0: whether it takes `item` in. A machine made with containers
    /// (F029329): built, its ingredient's container for it not full (a limit: under it; none:
    /// room). A StorageBuilding (F029328): built, its inventory with room, and the item kept
    /// (its typed containers') or, keeping none, of its category. (The machines still set up
    /// the old way take none in: their containers are not made.)
    pub fn accepts(&self, k: usize, item: &str) -> bool {
        let Some(s) = self.sites.get(k) else { return false };
        if !s.done {
            return false;
        }
        if s.containers {
            return s.feeds.iter().find(|f| f.item == item).is_some_and(|f| !f.full());
        }
        if s.class == BuildingClass::Storage {
            if !s.room.iter().any(|r| r == item) {
                return false;
            }
            if s.entries.is_empty() {
                return self.item_kinds.get(item).copied() == Some(s.category);
            }
            return s.entries.iter().any(|e| e.item == item);
        }
        false
    }

    /// Building `k`'s vtable +0x4a8 ("It's empty.", 62): a machine's input at none (F029332:
    /// an ingredient used, its container empty and nothing banked; a farm's only while growing
    /// and not dead or with all taken off, F022884); a wind generator's never
    /// (F038277); a StorageBuilding's typed container with room (F029373: under its limit, else
    /// room; keeping none, room for its category's item).
    pub fn input_empty(&self, k: usize) -> bool {
        let Some(s) = self.sites.get(k) else { return false };
        if s.class == BuildingClass::WindGenerator {
            return false;
        }
        // a farm's (F022884): growing and not dead, or all taken off, with its water at none
        if let Some((stage, death, threshold, harvested, plants)) = s.farm
            && !((stage < 1.0 && death <= threshold) || harvested >= plants)
        {
            return false;
        }
        if s.containers {
            return s.feeds.iter().any(|f| f.per != 0.0 && f.held == 0 && f.level <= 0.001);
        }
        if s.class == BuildingClass::Storage {
            return match s.entries.first() {
                Some(e) => {
                    if e.most > 0 { e.held < e.most as u32 } else { e.room }
                }
                None => category_item(s.category).is_some_and(|r| s.room.iter().any(|x| x == r)),
            };
        }
        false
    }

    /// Building `k`'s vtable +0x548, its items for the taking: a machine's output's item while
    /// it holds some (F029350); a StorageBuilding's items by type (F029349: none when it is kept
    /// from emptying).
    pub fn items_of(&self, k: usize) -> Vec<String> {
        let Some(s) = self.sites.get(k) else { return vec![] };
        if s.containers {
            return s.out.iter().filter(|o| o.held > 0).map(|o| o.item.clone()).collect();
        }
        if s.machine {
            return s.output.iter().map(|(r, _)| r.clone()).collect();
        }
        if s.class == BuildingClass::Storage && !s.kept {
            return s.stock.iter().map(|(r, _)| r.clone()).collect();
        }
        vec![]
    }

    /// Building `k`'s vtable +0x550 (F029371): its output's item; a StorageBuilding's typed
    /// container's, else its category's.
    pub fn item_kept(&self, k: usize) -> Option<String> {
        let s = self.sites.get(k)?;
        if s.containers {
            return s.out.as_ref().map(|o| o.item.clone());
        }
        if s.machine {
            return s.products.first().cloned();
        }
        if s.class == BuildingClass::Storage {
            return s.entries.first().map(|e| e.item.clone()).or_else(|| category_item(s.category).map(str::to_string));
        }
        None
    }

    /// Building `k`'s vtable +0x558 (F029265): what its output (a StorageBuilding's typed
    /// container) holds.
    fn kept_count(&self, k: usize) -> u32 {
        let Some(s) = self.sites.get(k) else { return 0 };
        if s.containers {
            return s.out.as_ref().map_or(0, |o| o.held);
        }
        if s.machine {
            return s.output.iter().map(|(_, n)| *n).sum();
        }
        s.entries.first().map_or(0, |e| e.held)
    }

    /// Building `k`'s vtable +0x560, what it wants: a machine's ingredients at none (F029353);
    /// a StorageBuilding's kept items while its typed container has room (F029352).
    pub fn wants(&self, k: usize) -> Vec<String> {
        let Some(s) = self.sites.get(k) else { return vec![] };
        if s.containers {
            return s.feeds.iter().filter(|f| f.per != 0.0 && f.held == 0).map(|f| f.item.clone()).collect();
        }
        if s.class == BuildingClass::Storage
            && let Some(e) = s.entries.first()
            && !e.full()
        {
            return s.entries.iter().map(|e| e.item.clone()).collect();
        }
        vec![]
    }

    /// Building `k`'s vtable +0x568, what it has room for: a machine's ingredients not full
    /// (F029351); a StorageBuilding's wants, its category's item with room, and its typed
    /// containers not full (F029372).
    pub fn has_room(&self, k: usize) -> Vec<String> {
        let Some(s) = self.sites.get(k) else { return vec![] };
        if s.containers {
            return s.feeds.iter().filter(|f| !f.full()).map(|f| f.item.clone()).collect();
        }
        if s.class == BuildingClass::Storage {
            let mut v = self.wants(k);
            if let Some(r) = category_item(s.category)
                && s.room.iter().any(|x| x == r)
            {
                v.push(r.to_string());
            }
            v.extend(s.entries.iter().filter(|e| !e.full()).map(|e| e.item.clone()));
            return v;
        }
        vec![]
    }

    /// @fn F038621
    ///
    /// The building for `item` (F04e1ed): of the player's buildings about with the hauling's
    /// inventory object (+0x2f8; all the player's are of my faction), built, not `except`,
    /// that take it in (vtable +0x4a0: a machine's ingredient, a storage's kept item), the
    /// nearest to `pos` within the AI's area.
    pub fn storage_for(&self, item: &str, except: Option<usize>, pos: [f32; 3]) -> Option<usize> {
        self.nearest((0..self.sites.len()).filter(|&k| Some(k) != except && self.sites[k].hauls && self.accepts(k, item)), pos)
    }

    /// @fn F038708
    ///
    /// The storage search (F038713's, with what I carry; F038709's, without): its items, those I
    /// carry and the target's (a storage's, when it holds any: its items (F029354); a machine's
    /// output (+0x548) that I carry or have room for, F038702), less the materials of an
    /// Engineer goal followed; the machines of my goals and the target that take one in
    /// (+0x4a0) are candidates and keep that item from the storages; for the rest, the building
    /// for it (F038621, not the target's), unless I am getting up (Character +0x278; the
    /// building's own area test, F038539 / F038606, holds for the player's: no parent); the
    /// nearest of them within the AI's area. A character's target adds the "death items" of
    /// "3979-gamedata.base" (`death_items`).
    pub fn storage_search(&self, me: usize, pos: [f32; 3], t: &Target, carried: bool) -> Option<Target> {
        let k = self.people.get(&me)?;
        let mut items: Vec<String> = Vec::new();
        let add = |r: &String, v: &mut Vec<String>| {
            if !v.contains(r) {
                v.push(r.clone());
            }
        };
        if matches!(t, Target::Person(_)) {
            for r in &self.death_items {
                add(r, &mut items);
            }
        }
        if carried {
            let mut c: Vec<&String> = k.all.iter().filter(|(_, n)| **n > 0).map(|(r, _)| r).collect();
            c.sort_unstable();
            for r in c {
                add(r, &mut items);
            }
        }
        let target = Self::building_of(t).filter(|&b| b < self.sites.len());
        let mut except = None;
        if let Some(b) = target {
            let s = &self.sites[b];
            if !s.machine && s.hauls {
                if !s.stock.is_empty() {
                    for e in s.entries.iter().filter(|e| e.held > 0) {
                        add(&e.item, &mut items);
                    }
                    except = Some(b);
                }
            } else if s.machine {
                except = Some(b);
                for r in self.items_of(b) {
                    if self.carries(me, &r) || k.fits.contains(&r) {
                        add(&r, &mut items);
                    }
                }
            }
        }
        if let Some(e) = k.engineer.and_then(|e| self.sites.get(e)) {
            items.retain(|r| !e.needed.iter().any(|(m, _)| m == r));
        }
        let mut found: Vec<usize> = Vec::new();
        let machines: Vec<usize> = k.goal_targets.iter().copied().chain(target).filter(|&b| self.sites.get(b).is_some_and(|s| s.machine)).collect();
        for b in machines {
            let s = &self.sites[b];
            items.retain(|r| {
                let takes = s.feeds.iter().any(|f| f.item == *r) && self.accepts(b, r);
                if takes {
                    found.push(b);
                }
                !takes
            });
        }
        if !k.getting_up {
            for r in &items {
                if let Some(b) = self.storage_for(r, except, pos) {
                    found.push(b);
                }
            }
        }
        self.nearest(found.into_iter(), pos).map(|b| self.as_target(b))
    }

    /// @fn F038632
    ///
    /// The item search for `item`, not at `except`, for me: of the items lying loose within 1200
    /// of me, the first of it I may take (F050233; the task system's +0x218 set of items left
    /// alone: (assumption) none, as nothing the rewrite runs adds to it); else the nearest
    /// StorageBuilding holding it (its items, +0x548, and its typed container not empty, +0x558)
    /// within the AI's area, else the nearest machine whose output holds it. (assumption) The
    /// AI's search cache (AI +0x208's map) is made afresh with each think.
    pub fn item_search(&self, me: usize, item: &str, except: usize, pos: [f32; 3]) -> Option<Target> {
        if let Some(g) = self.ground.iter().find(|g| g.item == item && (g.at[0] - pos[0]).hypot(g.at[2] - pos[2]) <= GROUND_SEARCH && self.may_take(me, g)) {
            return Some(Target::Item(g.id));
        }
        let holds = |k: usize| k != except && self.sites[k].done && self.sites[k].hauls && self.items_of(k).iter().any(|r| r == item) && self.kept_count(k) > 0;
        self.nearest((0..self.sites.len()).filter(|&k| holds(k) && !self.sites[k].machine), pos)
            .or_else(|| self.nearest((0..self.sites.len()).filter(|&k| holds(k) && self.sites[k].machine), pos))
            .map(|k| self.as_target(k))
    }

    /// @fn F038633
    ///
    /// Try filling machine's finder for target `t`: of what it wants (+0x560, else what it has
    /// room for, +0x568), one the goal's building (the task system's own target) holds (its
    /// +0x548): that building; else the item search (F038632) for each in order, the first
    /// found.
    pub fn fill_source(&self, me: usize, pos: [f32; 3], t: &Target) -> Option<Target> {
        let b = Self::building_of(t).filter(|&b| b < self.sites.len() && self.sites[b].hauls)?;
        let mut wants = self.wants(b);
        if wants.is_empty() {
            wants = self.has_room(b);
        }
        if let Some(g) = self.people.get(&me).and_then(|k| k.goal_building)
            && g != b
            && self.sites.get(g).is_some_and(|s| s.hauls)
        {
            let holds = self.items_of(g);
            if wants.iter().any(|w| holds.contains(w)) {
                return Some(self.as_target(g));
            }
        }
        wants.iter().find_map(|w| self.item_search(me, w, b, pos))
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
        if let Target::Item(id) = *t {
            let g = self.ground.iter().find(|g| g.id == id);
            return match c {
                // haveSomeResourcesFrom and HAVE_SOME_BUILD_MATS (F038535): a loose item in an
                // inventory (+0xd8): (one gone from the ground, taken)
                66 | 150 => Some(g.is_none()),
                // F038628: room for one of its kind
                82 => Some(g.is_some_and(|g| carrier.is_some_and(|k| k.fits.contains(&g.item)))),
                _ => None,
            };
        }
        let b = Self::building_of(t)?;
        let Some(s) = self.sites.get(b) else { return Some(false) };
        // a storage's being at: (assumption) within its radius and a fetcher's reach
        let reach = if matches!(t, Target::Store(_)) { STORE_REACH } else { SITE_REACH };
        Some(match c {
            // isAtLocation: (assumption) within its radius and a builder's reach
            1 => near(pos, s.at, s.radius + reach),
            // isBuildingComplete (F038523)
            2 => s.done,
            // buildingHasBuildingMaterials (F038690: not F037723)
            51 => !s.wants_materials,
            // isBuildingDamaged (F038575, the building's vtable +0x328)
            71 => s.damaged,
            // DOOR_IS_BROKEN (F038691)
            56 => s.door_broken,
            // "I don't have any materials" (0x14060e5b0: the AI +0x300 object's +0x70 flag, not
            // identified): (assumption) carrying one the building still needs
            50 => carries(&s.needed),
            // machineHasFreeOperatorSlot (F038577): a slot not taken by another
            73 => (s.operators.iter().filter(|&&o| o != me).count() as u32) < s.slots,
            // "It needs electricity."
            93 => s.powered,
            // "It's empty." (F038534: +0x4a8)
            62 => self.input_empty(b),
            // "The machine is full." (F038677: +0x578: a machine's output's full state, F029267;
            // a storage's F029266 false)
            // (a farm's, F022880: full and not dead)
            63 => s.machine && (!s.containers || s.out.is_some()) && s.full && s.farm.is_none_or(|(_, death, threshold, ..)| death < threshold),
            // isMachineOutputEmpty (F038679: +0x580: a machine's output, F029269 (none: false); a
            // storage's inventory, F029268)
            105 => {
                if s.containers {
                    s.out.as_ref().is_some_and(|o| o.held == 0)
                } else if s.machine {
                    s.output.is_empty()
                } else {
                    s.stock.is_empty()
                }
            }
            // haveSomeResourcesFrom (F038535) and HAVE_SOME_BUILD_MATS: carrying the target's
            // +0x550 item (Character +0x178)
            66 | 150 => self.item_kept(b).is_some_and(|r| self.carries(me, &r)),
            // "It's out of resources." (F038703)
            67 => self.out_of_resources(me, b),
            // "I don't have space to pick that up!" (F038628): room for one of the target's
            // +0x550 item, else of Raw Stone
            82 => {
                let r = self.item_kept(b).unwrap_or_else(|| NO_ITEM.to_string());
                carrier.is_some_and(|k| k.fits.contains(&r))
            }
            // haveSomeResourcesFromThisMachineButWantThemGoneIfPossible (F038680)
            89 => self.want_gone(me, b, pos),
            // isMachineInputInvalid (F038536): a crafting bench's +0x5a8 (the others: F023187
            // false); (depends on) the crafting benches' set-up (F029659), not yet theirs
            169 => false,
            _ => return None,
        })
    }

    /// @fn F038703
    ///
    /// "It's out of resources." (67) on building `b`: with no hauling inventory (+0x2f8) but an
    /// inventory, one of the things I carry that its +0x300 object takes in (a research bench:
    /// its inventory with room, F029262); of the category of items (+0x440 4: the machines and
    /// item storages), one of its containers (+0x588: a machine's ingredients, a storage's typed
    /// ones) I carry that is not full (F029309) (the last food carried, Character +0x2f0 one,
    /// kept back from one whose "item function" is 3); else (a storage of weapons or armour)
    /// carrying a thing of its category (Character +0x2f8, F039230).
    pub fn out_of_resources(&self, me: usize, b: usize) -> bool {
        let (Some(k), Some(s)) = (self.people.get(&me), self.sites.get(b)) else { return false };
        if !s.hauls {
            return s.class == BuildingClass::Research && k.carried.iter().any(|(r, n)| *n > 0 && s.room.contains(r));
        }
        if s.category == 4 {
            let containers: &[Feed] = if s.containers { &s.feeds } else { &s.entries };
            return containers.iter().any(|f| self.carries(me, &f.item) && !f.full() && (k.foods != 1 || !self.is_food(&f.item)));
        }
        k.kinds.contains(&s.category)
    }

    /// @fn F038680
    ///
    /// haveSomeResourcesFromThisMachineButWantThemGoneIfPossible (89) on building `b`: of the
    /// category of items (+0x440 4), its +0x550 item that I carry (Character +0x300, F038702:
    /// not my last food) with a building for it (F038621, none left out); else (a storage of
    /// weapons or armour) carrying a thing of its category (Character +0x2f8, F039230).
    pub fn want_gone(&self, me: usize, b: usize, pos: [f32; 3]) -> bool {
        let (Some(k), Some(s)) = (self.people.get(&me), self.sites.get(b)) else { return false };
        if !s.hauls {
            return false;
        }
        if s.category != 4 {
            return k.kinds.contains(&s.category);
        }
        let Some(r) = self.item_kept(b) else { return false };
        if self.is_food(&r) && k.foods == 1 {
            return false;
        }
        self.carries(me, &r) && self.storage_for(&r, None, pos).is_some()
    }

    /// Whether this view scores the definition.
    pub fn scores(d: &TaskDef) -> bool {
        d.score == COLLECT_SCORE || d.score == EMPTY_SCORE || d.score == AUTO_SCORE
    }

    /// @fn F038709
    ///
    /// Collecting resource's (92) score for one of the player's: the target's items for the
    /// taking (+0x548; none and the building not of my faction: its inventory's: the player's
    /// are mine); with room for the first (Character +0x170): 1 when my squad has no town
    /// (Character +0x78: (assumption) as F038650 reads it, the outpost I am in or at home),
    /// else 1 when F038708 finds a building, ignoring what I carry, else none (0.1 for a
    /// crafting bench, type 6). A loose item, not in an inventory: 1.
    pub fn empty_score(&self, me: usize, pos: [f32; 3], t: &Target) -> f32 {
        let Some(k) = self.people.get(&me) else { return 0.0 };
        // a loose item not in an inventory (+0xd8): 1
        if let Target::Item(id) = *t {
            return if self.ground_at(id).is_some() { 1.0 } else { 0.0 };
        }
        let Some(b) = Self::building_of(t).filter(|&b| self.sites.get(b).is_some_and(|s| s.hauls)) else { return 0.0 };
        let list = self.items_of(b);
        let Some(first) = list.first() else { return 0.0 };
        if !k.fits.contains(first) {
            return 0.0;
        }
        if k.outpost.is_none() && !k.home {
            return 1.0;
        }
        if self.storage_search(me, pos, t, false).is_some() {
            1.0
        } else if self.sites[b].kind == 6 {
            FUNCTION_6_SCORE
        } else {
            0.0
        }
    }

    /// @fn F038636
    ///
    /// Operating automatic machine's (152) score for one of the player's on a machine: 1 with an
    /// operator slot for me (+0x500, F023204: one of its operators, or fewer than its "max
    /// operators") or the machine full (+0x578); else 1 carrying one of what it has room for
    /// (+0x568) or with Try filling machine's finder (F038633, through the AI's search cache
    /// F039866) finding a source for it; else none.
    pub fn auto_score(&self, me: usize, pos: [f32; 3], t: &Target) -> f32 {
        let (Some(_), Target::Building(b)) = (self.people.get(&me), *t) else { return 0.0 };
        let Some(s) = self.sites.get(b) else { return 0.0 };
        if !s.hauls {
            return 0.0;
        }
        let others = s.operators.iter().filter(|&&o| o != me).count() as i32;
        if s.operators.contains(&me) || others < s.max_ops || (s.machine && s.full) {
            return 1.0;
        }
        let room = self.has_room(b);
        if room.iter().any(|r| self.carries(me, r)) {
            return 1.0;
        }
        if !room.is_empty() && self.fill_source(me, pos, t).is_some() {
            return 1.0;
        }
        0.0
    }

    /// @fn F038610
    ///
    /// Collecting resource's (247) score: 1 when one more of the target's item would fit me
    /// (Character +0x170): a loose item's kind, a building's first item for the taking
    /// (+0x548); else none.
    pub fn score(&self, me: usize, pos: [f32; 3], d: &TaskDef, t: &Target) -> f32 {
        if d.score == EMPTY_SCORE {
            return self.empty_score(me, pos, t);
        }
        if d.score == AUTO_SCORE {
            return self.auto_score(me, pos, t);
        }
        if d.score != COLLECT_SCORE {
            return 0.0;
        }
        let item = match *t {
            Target::Item(id) => self.ground.iter().find(|g| g.id == id).map(|g| g.item.clone()),
            _ => Self::building_of(t).filter(|&b| self.sites.get(b).is_some_and(|s| s.hauls)).and_then(|b| self.items_of(b).into_iter().next()),
        };
        match (item, self.people.get(&me)) {
            (Some(r), Some(k)) if k.fits.contains(&r) => 1.0,
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
            // Ditching resources' storage (F038713, worked out with the view)
            DITCHING_RESOURCES => self.people.get(&me).and_then(|k| k.ditch_to),
            // F038713 (Try emptying machine, Ditching to make space, Operating automatic
            // machine): F038708's building, with what I carry
            TRY_EMPTYING_MACHINE | DITCHING_TO_MAKE_SPACE | OPERATING_AUTOMATIC if self.people.contains_key(&me) => self.storage_search(me, pos, t, true),
            TRY_FILLING_MACHINE if self.people.contains_key(&me) => self.fill_source(me, pos, t),
            ENGINEER => self.engineer_site(me, pos).map(Target::Building),
            FINDING_BUILD_MATS => match *t {
                Target::Building(b) => self.materials_store(me, b, pos),
                _ => None,
            },
            _ => None,
        }
    }

    /// @fn F038676
    ///
    /// Finding building mats' finder: of the materials building `b` still needs (+0x228), in
    /// order, the first the item search (F038632) finds.
    pub fn materials_store(&self, me: usize, b: usize, pos: [f32; 3]) -> Option<Target> {
        let site = self.sites.get(b)?;
        site.needed.iter().find_map(|(m, _)| self.item_search(me, m, b, pos))
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
    /// whose plans work on it (F028562); not one whose construction is paused (F050345: its
    /// construction state's +1). F050345's other state tests, the construction state's +2 (set
    /// as the building is taken down, F029409) and +8 (damage waiting to be dealt, F037704),
    /// always pass here: a building taken down is gone at once and blows are dealt at once
    /// (crate::upkeep). (not ported) the 0.5 for a building whose vtable +0x2c8 answers, and
    /// the use test F038529 (the player's own buildings pass it).
    pub fn engineer_site(&self, me: usize, pos: [f32; 3]) -> Option<usize> {
        let carrier = self.people.get(&me)?;
        let mut best: Option<(usize, f32)> = None;
        for (b, s) in self.sites.iter().enumerate() {
            if !s.confirmed || (carrier.outpost.is_some() && s.outpost != carrier.outpost) {
                continue;
            }
            if (s.done && !s.damaged) || s.paused {
                continue;
            }
            let d2 = (s.at[0] - pos[0]).powi(2) + (s.at[1] - pos[1]).powi(2) + (s.at[2] - pos[2]).powi(2);
            let mut score = (d2 - ENGINEER_FALLOFF_SQ) / (0.0 - ENGINEER_FALLOFF_SQ) * ENGINEER_NEAR;
            if score <= 0.0 {
                score = ENGINEER_FLOOR;
            }
            if s.wants_materials && !s.needed.iter().any(|(r, _)| carrier.carried.get(r).is_some_and(|&n| n > 0)) && self.materials_store(me, b, pos).is_none() {
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
        let item = |r: &str| data.get(r).map(|rec| kenshi_char::inventory::Item::from_record(rec, 1));
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
            let func = rec.and_then(|r| crate::production::function_of(data, r));
            let class = rec.map_or(BuildingClass::Plain, |r| BuildingClass::of(data, r));
            // +0x3b0: a ProductionBuilding (and those made like it), a crafting bench, a furnace;
            // +0x2f8: those and a StorageBuilding
            let machine = pb.done && (class.is_production() || matches!(class, BuildingClass::Crafting | BuildingClass::Furnace));
            let hauls = machine || (pb.done && class == BuildingClass::Storage);
            // a machine made with containers (F029392): its ingredients' and output's
            let parts = if machine { self.machine_parts(b) } else { None };
            let containers = parts.as_ref().is_some_and(|(i, o)| !i.is_empty() || o.is_some());
            let feed = |s: usize, record: &str, most: i32, per: f32, level: f32| -> Feed {
                let sec = &pb.storage.as_ref().expect("containers").sections[s];
                Feed {
                    item: record.to_string(),
                    held: sec.count(),
                    most,
                    room: item(record).is_some_and(|it| sec.items.first().map_or(sec.takes(&it), |p| p.item.record == it.record && p.item.quantity < sec.stack_of(&it))),
                    per,
                    level,
                }
            };
            let (feeds, out) = match parts.filter(|_| containers) {
                Some((inputs, output)) => (
                    inputs.iter().enumerate().map(|(k, (s, r, per))| feed(*s, r, pb.storage.as_ref().map_or(0, |i| i.sections[*s].stack as i32), *per, pb.taken.get(k).copied().unwrap_or(0.0))).collect(),
                    output.map(|(s, r, most)| feed(s, &r, most as i32, 1.0, pb.made.first().copied().unwrap_or(0.0))),
                ),
                None => (vec![], None),
            };
            // a machine still set up the old way: its products, ingredients and output
            let (products, inputs): (Vec<String>, Vec<String>) = match func.filter(|_| machine && !containers) {
                Some((f, _)) => (
                    f.refs("produces").iter().chain(f.refs("item crafts").iter()).map(|(s, _)| s.clone()).collect(),
                    f.refs("consumes").iter().filter(|(_, v)| v[1] > 0).map(|(s, _)| s.clone()).collect(),
                ),
                None => (vec![], vec![]),
            };
            let output: Vec<(String, u32)> = if machine && !containers { pb.storage.as_ref().map_or(vec![], count).into_iter().filter(|(r, _)| products.contains(r)).collect() } else { vec![] };
            // a StorageBuilding's typed containers (F029410)
            let entries: Vec<Feed> = match (class, rec, pb.storage.as_ref().and_then(|i| i.sections.first())) {
                (BuildingClass::Storage, Some(r), Some(sec)) if pb.done => crate::base::storage_entries(r)
                    .iter()
                    .map(|(it, most)| Feed {
                        item: it.clone(),
                        held: sec.count(),
                        most: *most,
                        room: item(it).is_some_and(|i| one_fits(&kenshi_char::inventory::Inventory { sections: vec![sec.clone()] }, &i)),
                        per: 0.0,
                        level: 0.0,
                    })
                    .collect(),
                _ => vec![],
            };
            let max_ops = rec.map_or(1, |r| {
                let own = r.ints.get("max operators").copied().unwrap_or(-1);
                if own < 0 { func.and_then(|(f, _)| f.ints.get("max operators").copied()).unwrap_or(1) } else { own }
            });
            sites.push(Site {
                at: pb.at,
                radius: pb.radius,
                confirmed: pb.confirmed,
                done: pb.done,
                paused: pb.paused,
                damaged: self.needs_repair(b),
                door_broken: self.door_states.iter().any(|d| d.player_building == Some(b) && !d.gone && d.broken),
                wants_materials: !pb.done && pb.total() > 0 && pb.progress >= pb.used as f32,
                needed,
                outpost: pb.outpost,
                slots,
                operators,
                powered: self.has_power(b),
                stock: if pb.done { pb.storage.as_ref().map_or(vec![], count) } else { vec![] },
                class,
                kind: class.kind(),
                machine,
                hauls,
                function: func.map_or(0, |(_, k)| k),
                max_ops,
                containers,
                feeds,
                out,
                products,
                inputs,
                output,
                entries,
                category: rec.map_or(4, crate::base::storage_category),
                kept: rec.is_some_and(|r| class == BuildingClass::Storage && crate::base::storage_kept(data, r)),
                room: vec![],
                full: pb.full,
                farm: (class == BuildingClass::Farm).then(|| {
                    let threshold = rec.and_then(|r| r.refs("farm data").first().and_then(|(s, _)| data.get(s))).map_or(0.0, |fd| fd.floats.get("death threshold").copied().unwrap_or(0.0));
                    (pb.stage, pb.death, threshold, pb.harvested, pb.plants.len() as u32)
                }),
            });
        }
        // the items asked about: the materials wanted, the buildings' items, kept items and
        // containers', the machines' products, what the player's people carry, Raw Stone
        let mut asked: Vec<String> = vec![NO_ITEM.to_string()];
        let ask = |r: &String, v: &mut Vec<String>| {
            if !v.contains(r) {
                v.push(r.clone());
            }
        };
        for s in &sites {
            for r in s.needed.iter().chain(s.stock.iter()).map(|(r, _)| r).chain(s.products.iter()).chain(s.feeds.iter().chain(s.out.iter()).chain(s.entries.iter()).map(|f| &f.item)) {
                ask(r, &mut asked);
            }
            if let Some(r) = category_item(s.category) {
                ask(&r.to_string(), &mut asked);
            }
        }
        for p in self.folk.iter().filter(|p| p.player && !p.dead) {
            for q in p.inventory.sections.iter().chain(p.inventory.bag().into_iter().flat_map(|b| b.sections.iter())).flat_map(|s| s.items.iter()) {
                ask(&q.item.record, &mut asked);
            }
        }
        let item_kinds: HashMap<String, i32> = asked.iter().filter_map(|r| data.get(r).map(|rec| (r.clone(), rec.kind))).collect();
        let food: Vec<String> = asked.iter().filter(|r| data.get(r).is_some_and(|rec| rec.ints.get("item function").copied() == Some(3))).cloned().collect();
        // of the items asked about, what each building's inventory has room for one more of
        for (b, s) in sites.iter_mut().enumerate() {
            let Some(inv) = self.buildings[b].storage.as_ref().filter(|_| s.done) else { continue };
            s.room = asked.iter().filter(|r| item(r).is_some_and(|i| one_fits(inv, &i))).cloned().collect();
        }
        let opts = &self.ai_options;
        let at_home = |at: [f32; 3]| self.buildings.iter().any(|b| b.confirmed && near(b.at, at, HOME_RANGE));
        let goal_building = |t: &Target| JobsView::building_of(t).filter(|&b| b < self.buildings.len());
        let mut people = HashMap::new();
        for p in self.folk.iter().filter(|p| p.player && !p.dead) {
            let mut carried: HashMap<String, u32> = HashMap::new();
            let mut all: HashMap<String, u32> = HashMap::new();
            let mut kinds: Vec<i32> = Vec::new();
            let mut foods = 0;
            for (own, s) in p.inventory.sections.iter().map(|s| (true, s)).chain(p.inventory.bag().into_iter().flat_map(|b| b.sections.iter().map(|s| (false, s)))) {
                for q in &s.items {
                    if own {
                        *carried.entry(q.item.record.clone()).or_default() += q.item.quantity;
                    }
                    if !s.is_equipment() || !own {
                        *all.entry(q.item.record.clone()).or_default() += q.item.quantity;
                    }
                    let rec = data.get(&q.item.record);
                    if let Some(k) = rec.map(|r| r.kind)
                        && !kinds.contains(&k)
                    {
                        kinds.push(k);
                    }
                    if rec.is_some_and(|r| matches!(r.ints.get("item function").copied(), Some(3 | 15))) {
                        foods += 1;
                    }
                }
            }
            let fits = asked.iter().filter(|r| item(r).is_some_and(|i| one_fits(&p.inventory, &i))).cloned().collect();
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
            let current = p.ai.current.as_ref();
            people.insert(
                p.id,
                Carrier {
                    carried,
                    all,
                    fits,
                    foods,
                    kinds,
                    outpost,
                    in_town: p.town.is_some(),
                    home,
                    idle,
                    aid: m.aid_need(false).flesh,
                    health: if most > 0.0 { h / most } else { 1.0 },
                    in_bed: p.bed.is_some(),
                    ditch_to: None,
                    // F050241: alive and up (Character +0x5bc, +0x5b9) with an arm working (+0x5bd,
                    // +0x5be: (assumption) an arm not lost with health above none)
                    // (an animal's, the character's vtable +0x248, always)
                    arms: p.race.is_some() || (!p.dead && !p.down && m.parts.iter().any(|q| q.kind == 2 && q.slot != 1 && q.health() > 0.0)),
                    goal_building: current.and_then(|t| goal_building(&t.target)),
                    goal_targets: p.ai.goals.iter().filter_map(|t| goal_building(&t.target)).collect(),
                    engineer: current.filter(|t| t.task == ENGINEER).and_then(|t| goal_building(&t.target)),
                    getting_up: p.getting_up > 0.0,
                    player_town: outpost.is_some() || home,
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
        let death_items = data.get(DEATH_ITEMS).map_or(vec![], |r| r.refs("death items").iter().map(|(s, _)| s.clone()).collect());
        let ground = self
            .ground
            .iter()
            .map(|g| Loose {
                id: g.id,
                item: g.item.record.clone(),
                at: g.at,
                owner: self.spots.iter().find(|s| s.item == Some(g.id)).and_then(|s| s.owner.clone()).or_else(|| g.item.owner.clone()),
                in_building: kenshi_world::interior::building_at(&self.inner, g.at).is_some(),
            })
            .collect();
        let mut view = JobsView { sites, people, claims_on, beds, chairs, item_kinds, food, death_items, ground };
        // Ditching resources' storage (F038713 -> F038708, no target: of what I carry, the
        // nearest building that takes one in), (stand-in) at home with the habit on
        let ditch: Vec<(usize, Option<Target>)> = view
            .people
            .iter()
            .filter(|(_, k)| k.home && opts.ditch_items)
            .map(|(&id, _)| (id, view.storage_search(id, self.folk[id].mover.position_units(), &Target::None, true)))
            .collect();
        for (id, k) in ditch {
            if let Some(c) = view.people.get_mut(&id) {
                c.ditch_to = k;
            }
        }
        view
    }

    /// @fn F031289
    ///
    /// Task_Pickup's update: loose item `id` picked up by `me`. Only items, weapons, armour,
    /// containers, nest items, map items and the two blueprint-like kinds (record types 4, 2, 3,
    /// 0x2e, 0x56, 0x66, 0x6b, 0x6f) are. One in a building of a faction not mine (its owners,
    /// Item +0x2a8, not of my faction, the faction not "not real", +0x1d0) is stolen: the crime against
    /// that faction (F046495) and, picked up, the item kept as theirs (its owners set, vtable
    /// +0x2b0) and my thieving trained (F047469: 20). Into my inventory, else my backpack
    /// (Character +0x168): its sound (F039184 / F039186: "Character_Pick_Up_Ground" with the
    /// item's Inventory_Items, `Sim::ground_pickups`);
    /// none of it fitting: "I can't pick that up" said. (assumption) An animal's food (an animal
    /// picking up an item of "item function" 17: eaten) is not asked: animals have no jobs.
    /// Whether it was taken.
    pub fn pick_up_item(&mut self, me: usize, id: u64) -> bool {
        let Some(k) = self.ground.iter().position(|g| g.id == id) else { return false };
        let kind = self.ground[k].item.kind.map_or(-1, |t| t as i32);
        if !matches!(kind, 4 | 2 | 3 | 0x2e | 0x56 | 0x66 | 0x6b | 0x6f) {
            return false;
        }
        let at = self.ground[k].at;
        let owner = self.spots.iter().find(|s| s.item == Some(id)).and_then(|s| s.owner.clone()).or_else(|| self.ground[k].item.owner.clone());
        let mine = self.faction_id(me);
        let stolen = kenshi_world::interior::building_at(&self.inner, at).is_some()
            && owner.as_deref().is_some_and(|o| Some(o) != mine.as_deref() && !self.faction_not_real(o));
        let mut item = self.ground[k].item.clone();
        if stolen {
            item.owner = owner.clone();
        }
        let (name, record) = (item.name.clone(), item.record.clone());
        if !self.folk[me].inventory.add(item, false) {
            self.say(me, "I can't pick that up".to_string());
            return false;
        }
        self.ground.remove(k);
        if stolen {
            self.commit_crime(me, kenshi_ai::CrimeEnum::CrimeStealing, owner.as_deref());
            self.arena.f[me].skills.train(kenshi_char::StatsEnumerated::StatThieving, 1.0, 20.0);
        }
        if self.ground_pickups.len() < 64 {
            self.ground_pickups.push((me, record));
        }
        if self.folk[me].player {
            self.messages.push(format!("{} picked up {name}", self.display_name(me)));
        }
        true
    }

    /// One of `record` taken out of `me`'s own inventory (F043586: its items in order, any
    /// section), else (`bag`) its worn backpack's.
    fn take_one_carried(&mut self, me: usize, record: &str, bag: bool) -> Option<kenshi_char::inventory::Item> {
        let inv = if bag { self.folk[me].inventory.bag_mut()? } else { &mut self.folk[me].inventory };
        for s in &mut inv.sections {
            if let Some(i) = s.items.iter().position(|q| q.item.record == record) {
                let mut one = s.items[i].item.clone();
                one.quantity = 1;
                if s.items[i].item.quantity > 1 {
                    s.items[i].item.quantity -= 1;
                } else {
                    s.items.remove(i);
                }
                return Some(one);
            }
        }
        None
    }

    /// @fn F031335
    ///
    /// Task_EmptyMachine's count for `item` (its vtable +0x28): the room for it about me. One
    /// to begin with when I carry no food (Character +0x2f0) and it is food (F031329); then for
    /// each building of the player's about (the hauling's inventory object, not `except`) that
    /// takes it in (+0x4a0), its containers in order (+0x588 / +0x590: a machine's
    /// ingredients, a storage's typed ones): the one of the item adds its limit less what it
    /// holds (no limit: one if it has room), the building done with; one of another item, its
    /// inventory taking it (+0x160 +0x20): 5 at once.
    fn room_about(&self, me: usize, item: &str, except: usize) -> u32 {
        let view = &self.jobs_state.view;
        let mut n = u32::from(view.people.get(&me).is_some_and(|k| k.foods == 0) && view.food.iter().any(|r| r == item));
        for k in 0..view.sites.len() {
            let s = &view.sites[k];
            if k == except || !s.hauls || !view.accepts(k, item) {
                continue;
            }
            let containers: Vec<&Feed> = if s.containers { s.feeds.iter().collect() } else { s.entries.first().into_iter().chain(s.entries.iter()).collect() };
            for c in containers {
                if c.item == item {
                    n += if c.most == -1 {
                        u32::from(c.room)
                    } else {
                        (c.most - c.held as i32).max(0) as u32
                    };
                    break;
                }
                if s.room.iter().any(|r| r == item) {
                    return 5;
                }
            }
        }
        n
    }

    /// @fn F031048
    ///
    /// Task_EmptyMachine's update at building `b` (a machine's output, a storage's items): of
    /// its items for the taking (+0x548), the first with room about for it (F031335's count;
    /// none: a crafting bench's first I have room for, one); I with room for it (Character
    /// +0x170), as many as the count taken one by one while one more fits (F04c39d out of its
    /// inventory, Character +0x168 into mine). The full state (+0x468) 1 -> 2 for a
    /// ProductionBuilding (type 4). How many were taken; none (no item, no room): given up
    /// (F01953d).
    fn empty_machine(&mut self, me: usize, b: usize) -> u32 {
        let view = &self.jobs_state.view;
        let list = view.items_of(b);
        if list.is_empty() {
            return 0;
        }
        let mut pick: Option<(String, u32)> = None;
        for r in &list {
            let n = self.room_about(me, r, b);
            pick = Some((r.clone(), n));
            if n > 0 {
                break;
            }
        }
        let kind = view.sites.get(b).map_or(0, |s| s.kind);
        if pick.as_ref().is_some_and(|(_, n)| *n == 0) && kind == 6 {
            for r in &list {
                if view.people.get(&me).is_some_and(|k| k.fits.contains(r)) {
                    pick = Some((r.clone(), 1));
                }
            }
        }
        let Some((r, mut n)) = pick else { return 0 };
        let Some(one) = self.data.get(&r).map(|rec| kenshi_char::inventory::Item::from_record(rec, 1)) else { return 0 };
        let mut taken = 0;
        while n >= 1 && one_fits(&self.folk[me].inventory, &one) {
            let Some(got) = self.buildings[b].storage.as_mut().and_then(|inv| crate::production::take_item(inv, &r)) else { break };
            if !self.folk[me].inventory.add(got.clone(), false) {
                if let Some(inv) = self.buildings[b].storage.as_mut() {
                    inv.add(got, false);
                }
                break;
            }
            n -= 1;
            taken += 1;
        }
        if taken > 0 && kind == 4 && self.buildings[b].full {
            self.buildings[b].full = false;
        }
        taken
    }

    /// @fn F031179
    ///
    /// Task_FillMachine's update at building `b` (Filling machine, Hauling to), for my own
    /// inventory and then my worn backpack: a machine's containers filled (F031147: for each
    /// ingredient, while its container takes one and I carry one, one put in); a
    /// StorageBuilding's inventory (F031167: my things of its category, F043541, not equipped,
    /// each put in, stopping at the first it does not take); one with no hauling inventory but an
    /// inventory, everything I carry (F031146). How many went in (none: the task ends,
    /// F036886).
    fn fill_machine(&mut self, me: usize, b: usize) -> u32 {
        let Some(s) = self.jobs_state.view.sites.get(b).cloned() else { return 0 };
        let mut put = 0;
        for bag in [false, true] {
            if bag && self.folk[me].inventory.bag().is_none() {
                continue;
            }
            if s.containers {
                // F031147
                let Some((inputs, _)) = self.machine_parts(b) else { return put };
                for (sec, record, _) in inputs {
                    loop {
                        let takes = self.buildings[b].storage.as_ref().is_some_and(|inv| {
                            let c = &inv.sections[sec];
                            c.items.first().is_none_or(|p| p.item.quantity < c.stack_of(&p.item))
                        });
                        if !takes {
                            break;
                        }
                        let Some(one) = self.take_one_carried(me, &record, bag) else { break };
                        let into = self.buildings[b].storage.as_mut().expect("containers");
                        match into.sections[sec].items.first_mut() {
                            Some(p) => p.item.quantity += 1,
                            None => into.sections[sec].items.push(kenshi_char::inventory::Placed { item: one, x: 0, y: 0 }),
                        }
                        put += 1;
                    }
                }
            } else if s.class == BuildingClass::Storage {
                // F031167: my things of its category (F043541: not equipped), in order
                let inv = if bag { self.folk[me].inventory.bag() } else { Some(&self.folk[me].inventory) };
                let list: Vec<(usize, usize, String)> = inv.map_or(vec![], |inv| {
                    inv.sections
                        .iter()
                        .enumerate()
                        .filter(|(_, sec)| bag || !sec.is_equipment())
                        .flat_map(|(si, sec)| sec.items.iter().enumerate().map(move |(ii, q)| (si, ii, q.item.record.clone())))
                        .filter(|(_, _, r)| self.data.get(r).is_some_and(|rec| rec.kind == s.category))
                        .collect()
                });
                // each whole stack in turn (Inventory +0x10 takes it all, or none)
                let mut removed: Vec<(usize, usize)> = Vec::new();
                for (si, ii, _) in list {
                    let shift = removed.iter().filter(|&&(a, b2)| a == si && b2 < ii).count();
                    let idx = ii - shift;
                    let inv = if bag { self.folk[me].inventory.bag_mut().expect("bag") } else { &mut self.folk[me].inventory };
                    let Some((px, py)) = inv.sections.get(si).and_then(|sec| sec.items.get(idx)).map(|q| (q.x, q.y)) else { continue };
                    let Some(item) = inv.take(si, idx) else { continue };
                    let n = item.quantity;
                    let mut trial = self.buildings[b].storage.clone();
                    if trial.as_mut().is_some_and(|t| t.add(item.clone(), false)) {
                        self.buildings[b].storage = trial;
                        removed.push((si, ii));
                        put += n;
                    } else {
                        let inv = if bag { self.folk[me].inventory.bag_mut().expect("bag") } else { &mut self.folk[me].inventory };
                        inv.sections[si].items.insert(idx, kenshi_char::inventory::Placed { item, x: px, y: py });
                        // (back where it was)
                        return put;
                    }
                }
            } else if !s.hauls && self.buildings[b].storage.is_some() {
                // F031146: everything I carry (not equipped), each put in or kept
                let inv = if bag { self.folk[me].inventory.bag() } else { Some(&self.folk[me].inventory) };
                let list: Vec<(usize, usize)> = inv.map_or(vec![], |inv| {
                    inv.sections.iter().enumerate().filter(|(_, sec)| bag || !sec.is_equipment()).flat_map(|(si, sec)| (0..sec.items.len()).map(move |ii| (si, ii))).collect()
                });
                let mut removed: Vec<(usize, usize)> = Vec::new();
                for (si, ii) in list {
                    let idx = ii - removed.iter().filter(|&&(a, b2)| a == si && b2 < ii).count();
                    let inv = if bag { self.folk[me].inventory.bag_mut().expect("bag") } else { &mut self.folk[me].inventory };
                    let Some((px, py)) = inv.sections.get(si).and_then(|sec| sec.items.get(idx)).map(|q| (q.x, q.y)) else { continue };
                    let Some(item) = inv.take(si, idx) else { continue };
                    let n = item.quantity;
                    if self.buildings[b].storage.as_mut().is_some_and(|t| t.add(item.clone(), false)) {
                        removed.push((si, ii));
                        put += n;
                    } else {
                        let inv = if bag { self.folk[me].inventory.bag_mut().expect("bag") } else { &mut self.folk[me].inventory };
                        inv.sections[si].items.insert(idx, kenshi_char::inventory::Placed { item, x: px, y: py });
                    }
                }
            }
        }
        put
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
        // (in a fixed order: the map's own is the hasher's)
        let mut using: Vec<(usize, (u16, Target))> = self.jobs_state.using.iter().map(|(&k, &v)| (k, v)).collect();
        using.sort_unstable_by_key(|u| u.0);
        for (me, (task, t)) in using {
            let still = self.folk.get(me).is_some_and(|p| !p.down && !p.dead && p.ai.queue.first().is_some_and(|s| s.task == task && s.target == t));
            if !still {
                self.jobs_state.using.remove(&me);
                self.release(me, t);
            }
        }
        let ids: Vec<usize> = self.folk.iter().filter(|p| !p.dead && !p.down && !p.away && p.ai.queue.first().is_some_and(runs_here)).map(|p| p.id).collect();
        // a step that moved items: the view made afresh for the next think
        let mut moved = false;
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
                // Collecting resource (build mats) on a loose item: Task_Pickup (the factory's
                // other class for it, F030716), as for 92
                (COLLECTING_BUILD_MATS, Target::Item(id)) => {
                    let Some(at) = self.jobs_state.view.ground_at(id) else { give_up(self); continue };
                    if !near(pos, at, PICKUP_REACH) {
                        give_up(self);
                        continue;
                    }
                    if self.pick_up_item(me, id) {
                        moved = true;
                        done(self);
                    } else {
                        give_up(self);
                    }
                }
                // Task_EmptyMachine_BuildMats at a storage: of the materials the plan's building
                // still needs (the next building step's), what it holds, as far as they fit.
                // (assumption) All at once; the amounts are the building's wants less what I
                // carry.
                (COLLECTING_BUILD_MATS, Target::Store(_) | Target::Building(_)) => {
                    let Some(k) = JobsView::building_of(&step.target).filter(|&k| k < self.buildings.len()) else { give_up(self); continue };
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
                // Collecting resource on a loose item: Task_Pickup (the factory's other class for
                // it, F030716; its update F031289, `pick_up_item`), by it
                (COLLECTING_RESOURCE, Target::Item(id)) => {
                    let Some(at) = self.jobs_state.view.ground_at(id) else { give_up(self); continue };
                    if !near(pos, at, PICKUP_REACH) {
                        give_up(self);
                        continue;
                    }
                    if self.pick_up_item(me, id) {
                        moved = true;
                        done(self);
                    } else {
                        give_up(self);
                    }
                }
                // Task_EmptyMachine (F031048) at a building of the player's (a machine's output, a
                // storage's items): `empty_machine`; nothing taken: given up (F01953d).
                // (assumption) All in one update.
                (COLLECTING_RESOURCE, Target::Building(_) | Target::Store(_)) => {
                    let Some(b) = JobsView::building_of(&step.target).filter(|&b| b < self.buildings.len()) else { give_up(self); continue };
                    let Some(s) = self.jobs_state.view.sites.get(b) else { give_up(self); continue };
                    let reach = if matches!(step.target, Target::Store(_)) { STORE_REACH } else { OPERATE_REACH };
                    if !s.hauls || !near(pos, s.at, s.radius + reach) {
                        give_up(self);
                        continue;
                    }
                    let taken = self.empty_machine(me, b);
                    if std::env::var("KENSHI_JOBS_DEBUG").is_ok() {
                        eprintln!("jobs: {me} collects {taken} from {b}");
                    }
                    if taken == 0 {
                        give_up(self);
                        continue;
                    }
                    moved = true;
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
                // Task_FillMachine (F031179) at a building of the player's (Filling machine,
                // Hauling to): `fill_machine`. (assumption) All in one update: the task ends when
                // nothing more goes in.
                (FILLING_MACHINE | HAULING_TO, Target::Building(_) | Target::Store(_)) => {
                    let Some(b) = JobsView::building_of(&step.target).filter(|&b| b < self.buildings.len()) else { give_up(self); continue };
                    let (at, r) = (self.buildings[b].at, self.buildings[b].radius);
                    let reach = if matches!(step.target, Target::Store(_)) { STORE_REACH } else { OPERATE_REACH };
                    if !near(pos, at, r + reach) {
                        give_up(self);
                        continue;
                    }
                    let put = self.fill_machine(me, b);
                    if std::env::var("KENSHI_JOBS_DEBUG").is_ok() {
                        eprintln!("jobs: {me} put {put} things into {b}");
                    }
                    moved |= put > 0;
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
        if moved || self.tick.is_multiple_of(VIEW_TICKS) || self.jobs_state.view.sites.len() != self.buildings.len() {
            self.jobs_state.view = self.jobs_view();
        }
    }
}
