//! The player's working buildings (port_map `base`): machines and crafting benches worked by
//! their operators, research benches, farms, power, and the operators emptying full machines.
//!
//! Specs:
//! - F029390 / F029391 / F029392 (ProductionBuilding setup): the building's rate is "output rate"
//!   x the functionality's "production mult"; an "output per day" functionality's rate is
//!   1 / (day length x 2 / it); split over the building's "max operators" when more than one.
//!   Each "consumes" ingredient takes its second value x 0.01 a unit of output (the stone
//!   processor's 200: 2 raw stone).
//! - F029380: the building's resource level: 1, or for a functionality with "world resource
//!   mining" the resource at its place (F051624, `kenshi_world::resources`); a building the
//!   player doesn't own with none gets 0.5, one inside at least 0.1.
//! - F031350 (Task_OperateMachine::update) -> F029326 (ProductionBuilding::operate): output +=
//!   power efficiency (F029260 / F037662: supplied over needed, at most 1) x the resource x the
//!   rate x the research improvement (F029301) x "production speed" x the operator's skill
//!   multiplier (F047049: 0.5 + 0.9 x level / 100; no stat used: 1) x the frame time.
//! - F029359 / F029360: ingredients are taken as needed, the output limited by what there is;
//!   F029325: each whole unit makes the products.
//! - F029370 / F046230: a research bench's operators add their rate (as above) to the research
//!   rate; a research needs "time" x "research rate" x "research level increase rate" ^ (level
//!   - 1) of it.
//! - F022906 / F022907 / F022899 / F022900 / F022915 (FarmBuilding): water from rain and its
//!   tank, growth by the hour over "growth time" scaled by the fertility (dry: x "drought
//!   multiplier"), ripe for "harvest time" hours, then rotting over "death time"; drought kills
//!   over "drought death time"; operators harvest at "harvest rate" a day (the crop's "amount"
//!   plants x "output per plant") or clear a dead crop at "clear rate" a day.
//! - F029292 (WindGeneratorBuilding power: wind x output, none under 1), F029291
//!   (GeneratorBuilding: (output + research "power increase") x its fuel efficiency).
//!
//! (assumption) A unit of output makes one of each product (its second value x 0.01 when set:
//! the processor's 100; F029392 hands "produces" to its output container, not read here); the
//! research frame value that divides research time (+0x14 of the frame's time
//! block) is taken as hours per second, so research "time" is in research-rate hours; the wind
//! is the weather's wind speed / 100 (the generator reads its head's spin, set from the wind);
//! a fuel generator burns its "consumes" at its rate while there is draw; ground water ("water"
//! resource) waters farms as rain does at full rain; a Water item tops a farm's tank up by a
//! quarter; batteries ("power capacity") store spare power and give it back; every built
//! consumer draws its "power output" all the time. A full machine is emptied by one of its
//! operators carrying what fits to the nearest storage with room (Task_EmptyMachine's own rules
//! are not read).

use crate::base::PlayerBuilding;
use crate::sim::Sim;
use kenshi_char::StatsEnumerated;
use kenshi_char::inventory::{Inventory, Item};
use kenshi_data::{GameData, GameDataContainer};

/// The game day (seconds, F040787).
pub const DAY: f32 = 2618.18;
/// GLOBAL CONSTANTS "production speed" (GlobalConstants slot 89, shipped 2).
pub const PRODUCTION_SPEED: f32 = 2.0;
/// GLOBAL CONSTANTS "research rate" and "research level increase rate" (slots 85, 84).
pub const RESEARCH_RATE: f32 = 0.3;
pub const RESEARCH_LEVEL_INCREASE: f32 = 1.3;
/// The research function (BUILDING_FUNCTIONALITY "function" 3) and wind power (5).
const FUNCTION_RESEARCH: i32 = 3;
/// BuildingFunction 7: training equipment (training dummies, the training turret, the
/// locksmith's training box).
pub const FUNCTION_TRAINING: i32 = 7;
/// The right-click menu's targets for town furniture (a town's training dummies): FURN_TARGET +
/// the furniture's index.
pub const FURN_TARGET: usize = 1 << 26;
const FUNCTION_WIND: i32 = 5;
/// Storage (BUILDING_FUNCTIONALITY "function" 13, StorageBuilding).
const FUNCTION_STORAGE: i32 = 13;

/// A haul's `from` when it empties no machine: a character ditching what it carries
/// (`routines`, "Ditch items").
pub const DITCH: usize = usize::MAX;

/// A carrier's errand emptying a full machine (Task_EmptyMachine), or ditching its own load.
#[derive(Debug, Clone, PartialEq)]
pub struct Haul {
    /// The machine emptied and the storage it goes to.
    pub from: usize,
    pub to: usize,
    /// The item records carried.
    pub items: Vec<String>,
}

/// A building's BUILDING_FUNCTIONALITY record id, when it has one.
pub fn functionality(b: &GameData) -> Option<&String> {
    b.refs("functionality").first().map(|(s, _)| s)
}

/// A building's functionality record and its "function" (1 mining, 3 research, 4 processing...).
pub fn function_of<'a>(data: &'a GameDataContainer, b: &GameData) -> Option<(&'a GameData, i32)> {
    let f = data.get(functionality(b)?)?;
    Some((f, f.ints.get("function").copied().unwrap_or(0)))
}

/// Whether a building record is a farm (has "farm data").
pub fn is_farm(b: &GameData) -> bool {
    !b.refs("farm data").is_empty()
}

fn num(r: &GameData, k: &str) -> f32 {
    r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0)
}

/// @fn F047049
///
/// An operator's skill multiplier: 0.5 + (1.4 - 0.5) x level / 100; no stat: 1.
pub fn skill_mult(level: Option<f32>) -> f32 {
    match level {
        None => 1.0,
        Some(l) => (1.4 - 0.5) * l * 0.01 + 0.5,
    }
}

/// @fn F029392
///
/// A building's rate: "output rate" x the functionality's "production mult"; per day: 1 / (day x
/// 2 / it); over its "max operators" when more than one.
pub fn building_rate(b: &GameData, f: &GameData) -> f32 {
    let mut rate = num(b, "output rate") * num(f, "production mult");
    if f.bools.get("output per day").copied().unwrap_or(false) && rate > 0.0 {
        rate = 1.0 / (DAY * 2.0 / rate);
    }
    let ops = max_operators(b, f);
    if ops > 1 {
        rate /= ops as f32;
    }
    rate
}

/// The building's "max operators", else its functionality's (F029391), at least 1.
pub fn max_operators(b: &GameData, f: &GameData) -> u32 {
    let own = b.ints.get("max operators").copied().unwrap_or(-1);
    let n = if own < 0 { f.ints.get("max operators").copied().unwrap_or(1) } else { own };
    n.max(1) as u32
}

impl Sim {
    /// The ENVIRONMENT_RESOURCES record of the biome at world (x, z) (F048105: the biome map
    /// there; unmarked: the default biome).
    pub fn resources_at(&self, x: f32, z: f32) -> Option<&GameData> {
        let rec = self.biome_at(x, z)?;
        self.data.get(&rec.refs("resources").first()?.0)
    }

    /// The BIOME_GROUP record of the biome at world (x, z) (F048105: the biome map there;
    /// unmarked: the default biome).
    pub fn biome_at(&self, x: f32, z: f32) -> Option<&GameData> {
        let pop = self.population.as_ref()?;
        let c = pop.map.colour_at(x, z);
        let biome = pop.biomes.iter().find(|b| b.index == c).or_else(|| pop.biomes.iter().find(|b| b.record == kenshi_world::biome::DEFAULT_BIOME))?;
        self.data.get(&biome.record)
    }

    /// The level of resource `kind` at world point `at` (F051624).
    pub fn resource_level(&self, kind: i32, at: [f32; 3]) -> f32 {
        self.resources_at(at[0], at[2]).map_or(0.0, |r| kenshi_world::resources::resource_at(r, kind, at))
    }

    /// @fn F029380
    ///
    /// Building `b`'s resource level: 1 without "world resource mining", else the resource at
    /// its place (the player's own buildings outdoors: no floor).
    pub fn building_resource(&self, b: usize) -> f32 {
        let pb = &self.buildings[b];
        let Some((f, _)) = self.data.get(&pb.record).and_then(|r| function_of(&self.data, r)) else { return 1.0 };
        let kind = f.ints.get("world resource mining").copied().unwrap_or(0);
        if kind == 0 { 1.0 } else { self.resource_level(kind, pb.at) }
    }

    /// @fn F029301
    ///
    /// The research improvements of building record `record`: the product of the done
    /// research's "production mult" that "improve buildings" it, and the sum of their "power
    /// increase".
    pub fn improvements(&self, record: &str) -> (f32, f32) {
        let mut mult = 1.0;
        let mut power = 0.0;
        for id in &self.research_done {
            let Some(r) = self.data.get(id) else { continue };
            if r.refs("improve buildings").iter().any(|(s, _)| s == record) {
                mult *= num(r, "production mult").max(0.0001);
                power += num(r, "power increase");
            }
        }
        (mult, power)
    }

    /// Whether building record `record` produces: its functionality a mine, a production
    /// building, a crafting bench or a furnace (BuildingFunction 1, 4, 10, 14) with products or
    /// crafts, or a farm. ("is production building" is not a field of Kenshi's (fcs.def): only
    /// some records carry it, so it is not asked.)
    pub fn is_production_record(&self, record: &str) -> bool {
        let Some(r) = self.data.get(record) else { return false };
        is_farm(r)
            || function_of(&self.data, r).is_some_and(|(f, function)| {
                matches!(function, 1 | 4 | 10 | 14) && (!f.refs("produces").is_empty() || !f.refs("item crafts").is_empty())
            })
    }

    /// Whether building `b` is a built production building.
    pub fn is_production(&self, b: usize) -> bool {
        self.buildings.get(b).is_some_and(|pb| pb.done && self.is_production_record(&pb.record))
    }

    /// Whether building `b` is a built research bench, and its "tech level".
    pub fn research_bench(&self, b: usize) -> Option<i32> {
        let pb = self.buildings.get(b).filter(|pb| pb.done)?;
        let (f, function) = function_of(&self.data, self.data.get(&pb.record)?)?;
        (function == FUNCTION_RESEARCH).then(|| f.ints.get("tech level").copied().unwrap_or(1))
    }

    /// Whether building `b` can be operated: production or research.
    pub fn is_operated(&self, b: usize) -> bool {
        self.is_production(b) || self.research_bench(b).is_some() || self.is_turret(b) || self.training(b).is_some()
    }

    /// Built training equipment `b`: the skill it trains ("stat used") and the level it trains
    /// up to ("output rate": "For training dummies, its the max skill you can train to.").
    pub fn training(&self, b: usize) -> Option<(StatsEnumerated, f32)> {
        let pb = self.buildings.get(b).filter(|pb| pb.done)?;
        let rec = self.data.get(&pb.record)?;
        let (f, function) = function_of(&self.data, rec)?;
        if function != FUNCTION_TRAINING {
            return None;
        }
        let stat = f.ints.get("stat used").copied().filter(|&v| v > 0).and_then(StatsEnumerated::from_i32)?;
        Some((stat, num(rec, "output rate")))
    }

    /// Town training equipment `f` (furniture): the skill and the level it trains to.
    pub fn furniture_training(&self, f: usize) -> Option<(StatsEnumerated, f32)> {
        let rec = self.data.get(&self.furn.get(f)?.record)?;
        let (func, function) = function_of(&self.data, rec)?;
        if function != FUNCTION_TRAINING {
            return None;
        }
        let stat = func.ints.get("stat used").copied().filter(|&v| v > 0).and_then(StatsEnumerated::from_i32)?;
        Some((stat, num(rec, "output rate")))
    }

    /// The town training equipment nearest `at` (units) within `reach`.
    pub fn training_near(&self, at: [f32; 3], reach: f32) -> Option<usize> {
        (0..self.furn.len())
            .filter(|&k| self.furn[k].function == FUNCTION_TRAINING && (self.furn[k].position[1] - at[1]).abs() < 150.0)
            .map(|k| (k, (self.furn[k].position[0] - at[0]).hypot(self.furn[k].position[2] - at[2])))
            .filter(|&(_, d)| d < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// The "Training" order on town training equipment `f` (97, Task_UseTrainingDummy): an
    /// order on the task system's list for each of `ids`, planned (crate::jobs): walked up to,
    /// trained on until given another order (its operator slot taken).
    pub fn order_train_at(&mut self, ids: &[usize], f: usize) {
        let Some(at) = self.furn.get(f).map(|k| k.position) else { return };
        let ids: Vec<usize> = ids.iter().copied().filter(|&id| self.folk.get(id).is_some_and(|p| !p.down && !p.dead)).collect();
        self.order_task(&ids, crate::jobs::TRAINING, crate::sim::Target::Furniture(f), at);
    }

    /// @fn F029286
    ///
    /// Training (UseableStuff, each frame for each user of training equipment, Task_UseTrainingDummy
    /// F031352): the skill trained by the frame time x 2 up to the equipment's level
    /// (CharStats::trainUpTo, F047461); the progress bar ("Training", F031296) shows how far
    /// the skill is to its next level.
    /// (assumption) The trainee's hunger is not raised by the equipment's "hunger rate".
    pub fn training_tick(&mut self, dt: f32) {
        // town equipment: trained on once there and still; given up on stopping elsewhere
        let mut keep = Vec::new();
        for (id, f, mut age) in std::mem::take(&mut self.trainees) {
            age += dt;
            let (Some(furn), Some((stat, cap))) = (self.furn.get(f), self.furniture_training(f)) else { continue };
            let p = &self.folk[id];
            let me = p.mover.position_units();
            // (as a building's operators, within its reach + 40: the walk stops short of it)
            let dist = (me[0] - furn.position[0]).hypot(me[2] - furn.position[2]);
            let near = dist <= furn.use_range.max(10.0) + 40.0;
            let still = p.mover.velocity[0].hypot(p.mover.velocity[2]) < 0.5;
            // (a moment to set off first)
            if p.down || p.dead || (still && !near && age > 2.0) {
                if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                    eprintln!("training {id}: gave up {dist:.0} from {}", furn.name);
                }
                if self.folk[id].operating == Some(f) {
                    self.folk[id].operating = None;
                }
                continue;
            }
            if near && still {
                // (the furniture's "animation" plays: activity_anim reads `operating`)
                self.folk[id].operating = Some(f);
                self.arena.f[id].skills.train_up_to(stat, dt, 2.0, cap);
                if std::env::var("KENSHI_BUILD_DEBUG").is_ok() && self.tick.is_multiple_of(600) {
                    eprintln!("training {id} at {}: {stat:?} {:.2} (up to {cap})", furn.name, self.arena.f[id].skills.level(stat));
                }
            }
            keep.push((id, f, age));
        }
        self.trainees = keep;
        for (id, b, _) in self.operators() {
            let Some((stat, cap)) = self.training(b) else { continue };
            self.arena.f[id].skills.train_up_to(stat, dt, 2.0, cap);
            if std::env::var("KENSHI_BUILD_DEBUG").is_ok() && self.tick.is_multiple_of(600) {
                eprintln!("training {id}: {stat:?} {:.2} (up to {cap})", self.arena.f[id].skills.level(stat));
            }
        }
    }

    /// How far `id` training on equipment is to its skill's next level (0 .. 1), if it is.
    pub fn training_status(&self, id: usize) -> Option<f32> {
        if let Some(&(_, f, _)) = self.trainees.iter().find(|(o, ..)| *o == id)
            && self.folk[id].operating == Some(f)
        {
            let (stat, _) = self.furniture_training(f)?;
            return Some(self.arena.f[id].skills.level(stat).fract());
        }
        let b = self.folk.get(id)?.operate?;
        let (stat, _) = self.training(b)?;
        if !self.at_building(id, b) || self.folk[id].down {
            return None;
        }
        Some(self.arena.f[id].skills.level(stat).fract())
    }

    /// A mounted gun (its BUILDING has a "gun turret"): manned by whoever operates it.
    pub fn is_turret(&self, b: usize) -> bool {
        self.buildings.get(b).and_then(|pb| self.data.get(&pb.record)).is_some_and(|r| !r.refs("gun turret").is_empty())
    }

    /// The items crafting bench `b` can make (its functionality's "item crafts").
    pub fn crafts(&self, b: usize) -> Vec<String> {
        let Some(pb) = self.buildings.get(b) else { return Vec::new() };
        let Some(rec) = self.data.get(&pb.record) else { return Vec::new() };
        function_of(&self.data, rec).map_or(Vec::new(), |(f, _)| f.refs("item crafts").iter().map(|(s, _)| s.clone()).collect())
    }

    /// The crafting benches built.
    pub fn crafting_benches(&self) -> Vec<usize> {
        (0..self.buildings.len()).filter(|&b| self.buildings[b].done && !self.crafts(b).is_empty()).collect()
    }

    /// "Operate" (order 87, Operating machine: Task_OperateMachine) on building `b`: an order on
    /// the task system's list for each of `ids`, planned (crate::jobs): walked to and worked.
    pub fn order_operate(&mut self, ids: &[usize], b: usize) {
        let Some(site) = self.buildings.get(b) else { return };
        if !self.is_operated(b) {
            return;
        }
        let at = site.at;
        let ids: Vec<usize> = ids.iter().copied().filter(|&id| self.folk.get(id).is_some_and(|p| !p.down)).collect();
        self.order_task(&ids, crate::jobs::OPERATING_MACHINE, crate::sim::Target::Building(b), at);
    }

    /// Whether character `id` is at building `b` (within its radius + 40).
    fn at_building(&self, id: usize, b: usize) -> bool {
        let (site, r) = (self.buildings[b].at, self.buildings[b].radius);
        let me = self.folk[id].mover.position_units();
        (me[0] - site[0]).hypot(me[2] - site[2]) <= r + 40.0
    }

    /// The operators working at each operated building this tick (the first "max operators" of
    /// them by it), with their stat used.
    fn operators(&self) -> Vec<(usize, usize, Option<StatsEnumerated>)> {
        let mut count: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        let mut out = Vec::new();
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            let Some(b) = p.operate else { continue };
            if p.down || p.haul.is_some() || !self.is_operated(b) || !self.at_building(id, b) {
                continue;
            }
            let Some(rec) = self.data.get(&self.buildings[b].record) else { continue };
            let Some((f, _)) = function_of(&self.data, rec) else { continue };
            let n = count.entry(b).or_insert(0);
            if *n >= max_operators(rec, f) {
                continue;
            }
            *n += 1;
            let stat = f.ints.get("stat used").copied().filter(|&v| v > 0).and_then(StatsEnumerated::from_i32);
            out.push((id, b, stat));
        }
        out
    }

    /// One operator's rate at building `b` (F029326 without the frame time and the products):
    /// power efficiency x resource x rate x research improvement x production speed x skill.
    fn operator_rate(&self, id: usize, b: usize, stat: Option<StatsEnumerated>) -> f32 {
        let pb = &self.buildings[b];
        let Some(rec) = self.data.get(&pb.record) else { return 0.0 };
        let Some((f, _)) = function_of(&self.data, rec) else { return 0.0 };
        let level = stat.map(|s| self.arena.f[id].level(s));
        self.power_efficiency(b) * self.building_resource(b) * building_rate(rec, f) * self.improvements(&pb.record).0 * PRODUCTION_SPEED * self.advanced.production * skill_mult(level)
    }

    /// Clear the operating of characters whose building can no longer be operated.
    fn drop_operators(&mut self) {
        for id in 0..self.folk.len() {
            if let Some(b) = self.folk[id].operate
                && (self.folk[id].down || !self.is_operated(b))
            {
                self.folk[id].operate = None;
            }
        }
    }

    /// @fn F029326
    ///
    /// The production buildings worked this tick (see the module notes); farms are worked in
    /// [`Sim::farm_tick`].
    pub fn production_tick(&mut self, dt: f32) {
        self.drop_operators();
        // (a machine runs while it makes something, or burns fuel: its spinners turn,
        // `PlayerBuilding::running`; (assumption) one stopped winds down over two seconds)
        for b in self.buildings.iter_mut() {
            b.running = (b.running - dt * 0.5).max(0.0);
        }
        let data = self.data.clone();
        for (id, b, stat) in self.operators() {
            if !self.is_production(b) {
                continue;
            }
            let Some(rec) = data.get(&self.buildings[b].record) else { continue };
            if is_farm(rec) {
                continue;
            }
            let Some((f, _)) = function_of(&data, rec) else { continue };
            // a crafting bench makes its queue's first item
            let crafting = !f.refs("item crafts").is_empty();
            let products: Vec<(String, f32)> = if crafting {
                match self.buildings[b].craft_queue.first() {
                    Some(item) => vec![(item.clone(), 1.0)],
                    None => continue,
                }
            } else {
                f.refs("produces").iter().map(|(s, v)| (s.clone(), if v[1] > 0 { v[1] as f32 * 0.01 } else { 1.0 })).collect()
            };
            let ingredients: Vec<(String, f32)> = f.refs("consumes").iter().map(|(s, v)| (s.clone(), v[1].max(0) as f32 * 0.01)).collect();
            let mut amount = self.operator_rate(id, b, stat) * dt;
            if amount <= 0.0 {
                continue;
            }
            // F029360: no more output than the ingredients there are (those taken and those to
            // take)
            let pb = &self.buildings[b];
            for (k, (item, per)) in ingredients.iter().enumerate() {
                if *per <= 0.0 {
                    continue;
                }
                let have = pb.taken.get(k).copied().unwrap_or(0.0) + self.count_material(id, b, item) as f32;
                amount = amount.min(have / per);
            }
            if amount <= 0.0 {
                self.missing_material = ingredients.iter().find(|(i, _)| self.count_material(id, b, i) == 0).map(|i| i.0.clone());
                if std::env::var("KENSHI_BUILD_DEBUG").is_ok() && self.tick % 600 == 0 {
                    eprintln!("operator {id} at {}: waiting for {:?}", rec.name, self.missing_material);
                }
                continue;
            }
            // full: wait to be emptied
            if self.buildings[b].full {
                continue;
            }
            // F029359: the ingredients taken as needed
            for (k, (item, per)) in ingredients.iter().enumerate() {
                let need = amount * per;
                if self.buildings[b].taken.len() <= k {
                    self.buildings[b].taken.resize(k + 1, 0.0);
                }
                while self.buildings[b].taken[k] < need {
                    if !self.take_material_at(id, b, item) {
                        break;
                    }
                    self.buildings[b].taken[k] += 1.0;
                }
                self.buildings[b].taken[k] -= need;
            }
            self.buildings[b].running = self.power_efficiency(b).clamp(0.0, 1.0);
            // F029325: the products made by whole units
            let pb = &mut self.buildings[b];
            if pb.made.len() < products.len() {
                pb.made.resize(products.len(), 0.0);
            }
            let mut made = Vec::new();
            for (k, (item, per)) in products.iter().enumerate() {
                self.buildings[b].made[k] += amount * per;
                while self.buildings[b].made[k] >= 0.999 {
                    let Some(r) = data.get(item) else { break };
                    let Some(store) = self.buildings[b].storage.as_mut() else { break };
                    if !store.add(Item::from_record(r, 1), false) {
                        self.buildings[b].full = true;
                        break;
                    }
                    self.buildings[b].made[k] -= 1.0;
                    made.push(r.name.clone());
                    if crafting {
                        let pb = &mut self.buildings[b];
                        let done = pb.craft_queue.remove(0);
                        if pb.craft_repeat {
                            pb.craft_queue.push(done);
                        }
                        pb.made[k] = 0.0;
                        break;
                    }
                }
            }
            if let Some(s) = stat {
                self.arena.f[id].skills.train(s, dt * 0.05, 1.0);
            }
            if !made.is_empty() && std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                eprintln!("{} made {:?}", rec.name, made);
            }
        }
        self.haul_tick();
    }

    /// Research at the benches (F029370, F046230): the operators' rates summed into the
    /// research rate; the first queued research, once its "cost" and "money" are paid from the
    /// player's storage, the operators' inventories and money, gains the rate in hours of
    /// research an hour and is learnt at its research time. It needs a bench of at least its
    /// "level" (F035131).
    pub fn research_tick(&mut self, dt: f32) {
        let data = self.data.clone();
        let mut rate = 0.0;
        let mut workers = Vec::new();
        for (id, b, stat) in self.operators() {
            if self.research_bench(b).is_none() {
                continue;
            }
            rate += self.operator_rate(id, b, stat) / (PRODUCTION_SPEED * self.advanced.production);
            workers.push((id, stat));
        }
        self.research_rate = rate;
        let Some(first) = self.research_queue.first().cloned() else { return };
        let Some(r) = data.get(&first) else {
            self.research_queue.remove(0);
            return;
        };
        let level = r.ints.get("level").copied().unwrap_or(1);
        if self.bench_level() < level {
            self.research_message = Some(format!("Needs research bench level {level}"));
            return;
        }
        if workers.is_empty() {
            return;
        }
        if !self.research_paid {
            let cost: Vec<(String, u32)> = r.refs("cost").iter().map(|(s, v)| (s.clone(), v[0].max(1) as u32)).collect();
            let money = r.ints.get("money").copied().unwrap_or(0);
            let who = workers[0].0;
            let count = |inv: &Inventory, item: &str| inv.sections.iter().flat_map(|s| s.items.iter()).filter(|p| p.item.record == item).map(|p| p.item.quantity).sum::<u32>();
            let have = |sim: &Sim, item: &str| -> u32 {
                sim.buildings.iter().filter(|b| b.done).filter_map(|b| b.storage.as_ref()).map(|i| count(i, item)).sum::<u32>()
                    + workers.iter().map(|(w, _)| count(&sim.folk[*w].inventory, item)).sum::<u32>()
            };
            if cost.iter().any(|(i, n)| have(self, i) < *n) || self.money < money {
                self.research_message = Some("Not enough artifacts, research cannot begin. Find artifacts and place them in your research bench.".into());
                return;
            }
            for (i, n) in &cost {
                for _ in 0..*n {
                    if !workers.iter().any(|(w, _)| take_one(&mut self.folk[*w].inventory, i)) {
                        self.take_material(who, i);
                    }
                }
            }
            self.money -= money;
            self.research_paid = true;
        }
        self.research_message = None;
        self.research_progress += rate * self.advanced.research * dt / self.hour_secs as f32;
        for (id, stat) in &workers {
            if let Some(s) = stat {
                self.arena.f[*id].skills.train(*s, dt * 0.05, 1.0);
            }
        }
        if self.research_progress >= research_time(r) {
            self.research_done.push(first);
            self.research_queue.remove(0);
            self.research_progress = 0.0;
            self.research_paid = false;
            if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                eprintln!("researched {}", r.name);
            }
        }
    }

    /// The fertility a farm's crop grows by at its place (F022900 x F022899): 1 - (1 - the
    /// "farming" resource) x "fertility effect", none at or under "minimum fertility"; times
    /// the crop's fit to the ground (the biome's "arid", "green" and "swamp" against the
    /// crop's) when it has a fertility effect.
    pub fn farm_fertility(&self, b: usize) -> f32 {
        let pb = &self.buildings[b];
        let Some(fd) = self.data.get(&pb.record).and_then(|r| r.refs("farm data").first().and_then(|(s, _)| self.data.get(s))) else { return 0.0 };
        let fert = self.resource_level(6, pb.at);
        let effect = num(fd, "fertility effect");
        let mult = if fert <= num(fd, "minimum fertility") { 0.0 } else { 1.0 - (1.0 - fert) * effect };
        let fit = match self.resources_at(pb.at[0], pb.at[2]) {
            Some(res) if effect != 0.0 => num(res, "arid") * num(fd, "arid") + num(res, "green") * num(fd, "green") + num(res, "swamp") * num(fd, "swamp"),
            _ => 1.0,
        };
        (mult * fit).max(0.0)
    }

    /// @fn F022906
    ///
    /// Farms each tick (see the module notes); their operators harvest or clear (F022907).
    pub fn farm_tick(&mut self, dt: f32) {
        let hours = dt / self.hour_secs as f32;
        let data = self.data.clone();
        for b in 0..self.buildings.len() {
            if !self.buildings[b].done {
                continue;
            }
            let Some(rec) = data.get(&self.buildings[b].record) else { continue };
            let Some(fd) = rec.refs("farm data").first().and_then(|(s, _)| data.get(s)) else { continue };
            let consumption = num(fd, "consumption rate") / DAY;
            let at = self.buildings[b].at;
            // water: rain and ground water fill the tank, a Water item tops it up, the crop drinks
            let ground = self.resource_level(5, at).clamp(0.0, 1.0);
            let fill = (self.rain + ground) * consumption * dt * 3.0;
            let water_item = function_of(&data, rec).and_then(|(f, _)| f.refs("consumes").first().map(|(s, _)| s.clone()));
            let pb = &mut self.buildings[b];
            pb.water = (pb.water + fill).min(1.0);
            if pb.water < 0.25
                && let Some(w) = &water_item
                && let Some(store) = pb.storage.as_mut()
                && take_one(store, w)
            {
                pb.water += 0.25;
            }
            let drink = consumption * dt;
            let watered = pb.water >= drink;
            if watered {
                pb.water -= drink;
            }
            let fert = self.farm_fertility(b);
            let pb = &mut self.buildings[b];
            let mut grow = hours * fert / num(fd, "growth time").max(0.01);
            if !watered {
                grow *= num(fd, "drought multiplier");
            }
            if pb.stage >= 1.0 {
                if pb.stage >= 2.0 {
                    // rotting
                    pb.death += hours / num(fd, "death time").max(0.01);
                } else {
                    pb.stage += hours / num(fd, "harvest time").max(0.01);
                }
            } else if grow > 0.0 {
                pb.stage = (pb.stage + grow).min(1.0);
            }
            if std::env::var("KENSHI_BUILD_DEBUG").is_ok() && self.tick % 300 == 0 {
                eprintln!("farm {b}: stage {:.2}, death {:.2}, water {:.2}, fertility {fert:.2}, watered {watered}", pb.stage, pb.death, pb.water);
            }
            let ddt = num(fd, "drought death time");
            if ddt > 0.0 && pb.stage > 0.0 && pb.stage < 1.0 {
                if !watered {
                    pb.death += hours / ddt;
                } else if pb.death > 0.0 && pb.stage < 2.0 {
                    pb.death = (pb.death - hours / ddt).max(0.0);
                }
            }
        }
        // F022907: operators harvest the ripe or clear the dead
        for (id, b, stat) in self.operators() {
            let Some(rec) = data.get(&self.buildings[b].record) else { continue };
            let Some(fd) = rec.refs("farm data").first().and_then(|(s, _)| data.get(s)) else { continue };
            let level = stat.map(|s| self.arena.f[id].level(s));
            let work = self.power_efficiency(b) * PRODUCTION_SPEED * skill_mult(level) * dt;
            let threshold = num(fd, "death threshold");
            let pb = &mut self.buildings[b];
            if pb.stage >= 1.0 && pb.death < threshold {
                pb.work += work * num(fd, "harvest rate") / DAY;
                if pb.work >= 1.0 {
                    let plants = num(fd, "amount") * num(fd, "output per plant");
                    let product = function_of(&data, rec).and_then(|(f, _)| f.refs("produces").first().map(|(s, _)| s.clone()));
                    let mut made = 0;
                    if let (Some(p), Some(store)) = (product.and_then(|p| data.get(&p)), pb.storage.as_mut()) {
                        // the whole plants, then the fraction by chance
                        let whole = plants as u32;
                        let extra = u32::from(rand_unit(b as u32 ^ self.tick as u32) < plants - whole as f32);
                        for _ in 0..whole + extra {
                            if !store.add(Item::from_record(p, 1), false) {
                                pb.full = true;
                                break;
                            }
                            made += 1;
                        }
                    }
                    pb.work = 0.0;
                    pb.stage = 0.0;
                    pb.death = 0.0;
                    if let Some(s) = stat {
                        self.arena.f[id].skills.train(s, 1.0, 1.0);
                    }
                    if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                        eprintln!("{} harvested {made}", rec.name);
                    }
                }
            } else if pb.death >= threshold && pb.death > 0.0 {
                pb.work += work * num(fd, "clear rate") / DAY;
                if pb.work >= 1.0 {
                    pb.work = 0.0;
                    pb.stage = 0.0;
                    pb.death = 0.0;
                }
            }
        }
    }

    /// The base's power each tick: generators supply (F029292 wind: wind x output, none under 1;
    /// F029291 fuel: output + research "power increase" while it has fuel), consumers draw
    /// their "power output", batteries store the spare and give it back (see the module
    /// notes). The efficiency is what is supplied over what is drawn, at most 1 (F037662).
    pub fn power_tick(&mut self, dt: f32) {
        let hour = self.hour_secs as f32;
        let data = self.data.clone();
        let (mut supply, mut demand, mut capacity) = (0.0f32, 0.0f32, 0.0f32);
        let mut fuelled = Vec::new();
        for k in 0..self.buildings.len() {
            if !self.buildings[k].done {
                continue;
            }
            let Some(rec) = data.get(&self.buildings[k].record) else { continue };
            let (_, power_up) = self.improvements(&rec.string_id);
            capacity += num(rec, "power capacity");
            let out = num(rec, "power output");
            if out < 0.0 {
                demand -= out;
                continue;
            }
            if out == 0.0 {
                continue;
            }
            match function_of(&data, rec) {
                Some((_, FUNCTION_WIND)) => {
                    let p = self.wind * (out + power_up);
                    if p >= 1.0 {
                        supply += p;
                    }
                }
                Some((f, _)) if !f.refs("consumes").is_empty() => {
                    if self.buildings[k].fuel > 0.0 {
                        supply += out + power_up;
                    }
                    fuelled.push((k, f.refs("consumes").first().map(|(s, v)| (s.clone(), v[1].max(1) as f32 * 0.01)), building_rate(rec, f)));
                }
                _ => supply += out + power_up,
            }
        }
        // batteries
        let spare = supply - demand;
        self.battery = (self.battery + spare * dt / hour).clamp(0.0, capacity);
        let from_battery = if spare < 0.0 && self.battery > 0.0 { -spare } else { 0.0 };
        self.battery_capacity = capacity;
        self.power_supply = supply;
        self.power_demand = demand;
        self.power_eff = if demand <= 0.0 { 1.0 } else { ((supply + from_battery) / demand).min(1.0) };
        // fuel burnt while drawn on
        let load = if supply > 0.0 { (demand / supply).min(1.0) } else { 1.0 };
        let owner = self.folk.iter().find(|p| p.player).map(|p| p.id).unwrap_or(0);
        for (k, fuel, rate) in fuelled {
            let Some((item, per)) = fuel else { continue };
            let pb = &mut self.buildings[k];
            if pb.fuel > 0.0 {
                pb.running = load.clamp(0.0, 1.0);
            }
            pb.fuel -= rate * PRODUCTION_SPEED * load * per * dt;
            if pb.fuel <= 0.0 && demand > 0.0 {
                if self.take_material_at(owner, k, &item) {
                    self.buildings[k].fuel += 1.0;
                } else {
                    self.buildings[k].fuel = 0.0;
                }
            }
        }
    }

    /// Building `b`'s power efficiency: 1 when it draws none, else the base's (F037662).
    pub fn power_efficiency(&self, b: usize) -> f32 {
        let draws = self.buildings.get(b).and_then(|pb| self.data.get(&pb.record)).is_some_and(|r| num(r, "power output") < 0.0);
        if draws { self.power_eff } else { 1.0 }
    }

    /// Whether building `b` works for want of power.
    pub fn has_power(&self, b: usize) -> bool {
        self.power_efficiency(b) > 0.0
    }

    /// Task_EmptyMachine: an operator of a full machine takes what fits of its products and
    /// carries them to the nearest built storage with room, then goes back to work.
    fn haul_tick(&mut self) {
        let data = self.data.clone();
        // storage proper (StorageBuilding: "has inventory", its functionality storage, 13, or none)
        let plain = |o: &PlayerBuilding| {
            data.get(&o.record).is_some_and(|r| {
                r.bools.get("has inventory").copied().unwrap_or(false) && function_of(&data, r).is_none_or(|(_, f)| f == FUNCTION_STORAGE)
            })
        };
        // new errands: a full machine's operator; a machine with anything in it, its auto-hauler
        // ("Auto-Haul", `orders`)
        for b in 0..self.buildings.len() {
            let hauler = (0..self.folk.len()).find(|&i| self.folk[i].auto_haul == Some(b) && !self.folk[i].down && self.folk[i].haul.is_none() && self.at_building(i, b));
            let has_output = self.buildings[b].storage.as_ref().is_some_and(|s| s.sections.iter().any(|x| !x.items.is_empty()));
            if !self.buildings[b].full && !(hauler.is_some() && has_output) {
                continue;
            }
            if self.folk.iter().any(|p| p.haul.as_ref().is_some_and(|h| h.from == b)) {
                continue;
            }
            let Some(id) = hauler.or_else(|| (0..self.folk.len()).find(|&i| self.folk[i].operate == Some(b) && !self.folk[i].down && self.at_building(i, b))) else {
                if std::env::var("KENSHI_BUILD_DEBUG").is_ok() && self.tick % 300 == 0 {
                    eprintln!("building {b} full: no operator by it");
                }
                continue;
            };
            let at = self.buildings[b].at;
            let to = (0..self.buildings.len())
                .filter(|&k| k != b && self.buildings[k].done && self.buildings[k].storage.is_some() && plain(&self.buildings[k]))
                .min_by(|&x, &y| {
                    let d = |k: usize| (self.buildings[k].at[0] - at[0]).hypot(self.buildings[k].at[2] - at[2]);
                    d(x).total_cmp(&d(y))
                });
            let Some(to) = to else {
                if std::env::var("KENSHI_BUILD_DEBUG").is_ok() && self.tick % 300 == 0 {
                    eprintln!("building {b} full: no storage");
                }
                continue;
            };
            // what fits in the carrier's inventory
            let mut items = Vec::new();
            if let Some(store) = self.buildings[b].storage.as_mut() {
                for sec in &mut store.sections {
                    let mut k = 0;
                    while k < sec.items.len() {
                        if self.folk[id].inventory.add(sec.items[k].item.clone(), false) {
                            items.push(sec.items.remove(k).item.record);
                        } else {
                            k += 1;
                        }
                    }
                }
            }
            if items.is_empty() {
                continue;
            }
            self.buildings[b].full = false;
            let dest = self.buildings[to].at;
            self.order_move(&[id], dest);
            if self.folk[id].auto_haul != Some(b) {
                self.folk[id].operate = Some(b);
            }
            self.folk[id].haul = Some(Haul { from: b, to, items });
        }
        // errands arriving
        for id in 0..self.folk.len() {
            let Some(h) = self.folk[id].haul.clone() else { continue };
            if self.folk[id].down || h.to >= self.buildings.len() {
                self.folk[id].haul = None;
                continue;
            }
            if !self.at_building(id, h.to) {
                continue;
            }
            for record in &h.items {
                // (from the worn backpack too)
                let inv = &mut self.folk[id].inventory;
                let item = match inv.sections.iter().enumerate().find_map(|(s, sec)| sec.items.iter().position(|p| p.item.record == *record).map(|k| (s, k))) {
                    Some((s, k)) => inv.take(s, k),
                    None => inv.bag_mut().and_then(|b| {
                        let k = b.sections[0].items.iter().position(|p| p.item.record == *record)?;
                        b.take(0, k)
                    }),
                };
                let Some(item) = item else { continue };
                if let Some(store) = self.buildings[h.to].storage.as_mut()
                    && !store.add(item.clone(), false)
                {
                    let _ = self.folk[id].inventory.add(item, false);
                }
            }
            self.folk[id].haul = None;
            if h.from == DITCH {
                if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                    eprintln!("{id} ditched {} things into {}", h.items.len(), h.to);
                }
                continue;
            }
            if self.folk[id].auto_haul == Some(h.from) {
                // an auto-hauler back to the machine to wait for more
                let at = self.buildings[h.from].at;
                self.order_move(&[id], at);
            } else {
                self.order_operate(&[id], h.from);
            }
            if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                eprintln!("{id} emptied building {} into {}", h.from, h.to);
            }
        }
    }

    /// How many of item `record` building `b` can draw on: its own storage, the other built
    /// storage of the player, and the operator's inventory.
    pub fn count_material(&self, operator: usize, b: usize, record: &str) -> u32 {
        let count = |inv: &Inventory| inv.sections.iter().flat_map(|s| s.items.iter()).filter(|p| p.item.record == record).map(|p| p.item.quantity).sum::<u32>();
        let own = self.buildings[b].storage.as_ref().map_or(0, count);
        let stores: u32 = self.buildings.iter().enumerate().filter(|(k, o)| *k != b && o.done).filter_map(|(_, o)| o.storage.as_ref()).map(count).sum();
        own + stores + count(&self.folk[operator].inventory)
    }

    /// Take one of item `record` for building `b`: its own storage first, then the
    /// operator's inventory and the player's storage.
    pub(crate) fn take_material_at(&mut self, operator: usize, b: usize, record: &str) -> bool {
        if let Some(inv) = self.buildings[b].storage.as_mut()
            && take_one(inv, record)
        {
            return true;
        }
        self.take_material(operator, record)
    }
}

/// @fn F046230
///
/// A research's time (in research-rate hours): "time" x "research rate" x "research level
/// increase rate" ^ (level - 1).
pub fn research_time(r: &GameData) -> f32 {
    let level = r.ints.get("level").copied().unwrap_or(1).max(1);
    num(r, "time") * RESEARCH_RATE * RESEARCH_LEVEL_INCREASE.powi(level - 1)
}

/// A number in 0..1 from a seed (the harvest's chance of its last plant).
fn rand_unit(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65536.0
}

/// Take one of item `record` out of `inv`.
pub fn take_one(inv: &mut Inventory, record: &str) -> bool {
    for s in &mut inv.sections {
        if let Some(k) = s.items.iter().position(|p| p.item.record == record) {
            if s.items[k].item.quantity > 1 {
                s.items[k].item.quantity -= 1;
            } else {
                s.items.remove(k);
            }
            return true;
        }
    }
    // (and from a worn backpack)
    inv.bag_mut().is_some_and(|b| take_one(b, record))
}

/// One of item `record` taken out of `inv`: a single, its stack one less.
pub fn take_item(inv: &mut Inventory, record: &str) -> Option<kenshi_char::inventory::Item> {
    for s in &mut inv.sections {
        if let Some(k) = s.items.iter().position(|p| p.item.record == record) {
            let mut one = s.items[k].item.clone();
            one.quantity = 1;
            if s.items[k].item.quantity > 1 {
                s.items[k].item.quantity -= 1;
            } else {
                s.items.remove(k);
            }
            return Some(one);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_mult_ends() {
        assert_eq!(skill_mult(None), 1.0);
        assert!((skill_mult(Some(0.0)) - 0.5).abs() < 1e-6);
        assert!((skill_mult(Some(100.0)) - 1.4).abs() < 1e-6);
    }
}
