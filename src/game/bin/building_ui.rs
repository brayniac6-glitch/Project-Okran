//! A building selected: picked by a left click on it (Kenshi's GameWorld selection, F045559:
//! the characters selected stay so, the panel shows the object last picked) and shown in the
//! main panel's datapanels, each filled by the building's updateDatapanel (Building vtable
//! +0x140) with that datapanel's mode (F043077: NameText 5, StateText 3, the MedicalPanel 4,
//! StatsText 6, ExtendedStatsText 7):
//! - mode 5, its name and owner (F030560);
//! - mode 3, "Town:" (F030573), its upgrade lines (F030575) and its buttons (F030563);
//! - mode 4, its status (the class's slot 114: F030561) and its condition (F030574);
//! - mode 6, "Open to public:" and "For sale:" (F030573).
//!
//! The class is the one Building::create (F038257) makes for the record: "is wall gate" a
//! GatewayBuilding, "links together" a WallBuilding, "farm data" a FarmBuilding, else by its
//! functionality's "function" (BuildingFunction), none a plain Building.
use super::hud_ui::{Bar, LineButton, Row};
use kenshi_data::{GameData, GameDataContainer};
use kenshi_game::sim::Sim;

/// The building classes Building::create makes (F038257), by their vtables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Building (no functionality, BF_ANY, BF_FLUFF).
    Plain,
    /// GatewayBuilding ("is wall gate").
    Gateway,
    /// WallBuilding ("links together").
    Wall,
    /// FarmBuilding ("farm data").
    Farm,
    /// ProductionBuilding (BF_MINE, BF_REFINERY).
    Production,
    /// StorageBuilding (BF_RESOURCE_STORAGE).
    Storage,
    /// ResearchBuilding (BF_RESEARCH).
    Research,
    /// GeneratorBuilding (BF_GENERATOR consuming fuel, BF_BATTERY).
    Generator,
    /// WindGeneratorBuilding (BF_GENERATOR consuming nothing).
    WindGenerator,
    /// TortureBuilding (BF_CAGE that produces or has flags).
    Torture,
    /// CraftingBuilding (BF_CRAFTING).
    Crafting,
    /// TurretBuilding (BF_TURRET).
    Turret,
    /// FurnaceBuilding (BF_ITEM_FURNACE).
    Furnace,
    /// LightBuilding (BF_LIGHT).
    Light,
    /// RainCollectorBuilding (BF_RAIN_COLLECTOR).
    RainCollector,
    /// UseableStuff (every other function: beds, training, shops, storage, cages...).
    Useable,
}

/// @fn F038257
///
/// The class Building::create makes for BUILDING record `r`.
pub fn class_of(data: &GameDataContainer, r: &GameData) -> Class {
    let flag = |k: &str| r.bools.get(k).copied().unwrap_or(false);
    if flag("is wall gate") {
        return Class::Gateway;
    }
    if flag("links together") {
        return Class::Wall;
    }
    if !r.refs("farm data").is_empty() {
        return Class::Farm;
    }
    let Some(f) = r.refs("functionality").first().and_then(|(id, _)| data.get(id)) else { return Class::Plain };
    match f.ints.get("function").copied().unwrap_or(0) {
        0 | 0x12 => Class::Plain,
        1 | 4 => Class::Production,
        2 => Class::Storage,
        3 => Class::Research,
        5 if f.refs("consumes").is_empty() => Class::WindGenerator,
        5 | 0x17 => Class::Generator,
        8 if !f.refs("produces").is_empty() || f.ints.get("flags").copied().unwrap_or(0) != 0 => Class::Torture,
        10 => Class::Crafting,
        0xc => Class::Turret,
        0xe => Class::Furnace,
        0xf => Class::Light,
        0x1a => Class::RainCollector,
        _ => Class::Useable,
    }
}

/// A building as the sim keeps it: one of the player's (`Sim::buildings`), a town's
/// (`Sim::placed`), a piece of a town building's furniture (`Sim::furn`), a town's container
/// (`Sim::boxes`) or a door (`Sim::door_states`). Kenshi's are all Buildings (DoorStuff,
/// UseableStuff and the rest, F038257), each picked the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Player(usize),
    Town(usize),
    Furniture(usize),
    Container(usize),
    Door(usize),
}

/// The building selected, known again by its record and place as the lists shift (a building
/// before it gone).
#[derive(Clone, Debug, PartialEq)]
pub struct Selected {
    pub place: Place,
    record: String,
    at: [f32; 3],
}

/// Building `place`'s record and where it stands (units), none once it is gone.
fn record_at(sim: &Sim, place: Place) -> Option<(&str, [f32; 3])> {
    match place {
        Place::Player(b) => sim.buildings.get(b).map(|pb| (pb.record.as_str(), pb.at)),
        Place::Town(p) => Some((sim.placed.get(p)?.building.as_str(), *sim.placed_at.get(p)?)),
        Place::Furniture(f) => sim.furn.get(f).map(|f| (f.record.as_str(), f.position)),
        Place::Container(k) => sim.boxes.get(k).map(|c| (c.at.record.as_str(), c.at.position)),
        Place::Door(d) => sim.door_states.get(d).filter(|s| !s.gone).map(|s| (s.door.record.as_str(), s.door.position)),
    }
}

impl Selected {
    pub fn new(sim: &Sim, place: Place) -> Option<Selected> {
        let (record, at) = record_at(sim, place)?;
        Some(Selected { place, record: record.to_string(), at })
    }

    pub fn player(sim: &Sim, b: usize) -> Option<Selected> {
        Selected::new(sim, Place::Player(b))
    }

    /// The building now: where its list has it, none once it is gone. One of the player's is
    /// known by its place alone (its record changes as it is upgraded or downgraded).
    pub fn resolve(&mut self, sim: &Sim) -> Option<Place> {
        let mine = matches!(self.place, Place::Player(_));
        let same = |place: Place| record_at(sim, place).is_some_and(|(r, at)| (mine || r == self.record) && at == self.at);
        if !same(self.place) {
            // the player's buildings shift as one before them goes; the rest are not taken away
            if !mine {
                return None;
            }
            self.place = (0..sim.buildings.len()).map(Place::Player).find(|&p| same(p))?;
        }
        if mine && let Some((r, _)) = record_at(sim, self.place) {
            self.record = r.to_string();
        }
        Some(self.place)
    }
}

/// The building a panel shows: where it stands and what it is (`Place`), its record.
#[derive(Clone, Copy)]
struct Site<'a> {
    sim: &'a Sim,
    place: Place,
    record: &'a str,
    rec: &'a GameData,
    at: [f32; 3],
}

impl<'a> Site<'a> {
    fn new(sim: &'a Sim, place: Place) -> Option<Site<'a>> {
        let (record, at) = record_at(sim, place)?;
        Some(Site { sim, place, record, rec: sim.data.get(record)?, at })
    }

    /// One of the player's buildings.
    fn pb(&self) -> Option<&'a kenshi_game::base::PlayerBuilding> {
        match self.place {
            Place::Player(b) => self.sim.buildings.get(b),
            _ => None,
        }
    }

    fn player(&self) -> Option<usize> {
        match self.place {
            Place::Player(b) => Some(b),
            _ => None,
        }
    }

    /// Built (Building +0x160): a town's always.
    fn done(&self) -> bool {
        self.pb().is_none_or(|pb| pb.done)
    }

    /// The town building it is or stands in (`Sim::placed`).
    fn placement(&self) -> Option<usize> {
        match self.place {
            Place::Player(_) => None,
            Place::Town(p) => Some(p),
            Place::Furniture(f) => self.sim.furn.get(f).map(|f| f.placement),
            Place::Container(k) => self.sim.boxes.get(k).map(|c| c.at.placement),
            Place::Door(d) => self.sim.door_states.get(d).map(|s| s.door.placement),
        }
    }

    /// Its owner's faction (Building +0x10): the player's for the player's; a town's building
    /// (and what stands in it) its town's faction (F038257: a fresh zone's building takes its
    /// town's), else its squad's or residents' (as `Sim::door_owner` has a door's).
    fn owner(&self) -> Option<String> {
        if self.pb().is_some() {
            return Some(kenshi_game::sim::PLAYER_FACTION.to_string());
        }
        if let Place::Door(d) = self.place {
            return self.sim.door_owner(d);
        }
        let p = self.placement()?;
        if let Some(t) = self.town()
            && let Some(f) = self.sim.towns[t].faction.clone()
        {
            return Some(f);
        }
        let pl = self.sim.placed.get(p)?;
        [&pl.squad, &pl.residents].into_iter().flatten().find_map(|t| self.sim.data.get(t)?.refs("faction").first().map(|(f, _)| f.clone()))
    }

    /// The player's own (its owner's +0x250).
    fn mine(&self) -> bool {
        self.owner().as_deref() == Some(kenshi_game::sim::PLAYER_FACTION)
    }

    /// Building::getTown (F023168): a town's building its town; one of the player's the town
    /// nearest it within that town's radius (F048806), the player's own outpost among them.
    fn town(&self) -> Option<usize> {
        if let Some(p) = self.placement() {
            return self.sim.placed_town.get(p).copied().flatten();
        }
        let at = self.at;
        self.sim
            .towns
            .iter()
            .enumerate()
            .filter(|(_, t)| t.contains(at, 1.0))
            .min_by(|(_, a), (_, c)| {
                let d = |t: &kenshi_world::town::Town| (t.position[0] - at[0]).powi(2) + (t.position[2] - at[2]).powi(2);
                d(a).total_cmp(&d(c))
            })
            .map(|(k, _)| k)
    }
}

/// What the main panel's datapanels show of a building: NameText (mode 5), StateText (mode
/// 3), the medical panel's status and bars (mode 4), StatsText (mode 6), ExtendedStatsText
/// (mode 7).
#[derive(Default)]
pub struct View {
    pub name: Vec<Row>,
    pub info: Vec<Row>,
    pub status: (String, String),
    pub bars: Vec<Bar>,
    pub stats: Vec<Row>,
    pub extended: Vec<Row>,
}

/// The game's colour strings (`hud_ui::tag`, 0x141f49248 and after).
const TAN: &str = "#afa68b";
const TIP_TEXT: &str = "#140806";
const DARK_RED: &str = "#492620";
const BROWN: &str = "#59231a";
const RED: &str = "#bf4834";
const GREEN: &str = "#a8b774";
const GREY: &str = "#777777";

/// A datapanel button's actions (the click's widget name after "Datapanel:").
pub const DISMANTLE: &str = "Dismantle";
pub const PAUSE: &str = "PauseConstruction";
pub const PUBLIC: &str = "OpenToPublic";
pub const POWER: &str = "Power";
pub const BATTERY: &str = "Battery";
pub const BUY: &str = "Buy";
pub const DOOR_TOGGLE: &str = "DoorToggle";
pub const DOOR_LOCK: &str = "DoorLock";
pub const UPGRADE: &str = "Upgrade";
pub const DOWNGRADE: &str = "Downgrade";

fn line(label: impl Into<String>, value: impl Into<String>) -> Row {
    Row { label: label.into(), value: value.into(), colour: super::hud_ui::LIGHT, tip: String::new(), button: None }
}

fn tipped(label: impl Into<String>, value: impl Into<String>, tip: impl Into<String>) -> Row {
    Row { tip: tip.into(), ..line(label, value) }
}

/// A button line (DataPanelLine_Button, F042415): `caption` on a Kenshi_Button2 `width` of the
/// line wide at its right (to 0.99), `label` before it (none over 0.9 wide).
fn button(label: impl Into<String>, caption: impl Into<String>, action: &str, width: f32) -> Row {
    let b = LineButton { action: action.to_string(), width: width.clamp(0.0, 0.98), skin: "Kenshi_Button2", checked: None };
    Row { button: Some(b), ..line(label, caption) }
}

/// A tick box line (F042417): a Kenshi_TickButton2 the whole line wide, held down while on.
fn tick(caption: impl Into<String>, action: &str, on: bool, tip: impl Into<String>) -> Row {
    let b = LineButton { action: action.to_string(), width: 0.98, skin: "Kenshi_TickButton2", checked: Some(on) };
    Row { button: Some(b), tip: tip.into(), ..line("", caption) }
}

/// A bar of the medical panel (F043071: its first share alone; F043057 with an arrow text).
fn bar(label: impl Into<String>, value: impl Into<String>, share: f32, tip: String) -> Bar {
    Bar { label: label.into(), value: value.into(), arrow: String::new(), tip, widths: [share, 0.0, 0.0, 0.0], robot: false, crushed: false }
}

/// A key/value tooltip line (F023085: the value's own colour code, the label in the tooltip's).
fn pair(label: &str, value: &str) -> String {
    let (c, v) = match super::hud_ui::colour_code(value) {
        (Some(_), v) => (&value[..7], v),
        (None, v) => (TIP_TEXT, v),
    };
    format!("{label}\t{v}\t{TIP_TEXT}/{c}")
}

fn num(v: i64) -> String {
    super::hud_ui::num(v)
}

fn float(r: &GameData, k: &str) -> f32 {
    r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0)
}

/// @fn F047078
///
/// A skill's name as the game writes it (StatsEnumerated), none for the rest.
pub fn stat_display_name(stat: i32) -> &'static str {
    match stat {
        1 => "Strength",
        2 => "Attack",
        3 => "Labouring",
        4 => "Science",
        5 => "Engineering",
        6 => "Robotics",
        7 => "Weapon Smithing",
        8 => "Armour Smithing",
        9 => "First Aid",
        10 => "Thievery",
        11 => "Turrets",
        12 => "Farming",
        13 => "Cooking",
        14 | 15 => "Vet",
        16 => "Stealth",
        17 => "Athletics",
        18 => "Dexterity",
        19 => "Defence",
        20 => "Weaponry",
        21 => "Toughness",
        22 => "Assassination",
        23 => "Swimming",
        24 => "Perception",
        25 => "Katanas",
        26 => "Sabres",
        27 => "Hackers",
        28 => "Heavy weapons",
        29 => "Blunt",
        30 => "Martial arts",
        32 => "Dodging",
        35 => "Crossbows",
        36 => "Precision Shooting",
        37 => "Lockpicking",
        38 => "Crossbow Smithing",
        64 => "Encumbrance",
        65 => "Combat Speed",
        _ => "",
    }
}

/// A machine's state as Kenshi's ProductionBuilding keeps it (its fields by their offsets),
/// read off the sim's building and base power.
struct Machine<'a> {
    rec: &'a GameData,
    func: Option<&'a GameData>,
    /// The functionality's "function" (Building +0x158).
    function: i32,
    done: bool,
    /// Its construction against its materials' worth (F037662).
    share: f32,
    /// F029289: its "power output" (+0x3bc) and the research's power increase: over 0 a
    /// generator, under a consumer.
    rating: f32,
    /// F029288: its battery capacity, "power capacity" (+0x3c8) by its construction; what it
    /// holds (+0x3c4); what it takes in this frame (+0x3c0) and gives out (+0x3b8).
    capacity_rec: f32,
    capacity: f32,
    stored: f32,
    power_in: f32,
    power_out: f32,
    /// F029344: the power it asks for now.
    need: f32,
    /// F029290: the power it gives now.
    output: f32,
    /// F023179 / the generator's: how fast its fuel burns (0 .. 1).
    fuel_use: f32,
    switched_on: bool,
    on_battery: bool,
    /// Broken down (+0x3b6).
    broken: bool,
    /// Its "max operators" (+0x3ac: 0 fully automatic) and those working it (+0x3e0).
    max_ops: i32,
    ops: usize,
    /// The production state (+0x468): 1 output full, 3 nothing queued, 0 out of an ingredient
    /// (F029326's), else 2 (as the constructor F029411 starts it).
    state: i32,
    /// F029332: an ingredient it has run out of.
    out_of_resource: bool,
    /// F029259: the resource at its place (+0x46c).
    resource: f32,
    /// F029302: its record's "output rate" (+0x1a8) by the research's production mult (the
    /// mult alone kept for a turret's, F029303).
    max_eff: f32,
    prod_mult: f32,
    /// A turret (TurretBuilding: no resource, F029462; its max efficiency the research's
    /// production mult, F029303).
    turret: bool,
    /// ProductionBuilding vtable +0x520: it is working (built, and manned or automatic).
    working: bool,
    /// F029341: power reaches it (none needed, or the base's power, or the batteries' when it
    /// may run off them).
    has_power: bool,
    /// F029323: the rain at its place.
    rain: f32,
}

impl Machine<'_> {
    /// F029260 (powerFactor): working, the power taken in over that asked for (1 asking none),
    /// by its construction; else none.
    fn power_factor(&self) -> f32 {
        if !self.working {
            return 0.0;
        }
        let p = if self.need > 0.0 { self.power_in / self.need } else { 1.0 };
        p * self.share
    }

    /// F037648 (vtable +0x3b8): the resource by the possible efficiency; a turret's the
    /// research's production mult (F029303).
    fn max_efficiency(&self) -> f32 {
        if self.turret { self.prod_mult } else { self.resource * self.max_eff }
    }

    /// The operators' share of its "max operators" (1 for an automatic one).
    fn ops_share(&self) -> f32 {
        if self.max_ops > 0 { self.ops as f32 / self.max_ops as f32 } else { 1.0 }
    }
}

/// How many of item `record` the player's built storage holds (the machine's own first).
fn stock(sim: &Sim, record: &str) -> u32 {
    let count = |inv: &kenshi_char::inventory::Inventory| inv.sections.iter().flat_map(|s| s.items.iter()).filter(|p| p.item.record == record).map(|p| p.item.quantity).sum::<u32>();
    sim.buildings.iter().filter(|o| o.done).filter_map(|o| o.storage.as_ref()).map(count).sum()
}

/// A machine as Kenshi keeps it, from the site. (stand-in) The sim runs neither a town's machines
/// nor its power (Kenshi's per-town power manager, its operators, fuel and products): a town's
/// machine is shown built, unmanned, taking and giving no power, as Kenshi shows one idle.
fn machine<'a>(s: &Site<'a>) -> Machine<'a> {
    let sim = s.sim;
    let rec = s.rec;
    let pb = s.pb();
    let fu = kenshi_game::production::function_of(&sim.data, rec);
    let func = fu.map(|(f, _)| f);
    let function = fu.map_or(0, |(_, k)| k);
    let share = pb.map_or(1.0, |pb| {
        let total = pb.total() as f32;
        if total > 0.0 { (pb.progress / total).min(1.0) } else { 1.0 }
    });
    // the research's improvements are the player's
    let (prod_mult, power_up) = if pb.is_some() { sim.improvements(s.record) } else { (1.0, 0.0) };
    let out = float(rec, "power output");
    let rating = out + power_up;
    let capacity_rec = float(rec, "power capacity");
    let capacity = capacity_rec * share;
    let base = pb.is_some();
    let stored = if base && sim.battery_capacity > 0.0 { sim.battery * capacity_rec / sim.battery_capacity } else { 0.0 };
    // "max operators": the record's, else its functionality's (F029391; 0 automatic)
    let own = rec.ints.get("max operators").copied().unwrap_or(-1);
    let max_ops = if own < 0 { func.and_then(|f| f.ints.get("max operators").copied()).unwrap_or(1) } else { own };
    let ops = s.player().map_or(0, |b| sim.operators_on(b));
    let working = s.done() && (max_ops <= 0 || ops > 0);
    let ops_share = if max_ops > 0 { ops as f32 / max_ops as f32 } else { 1.0 };
    // F029344: a battery asks to be charged while not full; a consumer at work its rating by
    // the operators' share
    let need = if capacity_rec > 0.0 {
        if capacity > stored { rating.abs() } else { 0.0 }
    } else if rating > 0.0 || !working {
        0.0
    } else {
        rating.abs() * ops_share
    };
    let spare = if base { sim.power_supply - sim.power_demand } else { 0.0 };
    let power_in = match s.player() {
        Some(_) if capacity_rec > 0.0 => {
            if spare > 0.0 && stored < capacity { need } else { 0.0 }
        }
        Some(b) => need * sim.power_efficiency(b),
        None => 0.0,
    };
    let power_out = if capacity_rec > 0.0 && spare < 0.0 && stored > 0.0 { rating.abs() } else { 0.0 };
    // the power it gives (F029290; the wind's and the fuel's as the base's power has them)
    let fuelled = func.is_some_and(|f| !f.refs("consumes").is_empty());
    let fuel = pb.map_or(0.0, |pb| pb.fuel);
    let switched_on = pb.is_none_or(|pb| pb.switched_on);
    let output = if !base || rating <= 0.0 || !switched_on || !s.done() || (capacity_rec > 0.0 && stored <= 0.0) {
        0.0
    } else if function == 5 && !fuelled {
        let p = sim.wind * rating;
        if p >= 1.0 { p * share } else { 0.0 }
    } else if fuelled && fuel <= 0.0 {
        0.0
    } else {
        rating * share
    };
    let fuel_use = if fuelled && output > 0.0 && sim.power_supply > 0.0 { (sim.power_demand / sim.power_supply).min(1.0) } else if fuelled { 0.0 } else { 1.0 };
    // F029332: an ingredient wanted with none left in the base's storage nor taken ahead
    let out_of_resource = pb.is_some_and(|pb| {
        func.is_some_and(|f| {
            f.refs("consumes").iter().enumerate().any(|(k, (item, v))| v[1] > 0 && pb.taken.get(k).copied().unwrap_or(0.0) <= 0.001 && stock(sim, item) == 0)
        }) && !(fuelled && pb.fuel > 0.0)
    });
    let crafting = func.is_some_and(|f| !f.refs("item crafts").is_empty());
    let state = if pb.is_some_and(|pb| pb.full) {
        1
    } else if crafting && pb.is_none_or(|pb| pb.craft_queue.is_empty()) {
        3
    } else if out_of_resource {
        0
    } else {
        2
    };
    // F029259: the resource at its place ("world resource mining"; 1 with none)
    let resource = if function == 0xc {
        0.0
    } else if let Some(b) = s.player() {
        sim.building_resource(b)
    } else {
        match func.and_then(|f| f.ints.get("world resource mining").copied()).unwrap_or(0) {
            0 => 1.0,
            kind => sim.resource_level(kind, s.at),
        }
    };
    Machine {
        rec,
        func,
        function,
        done: s.done(),
        share,
        rating,
        capacity_rec,
        capacity,
        stored,
        power_in,
        power_out,
        need,
        output,
        fuel_use,
        switched_on,
        on_battery: pb.is_none_or(|pb| pb.on_battery),
        broken: false,
        max_ops,
        ops,
        state,
        out_of_resource,
        resource,
        max_eff: float(rec, "output rate") * prod_mult,
        prod_mult,
        turret: function == 0xc,
        working,
        has_power: rating >= 0.0 || s.player().is_some_and(|b| sim.has_power(b)),
        rain: sim.rain,
    }
}

/// Building `place` as the main panel shows it: each datapanel filled by its class's
/// updateDatapanel (the vtable +0x140 of the class Building::create made, `class_of`).
pub fn view(sim: &Sim, place: Place) -> View {
    if let Place::Door(d) = place {
        return door_view(sim, d);
    }
    let mut v = View::default();
    let Some(s) = Site::new(sim, place) else { return v };
    let m = machine(&s);
    let rec = s.rec;
    let done = s.done();
    let class = class_of(&sim.data, rec);
    v.name = name_line(&s);
    use Class::*;
    match class {
        // F030573 (Building, GatewayBuilding, WallBuilding)
        Plain | Gateway | Wall => {
            v.info = info_plain(&s);
            v.status = status_plain(done, false);
            v.bars = condition_bars(&s);
            v.stats = public_lines(&s);
        }
        // F030577 (UseableStuff, StorageBuilding, LightBuilding)
        Useable | Storage | Light => {
            v.info = info_useable(&s, &m);
            v.status = status_plain(done, m.broken);
            v.bars = condition_bars(&s);
            v.bars.extend(power_bars(&m));
            v.stats = stats_useable(&m, class);
        }
        // F030583 (ResearchBuilding)
        Research => {
            v.info = info_town_and_buttons(&s);
            v.status = status_plain(done, m.broken);
            v.bars = condition_bars(&s);
            v.bars.extend(power_bars(&m));
            v.stats = workers_and_production(&m, class);
            v.stats.extend(power_ticks(&m));
        }
        // F030564 (CraftingBuilding)
        Crafting => {
            v.info = info_crafting(&s);
            v.status = status_crafting(&s, &m);
            v.bars = condition_bars(&s);
            v.bars.extend(power_bars(&m));
            v.stats = workers_and_production(&m, class);
            v.stats.extend(power_ticks(&m));
        }
        // F030567 (ProductionBuilding and its kin; FurnaceBuilding's F030588 hands its main
        // panel modes to it, F030588 drawing only the inventory window's mode 15)
        Production | Generator | WindGenerator | RainCollector | Torture | Furnace => {
            v.info = info_town_and_buttons(&s);
            v.status = match class {
                Generator => status_generator(&s, &m),
                WindGenerator => status_wind(sim, &m),
                RainCollector => status_rain(&m),
                _ => status_production(&m),
            };
            v.bars = condition_bars(&s);
            v.bars.extend(if class == WindGenerator { wind_bars(sim, &m) } else { power_bars(&m) });
            v.stats = resource_line(&m);
            v.stats.extend(workers_and_production(&m, class));
            if class == WindGenerator {
                v.stats.extend(wind_speed_line(sim));
            }
            v.stats.extend(power_ticks(&m));
        }
        // F030590 (TurretBuilding)
        Turret => {
            v.info = info_turret(&s);
            v.status = status_turret(&s, &m);
            v.bars = condition_bars(&s);
            v.bars.extend(loaded_bar(&s));
            v.bars.extend(power_bars(&m));
            v.stats = production_lines(&m, class);
            v.extended = vec![workers_line(&m)];
            v.extended.extend(production_lines(&m, class));
        }
        // F022895 (FarmBuilding): the machine's panel (F030567) with the farm's own status,
        // bars, crop lines, workers and production
        Farm => {
            let f = farm(&s);
            v.info = info_town_and_buttons(&s);
            v.status = status_farm(&m, &f);
            v.bars = condition_bars(&s);
            v.bars.extend(power_bars(&m));
            if done {
                v.bars.extend(farm_bars(&f));
            }
            v.stats = crop_lines(&m, &f);
            v.stats.push(if f.stage >= 1.0 { workers_line(&m) } else { tipped(format!("{TAN}Workers:"), "-", "No workers required until the crop is ready to harvest.") });
            v.stats.extend(farm_production(&m, &f));
            v.stats.extend(power_ticks(&m));
        }
    }
    v
}

/// The building a door is of: one of the player's, else its town building.
fn door_parent(sim: &Sim, d: usize) -> Option<Place> {
    let s = sim.door_states.get(d)?;
    Some(match s.player_building {
        Some(b) => Place::Player(b),
        None => Place::Town(s.door.placement),
    })
}

/// @fn F030576
///
/// A door's panel (DoorStuff), then its building's (its parent's updateDatapanel, +0x368) in
/// each mode. Mode 4: "Door Condition", its construction (whole numbers; #492620 under 0)
/// against its materials' worth (F037662), "The door is in good condition." at 0 or more, else
/// "The door is damaged.". Mode 6: "Open/Close:" (#afa68b) "The door is broken" (#bf4834),
/// "The door is open" (#a8b774, open or broken: F030332), else "The door is closed" (#bf4834
/// locked, F030330, else #a8b774), its tooltip the same with, broken, "It will no longer close
/// and needs to be repaired." (#140806) under it; "Security:" (#afa68b) "Unlocked" (#a8b774)
/// or "Locked" (#bf4834: a lock wanted, +0x384, or locked, F029432) and " (lvl n)", its tooltip
/// "The door is unlocked." / "The door is locked and will keep out intruders.". For the
/// player's own door (F037652: its owner the player's faction) both are buttons the line wide
/// (Kenshi_Button2): opening or closing it (F037658) and turning its lock (F037659).
fn door_view(sim: &Sim, d: usize) -> View {
    let Some(ds) = sim.door_states.get(d) else { return View::default() };
    let mut v = door_parent(sim, d).map(|p| view(sim, p)).unwrap_or_default();
    // mode 4: the door's bar first
    let share = if ds.full > 0.0 { (ds.health / ds.full).min(1.0) } else { 1.0 };
    let value = if ds.health >= 0.0 { num(ds.health as i64) } else { format!("{DARK_RED}{}", num(ds.health as i64)) };
    let tip = if ds.health >= 0.0 { "The door is in good condition." } else { "The door is damaged." };
    v.bars.insert(0, bar("Door Condition", value, share, tip.into()));
    // mode 6: its lines first
    let mine = sim.door_owner(d).as_deref() == Some(kenshi_game::sim::PLAYER_FACTION);
    let locked = ds.locked && ds.level > 0;
    let state = if ds.broken {
        format!("{RED}The door is broken")
    } else if ds.phase == kenshi_game::sim::DOOR_OPEN {
        format!("{GREEN}The door is open")
    } else if locked {
        format!("{RED}The door is closed")
    } else {
        format!("{GREEN}The door is closed")
    };
    let mut tip = state.clone();
    if ds.broken {
        tip.push_str(&format!(" {TIP_TEXT}It will no longer close and needs to be repaired."));
    }
    let security_locked = ds.wants_lock || (ds.locked && !ds.broken && ds.level > 0);
    let security = format!("{} (lvl {})", if security_locked { format!("{RED}Locked") } else { format!("{GREEN}Unlocked") }, num(ds.level as i64));
    let security_tip = if security_locked { "The door is locked and will keep out intruders." } else { "The door is unlocked." };
    let label = |l: &str| format!("{TAN}{l}");
    let lines = if mine {
        vec![
            Row { tip, ..button(label("Open/Close:"), state, DOOR_TOGGLE, 1.0) },
            Row { tip: security_tip.into(), ..button(label("Security:"), security, DOOR_LOCK, 1.0) },
        ]
    } else {
        vec![tipped(label("Open/Close:"), state, tip), tipped(label("Security:"), security, security_tip)]
    };
    v.stats.splice(0..0, lines);
    v
}

/// @fn F030560
///
/// Mode 5 (NameText): the building's name and its owner's name, both in #492620: the player's
/// faction by its name for one of the player's; another's faction's name while the building
/// stands in a town (F023168), else none.
fn name_line(s: &Site) -> Vec<Row> {
    let sim = s.sim;
    let owner = if s.mine() {
        sim.player_faction_name()
    } else {
        s.owner().filter(|_| s.town().is_some()).and_then(|f| sim.data.get(&f)).map_or(String::new(), |r| r.name.clone())
    };
    vec![line(format!("{DARK_RED}{}", s.rec.name), format!("{DARK_RED}{owner}"))]
}

/// @fn F023168
///
/// The town a building is in (Building::getTown: a town's its town; one of the player's the
/// town nearest it within that town's radius, F048806, the player's own outpost among them),
/// none for the manager's NO_TOWN ("Nowhere") a building in no town has.
fn town_of(s: &Site) -> Option<String> {
    if s.pb().is_some() && s.sim.in_outpost(s.at) {
        return Some(s.sim.outpost_name.clone());
    }
    s.town().map(|t| town_name(s.sim, t))
}

/// @fn F048836
///
/// A town's name as shown: its name once explored, else its "unexplored name" (its name with
/// none).
fn town_name(sim: &Sim, t: usize) -> String {
    let town = &sim.towns[t];
    if sim.world.explored.contains(&town.record) {
        return town.name.clone();
    }
    sim.data.get(&town.record).and_then(|r| r.strings.get("unexplored name")).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| town.name.clone())
}

/// @fn F030573
///
/// Mode 3 (StateText) of a plain Building: "Type:" with the record's name when the building is
/// called otherwise (never here: the player's buildings go by their records' names), "Town:"
/// with its town (NO_TOWN's "Nowhere" for one in none), then its upgrade lines (F030575,
/// vtable +0x3a0) and its buttons (F030563, +0x3a8).
fn info_plain(s: &Site) -> Vec<Row> {
    let mut rows = vec![line("Town:", town_of(s).unwrap_or_else(|| "Nowhere".into()))];
    rows.extend(upgrades_and_buttons(s));
    rows
}

/// @fn F030567
///
/// Mode 3 of the machines (ProductionBuilding, ResearchBuilding F030583): "Town:" with its
/// town unless it is in none (NO_TOWN, F048775), then its upgrade lines and buttons.
fn info_town_and_buttons(s: &Site) -> Vec<Row> {
    let mut rows = Vec::new();
    if let Some(t) = town_of(s) {
        rows.push(line("Town:", t));
    }
    rows.extend(upgrades_and_buttons(s));
    rows
}

/// @fn F030564
///
/// Mode 3 of a crafting bench: "Town:", then "Crafting:" (#492620) with its queue's first item
/// (#afa68b) or "Nothing Queued" with the tooltip "Nothing has been ordered for crafting.",
/// then its upgrade lines and buttons.
fn info_crafting(s: &Site) -> Vec<Row> {
    let sim = s.sim;
    let mut rows = Vec::new();
    if let Some(t) = town_of(s) {
        rows.push(line("Town:", t));
    }
    match s.pb().and_then(|pb| pb.craft_queue.first()).and_then(|i| sim.data.get(i)) {
        Some(item) => rows.push(line(format!("{DARK_RED}Crafting:"), format!("{TAN}{}", item.name))),
        None => rows.push(tipped(format!("{DARK_RED}Crafting:"), format!("{TAN}Nothing Queued"), "Nothing has been ordered for crafting.")),
    }
    rows.extend(upgrades_and_buttons(s));
    rows
}

/// @fn F030577
///
/// Mode 3 of UseableStuff (storage, beds, training, lights): "Town:" unless in none; its
/// inventory empty, "[EMPTY]" (#492620, the caption upper-cased by the locale, F024479);
/// training equipment (BF_TRAINING) "Training:" with its skill (F047078); "Cost to use:" for a
/// bed (BF_BED, BF_SKELETON_BED) of a faction not the player's (F030720: "(Pay c.n)" or
/// "Private", #492620); for one with a lock, "Lock state:" "Unlocked" (#a8b774) or "Locked"
/// (#492620) and "Lock complexity:" "Level n" ("-" at none); then its upgrade lines and
/// buttons.
fn info_useable(s: &Site, m: &Machine) -> Vec<Row> {
    let sim = s.sim;
    let mut rows = Vec::new();
    if let Some(t) = town_of(s) {
        rows.push(line("Town:", t));
    }
    let inventory = match s.place {
        Place::Player(_) => s.pb().and_then(|pb| pb.storage.as_ref()),
        Place::Container(k) => sim.boxes.get(k).map(|c| &c.inventory),
        _ => None,
    };
    let empty = inventory.is_some_and(|inv| inv.sections.iter().all(|s| s.items.is_empty()));
    if empty {
        rows.push(line(format!("{DARK_RED}[EMPTY]"), ""));
    }
    if m.function == kenshi_game::production::FUNCTION_TRAINING {
        let stat = m.func.and_then(|f| f.ints.get("stat used").copied()).unwrap_or(0);
        rows.push(line(format!("Training: {}", stat_display_name(stat)), ""));
    }
    if matches!(m.function, kenshi_game::sim::BF_BED | 0x19) && !s.mine() {
        let cost = bed_cost(s, m.function);
        let shown = if cost < 0 { format!("{DARK_RED}Private") } else { format!("{DARK_RED}(Pay c.{})", num(cost as i64)) };
        rows.push(line("Cost to use:", shown));
    }
    if m.rec.bools.get("has lock").copied().unwrap_or(false) && m.function != 8 {
        // (stand-in) the player's storage keeps no lock of its own (no container locks in the
        // sim): unlocked, its level the record's "lock level bonus"; the level is left out for
        // the player's own empty storage, as F030577 does
        rows.push(line("Lock state:", format!("{GREEN}Unlocked")));
        if !empty {
            let level = m.rec.ints.get("lock level bonus").copied().unwrap_or(0).clamp(0, 100);
            rows.push(line("Lock complexity:", if level < 1 { "-".to_string() } else { format!("Level {}", num(level as i64)) }));
        }
    }
    rows.extend(upgrades_and_buttons(s));
    rows
}

/// @fn F030720
///
/// What a town's bed costs the player's first selected character for a night: nothing for one
/// in no building, of no owner, of a faction allied with the character's, or in no town or a
/// town of type 0, 4 or 9; else the building's residents' "bed usage cost" (four times for a
/// BF_SKELETON_BED, none under 0) when its beds are public (the residents' "public beds", or
/// a bar's: "building designation" 3), else -1 ("Private").
/// (stand-in) The residents are the building's state's "residents template"; F03bd0e's
/// further say on the character's using it is taken as yes, and the owner's faction type 8
/// test (Faction +0x34) is not made: the sim keeps no faction types.
fn bed_cost(s: &Site, function: i32) -> i32 {
    let sim = s.sim;
    let Some(p) = s.placement() else { return 0 };
    let Some(owner) = s.owner() else { return 0 };
    let me = sim.folk.iter().find(|q| q.player && !q.dead).and_then(|q| q.faction.clone()).unwrap_or_else(|| kenshi_game::sim::PLAYER_FACTION.to_string());
    if sim.relations.get(&owner).is_some_and(|r| r.is_ally(Some(&me))) {
        return 0;
    }
    let Some(t) = s.town() else { return 0 };
    if matches!(sim.towns[t].kind, 0 | 4 | 9) {
        return 0;
    }
    let Some(res) = sim.placed.get(p).and_then(|pl| pl.residents.as_ref()).and_then(|r| sim.data.get(r)) else { return -1 };
    let bar = res.ints.get("building designation").copied() == Some(3);
    if !bar && !res.bools.get("public beds").copied().unwrap_or(false) {
        return -1;
    }
    let mut cost = res.ints.get("bed usage cost").copied().unwrap_or(0);
    if function == 0x19 {
        cost *= 4;
    }
    cost.max(0)
}

/// @fn F029378
///
/// Whether a building of the player's can be dismantled: always, but for one standing in a
/// town not the player's whose record "creates player town".
fn can_dismantle(s: &Site) -> bool {
    let sim = s.sim;
    if !s.mine() {
        return false;
    }
    if sim.in_outpost(s.at) {
        return true;
    }
    let foreign = sim.towns.iter().any(|t| t.contains(s.at, 1.0) && t.faction.as_deref() != Some(kenshi_game::sim::PLAYER_FACTION));
    !(foreign && s.rec.bools.get("creates player town").copied().unwrap_or(false))
}

/// @fn F030563
///
/// Mode 3's last lines: the class's upgrade lines (vtable +0x3a0: F030575, a wall's F030565)
/// and its buttons (+0x3a8: F030563, a wall's F030566 the same).
fn upgrades_and_buttons(s: &Site) -> Vec<Row> {
    let mut rows = if class_of(&s.sim.data, s.rec) == Class::Wall { wall_upgrade_lines(s) } else { upgrade_lines(s) };
    rows.extend(buttons(s));
    rows
}

/// The upgrade tooltip's cost lines: each material the record it upgrades to needs beyond what
/// is in (F037721's copy of its construction made for that record, F037832), "      name"
/// (#afa68b) "+n".
fn upgrade_costs(pb: &kenshi_game::base::PlayerBuilding, sim: &Sim, target: &GameData) -> Vec<String> {
    let had = kenshi_game::upgrade::materials_in(pb);
    let need = kenshi_game::base::PlayerBuilding::new(target, [0.0; 3], 0.0).materials;
    need.iter()
        .filter_map(|(m, n)| {
            let have = had.iter().filter(|o| o.0 == *m).map(|o| o.1).sum::<u32>();
            let more = *n as i64 - have as i64;
            let name = sim.data.get(m).map_or(m.clone(), |r| r.name.clone());
            (more > 0).then(|| format!("      {name}\t+{}\t{TAN}/{TIP_TEXT}", num(more)))
        })
        .collect()
}

/// @fn F030575
///
/// The upgrade lines (mode 3). With a record to upgrade to (F029376): "Upgrade" (#a8b774) on
/// a button the line wide, "[UPGRADE]" (#a8b774, the caption upper-cased by the locale,
/// F024479), upgrading it (F029413); its tooltip "Upgrades To:" the record's name, the cost
/// lines, and for a power user (vtable +0x300: UseableStuff and its kin) "Power" with the
/// change in "power output" (#59231a under 0, else #a8b774 "+n"). With a record to downgrade
/// to (F029415) while it is being built: "Downgrade" (#afa68b) "[DOWNGRADE]", downgrading it
/// (F029414); its tooltip "Downgrade To:" that record's name.
fn upgrade_lines(s: &Site) -> Vec<Row> {
    let sim = s.sim;
    let (Some(b), Some(pb)) = (s.player(), s.pb()) else { return vec![] };
    let mut rows = Vec::new();
    if let Some(target) = sim.upgrade_target(b) {
        let mut tip = vec![pair("Upgrades To:", &target.name)];
        tip.extend(upgrade_costs(pb, sim, target));
        if !matches!(class_of(&sim.data, s.rec), Class::Plain | Class::Gateway | Class::Wall) {
            let power = |r: &GameData| r.ints.get("power output").copied().unwrap_or(0);
            let change = power(target) - power(s.rec);
            if change != 0 {
                let shown = if change < 0 { format!("{BROWN}{}", num(change as i64)) } else { format!("{GREEN}+{}", num(change as i64)) };
                tip.push(pair("Power", &shown));
            }
        }
        rows.push(Row { tip: tip.join("\n"), ..button(format!("{GREEN}Upgrade"), format!("{GREEN}[UPGRADE]"), UPGRADE, 1.0) });
    }
    if !pb.done
        && let Some(source) = sim.downgrade_target(b)
    {
        let tip = pair("Downgrade To:", &source.name);
        rows.push(Row { tip, ..button(format!("{TAN}Downgrade"), format!("{TAN}[DOWNGRADE]"), DOWNGRADE, 1.0) });
    }
    rows
}

/// @fn F030565
///
/// A wall's upgrade line: with a record to upgrade to (F029376), "Upgradeable to:" (#a8b774) on
/// a button the line wide, "[UPGRADE]" (#a8b774), upgrading it (F029413); its tooltip
/// "Upgrades To:" the record's name and the cost lines. No downgrade, no power.
fn wall_upgrade_lines(s: &Site) -> Vec<Row> {
    let sim = s.sim;
    let (Some(b), Some(pb)) = (s.player(), s.pb()) else { return vec![] };
    let Some(target) = sim.upgrade_target(b) else { return vec![] };
    let mut tip = vec![pair("Upgrades To:", &target.name)];
    tip.extend(upgrade_costs(pb, sim, target));
    vec![Row { tip: tip.join("\n"), ..button(format!("{GREEN}Upgradeable to:"), format!("{GREEN}[UPGRADE]"), UPGRADE, 1.0) }]
}

/// The building's buttons, while it can be dismantled (F029378): "Dismantle" (in #bf4834, the
/// whole line; it asks first, F037762) and while it is being built "PAUSE CONSTRUCTION"
/// (#afa68b), "[PAUSED]" (#777777) while paused, which turns the pause (F037657).
fn buttons(s: &Site) -> Vec<Row> {
    let mut rows = Vec::new();
    let Some(pb) = s.pb().filter(|_| can_dismantle(s)) else { return rows };
    rows.push(button("Dismantle", format!("{RED}Dismantle"), DISMANTLE, 1.0));
    if !pb.done {
        let caption = if pb.paused { format!("{GREY}[PAUSED]") } else { format!("{TAN}PAUSE CONSTRUCTION") };
        rows.push(button("PAUSE CONSTRUCTION", caption, PAUSE, 1.0));
    }
    rows
}

/// A farm as FarmBuilding keeps it (F022915: its "farm data"), read off the sim's farm.
struct Crop {
    /// Growth (+0x518: under 1 growing, 1 to 2 ripe, over 2 rotting), from +0x524 (0).
    stage: f32,
    /// How near death (+0x51c) against its "death threshold" (+0x54c).
    death: f32,
    threshold: f32,
    /// "growth time" (+0x53c), "harvest time" (+0x540), "death time" (+0x544), "drought death
    /// time" (+0x548), "drought multiplier" (+0x550) hours and factors.
    growth: f32,
    harvest: f32,
    death_time: f32,
    drought_death: f32,
    drought: f32,
    /// The ground's fertility for the crop (+0x554 with the biome's fit, F022899 / F022900).
    fertility: f32,
    /// No "fertility effect" (+0x558: an indoor farm).
    indoors: bool,
    /// Its plants (+0x4f0) and those harvested (+0x528).
    plants: u32,
    harvested: u32,
    /// The production state as the farm's update keeps it (F022906's +0x468): ripe and alive
    /// with plants left, 1 while its output stays full else 2; dead 2; growing 3 watered, 0 dry.
    state: i32,
    /// Its water tank has water (FarmBuilding +0x488's first).
    has_water: bool,
    /// The farmers' average farming skill (0 .. 1), none working it.
    skill: Option<f32>,
}

/// (stand-in) The sim grows no town's crops: a town's farm is shown just planted, dry, with
/// nobody farming it.
fn farm(s: &Site) -> Crop {
    let sim = s.sim;
    let fd = s.rec.refs("farm data").first().and_then(|(id, _)| sim.data.get(id));
    let f = |k: &str| fd.map_or(0.0, |d| float(d, k));
    let farmers: Vec<f32> = s.player().map_or(vec![], |b| sim.operator_ids(b)).into_iter().map(|id| sim.arena.f[id].level(kenshi_char::StatsEnumerated::StatFarming) * 0.01).collect();
    let none = kenshi_game::base::PlayerBuilding::new(s.rec, s.at, 0.0);
    let pb = s.pb().unwrap_or(&none);
    let plants = fd.map_or(0.0, |d| float(d, "amount")).max(0.0) as u32;
    let threshold = fd.map_or(0.0, |d| float(d, "death threshold"));
    let harvested = if pb.stage >= 1.0 { (pb.work.clamp(0.0, 1.0) * plants as f32) as u32 } else { 0 };
    let state = if pb.stage >= 1.0 && pb.death < threshold && harvested < plants {
        if pb.full { 1 } else { 2 }
    } else if pb.death > threshold {
        2
    } else if pb.water > 0.0 {
        3
    } else {
        0
    };
    Crop {
        stage: pb.stage,
        death: pb.death,
        threshold: f("death threshold"),
        growth: f("growth time"),
        harvest: f("harvest time"),
        death_time: f("death time"),
        drought_death: f("drought death time"),
        drought: f("drought multiplier"),
        fertility: s.player().map_or(0.0, |b| sim.farm_fertility(b)),
        indoors: f("fertility effect") == 0.0,
        plants,
        // (stand-in) Kenshi's farmers take the plants one by one (F022907's +0x528); the
        // sim's take the whole crop at once when the harvest work is done, so those taken
        // are the work's share of the plants
        harvested,
        state,
        has_water: pb.water > 0.0,
        skill: (!farmers.is_empty()).then(|| farmers.iter().sum::<f32>() / farmers.len() as f32),
    }
}

impl Crop {
    /// @fn F022891
    ///
    /// The crop yield at farming skill `skill` (0 .. 1): the ground's fertility for the crop by
    /// lerp(skill, 0.5, 2) held in 0 .. 1.
    fn crop_yield(&self, skill: f32) -> f32 {
        (0.5 + 1.5 * skill).clamp(0.0, 1.0) * self.fertility
    }
}

/// @fn F022893
///
/// A farm's status, built: ripe or dying, "Dead" (not ripe, or past its death threshold: "Your
/// crops are dead. Clear them out to start again"), "Output full" (#59231a), "Harvesting"
/// ("Working...") with farmers at it or none needed, else "Ready To harvest" (#59231a);
/// growing on fertile ground, "Needs water" (#59231a, the tank dry: "Out of water. Growth rate
/// will be significantly reduced and your plants may die.") or "Growing" ("Your crops are
/// growing nicely. You don't have to do anything."); else "Infertile" (#59231a). Broken or
/// being built, the machine's.
fn status_farm(m: &Machine, f: &Crop) -> (String, String) {
    if m.broken || !m.done {
        return status_production(m);
    }
    if f.stage >= 1.0 || f.threshold <= f.death {
        if f.stage < 1.0 || f.threshold <= f.death {
            return ("Dead".into(), "Your crops are dead. Clear them out to start again".into());
        }
        if f.state == 1 {
            return (format!("{BROWN}Output full"), "The output is full. Empty out the items and put them in a storage box or something.".into());
        }
        if m.max_ops < 1 || m.ops != 0 {
            return ("Harvesting".into(), "Working...".into());
        }
        return (format!("{BROWN}Ready To harvest"), "Nobody is working at this farm at the present moment, you need to hurry up and harvest them before they die.".into());
    }
    if f.fertility > 0.01 {
        if f.state == 0 {
            return (format!("{BROWN}Needs water"), "Out of water. Growth rate will be significantly reduced and your plants may die.".into());
        }
        return ("Growing".into(), "Your crops are growing nicely. You don't have to do anything.".into());
    }
    (format!("{BROWN}Infertile"), "The soil here is unsuitable for growing crops. This farm is useless.".into())
}

/// @fn F022895
///
/// A built farm's mode 4 bars: "Growing" (its growth from where it began, held in -0.5 .. 1,
/// "n%"; "--" on infertile ground), "Plant Condition" (1 - death / threshold), "Number of
/// Plants" (those left of its plants), "Time left to harvest" while ripe and alive: the hours
/// left ((threshold - 1 once rotting) x death time + harvest time x what is left of the ripe
/// time) against the whole, none for a drought-killable crop with its tank dry.
fn farm_bars(f: &Crop) -> Vec<Bar> {
    let mut bars = Vec::new();
    let grown = f.stage.clamp(-0.5, 1.0);
    let shown = if f.fertility <= 0.01 { "--".to_string() } else { format!("{}%", num((grown * 100.0).round() as i64)) };
    bars.push(bar("Growing", shown, grown, "Once the plants have grown 100% you can begin to harvest them. Until then all you have to do is make sure the farm has water.".into()));
    let condition = if f.threshold > 0.0 { (1.0 - f.death / f.threshold).clamp(0.0, 1.0) } else { 0.0 };
    bars.push(bar(
        "Plant Condition",
        format!("{}%", num((condition * 100.0).round() as i64)),
        condition,
        "The health of the plants, it will start to degenerate once they are fully grown, or if they run out of water. If it reaches 0 then the dead plants will have to be cleared and re-planted.".into(),
    ));
    let left = f.plants.saturating_sub(f.harvested);
    let share = if f.plants > 0 { left as f32 / f.plants as f32 } else { 0.0 };
    bars.push(bar("Number of Plants", num(left as i64), share, "The number of plants remaining. This number will decrease when you harvest them, or if some pest starts eating them.".into()));
    if f.stage <= 1.0 || f.threshold <= f.death {
        bars.push(bar("Time left to harvest", "", 0.0, String::new()));
    } else {
        let rotting = if f.stage < 2.0 { 0.0 } else { 1.0 };
        let hours = (f.threshold - rotting) * f.death_time + f.harvest * (2.0 - f.stage).max(0.0);
        let whole = f.death_time * f.threshold + f.harvest;
        let share = if f.drought_death <= 0.0 || f.has_water { if whole > 0.0 { hours / whole } else { 0.0 } } else { 0.0 };
        let h = hours.round() as i64;
        let unit = if h == 1 { "hr" } else { "hrs" };
        bars.push(bar("Time left to harvest", format!("{} {unit}", num(h)), share, "Time left to harvest all plants before they go bad.".into()));
    }
    bars
}

/// @fn F022894
///
/// A farm's mode 6 crop lines. "Crop Yield" (#afa68b): the yield at its farmers' average skill
/// (at 1 with none), n% (#59231a under 0.3); its tooltip "Environment:" (the yield at skill 1),
/// "Farmers Skill:" ("--" in #444444 with none), the yield's text and under 0.15 its warning.
/// "Growth Rate" (#afa68b): the fertility by the power factor (rated) and the drought
/// multiplier (dry), n% (#59231a under 0.3); its tooltip "Normal Growth Time:", "Environment
/// Effect:" (outdoors), "Power:" (rated), "No Water:" (dry) and "Actual Growth Time:".
fn crop_lines(m: &Machine, f: &Crop) -> Vec<Row> {
    let yields = f.crop_yield(f.skill.unwrap_or(1.0));
    let low = |v: f32| if v < 0.3 { BROWN } else { "" };
    let mut tip = vec![pair("Environment:", &format!("{}%", num((f.crop_yield(1.0) * 100.0) as i64)))];
    tip.push(match f.skill {
        None => pair("Farmers Skill:", "#444444--"),
        Some(k) => pair("Farmers Skill:", &format!("{}%", num(((0.5 + 1.5 * k).clamp(0.0, 1.0) * 100.0) as i64))),
    });
    let mut text = "This is the percentage yield of the farm. A lower number means that you get less crops from a harvest. Skillful farmers are important.".to_string();
    if yields < 0.15 {
        text.push_str(&format!("{BROWN}\nWARNING! Crop yield is too low to be productive, probably the crop is not suitable for this area."));
    }
    tip.push(text);
    let crop = tipped(format!("{TAN}Crop Yield"), format!("{}{}%", low(yields), num((yields * 100.0) as i64)), tip.join("\n"));
    let power = if m.rating != 0.0 { m.power_factor() } else { 1.0 };
    let dry = if f.state == 0 { f.drought } else { 1.0 };
    let rate = f.fertility * power * dry;
    let mut tip = vec![pair("Normal Growth Time:", &{
        let h = f.growth as i64;
        format!("{} {}", num(h), if h == 1 { "hr" } else { "hrs" })
    })];
    if !f.indoors {
        tip.push(pair("Environment Effect:", &format!("{}{}%", low(f.fertility), num((f.fertility * 100.0) as i64))));
    }
    if m.rating != 0.0 {
        tip.push(if power >= 1.0 { pair("Power:", &format!("{GREEN}OK")) } else { pair("Power:", &format!("{BROWN}-{}%", num(((1.0 - power) * 100.0) as i64))) });
    }
    if f.state == 0 {
        tip.push(pair("No Water:", &format!("{BROWN}{}%", num(((f.drought - 1.0) * 100.0) as i64))));
    }
    tip.push(if rate == 0.0 {
        pair("Actual Growth Time:", "-")
    } else {
        let h = (f.growth / rate) as i64;
        pair("Actual Growth Time:", &format!("{} {}", num(h), if h == 1 { "hr" } else { "hrs" }))
    });
    let growth = tipped(format!("{TAN}Growth Rate"), format!("{}{}%", low(rate), num((rate * 100.0) as i64)), tip.join("\n"));
    vec![crop, growth]
}

/// @fn F022896
///
/// An indoor farm's (no fertility effect) "Efficiency:": the power factor x 100 x its max
/// efficiency (x the drought multiplier when dry), #140806 at 90 or more else #59231a; its
/// tooltip "Damage:", "Power:" (consumer), "No Water:" (dry) and "Total:".
fn farm_production(m: &Machine, f: &Crop) -> Vec<Row> {
    if !f.indoors {
        return vec![];
    }
    let pf = m.power_factor() * 100.0;
    let mut total = m.max_efficiency() * pf;
    if f.state == 0 {
        total *= f.drought;
    }
    let colour = if pf >= 90.0 { TIP_TEXT } else { BROWN };
    let mut tip = Vec::new();
    let max = m.max_efficiency();
    tip.push(if m.share < 1.0 { pair("Damage:", &format!("{BROWN}-{}%", num(((1.0 - m.share) * max * 100.0) as i64))) } else { pair("Damage:", &format!("{GREEN}OK")) });
    if m.rating < 0.0 {
        let p = if m.need > 0.0 { m.power_in / m.need } else { 1.0 };
        tip.push(if p >= 1.0 { pair("Power:", &format!("{GREEN}OK")) } else { pair("Power:", &format!("{BROWN}-{}%", num(((1.0 - p) * max * 100.0) as i64))) });
    }
    if f.state == 0 {
        tip.push(pair("No Water:", &format!("{BROWN}{}%", num(((f.drought - 1.0) * 100.0) as i64))));
    }
    tip.push(pair("Total:", &format!("{TAN}{}%", num(total as i64))));
    vec![tipped(format!("{TAN}Efficiency:"), format!("{colour}{}%", num(total as i64)), tip.join("\n"))]
}

/// The arena's turret of a building: one of the player's, or a town's furniture.
fn turret_of<'a>(s: &Site<'a>) -> Option<&'a kenshi_combat::ranged::Turret> {
    s.sim.arena.turrets.iter().find(|t| match s.place {
        Place::Player(b) => t.building == Some(b),
        Place::Furniture(f) => t.furniture == Some(f),
        _ => false,
    })
}

/// @fn F030590
///
/// Mode 3 of a turret: "Town:" unless in none; "Height:" "n m", its height over the last
/// target it aimed at (that target's height, else the ground's at it: here its own place) x
/// 0.1 ("The turrets height above the last target we aimed at."); "Height bonus:" "+n%", the
/// height factor (F033824) less 1 x 100, its tooltip the bonus's text with the height in
/// #492620 (the penalty's under a factor below 0, never); then its upgrade lines and buttons.
fn info_turret(s: &Site) -> Vec<Row> {
    let sim = s.sim;
    let mut rows = Vec::new();
    if let Some(t) = town_of(s) {
        rows.push(line("Town:", t));
    }
    let y = s.at[1];
    let target_y = turret_of(s).and_then(|t| t.target).and_then(|q| sim.folk.get(q)).map(|q| q.mover.position_units()[1]);
    let factor = kenshi_combat::ranged::height_factor(y, target_y.unwrap_or(0.0));
    let height = format!("{} m", num(((y - target_y.unwrap_or(y)) * 0.1) as i64));
    rows.push(tipped("Height:", height.clone(), "The turrets height above the last target we aimed at."));
    let shown = format!("{DARK_RED}{height}{TIP_TEXT}");
    let tip = if factor >= 0.0 {
        format!("The additional bonus to range, speed and power that resulted from having a height difference of {shown} above the last target.")
    } else {
        format!("The additional penalty to range, speed and power that resulted from having a height difference of {shown} below the last target.")
    };
    rows.push(tipped("Height bonus:", format!("+{}%", num(((factor - 1.0) * 100.0) as i64)), tip));
    rows.extend(upgrades_and_buttons(s));
    rows
}

/// @fn F030562
///
/// A turret's status: "Broken"; "Under construction"; "Reloading..." while it reloads (its
/// gun out of shots with a reload begun); "Unused" with nobody manning it; "Tracking" while
/// its gunner aims at a target ((assumption) TurretBuilding +0x464 as the gun's aim timer with
/// a target), else "Ready".
fn status_turret(s: &Site, m: &Machine) -> (String, String) {
    if m.broken {
        return ("Broken".into(), "This machine has broken down as a result of damage. Send one of your engineers to repair it.".into());
    }
    if !m.done {
        return ("Under construction".into(), "This machine is currently under construction.".into());
    }
    let t = turret_of(s);
    let reloading = t.is_some_and(|t| !(t.ranged.progress <= 0.0 || t.gun.shots != 0));
    if reloading {
        return ("Reloading...".into(), "Turret is currently reloading.".into());
    }
    if m.ops == 0 {
        return ("Unused".into(), "Nobody is manning this turret.".into());
    }
    if t.is_some_and(|t| t.target.is_some() && t.ranged.aim > 0.0) {
        return ("Tracking".into(), "Gunner is tracking a target, about to take a shot.".into());
    }
    ("Ready".into(), "Turret is ready to fire.".into())
}

/// @fn F030592
///
/// The turret gun's mode 4 bar: "Loaded" "shots/num shots", its share (shots + 1 - the reload's
/// progress) / num shots; its tooltip "Base Reload Speed:" (min-max seconds), "User Skill:" (x
/// 100) and with a gunner "User Reload Speed:" (F033825 at the skill: "n second(s)").
fn loaded_bar(s: &Site) -> Vec<Bar> {
    let Some(t) = turret_of(s) else { return vec![] };
    let g = &t.gun;
    let n = g.num_shots.max(1) as f32;
    let share = (g.shots as f32 + 1.0 - t.ranged.progress) / n;
    let mut tip = vec![
        pair("Base Reload Speed:", &format!("{:.2}-{:.2} seconds", g.reload_min, g.reload_max)),
        pair("User Skill:", &num((g.skill * 100.0) as i64)),
    ];
    if t.operator.is_some() {
        let r = g.reload_time(g.skill);
        let unit = if r as i32 == 1 { "second" } else { "seconds" };
        tip.push(pair("User Reload Speed:", &format!("{r:.2} {unit}")));
    }
    vec![bar("Loaded", format!("{}/{}", num(g.shots as i64), num(g.num_shots as i64)), share, tip.join("\n"))]
}

/// @fn F030561
///
/// The status (the medical panel's status line and its tooltip, F042971): "Broken" for a
/// machine broken down by damage, "Under construction" while being built, else "Normal".
fn status_plain(done: bool, broken: bool) -> (String, String) {
    if broken {
        return ("Broken".into(), "This machine has broken down as a result of damage. Send one of your engineers to repair it.".into());
    }
    if !done {
        return ("Under construction".into(), "This machine is currently under construction.".into());
    }
    ("Normal".into(), "Building is in good condition.".into())
}

/// @fn F030568
///
/// A machine's status, the first that holds (all but "Under construction" and "Working" in
/// #59231a): "Broken"; "Under construction"; "Switched off" (rated, switched off); "No power"
/// (F029341: no power reaching it); "Production queue empty" (state 3); "Out of resource"
/// (state 0 and an ingredient out, F029332); "Output full" (state 1); "No power" (asking for
/// power and none in); "No workers" (manned, none at it); else "Working" with F030582's text.
fn status_production(m: &Machine) -> (String, String) {
    let bad = |s: &str, t: &str| (format!("{BROWN}{s}"), t.to_string());
    if m.broken {
        return bad("Broken", "This machine has broken down as a result of damage. Send one of your engineers to repair it.");
    }
    if !m.done {
        return ("Under construction".into(), "This machine is currently under construction.".into());
    }
    if !m.switched_on && m.rating != 0.0 {
        return bad("Switched off", "This machine is switched off.");
    }
    if !m.has_power {
        return bad("No power", "This machine needs electricity to function. You either need to build generators, or you are out of fuel / there is no wind.");
    }
    if m.state == 3 {
        return bad("Production queue empty", "No items have been queued up for crafting, so there is nothing to do.");
    }
    if m.state == 0 && m.out_of_resource {
        return bad("Out of resource", "The machine has run out of raw materials to work with. Find some more and fill it up.");
    }
    if m.state == 1 {
        return bad("Output full", "The machine output is full. Empty out the items and put them in a storage box or something.");
    }
    if m.need > 0.0 && m.power_in <= 0.0 {
        return bad("No power", "This machine needs electricity to function. Build a power generator or windmill.");
    }
    if m.max_ops > 0 && m.ops == 0 {
        return bad("No workers", "Nobody is working at this machine at the present moment, so nothing is getting done.");
    }
    ("Working".into(), working_text(m))
}

/// @fn F030582
///
/// "Working"'s text: a consumer getting under 0.9 of the power it asks for, or a generator
/// giving under 0.9 of its rating, "This machine is working fine, but due to low power is only
/// operating at n% of its full potential." (n in #492620), else "This machine is working fine."
fn working_text(m: &Machine) -> String {
    let ratio = if m.rating <= 0.0 {
        (m.need > 0.0).then(|| m.power_in / m.need)
    } else {
        Some(m.output / m.rating)
    };
    match ratio {
        Some(r) if r <= 0.9 => format!("This machine is working fine, but due to low power is only operating at {DARK_RED}{}% {TIP_TEXT}of its full potential.", num((r * 100.0) as i64)),
        _ => "This machine is working fine.".into(),
    }
}

/// @fn F030569
///
/// A generator's status: the machine's (F030568), then over it "Out of fuel" (#59231a) when out
/// of fuel, and for a battery (switched on, with a capacity) in a town: "Fully Charged"
/// (#a8b774) while full, "Empty" (#59231a) at none, "Charging" (#a8b774) while the town's power
/// charges it, "Draining" while drained, else "Stored".
fn status_generator(site: &Site, m: &Machine) -> (String, String) {
    let mut s = status_production(m);
    if m.out_of_resource {
        s = (format!("{BROWN}Out of fuel"), "It has ran out of fuel. Find some more fuel and fill it up.".into());
    }
    if m.switched_on && m.capacity_rec > 0.0 && town_of(site).is_some() {
        s = if m.stored >= m.capacity {
            (format!("{GREEN}Fully Charged"), "This battery is fully charged.".into())
        } else if m.stored <= 0.0 {
            (format!("{BROWN}Empty"), "This battery is flat, it needs to be charged up.".into())
        } else if m.power_in > 0.0 {
            (format!("{GREEN}Charging"), "This battery is being charged.".into())
        } else if m.power_out > 0.0 {
            ("Draining".into(), "This battery is in use and being drained of its power.".into())
        } else {
            ("Stored".into(), "This battery is not in use.".into())
        };
    }
    s
}

/// @fn F030570
///
/// A wind generator's status: the machine's, "No Wind" (#59231a) while its head turns with no
/// wind.
fn status_wind(sim: &Sim, m: &Machine) -> (String, String) {
    let s = status_production(m);
    if sim.wind > 0.0 {
        return s;
    }
    (format!("{BROWN}No Wind"), "There is currently not enough wind to drive the machine.".into())
}

/// @fn F030571
///
/// A rain collector's status: built, not broken and its output not full, "Collecting"
/// (#a8b774) in rain, else "Not Raining" (#59231a); otherwise the machine's.
fn status_rain(m: &Machine) -> (String, String) {
    if !m.broken && m.done && m.state != 1 {
        return if m.rain > 0.0 {
            (format!("{GREEN}Collecting"), "Rain is being collected.".into())
        } else {
            (format!("{BROWN}Not Raining"), "It is not raining. Nothing to collect.".into())
        };
    }
    status_production(m)
}

/// @fn F030572
///
/// A crafting bench's status: "Needs upgrading" (#492620) when its queue's first item is one
/// it cannot make (Building +0x4f1 with its working flag); otherwise the machine's.
fn status_crafting(s: &Site, m: &Machine) -> (String, String) {
    if let Some(b) = s.player()
        && let Some(item) = s.sim.buildings[b].craft_queue.first()
        && !m.broken
        && m.working
        && !s.sim.crafts(b).contains(item)
    {
        return (format!("{DARK_RED}Needs upgrading"), "You are trying to craft something that this craft bench cannot do. It needs upgrading to produce this item.".into());
    }
    status_production(m)
}

/// @fn F030574
///
/// Mode 4's bars: "Condition" (its construction, whole, against its materials' worth, F037662),
/// its tooltip by its state; while being built a bar for each material, "have/needed" with
/// what is there against what is needed, "All necessary materials are ready." once all are,
/// else how many more are wanted.
fn condition_bars(s: &Site) -> Vec<Bar> {
    let sim = s.sim;
    // a town's building stands whole: its construction its materials' worth
    let none = kenshi_game::base::PlayerBuilding::new(s.rec, s.at, 0.0);
    let whole = kenshi_game::base::PlayerBuilding { progress: none.total() as f32, used: none.total(), done: true, ..none };
    let pb = s.pb().unwrap_or(&whole);
    let total = pb.total() as f32;
    let share = if total > 0.0 { (pb.progress / total).min(1.0) } else { 1.0 };
    let tip = if !pb.done {
        format!("The building is under construction and at {}% completion.", num((share * 100.0) as i64))
    } else if share >= 1.0 {
        "The building is in good condition.".into()
    } else {
        "The building is damaged.".into()
    };
    let mut bars = vec![bar("Condition", format!("{}", pb.progress as i32), share, tip)];
    if !pb.done {
        // the materials are put in in their order: each has what of `used` it reached; one bar
        // a kind (Kenshi's construction keeps one entry a material)
        let mut left = pb.used;
        let mut kinds: Vec<(&String, u32, u32)> = Vec::new();
        for (mat, n) in &pb.materials {
            let have = left.min(*n);
            left -= have;
            match kinds.iter_mut().find(|k| k.0 == mat) {
                Some(k) => (k.1, k.2) = (k.1 + have, k.2 + n),
                None => kinds.push((mat, have, *n)),
            }
        }
        // those kept from the record before (an upgrade's), on the list with no need
        for (mat, have) in &pb.spare {
            match kinds.iter_mut().find(|k| k.0 == mat) {
                Some(k) => k.1 += have,
                None => kinds.push((mat, *have, 0)),
            }
        }
        for (mat, have, n) in kinds {
            let n = &n;
            let name = sim.data.get(mat).map_or(mat.clone(), |r| r.name.clone());
            let tip = if have >= *n {
                "All necessary materials are ready.".to_string()
            } else {
                format!("{DARK_RED}{}{TIP_TEXT} more 'Building Materials' will be needed to complete this construction.", num((*n - have) as i64))
            };
            let share = if *n > 0 { have as f32 / *n as f32 } else { 1.0 };
            bars.push(bar(name, format!("{}/{}", num(have as i64), num(*n as i64)), share, tip));
        }
    }
    bars
}

/// @fn F030581
///
/// Mode 4's power bars. A consumer (rated under 0, no battery): "Power" with what it takes in
/// against what it can use (F029296) and F030582's text; taking none, the bar's arrow says why:
/// "Inactive" (asking none: "Nobody is operating this machine at the moment, so it is not
/// consuming power."), "Switched Off" ("You have ordered for this machine to remain switched
/// off.") or "No Power!" ("No power is getting to this machine..."). A generator: "Power Output"
/// against its rating, its tooltip "Power is switched off." / "Currently out of fuel, so power
/// output is zero" / "Power output is running at reduced efficiency due to damage." / "Power
/// output is running at maximum.", then "Fuel Consumption" as a share ("Fuel is being burned
/// at n% speed..."). A battery: "Power In", "Power Out" and "Power Stored".
fn power_bars(m: &Machine) -> Vec<Bar> {
    let mut bars = Vec::new();
    let inv = if m.rating == 0.0 { 0.0 } else { 1.0 / m.rating };
    if m.capacity_rec <= 0.0 {
        if m.rating < 0.0 {
            let max_use = m.rating.abs();
            let n = num(m.power_in as i64);
            let share = if max_use > 0.0 { m.power_in / max_use } else { 0.0 };
            let with = |arrow: &str, tip: String| Bar { arrow: arrow.into(), ..bar("Power", n.clone(), share, tip) };
            if max_use <= 0.0 || m.power_in != 0.0 {
                bars.push(with(&n, working_text(m)));
            } else if m.need <= 0.0 {
                bars.push(with("Inactive", "Nobody is operating this machine at the moment, so it is not consuming power.".into()));
            } else if !m.switched_on {
                bars.push(with("Switched Off", "You have ordered for this machine to remain switched off.".into()));
            } else {
                bars.push(with("No Power!", "No power is getting to this machine. You need to build some generators, or fix or fuel your existing ones or something.".into()));
            }
        } else if m.rating > 0.0 {
            let tip = if !m.switched_on {
                "Power is switched off."
            } else if m.output <= m.rating - 1.0 {
                if m.out_of_resource { "Currently out of fuel, so power output is zero" } else { "Power output is running at reduced efficiency due to damage." }
            } else {
                "Power output is running at maximum."
            };
            bars.push(bar("Power Output", num(m.output as i64), m.output * inv, tip.into()));
            let fuel = m.fuel_use * 100.0;
            bars.push(bar("Fuel Consumption", num(fuel as i64), m.fuel_use, format!("Fuel is being burned at {}% speed. It will be used up more slowly when less power is being used", num(fuel as i64))));
        }
    } else {
        let inv = if m.rating == 0.0 { 0.0 } else { 1.0 / m.rating.abs() };
        bars.push(bar("Power In", num(m.power_in as i64), inv * m.power_in, "If the battery is charging this is the amount of power it is consuming.".into()));
        bars.push(bar("Power Out", num(m.power_out as i64), inv * m.power_out, "The power output when the battery is in use and being drained.".into()));
        let cap = m.capacity;
        bars.push(bar("Power Stored", num(m.stored as i64), if cap > 0.0 { m.stored / cap } else { 0.0 }, "The amount of power currently stored in the battery for use in emergencies.".into()));
    }
    bars
}

/// @fn F030586
///
/// A wind generator's mode 4 bars: "Wind" (the wind's share, n%: "The current amount of
/// wind."), then "Power Output" against its rating: "Power is switched off.", "Currently not
/// enough wind to generate any power" (none), "Power output is running at reduced efficiency
/// due to damage." (some but short), or "Power output is at maximum.".
fn wind_bars(sim: &Sim, m: &Machine) -> Vec<Bar> {
    let mut bars = Vec::new();
    if m.rating <= 0.0 {
        return bars;
    }
    let wind = sim.wind.clamp(0.0, 1.0);
    bars.push(bar("Wind", format!("{}%", num((wind * 100.0) as i64)), wind, "The current amount of wind.".into()));
    let tip = if !m.switched_on {
        "Power is switched off."
    } else if m.output <= m.rating - 1.0 {
        if wind > 0.0 { "Power output is running at reduced efficiency due to damage." } else { "Currently not enough wind to generate any power" }
    } else {
        "Power output is at maximum."
    };
    bars.push(bar("Power Output", num(m.output as i64), m.output / m.rating, tip.into()));
    bars
}

/// @fn F030586
///
/// A wind generator's mode 6: "Wind Speed" (#afa68b) with the wind at its place in mph (the
/// weather's wind x 0.223694, "{1,num=fix,p=1} mph": "The current wind speed").
fn wind_speed_line(sim: &Sim) -> Vec<Row> {
    let mph = sim.wind * 100.0 * 0.223694;
    vec![tipped(format!("{TAN}Wind Speed"), format!("{mph:.1} mph"), "The current wind speed")]
}

/// @fn F030581
///
/// Mode 6's power ticks, for a machine of the player's with a rating: a consumer's "Battery"
/// tick (run off the batteries, F029300) and every one's "Power" tick (switched on, F029299),
/// held down while on, with their tooltips.
fn power_ticks(m: &Machine) -> Vec<Row> {
    let mut rows = Vec::new();
    if m.rating == 0.0 {
        return rows;
    }
    if m.rating < 0.0 {
        let tip = if m.on_battery {
            "This machine will run off of battery power if the main generators should run low or break down."
        } else {
            "This machine will NOT run off of battery power if the main generators should run low or break down."
        };
        rows.push(tick("Battery", BATTERY, m.on_battery, tip));
    }
    let tip = if m.switched_on { "This machine is switched On".to_string() } else { format!("This machine is switched {DARK_RED}Off") };
    rows.push(tick("Power", POWER, m.switched_on, tip));
    rows
}

/// @fn F030584
///
/// Mode 6's resource line for a mine (BF_MINE, BF_MINE_NATURAL) with "world resource mining":
/// "Iron Quality:", "Stone Richness:", "Copper Quality:", "Carbon Quality:", "Water Available:"
/// or "Ground Fertility:" (#afa68b) with the resource x 100 coloured from red to green
/// (MyGUI Colour(0.8 - r, r x 0.8, 0.1), each held in 0 .. 1), its tooltip "The richness of
/// natural {resource} present in this location. Anything over 50% is considered good."
fn resource_line(m: &Machine) -> Vec<Row> {
    let Some(f) = m.func.filter(|_| matches!(m.function, 1 | 0x1b)) else { return vec![] };
    let kind = f.ints.get("world resource mining").copied().unwrap_or(0);
    let label = match kind {
        1 => "Iron Quality:",
        2 => "Stone Richness:",
        3 => "Copper Quality:",
        4 => "Carbon Quality:",
        5 => "Water Available:",
        6 => "Ground Fertility:",
        _ => "",
    };
    let r = m.resource;
    let green = (r * 0.8).clamp(0.0, 1.0);
    let red = (0.8 - r).clamp(0.0, 1.0);
    let hex = |v: f32| ((v * 255.0).round() as u32).min(255);
    let colour = format!("#{:02x}{:02x}{:02x}", hex(red), hex(green), hex(0.1));
    // F029e8d: the resource's name (MiningResource)
    let what = match kind {
        1 => "iron",
        2 => "stone",
        3 => "copper",
        4 => "carbon",
        5 => "water",
        6 => "fertility",
        _ => "",
    };
    vec![tipped(format!("{TAN}{label}"), format!("{colour}{}", num((r * 100.0) as i64)), format!("The richness of natural {what} present in this location. Anything over 50% is considered good."))]
}

/// @fn F030580
///
/// "Workers:" (#afa68b): "-" for a fully automatic machine ("This machine is fully
/// automatic."), else "n/max" (#59231a while some are wanted... #140806 when manned or
/// automatic), "The machine is under-staffed." (#492620) or "The machine is fully staffed.".
fn workers_line(m: &Machine) -> Row {
    let colour = if m.ops > 0 || m.max_ops < 1 { TIP_TEXT } else { BROWN };
    if m.max_ops == 0 {
        return tipped(format!("{TAN}Workers:"), format!("{colour}-"), "This machine is fully automatic.");
    }
    let value = format!("{colour}{}/{}", num(m.ops as i64), num(m.max_ops as i64));
    let tip = if (m.max_ops as usize) > m.ops {
        format!("{DARK_RED}The machine is under-staffed.")
    } else if m.max_ops as usize == m.ops {
        "The machine is fully staffed.".into()
    } else {
        String::new()
    };
    tipped(format!("{TAN}Workers:"), value, tip)
}

/// @fn F030579
///
/// Mode 6's production lines. "Efficiency:" (#afa68b): the power factor (F029260) x the
/// operators' share x 100 x the max efficiency (F037648), #140806 at 90 or more else #59231a;
/// its tooltip the class's efficiency lines (vtable +0x4e0), "Damage:" (OK, or the share the
/// construction lacks off), "Power:" for a consumer (OK, or the power short), "Num workers:" (OK,
/// or the operators short), and "Total:". "Max efficiency:" the max efficiency x 100, its
/// tooltip the class's lines and "Total:".
fn production_lines(m: &Machine, class: Class) -> Vec<Row> {
    let share = m.ops_share();
    let eff = m.power_factor() * share * 100.0;
    let max = m.max_efficiency();
    let colour = if eff >= 90.0 { TIP_TEXT } else { BROWN };
    let shown = (max * eff) as i64;
    let mut tip = efficiency_lines(m, class);
    // F029302 (the turret's taken as 1)
    let possible = if m.function != 0xc { m.max_eff } else { 1.0 };
    let short = |lack: f32| format!("{BROWN}-{}%", num((lack * possible * 100.0) as i64));
    tip.push(if m.share < 1.0 { pair("Damage:", &short(1.0 - m.share)) } else { pair("Damage:", &format!("{GREEN}OK")) });
    if m.rating < 0.0 {
        let p = if m.need > 0.0 { m.power_in / m.need } else { 1.0 };
        tip.push(if p >= 1.0 { pair("Power:", &format!("{GREEN}OK")) } else { pair("Power:", &short(1.0 - p)) });
    }
    tip.push(if m.max_ops.max(0) as usize == m.ops { pair("Num workers:", &format!("{GREEN}OK")) } else { pair("Num workers:", &short(1.0 - share)) });
    tip.push(pair("Total:", &format!("{TAN}{}%", num(shown))));
    let efficiency = tipped(format!("{TAN}Efficiency:"), format!("{colour}{}%", num(shown)), tip.join("\n"));
    let mut tip = efficiency_lines(m, class);
    tip.push(pair("Total:", &format!("{TAN}{}%", num((max * 100.0) as i64))));
    let max_line = tipped(format!("{TAN}Max efficiency:"), format!("{TIP_TEXT}{}%", num((max * 100.0) as i64)), tip.join("\n"));
    vec![efficiency, max_line]
}

/// The class's efficiency lines (vtable +0x4e0): "Max possible efficiency:" (F030578; a turret's
/// its max efficiency, F030591;
/// the resource's line besides for the machines and mines, F030585; the rain's for a rain
/// collector, F030587).
fn efficiency_lines(m: &Machine, class: Class) -> Vec<String> {
    match class {
        // F030587: "Rain strength:" n% (#afa68b, #59231a at none)
        Class::RainCollector => {
            let colour = if m.rain <= 0.0 { BROWN } else { TAN };
            vec![pair("Rain strength:", &format!("{colour}{}%", num((m.rain * 100.0) as i64)))]
        }
        // F030591
        Class::Turret => vec![pair("Max possible efficiency:", &format!("{TAN}{}%", num((m.max_efficiency() * 100.0) as i64)))],
        _ => {
            let mut lines = vec![pair("Max possible efficiency:", &format!("{TAN}{}%", num((m.max_eff * 100.0) as i64)))];
            // F030585: a mine's resource, "-n%" short of 1 (#59231a), "+n%" over (#140806)
            if matches!(class, Class::Production | Class::Generator | Class::WindGenerator | Class::Torture | Class::Crafting | Class::Furnace)
                && matches!(m.function, 1 | 0x1b)
                && let Some(f) = m.func
            {
                let label = match f.ints.get("world resource mining").copied().unwrap_or(0) {
                    1 => Some("Iron under ground:"),
                    2 => Some("Stone under ground:"),
                    3 => Some("Copper under ground:"),
                    4 => Some("Carbon under ground:"),
                    5 => Some("Water under ground:"),
                    6 => Some("Ground Fertility:"),
                    _ => None,
                };
                if let Some(label) = label {
                    let n = ((1.0 - m.resource) * m.max_eff * 100.0) as i64;
                    let colour = if m.resource < 1.0 { BROWN } else { TIP_TEXT };
                    let sign = if n < 0 { "+" } else { "" };
                    lines.push(pair(label, &format!("{colour}{sign}{}%", num(-n))));
                }
            }
            lines
        }
    }
}

/// Mode 6 of the worked machines (F030567 / F030583 / F030564): "Workers:" (F030580, vtable
/// +0x4d8) and the production lines (F030579, +0x4d0).
fn workers_and_production(m: &Machine, class: Class) -> Vec<Row> {
    let mut rows = vec![workers_line(m)];
    rows.extend(production_lines(m, class));
    rows
}

/// @fn F030577
///
/// Mode 6 of UseableStuff: a bed (BF_BED) "Healing bonus:" (#afa68b) "x" its "output rate";
/// else "Workers:" and, for training equipment, "Max Skill Level:" (#afa68b) its "output rate"
/// less 1 ("This is the maximum skill a person can train up to using this equipment. For
/// anyone higher it is a waste of time."), for the rest the production lines; then the power
/// ticks.
fn stats_useable(m: &Machine, class: Class) -> Vec<Row> {
    let rate = float(m.rec, "output rate");
    if m.function == 6 {
        return vec![line(format!("{TAN}Healing bonus:"), format!("x{}", fmt_num(rate)))];
    }
    let mut rows = vec![workers_line(m)];
    if m.function == kenshi_game::production::FUNCTION_TRAINING {
        rows.push(tipped(format!("{TAN}Max Skill Level:"), num(rate as i64 - 1), "This is the maximum skill a person can train up to using this equipment. For anyone higher it is a waste of time."));
    } else {
        rows.extend(production_lines(m, class));
    }
    rows.extend(power_ticks(m));
    rows
}

/// "{1,num}" of a float: whole numbers without decimals.
fn fmt_num(v: f32) -> String {
    if v.fract() == 0.0 { num(v as i64) } else { format!("{v}") }
}

/// @fn F030573
///
/// Mode 6 (StatsText): "Open to public:" (#afa68b): for another's building a line, "Public"
/// (#a8b774) or "Private" (#59231a) by Building::isPublic (F037699); for the player's own with an
/// interior a button 0.3 wide, "Public" (#a8b774) or "Private" (#bf4834), turning it (F037656).
/// Then for one for sale (F029379) "FOR SALE:" (#a8b774, upper-cased by the locale, F024479) on
/// a button 0.4 wide, its price "c.n" (#a8b774, F044617), buying it (F044618).
fn public_lines(s: &Site) -> Vec<Row> {
    let sim = s.sim;
    let mut rows = Vec::new();
    if let Some(pb) = s.pb() {
        if !s.rec.refs("interior").is_empty() {
            let caption = if pb.public { format!("{GREEN}Public") } else { format!("{RED}Private") };
            rows.push(button(format!("{TAN}Open to public:"), caption, PUBLIC, 0.3));
        }
    } else {
        let public = s.placement().is_none_or(|p| sim.building_public(p));
        let shown = if public { format!("{GREEN}Public") } else { format!("{BROWN}Private") };
        rows.push(line(format!("{TAN}Open to public:"), shown));
    }
    if for_sale(s) {
        rows.push(button(format!("{GREEN}FOR SALE:"), format!("{GREEN}c.{}", num(price(s) as i64)), BUY, 0.4));
    }
    rows
}

/// @fn F029379
///
/// Building::isForSale: a town's building (no part of another) with an interior, of no owner or
/// one not the player's that takes the player's faction for no enemy (F041496), in a town of
/// type 1, 2 or 3, with no residents or residents whose template says "sell home".
/// (stand-in) The residents are the building's state's "residents template"; the interior's
/// own +0x2c flag is not kept (taken as clear).
fn for_sale(s: &Site) -> bool {
    let sim = s.sim;
    let Place::Town(p) = s.place else { return false };
    if s.rec.refs("interior").is_empty() || s.mine() {
        return false;
    }
    if let Some(owner) = s.owner()
        && sim.relations.get(&owner).is_some_and(|r| r.is_enemy(Some(kenshi_game::sim::PLAYER_FACTION)))
    {
        return false;
    }
    if !s.town().is_some_and(|t| matches!(sim.towns[t].kind, 1..=3)) {
        return false;
    }
    match sim.placed.get(p).and_then(|pl| pl.residents.as_ref()) {
        None => true,
        Some(r) => sim.data.get(r).is_some_and(|t| t.bools.get("sell home").copied().unwrap_or(false)),
    }
}

/// @fn F044617
///
/// A building's price: its construction's whole (its materials' worth) x its owner's faction's
/// "building cost mult" x 400, x 0.375 for a ruin (+0x1a1).
fn price(s: &Site) -> i32 {
    let sim = s.sim;
    let total = kenshi_game::base::PlayerBuilding::new(s.rec, s.at, 0.0).total() as f32;
    let mult = s.owner().and_then(|f| sim.data.get(&f)).map_or(0.0, |f| float(f, "building cost mult"));
    let ruin = s.placement().and_then(|p| sim.placed.get(p)).is_some_and(|pl| pl.destroyed);
    let k = if ruin { 0.375 } else { 1.0 };
    (k * total * mult * 400.0) as i32
}

/// A datapanel button clicked (its action) for building `place`.
pub fn act(game_sim: &mut Sim, selected: &[usize], place: Place, action: &str, dismantle: &mut Option<(Vec<usize>, usize)>) {
    if let Place::Door(d) = place {
        match action {
            // F037658
            DOOR_TOGGLE => {
                if let Some(say) = game_sim.toggle_door(d) {
                    game_sim.messages.push(say.to_string());
                }
            }
            // F037659: the lock wanted turned; shut, it locks; no longer wanted, it unlocks
            // (its sound: sound::door_locks)
            DOOR_LOCK => {
                if let Some(s) = game_sim.door_states.get_mut(d) {
                    s.wants_lock = !s.wants_lock;
                    if s.phase == kenshi_game::sim::DOOR_CLOSED && s.wants_lock {
                        s.locked = true;
                    }
                    if s.locked && !s.broken && s.level > 0 && !s.wants_lock {
                        s.locked = false;
                    }
                }
            }
            _ => {
                if let Some(p) = door_parent(game_sim, d) {
                    act(game_sim, selected, p, action, dismantle);
                }
            }
        }
        return;
    }
    let Place::Player(b) = place else { return };
    let Some(pb) = game_sim.buildings.get_mut(b) else { return };
    match action {
        // F037762: asked first (message_box::ask_dismantle)
        DISMANTLE => *dismantle = Some((selected.to_vec(), b)),
        // F037657
        PAUSE => pb.paused = !pb.paused,
        // F037656
        PUBLIC => pb.public = !pb.public,
        // F029299 (its "UI_Generator" sound: sound::machines)
        POWER => pb.switched_on = !pb.switched_on,
        // F029300
        BATTERY => pb.on_battery = !pb.on_battery,
        // F029413 / F029414
        UPGRADE => {
            game_sim.upgrade_building(b);
        }
        DOWNGRADE => {
            game_sim.downgrade_building(b);
        }
        _ => {}
    }
}

/// (testing) The building of a kind (town, furniture, container, door) nearest the player's
/// first character whose record's name holds `name`.
fn nearest(sim: &Sim, kind: &str, name: &str) -> Option<usize> {
    let me = sim.folk.iter().find(|p| p.player)?.mover.position_units();
    let n = match kind {
        "town" => sim.placed.len(),
        "furniture" => sim.furn.len(),
        "container" => sim.boxes.len(),
        "door" => sim.door_states.len(),
        _ => sim.buildings.len(),
    };
    let place = |k: usize| match kind {
        "town" => Place::Town(k),
        "furniture" => Place::Furniture(k),
        "container" => Place::Container(k),
        "door" => Place::Door(k),
        _ => Place::Player(k),
    };
    (0..n)
        .filter_map(|k| {
            let (r, at) = record_at(sim, place(k))?;
            let named = sim.data.get(r).is_some_and(|r| r.name.contains(name));
            named.then(|| (k, (at[0] - me[0]).hypot(at[2] - me[2])))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// (testing) KENSHI_PICK_BUILDING=frame[@k]: the player's building k (the last made when not
/// given) picked at that frame, as a click on it. KENSHI_PICK_PLACE=frame@kind:k: building k of
/// a kind (town, furniture, container, door, player) picked; kind:near[:name] the one nearest the
/// player's first character (its record's name holding `name`); door:last the last door. KENSHI_DATAPANEL=frame@action[;...]:
/// that datapanel button of the building picked clicked then. KENSHI_PANEL_DUMP=frame: the
/// panel's lines, tooltips and bars of the building picked written out then.
pub fn script(mut frames: bevy::prelude::Local<u32>, mut game: bevy::prelude::ResMut<super::Game>) {
    *frames += 1;
    if std::env::var("KENSHI_PANEL_DUMP").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames) {
        let place = {
            let super::Game { selected_building, sim, .. } = &mut *game;
            selected_building.as_mut().and_then(|s| s.resolve(sim))
        };
        if let Some(p) = place {
            let v = view(&game.sim, p);
            let rows = v.name.iter().chain(&v.info).chain(&v.stats).chain(&v.extended);
            for r in rows {
                eprintln!("panel: {:?} = {:?} [{}]", r.label, r.value, r.tip.replace('\n', " | "));
            }
            eprintln!("panel status: {:?}", v.status);
            for b in &v.bars {
                eprintln!("panel bar: {:?} = {:?} {:.2} [{}]", b.label, b.value, b.widths[0], b.tip.replace('\n', " | "));
            }
        }
    }
    let clicks = std::env::var("KENSHI_DATAPANEL").unwrap_or_default();
    for (f, action) in clicks.split(';').filter_map(|c| c.split_once('@')) {
        if f.parse::<u32>().ok() != Some(*frames) {
            continue;
        }
        let super::Game { selected_building, sim, selected, dismantle_request, .. } = &mut *game;
        if let Some(p) = selected_building.as_mut().and_then(|s| s.resolve(sim)) {
            act(sim, selected, p, action, dismantle_request);
        }
    }
    if let Ok(v) = std::env::var("KENSHI_PICK_PLACE")
        && let Some((f, what)) = v.split_once('@')
        && f.parse::<u32>().ok() == Some(*frames)
        && let Some((kind, k)) = what.split_once(':')
        && let Some(k) = k
            .parse::<usize>()
            .ok()
            .or_else(|| (k == "last").then(|| game.sim.door_states.len().checked_sub(1)).flatten())
            .or_else(|| nearest(&game.sim, kind, k.strip_prefix("near")?.trim_start_matches(':')))
    {
        let place = match kind {
            "town" => Place::Town(k),
            "furniture" => Place::Furniture(k),
            "container" => Place::Container(k),
            "door" => Place::Door(k),
            _ => Place::Player(k),
        };
        game.selected_building = Selected::new(&game.sim, place);
        let name = game.selected_building.as_ref().and_then(|s| game.sim.data.get(&s.record)).map(|r| r.name.clone());
        eprintln!("pick place: {:?} {name:?}", game.selected_building);
    }
    let Ok(v) = std::env::var("KENSHI_PICK_BUILDING") else { return };
    let (at, k) = match v.split_once('@') {
        Some((a, k)) => (a.parse::<u32>().unwrap_or(0), k.parse::<usize>().ok()),
        None => (v.parse::<u32>().unwrap_or(0), None),
    };
    if *frames != at {
        return;
    }
    let b = k.or(game.sim.buildings.len().checked_sub(1));
    game.selected_building = b.and_then(|b| Selected::player(&game.sim, b));
    eprintln!("pick building: {:?}", game.selected_building);
}
