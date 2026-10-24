//! The player's working buildings (port_map `base`): machines and crafting benches worked by
//! their operators, research benches, farms and power. (Full machines are emptied on the AI
//! planner: crate::jobs, docs/HAULING.md.)
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
//! - F022906 / F022907 / F022888 / F022891 / F022899 / F022900 / F022903 / F022904 / F022915
//!   (FarmBuilding, with ProductionBuilding's containers: its water "in1", its crop "out"):
//!   rain outdoors tops the water's banked level up, the crop drinks it (F022888: a Water item
//!   taken from its container as the level runs short), its plants (FARM_DATA "amount") grow by
//!   the hour over "growth time" by the fertility (dry: x "drought multiplier"), ripe for
//!   "harvest time" hours, then rotting over "death time"; drought kills over "drought death
//!   time"; operators harvest plant by plant at "harvest rate" a day (each of a plant's "output
//!   per plant" by the harvest chance, F022891) or clear a dead crop at "clear rate" a day; dead
//!   or all taken off, it is replanted.
//! - F047468: operators train their machine's "stat used" each frame they work it.
//! - F029292 (WindGeneratorBuilding power: wind x output, none under 1), F029291
//!   (GeneratorBuilding: (output + research "power increase") x its fuel efficiency).
//!
//! (assumption) A unit of output makes one of each product (its second value x 0.01 when set:
//! the processor's 100; F029392 hands "produces" to its output container, not read here); the
//! research frame value that divides research time (+0x14 of the frame's time
//! block) is taken as hours per second, so research "time" is in research-rate hours; the wind
//! is the weather's wind speed / 100 (the generator reads its head's spin, set from the wind);
//! a fuel generator burns its "consumes" at its rate while there is draw; batteries ("power
//! capacity") store spare power and give it back; every built consumer draws its "power
//! output" all the time (the town power manager, F048864, and the generators' operate are the
//! next step). (assumption) A farm's plants grow alike (the layout's own places, F022913, are
//! for show): its stage is the least of them; a farm is outdoors unless in a town's building.

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
/// Processing (BUILDING_FUNCTIONALITY "function" 4: the refineries, looms, stone processors).
pub const FUNCTION_PROCESSING: i32 = 4;
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

/// The names of a machine's containers (F029392: "in" and the ingredient's number, "out"; the
/// machine window's widgets of the same names show them, F024440).
pub const INPUTS: [&str; 4] = ["in1", "in2", "in3", "in4"];
pub const OUTPUT: &str = "out";

/// @fn F029392
///
/// A ProductionBuilding's containers (setupFunctionality): for each "consumes" ingredient in
/// order, "in1", "in2".. of the item's "inventory footprint width" x "height" taking only it, a
/// stack of up to the ingredient's first value (InventorySection +0x88, its ingredient entry's
/// +0x8 limit); for the first "produces", "out" likewise (F029412: the output entry's limit its
/// first value). None for a building of another class (crate::base::BuildingClass), or with
/// neither (F029411 makes it no inventory).
pub fn machine_inventory(data: &GameDataContainer, b: &GameData) -> Option<Inventory> {
    if !crate::base::BuildingClass::of(data, b).is_production() {
        return None;
    }
    let (f, _) = function_of(data, b)?;
    let container = |name: &'static str, record: &str, most: i32| -> Option<kenshi_char::inventory::Section> {
        let item = Item::from_record(data.get(record)?, 1);
        Some(kenshi_char::inventory::Section::container(name, item.w, item.h, record, most.max(0) as u32))
    };
    let mut sections: Vec<kenshi_char::inventory::Section> = f.refs("consumes").iter().zip(INPUTS).filter_map(|((r, v), name)| container(name, r, v[0])).collect();
    if let Some((r, v)) = f.refs("produces").first() {
        sections.extend(container(OUTPUT, r, v[0]));
    }
    (!sections.is_empty()).then_some(Inventory { sections })
}

/// @fn F029391
///
/// Whether a building needs an operator (+0x3a9, loadRates): its "max operators", else its
/// functionality's when negative, is not none (both negative: none). A machine that needs
/// none works on its own (F029366).
pub fn needs_operator(b: &GameData, f: &GameData) -> bool {
    let own = b.ints.get("max operators").copied().unwrap_or(-1);
    if own >= 0 {
        return own != 0;
    }
    let theirs = f.ints.get("max operators").copied().unwrap_or(1);
    theirs > 0
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
            if p.down || !self.is_operated(b) || !self.at_building(id, b) {
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
        let level = stat.map(|s| self.arena.f[id].level(s));
        self.machine_rate(b, skill_mult(level))
    }

    /// Building `b`'s rate worked at multiplier `mult` (an operator's skill, F047049; 1 for a
    /// machine working on its own, F029366).
    fn machine_rate(&self, b: usize, mult: f32) -> f32 {
        let pb = &self.buildings[b];
        let Some(rec) = self.data.get(&pb.record) else { return 0.0 };
        let Some((f, _)) = function_of(&self.data, rec) else { return 0.0 };
        self.power_efficiency(b) * self.building_resource(b) * building_rate(rec, f) * self.improvements(&pb.record).0 * PRODUCTION_SPEED * self.advanced.production * mult
    }

    /// The buildings built made their inventory as their class makes it
    /// (`crate::base::building_inventory`: a machine's containers, F029392; a storage's section,
    /// F037797 / F029410): what their storage held put in as far as it goes, the rest put down by
    /// the building.
    pub fn setup_inventories(&mut self) {
        for b in 0..self.buildings.len() {
            if self.buildings[b].containers_made || !self.buildings[b].done {
                continue;
            }
            self.buildings[b].containers_made = true;
            let Some(made) = self.data.get(&self.buildings[b].record).and_then(|r| crate::base::building_inventory(&self.data, r)) else { continue };
            let old = std::mem::replace(&mut self.buildings[b].storage, made);
            let mut rest = Vec::new();
            for item in old.iter().flat_map(|i| i.sections.iter().flat_map(|s| s.items.iter().map(|p| p.item.clone()))) {
                let mut one = item.clone();
                one.quantity = 1;
                for _ in 0..item.quantity {
                    if !self.buildings[b].storage.as_mut().is_some_and(|into| into.add(one.clone(), false)) {
                        rest.push(one.clone());
                    }
                }
            }
            let at = self.buildings[b].at;
            for item in rest {
                self.next_ground += 1;
                self.ground.push(crate::loot::GroundItem { id: self.next_ground, item, at, yaw: 0.0 });
            }
        }
    }

    /// Machine `b`'s ingredients (its "consumes" in order: the container's section in its
    /// storage, the item, its amount a unit of output, F029392's second value x 0.01) and its
    /// output's container section, item and limit, for one made with containers.
    #[allow(clippy::type_complexity)]
    pub(crate) fn machine_parts(&self, b: usize) -> Option<(Vec<(usize, String, f32)>, Option<(usize, String, u32)>)> {
        let pb = self.buildings.get(b).filter(|pb| pb.containers_made)?;
        let inv = pb.storage.as_ref()?;
        let rec = self.data.get(&pb.record)?;
        if !crate::base::BuildingClass::of(&self.data, rec).is_production() {
            return None;
        }
        let (f, _) = function_of(&self.data, rec)?;
        let inputs = f
            .refs("consumes")
            .iter()
            .zip(INPUTS)
            .filter_map(|((r, v), name)| inv.section(name).map(|k| (k, r.clone(), v[1] as f32 * 0.01)))
            .collect();
        let output = inv.section(OUTPUT).map(|k| (k, inv.sections[k].only.first().cloned().unwrap_or_default(), inv.sections[k].stack));
        Some((inputs, output))
    }

    /// @fn F029326
    ///
    /// ProductionBuilding::operate for machine `b` (made with containers) with `amount` of
    /// work (its rate x the frame time): limited by its ingredients and its output's room
    /// (F029360), the ingredients used (F029359) and the output made (F029325); its state
    /// (+0x468): working, else full (1) when its output takes no more, else idle. Whether it
    /// worked.
    pub fn operate_machine(&mut self, b: usize, mut amount: f32) -> bool {
        let Some((inputs, output)) = self.machine_parts(b) else { return false };
        if inputs.is_empty() && output.is_none() {
            return false;
        }
        let function = self.data.get(&self.buildings[b].record).and_then(|r| function_of(&self.data, r)).map_or(0, |(_, k)| k);
        let product = output.as_ref().and_then(|(_, r, _)| self.data.get(r)).map(|r| Item::from_record(r, 1));
        let pb = &mut self.buildings[b];
        if pb.taken.len() < inputs.len() {
            pb.taken.resize(inputs.len(), 0.0);
        }
        if pb.made.is_empty() {
            pb.made.push(0.0);
        }
        let inv = pb.storage.as_mut().expect("containers");
        // F029360: no more than the ingredients there are (banked and in their containers), nor
        // than the output's room under its limit
        for (k, (s, _, per)) in inputs.iter().enumerate() {
            if *per == 0.0 {
                continue;
            }
            let avail = pb.taken[k] + inv.sections[*s].items.first().map_or(0, |p| p.item.quantity) as f32;
            if avail < per * amount {
                amount = amount.min(avail / per);
            }
        }
        let accepts = |inv: &Inventory, s: usize| inv.sections[s].items.first().is_none_or(|p| p.item.quantity < inv.sections[s].stack_of(&p.item));
        match &output {
            Some((s, _, most)) => {
                let held = inv.sections[*s].items.first().map_or(0, |p| p.item.quantity);
                amount = amount.min(*most as f32 - held as f32 - pb.made[0]);
            }
            // (a generator's power output instead: not here, `power_tick`)
            None => amount = 0.0,
        }
        if amount <= 0.0 {
            if let Some((s, ..)) = &output {
                pb.full = !accepts(inv, *s);
            }
            self.missing_material = inputs.iter().find(|(s, _, per)| *per != 0.0 && inv.sections[*s].items.is_empty()).map(|(_, r, _)| r.clone());
            return false;
        }
        pb.full = false;
        // F029359: each ingredient's amount taken from its container one by one as needed
        for (k, (s, _, per)) in inputs.iter().enumerate() {
            if *per == 0.0 {
                continue;
            }
            let need = per * amount;
            while pb.taken[k] < need {
                let sec = &mut inv.sections[*s];
                let Some(first) = sec.items.first_mut() else { break };
                if first.item.quantity < 2 {
                    sec.items.remove(0);
                } else {
                    first.item.quantity -= 1;
                }
                pb.taken[k] += 1.0;
            }
            pb.taken[k] -= need;
            if pb.taken[k] <= 0.001 && function != 6 {
                pb.taken[k] = 0.0;
            }
        }
        // F029325: the output's level; at 0.999 one made (its excess let go), each ingredient's
        // banked level cut back to none or 1
        let mut made = false;
        if let Some((s, ..)) = &output {
            pb.made[0] += amount;
            while pb.made[0] >= 0.999 {
                for (k, (_, _, per)) in inputs.iter().enumerate() {
                    if *per <= 0.0 {
                        continue;
                    }
                    let extra = if pb.made[0] >= 1.0 { (pb.made[0] as f64 - 1.0) * *per as f64 } else { 0.0 };
                    if extra + pb.taken[k] as f64 <= 0.999 {
                        if pb.taken[k] < 0.001 {
                            pb.taken[k] = 0.0;
                        }
                    } else {
                        pb.taken[k] = 1.0;
                    }
                }
                if !accepts(inv, *s) {
                    break;
                }
                let Some(item) = product.clone() else { break };
                match inv.sections[*s].items.first_mut() {
                    Some(p) => p.item.quantity += 1,
                    None => inv.sections[*s].items.push(kenshi_char::inventory::Placed { item, x: 0, y: 0 }),
                }
                pb.made[0] = 0.0;
                made = true;
            }
            if !accepts(inv, *s) {
                pb.made[0] = 0.0;
            }
        }
        let eff = self.power_efficiency(b).clamp(0.0, 1.0);
        self.buildings[b].running = eff;
        if made && std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
            eprintln!("machine {b} made one {:?}", output.as_ref().map(|o| &o.1));
        }
        true
    }

    /// @fn F029366
    ///
    /// A machine's update each frame (made with containers): one needing no operator
    /// (F029391's +0x3a9) works on its own (operate at a multiplier of 1); one full lets go of it
    /// once its output has room.
    fn machines_tick(&mut self, dt: f32) {
        for b in 0..self.buildings.len() {
            if !self.buildings[b].containers_made || !self.is_production(b) {
                continue;
            }
            let Some(rec) = self.data.get(&self.buildings[b].record) else { continue };
            // (a farm's update is its own, F022906: `farm_tick`)
            if crate::base::BuildingClass::of(&self.data, rec) != crate::base::BuildingClass::Production {
                continue;
            }
            let Some((f, _)) = function_of(&self.data, rec) else { continue };
            if !needs_operator(rec, f) {
                let amount = self.machine_rate(b, 1.0) * dt;
                self.operate_machine(b, amount);
                continue;
            }
            if self.buildings[b].full
                && let Some((_, Some((s, ..)))) = self.machine_parts(b)
                && let Some(inv) = self.buildings[b].storage.as_ref()
                && inv.sections[s].items.first().is_none_or(|p| p.item.quantity < inv.sections[s].stack_of(&p.item))
            {
                self.buildings[b].full = false;
            }
        }
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
        self.setup_inventories();
        self.drop_operators();
        self.refresh_full();
        self.machines_tick(dt);
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
            // a machine made with containers: Task_OperateMachine's update (F031350) works it at
            // the operator's skill multiplier (ProductionBuilding::operate, F029326)
            if self.buildings[b].containers_made && crate::base::BuildingClass::of(&data, rec) == crate::base::BuildingClass::Production {
                if let Some(s) = stat {
                    self.operator_train(id, s, dt);
                }
                let amount = self.operator_rate(id, b, stat) * dt;
                self.operate_machine(b, amount);
                continue;
            }
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
                    let mut made_item = Item::from_record(r, 1);
                    // addFinishedCraftItem (F029664): a crafted weapon, armour, crossbow or limb
                    // (Item vtable slot 88) takes its maker's name (+0x1f8)
                    if crafting
                        && matches!(made_item.kind, Some(kenshi_data::ItemType::Weapon | kenshi_data::ItemType::Armour | kenshi_data::ItemType::Crossbow | kenshi_data::ItemType::LimbReplacement))
                    {
                        made_item.crafter = Some(self.folk[id].name.clone());
                    }
                    let Some(store) = self.buildings[b].storage.as_mut() else { break };
                    if !store.add(made_item, false) {
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
                self.operator_train(id, s, dt);
            }
            if !made.is_empty() && std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                eprintln!("{} made {:?}", rec.name, made);
            }
        }
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
        for &(id, stat) in &workers {
            if let Some(s) = stat {
                self.operator_train(id, s, dt);
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

    /// @fn F022900
    ///
    /// A farm's fertility (+0x554, set by F022914 from its place's resource level): 1 - (1 - the
    /// "farming" resource) x "fertility effect", none at or under "minimum fertility".
    pub fn farm_fertility(&self, b: usize) -> f32 {
        let pb = &self.buildings[b];
        let Some(fd) = self.data.get(&pb.record).and_then(|r| r.refs("farm data").first().and_then(|(s, _)| self.data.get(s))) else { return 0.0 };
        let fert = self.resource_level(6, pb.at);
        let effect = num(fd, "fertility effect");
        if fert <= num(fd, "minimum fertility") { 0.0 } else { (1.0 - (1.0 - fert) * effect).max(0.0) }
    }

    /// @fn F022899
    ///
    /// A farm's crop's fit to the ground, outdoors: the biome's "arid", "green" and "swamp"
    /// against the crop's; in a building 1.
    pub fn crop_fit(&self, b: usize) -> f32 {
        let pb = &self.buildings[b];
        if kenshi_world::interior::building_at(&self.inner, pb.at).is_some() {
            return 1.0;
        }
        let Some(fd) = self.data.get(&pb.record).and_then(|r| r.refs("farm data").first().and_then(|(s, _)| self.data.get(s))) else { return 1.0 };
        match self.resources_at(pb.at[0], pb.at[2]) {
            Some(res) => num(res, "arid") * num(fd, "arid") + num(res, "green") * num(fd, "green") + num(res, "swamp") * num(fd, "swamp"),
            None => 1.0,
        }
    }

    /// @fn F022891
    ///
    /// A harvested plant's chance of each of its "output per plant": lerp(the operator's
    /// farming / 100, 0.5, 2) held to 0 .. 1, times the crop's fit and the farm's fertility.
    fn harvest_chance(&self, b: usize, farming: f32) -> f32 {
        (0.5 + (2.0 - 0.5) * farming * 0.01).clamp(0.0, 1.0) * self.crop_fit(b) * self.farm_fertility(b)
    }

    /// @fn F022904
    ///
    /// A farm replanted: each of its plants (FARM_DATA "amount", the layout F022913's) at a
    /// growth of rand x -0.2, the crop's stage the least of them, none harvested, its death and
    /// clearing work none, its harvest none.
    fn replant(&mut self, b: usize, plants: usize) {
        let mut least = 0.0f32;
        let mut v = Vec::with_capacity(plants);
        for k in 0..plants {
            let g = rand_unit((b as u32).wrapping_mul(7919) ^ (k as u32).wrapping_mul(104729) ^ self.tick as u32) * -0.2;
            least = least.min(g);
            v.push(g);
        }
        let pb = &mut self.buildings[b];
        pb.plants = v;
        pb.harvested = 0;
        pb.death = 0.0;
        pb.work = 0.0;
        pb.stage = least;
        if let Some(m) = pb.made.first_mut() {
            *m = 0.0;
        }
    }

    /// @fn F022888
    ///
    /// A farm's crop drinks `amount` (each ingredient its amount a unit, 1, times it): an
    /// ingredient short with its container empty: dry (its level none); else each takes an item
    /// from its container while its level is short (a unit each), then drinks. Whether it was
    /// watered.
    fn farm_drink(&mut self, b: usize, amount: f32) -> bool {
        let Some((inputs, _)) = self.machine_parts(b) else { return false };
        let pb = &mut self.buildings[b];
        if pb.taken.len() < inputs.len() {
            pb.taken.resize(inputs.len(), 0.0);
        }
        let Some(inv) = pb.storage.as_mut() else { return false };
        for (k, (s, ..)) in inputs.iter().enumerate() {
            if (pb.taken[k] < amount || pb.taken[k] <= 0.0) && inv.sections[*s].items.is_empty() {
                pb.taken[k] = 0.0;
                return false;
            }
        }
        for (k, (s, ..)) in inputs.iter().enumerate() {
            if pb.taken[k] < amount {
                let sec = &mut inv.sections[*s];
                let Some(first) = sec.items.first_mut() else { return false };
                if first.item.quantity < 2 {
                    sec.items.remove(0);
                } else {
                    first.item.quantity -= 1;
                }
                pb.taken[k] += 1.0;
            }
            pb.taken[k] -= amount;
        }
        true
    }

    /// @fn F022906
    ///
    /// Farms each frame (FarmBuilding::update; F022915 reads its FARM_DATA): rain outdoors
    /// tops up the water ingredient's level (rain x "consumption rate" a day x the biome's "farm
    /// water usage" x 3, to 1); the crop drinks (F022888); it grows by the hours x its
    /// fertility / "growth time" (dry: x "drought multiplier"; a farm with a "power output":
    /// x its power factor): each plant not harvested, the stage the least of them; ripe at 1
    /// for "harvest time" hours, then rotting at 2 over "death time"; drought kills over
    /// "drought death time" (watered again: recovering under 0.6); dead or all harvested:
    /// replanted (F022904). Its state (+0x468): working, idle (3 watered, 0 dry) or full (kept
    /// only while its output is full). Its operators harvest (F022907).
    pub fn farm_tick(&mut self, dt: f32) {
        let hours = dt / self.hour_secs as f32;
        let data = self.data.clone();
        for b in 0..self.buildings.len() {
            if !self.buildings[b].done || !self.buildings[b].containers_made {
                continue;
            }
            let Some(rec) = data.get(&self.buildings[b].record) else { continue };
            let Some(fd) = rec.refs("farm data").first().and_then(|(s, _)| data.get(s)) else { continue };
            let at = self.buildings[b].at;
            let usage = self.biome_at(at[0], at[2]).map_or(1.0, |r| r.floats.get("farm water usage").copied().unwrap_or(1.0));
            let consumption = num(fd, "consumption rate") / DAY * usage;
            let plants = num(fd, "amount").max(0.0) as usize;
            if self.buildings[b].plants.len() != plants {
                self.replant(b, plants);
            }
            // rain, outdoors: into the first ingredient's level
            let inputs = self.machine_parts(b).map_or(0, |(i, _)| i.len());
            if self.buildings[b].taken.len() < inputs {
                self.buildings[b].taken.resize(inputs, 0.0);
            }
            if inputs > 0 && self.rain > 0.0 && kenshi_world::interior::building_at(&self.inner, at).is_none() {
                let level = &mut self.buildings[b].taken[0];
                *level = (*level + self.rain * consumption * dt * 3.0).min(1.0);
            }
            let watered = self.farm_drink(b, dt * consumption);
            let mut growth = hours * self.farm_fertility(b) / num(fd, "growth time");
            if !watered {
                growth *= num(fd, "drought multiplier");
            }
            if num(rec, "power output") + self.improvements(&rec.string_id).1 != 0.0 {
                growth *= self.power_efficiency(b);
            }
            let (harvest_time, death_time, ddt, threshold) = (num(fd, "harvest time").max(1e-6), num(fd, "death time"), num(fd, "drought death time"), num(fd, "death threshold"));
            let pb = &mut self.buildings[b];
            let mut changed = false;
            if pb.stage >= 1.0 {
                if pb.stage >= 2.0 {
                    changed = true;
                    pb.death += hours / death_time;
                } else {
                    pb.stage += hours / harvest_time;
                }
            } else if growth > 0.0 {
                changed = true;
                pb.stage = 1.0;
                for g in pb.plants.iter_mut() {
                    if *g < 1.0 && *g >= -1.0 {
                        *g += growth;
                        if *g < pb.stage {
                            pb.stage = *g;
                        }
                    }
                }
            }
            if ddt > 0.0 && pb.stage > 0.0 && pb.stage < 1.0 {
                if !watered {
                    pb.death += hours / ddt;
                } else if pb.death > 0.0 && (pb.death as f64) < 0.6 && pb.stage < 2.0 {
                    pb.death -= hours / ddt;
                }
                if pb.death > 0.0 {
                    changed = true;
                }
            }
            let _ = changed;
            let all = pb.harvested as usize >= plants;
            if pb.death >= 1.0 || pb.harvested as usize == plants {
                self.replant(b, plants);
            }
            let pb = &mut self.buildings[b];
            // its state: full kept only while the output takes no more (idle, 3 / 0, or
            // working otherwise: not full)
            if pb.stage < 1.0 || threshold <= pb.death || all {
                pb.full = false;
            } else if pb.full {
                let still = pb.storage.as_ref().and_then(|inv| inv.section(OUTPUT).map(|s| &inv.sections[s])).is_some_and(|sec| sec.items.first().is_some_and(|p| p.item.quantity >= sec.stack_of(&p.item)));
                pb.full = still;
            }
            pb.water = pb.taken.first().copied().unwrap_or(0.0);
            if std::env::var("KENSHI_BUILD_DEBUG").is_ok() && self.tick % 300 == 0 {
                let pb = &self.buildings[b];
                eprintln!("farm {b}: stage {:.2}, death {:.2}, water {:.2}, harvested {}/{plants}, watered {watered}", pb.stage, pb.death, pb.water, pb.harvested);
            }
        }
        // F022907: operators harvest the ripe or clear the dead
        for (id, b, stat) in self.operators() {
            let Some(rec) = data.get(&self.buildings[b].record) else { continue };
            let Some(fd) = rec.refs("farm data").first().and_then(|(s, _)| data.get(s)) else { continue };
            if !self.buildings[b].containers_made {
                continue;
            }
            if let Some(s) = stat {
                self.operator_train(id, s, dt);
            }
            let level = stat.map(|s| self.arena.f[id].level(s));
            self.farm_operate(b, fd, skill_mult(level) * dt, Some(id));
        }
    }

    /// @fn F022907
    ///
    /// FarmBuilding::operate with `work` (the frame time x the operator's multiplier): not ripe,
    /// dead (at its "death threshold"), all harvested or full: a dead crop cleared at its
    /// "clear rate" a day (cleared: one plant taken off, F022903); else the harvest at its
    /// "harvest rate" a day: at a whole one, its output full: full (state 1); else of each of
    /// its "output per plant" (and the fraction's chance) one made with the harvest chance
    /// (F022891, the operator's farming), one plant taken off (F022903).
    fn farm_operate(&mut self, b: usize, fd: &GameData, work: f32, operator: Option<usize>) {
        let plants = self.buildings[b].plants.len();
        let threshold = num(fd, "death threshold");
        let rate = self.power_efficiency(b) * PRODUCTION_SPEED * self.advanced.production * work;
        let Some((_, Some((out, product, _)))) = self.machine_parts(b) else { return };
        let pb = &self.buildings[b];
        if pb.made.is_empty() {
            self.buildings[b].made.push(0.0);
        }
        let pb = &self.buildings[b];
        if pb.stage < 1.0 || threshold <= pb.death || pb.harvested as usize >= plants || pb.full {
            if threshold <= pb.death {
                let pb = &mut self.buildings[b];
                pb.work += rate * num(fd, "clear rate") / DAY;
                if pb.work >= 1.0 {
                    pb.work = 0.0;
                    self.harvest_plant(b, plants);
                }
            }
            return;
        }
        self.buildings[b].made[0] += rate * num(fd, "harvest rate") / DAY;
        if self.buildings[b].made[0] < 1.0 {
            return;
        }
        let farming = operator.map_or(0.0, |id| self.arena.f[id].level(StatsEnumerated::StatFarming));
        let chance = self.harvest_chance(b, farming);
        let Some(item) = self.data.get(&product).map(|r| Item::from_record(r, 1)) else { return };
        let per = num(fd, "output per plant");
        let tick = self.tick as u32;
        let pb = &mut self.buildings[b];
        let Some(inv) = pb.storage.as_mut() else { return };
        let room = |inv: &Inventory| inv.sections[out].items.first().is_none_or(|p| p.item.quantity < inv.sections[out].stack_of(&p.item));
        if !room(inv) {
            pb.full = true;
            return;
        }
        let mut made = 0;
        let add = |inv: &mut Inventory| match inv.sections[out].items.first_mut() {
            Some(p) => p.item.quantity += 1,
            None => inv.sections[out].items.push(kenshi_char::inventory::Placed { item: item.clone(), x: 0, y: 0 }),
        };
        let mut k = 0u32;
        loop {
            if rand_unit((b as u32) ^ tick.wrapping_add(k.wrapping_mul(2654435761))) < chance {
                add(inv);
                made += 1;
            }
            k += 1;
            if (k as f32) >= per {
                break;
            }
        }
        let frac = per - (per as i32) as f32;
        if frac > 0.0 && rand_unit((b as u32).wrapping_mul(31) ^ tick ^ 0x5bd1_e995) < frac {
            add(inv);
            made += 1;
        }
        pb.made[0] = 0.0;
        self.harvest_plant(b, plants);
        if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
            eprintln!("farm {b} harvested {made} (chance {chance:.2})");
        }
    }

    /// @fn F022903
    ///
    /// One plant of a farm taken off (harvested or cleared): the next not taken marked so
    /// (growth -10), one more harvested.
    fn harvest_plant(&mut self, b: usize, plants: usize) {
        let pb = &mut self.buildings[b];
        let k = pb.harvested as usize;
        if k == plants {
            return;
        }
        if let Some(g) = pb.plants.get_mut(k) {
            *g = -10.0;
        }
        pb.harvested += 1;
    }

    /// @fn F047468
    ///
    /// An operator's stat trained each frame of its work (Task_OperateMachine F031350, but on
    /// training equipment): F047460 with the frame time at 0.5 (thieving and 37 0.1, turrets 1,
    /// perception 3, 35 1.2, farming 0.75).
    pub fn operator_train(&mut self, id: usize, stat: StatsEnumerated, dt: f32) {
        let mult = match stat as i32 {
            10 | 0x25 => 0.1,
            0xb => 1.0,
            0x18 => 3.0,
            0x23 => 0.8 * 1.5,
            0xc => 0.5 * 1.5,
            _ => 0.5,
        };
        self.arena.f[id].skills.train(stat, dt, mult);
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
            // (switched off: neither drawn on nor giving, F029290 / F029341)
            if !self.buildings[k].done || !self.buildings[k].switched_on {
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
        self.power_eff_mains = if demand <= 0.0 { 1.0 } else { (supply / demand).min(1.0) };
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

    /// Building `b`'s power efficiency: 1 when it draws none, else the base's (F037662): none
    /// switched off, and for one that may not run off the batteries the mains' alone (F029341).
    pub fn power_efficiency(&self, b: usize) -> f32 {
        let Some(pb) = self.buildings.get(b) else { return 1.0 };
        let draws = self.data.get(&pb.record).is_some_and(|r| num(r, "power output") < 0.0);
        if !draws {
            1.0
        } else if !pb.switched_on {
            0.0
        } else if pb.on_battery {
            self.power_eff
        } else {
            self.power_eff_mains
        }
    }

    /// How many work building `b` now (Building +0x3e0, its operators: at most its "max
    /// operators", F029341's share).
    pub fn operators_on(&self, b: usize) -> usize {
        self.operators().iter().filter(|o| o.1 == b).count()
    }

    /// The people operating building `b` (as operators_on counts them).
    pub fn operator_ids(&self, b: usize) -> Vec<usize> {
        self.operators().iter().filter(|o| o.1 == b).map(|o| o.0).collect()
    }

    /// Whether building `b` works for want of power.
    pub fn has_power(&self, b: usize) -> bool {
        self.power_efficiency(b) > 0.0
    }

    /// The machines' full state (+0x468, F029267) let go once the output has room again: as the
    /// farm's update (F022906) keeps it only while its output container is full (F03d21c).
    /// (assumption) The same for every machine: room for one more of its first product (a
    /// bench's queued item).
    fn refresh_full(&mut self) {
        for b in 0..self.buildings.len() {
            // (a machine made with containers: F029366, `machines_tick`)
            if !self.buildings[b].full || self.buildings[b].containers_made {
                continue;
            }
            let pb = &self.buildings[b];
            let Some(rec) = self.data.get(&pb.record) else { continue };
            let product = function_of(&self.data, rec).and_then(|(f, _)| pb.craft_queue.first().cloned().or_else(|| f.refs("produces").first().map(|(s, _)| s.clone())));
            let Some(item) = product.and_then(|p| self.data.get(&p)).map(|r| Item::from_record(r, 1)) else { continue };
            let room = pb.storage.as_ref().is_some_and(|inv| {
                let mut trial = inv.clone();
                trial.add(item, false)
            });
            if room {
                self.buildings[b].full = false;
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
