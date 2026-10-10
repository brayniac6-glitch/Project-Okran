//! The player's base: buildings placed from the build menu, built by the squad with building
//! materials, and what they do once built.
//!
//! Specs: F036238 (the build window's figures: materials cost, estimated build time), F037833 /
//! F037796 (estimated build time = materials / ("build speed mult" / (day length / 24)) hours),
//! F031256 (Task_Build: a builder's rate lerp(engineer / 100, 0.3, 2.0), the building's
//! construction done at progress 1).
//! (stand-in) Building materials are taken from the builder's own inventory first, then from a
//! built storage building of the player's, one as each material's worth of work begins (the
//! game fetches them from storage; its delivery is not ported).

use crate::sim::Sim;
use kenshi_char::inventory::{Inventory, Item, Section};
use crate::production::{functionality, take_one};
use kenshi_data::{GameData, GameDataContainer, ItemType};

/// Inventory owners from here on are the player's buildings (`Sim::inv`).
pub const BUILDING_OWNER: usize = 1 << 20;

/// The default construction material (F036238 "580-gamedata.base": Building Material).
pub const BUILDING_MATERIAL: &str = "580-gamedata.base";

pub use crate::production::is_farm;

/// A building of the player's.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerBuilding {
    /// The BUILDING record.
    pub record: String,
    /// Where it stands (units) and its turn about the vertical (radians).
    pub at: [f32; 3],
    pub yaw: f32,
    /// Its materials (item record, count) and how many have gone in.
    pub materials: Vec<(String, u32)>,
    pub used: u32,
    /// Work done, in materials' worth (built at the materials' total).
    pub progress: f32,
    /// "build speed mult".
    pub speed: f32,
    /// Placed and confirmed (unconfirmed ones can still be undone).
    pub confirmed: bool,
    /// Built.
    pub done: bool,
    /// Its storage ("has inventory"): one section of "storage size width" x "height".
    #[serde(skip)]
    pub storage: Option<Inventory>,
    /// The building's footprint radius (units), for reaching and picking it.
    pub radius: f32,
    /// A machine's products made toward their next item and ingredients taken ahead (by its
    /// functionality's order; F029325, F029359), and whether its storage is full (waiting to be
    /// emptied).
    #[serde(default)]
    pub made: Vec<f32>,
    #[serde(default)]
    pub taken: Vec<f32>,
    #[serde(default)]
    pub full: bool,
    /// A farm's crop (F022906): its stage (under 1 growing, 1 to 2 ripe, 2 on rotting), how
    /// near death, its water tank (0 to 1), and the harvest or clearing work done.
    #[serde(default)]
    pub stage: f32,
    #[serde(default)]
    pub death: f32,
    #[serde(default)]
    pub water: f32,
    #[serde(default)]
    pub work: f32,
    /// A fuel generator's fuel left (in items' worth).
    #[serde(default)]
    pub fuel: f32,
    /// A crafting bench's queue (item records) and whether it repeats (the crafting tab).
    #[serde(default)]
    pub craft_queue: Vec<String>,
    #[serde(default)]
    pub craft_repeat: bool,
}

impl PlayerBuilding {
    /// A new building of record `b` at `at` turned `yaw`, not yet confirmed.
    pub fn new(b: &GameData, at: [f32; 3], yaw: f32) -> PlayerBuilding {
        let int = |k: &str| b.ints.get(k).copied().unwrap_or(0);
        let float = |k: &str| b.floats.get(k).copied().or_else(|| b.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
        let mut materials: Vec<(String, u32)> = b.refs("construction").iter().map(|(s, v)| (s.clone(), v[0].max(0) as u32)).filter(|(_, n)| *n > 0).collect();
        if materials.is_empty() && int("build materials") > 0 {
            materials.push((BUILDING_MATERIAL.to_string(), int("build materials") as u32));
        }
        // a production building keeps what it makes (its storage size) until emptied, a research
        // bench the artifacts put in it (F035132)
        let producing = functionality(b).is_some();
        let storage = (b.bools.get("has inventory").copied().unwrap_or(false) || producing).then(|| {
            let (w, h) = (int("storage size width").max(1) as u32, int("storage size height").max(1) as u32);
            Inventory { sections: vec![Section::new("main", w, h, kenshi_char::inventory::ANY)] }
        });
        // (assumption) the footprint from the building's height: about half of it across, at
        // least 15 units
        let radius = (float("building height") * 0.6).max(15.0);
        PlayerBuilding {
            record: b.string_id.clone(),
            at,
            yaw,
            done: materials.is_empty(),
            progress: 0.0,
            used: 0,
            speed: float("build speed mult").max(0.01),
            materials,
            confirmed: false,
            storage,
            radius,
            made: Vec::new(),
            taken: Vec::new(),
            full: false,
            stage: 0.0,
            death: 0.0,
            water: 0.0,
            work: 0.0,
            fuel: 0.0,
            craft_queue: Vec::new(),
            craft_repeat: false,
        }
    }

    /// The materials it takes in all.
    pub fn total(&self) -> u32 {
        self.materials.iter().map(|m| m.1).sum()
    }

    /// The material the next unit of work needs (in the order listed).
    pub fn next_material(&self) -> Option<&str> {
        let mut k = self.used;
        for (m, n) in &self.materials {
            if k < *n {
                return Some(m);
            }
            k -= n;
        }
        None
    }
}

/// @fn F037833
///
/// A building's estimated build time (game hours, at one worker's rate of 1): its materials over
/// its "build speed mult".
pub fn estimated_hours(b: &GameData) -> f32 {
    let pb = PlayerBuilding::new(b, [0.0; 3], 0.0);
    pb.total() as f32 / pb.speed
}

/// @fn F031256
///
/// A builder's rate: lerp(engineer / 100 (clamped), 0.3, 2.0).
pub fn builder_rate(engineer: f32) -> f32 {
    let t = (engineer * 0.01).clamp(0.0, 1.0);
    0.3 + (2.0 - 0.3) * t
}

/// The buildings the player can build: those the research done enables ("enable buildings"),
/// starting with the "_Default Start" research's.
pub fn unlocked<'a>(data: &'a GameDataContainer, done: &[String]) -> Vec<&'a GameData> {
    let mut out: Vec<&GameData> = Vec::new();
    for r in data.of_type(ItemType::Research) {
        if r.name != "_Default Start" && !done.contains(&r.string_id) {
            continue;
        }
        for (sid, _) in r.refs("enable buildings") {
            if let Some(b) = data.get(sid)
                && !out.iter().any(|o| o.string_id == b.string_id)
            {
                out.push(b);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

impl Sim {
    /// Order characters to build building `b`: each walks to it and builds while there are
    /// materials ([`Sim::build_tick`]).
    pub fn order_build(&mut self, ids: &[usize], b: usize) {
        let Some(site) = self.buildings.get(b) else { return };
        if site.done {
            return;
        }
        let (at, r) = (site.at, site.radius);
        for &id in ids {
            if self.folk.get(id).is_some_and(|p| !p.down) {
                // (assumption) to the site's edge on the builder's side
                let me = self.folk[id].mover.position_units();
                let (dx, dz) = (me[0] - at[0], me[2] - at[2]);
                let d = dx.hypot(dz).max(1e-3);
                let to = [at[0] + dx / d * r, at[1], at[2] + dz / d * r];
                self.order_move(&[id], to);
                self.folk[id].build = Some(b);
            }
        }
    }

    /// The player's building under `at` (units), if any.
    pub fn building_at(&self, at: [f32; 3]) -> Option<usize> {
        self.buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.confirmed)
            .map(|(k, b)| (k, (b.at[0] - at[0]).hypot(b.at[2] - at[2]), b.radius))
            .filter(|(_, d, r)| d < r)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _, _)| k)
    }

    /// Whether `at` (units) is in the player's outpost (the HUD's town panel shows there,
    /// F043067: the player's nearest town, within its radius).
    /// (stand-in) The outpost is the built buildings: its centre their middle, its radius the
    /// farthest one's edge from there plus 1000 units (the game's town radius is its own field).
    pub fn in_outpost(&self, at: [f32; 3]) -> bool {
        let built: Vec<&PlayerBuilding> = self.buildings.iter().filter(|b| b.done).collect();
        if built.is_empty() {
            return false;
        }
        let n = built.len() as f32;
        let c = [built.iter().map(|b| b.at[0]).sum::<f32>() / n, built.iter().map(|b| b.at[2]).sum::<f32>() / n];
        let r = built.iter().map(|b| (b.at[0] - c[0]).hypot(b.at[2] - c[1]) + b.radius).fold(0.0, f32::max) + 1000.0;
        (at[0] - c[0]).hypot(at[2] - c[1]) < r
    }

    /// Take one of item `record` for building work: from the builder's inventory, else from a
    /// built storage building of the player's. Whether there was one.
    pub(crate) fn take_material(&mut self, builder: usize, record: &str) -> bool {
        if take_one(&mut self.folk[builder].inventory, record) {
            return true;
        }
        self.buildings.iter_mut().filter(|b| b.done).filter_map(|b| b.storage.as_mut()).any(|inv| take_one(inv, record))
    }

    /// Task_Build each tick: a builder by its building (within its radius + 15) works at its rate
    /// (F031256) times the building's speed per game hour of materials; each material's worth
    /// starts by taking that material ([`Sim::take_material`]); without one, the work waits.
    /// Done at the materials' total: the building is built and its builders stop.
    pub fn build_tick(&mut self, dt: f32) {
        let hour = self.hour_secs as f32;
        for id in 0..self.folk.len() {
            let Some(b) = self.folk[id].build else { continue };
            if self.folk[id].down || b >= self.buildings.len() || (self.buildings[b].done && !self.needs_repair(b)) {
                self.folk[id].build = None;
                continue;
            }
            let (site, r) = (self.buildings[b].at, self.buildings[b].radius);
            let me = self.folk[id].mover.position_units();
            if (me[0] - site[0]).hypot(me[2] - site[2]) > r + 15.0 {
                continue;
            }
            // a built one damaged: repaired (crate::upkeep)
            if self.buildings[b].done {
                if self.repair_work(id, b, dt) {
                    self.folk[id].build = None;
                }
                self.arena.f[id].skills.train(kenshi_char::StatsEnumerated::StatEngineering, dt * 0.05, 1.0);
                continue;
            }
            // the next material's worth needs that material in hand
            let site_b = &self.buildings[b];
            if site_b.progress >= site_b.used as f32 {
                let Some(m) = site_b.next_material().map(str::to_string) else { continue };
                if !self.take_material(id, &m) {
                    self.missing_material = Some(m);
                    continue;
                }
                self.buildings[b].used += 1;
            }
            let rate = builder_rate(self.arena.f[id].level(kenshi_char::StatsEnumerated::StatEngineering));
            let site_b = &mut self.buildings[b];
            site_b.progress = (site_b.progress + rate * site_b.speed * self.advanced.building * dt / hour).min(site_b.used as f32);
            if site_b.progress >= site_b.total() as f32 - 1e-4 {
                site_b.done = true;
                site_b.progress = site_b.total() as f32;
                if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                    eprintln!("built {}", site_b.record);
                }
            }
            // (assumption) builders train engineering as they work (F031256's training call, its
            // amount not read): as a hit of strength training scaled to the time
            self.arena.f[id].skills.train(kenshi_char::StatsEnumerated::StatEngineering, dt * 0.05, 1.0);
        }
    }
}

impl Sim {

    /// The best research bench level built (0: none).
    pub fn bench_level(&self) -> i32 {
        (0..self.buildings.len()).filter_map(|b| self.research_bench(b)).max().unwrap_or(0)
    }

    /// Whether research `r` can be learnt: not done, its "requirements" done.
    pub fn research_available(&self, r: &GameData) -> bool {
        !self.research_done.contains(&r.string_id) && r.refs("requirements").iter().all(|(s, _)| self.research_done.contains(s))
    }

    /// Queue research `id` ("Right-click to learn tech", F035133); F035132's refusal when its
    /// requirements are not met.
    pub fn queue_research(&mut self, id: &str) -> Result<(), String> {
        let Some(r) = self.data.get(id) else { return Err("no such research".into()) };
        if self.research_done.iter().any(|d| d == id) {
            return Err("Research already known".into());
        }
        if !self.research_available(r) {
            return Err(format!("Cannot research '{}'. Requirements not met.", r.name));
        }
        if !self.research_queue.iter().any(|q| q == id) {
            self.research_queue.push(id.to_string());
        }
        Ok(())
    }

    /// Take queued research `k` off the queue (the first loses its progress; its cost is not
    /// given back: assumption).
    pub fn unqueue_research(&mut self, k: usize) {
        if k < self.research_queue.len() {
            self.research_queue.remove(k);
            if k == 0 {
                self.research_progress = 0.0;
                self.research_paid = false;
            }
        }
    }
}

/// A placed building's items list for the save (the storage's contents).
pub fn storage_items(b: &PlayerBuilding) -> Vec<Item> {
    b.storage.iter().flat_map(|s| s.sections.iter().flat_map(|sec| sec.items.iter().map(|p| p.item.clone()))).collect()
}
