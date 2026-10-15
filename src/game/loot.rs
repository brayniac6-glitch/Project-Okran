//! Items lying on the ground (port_map `items`): each town's "loot spawn" (a bandit camp's,
//! a nest's), put down round it when its zone first loads, and picked up by walking to them.
//!
//! Specs: F048909 (a town's spawn: for each "loot spawn" entry, a VENDOR_LIST, a random count
//! between its two values of items generated from the list (F049397, `trade::generate`), each
//! put at a random place near the town on the ground).
//! (assumption) The places are within 400 units of the town's centre, on the navmesh; a town
//! spawns its loot once (the world's own respawning is not ported); an item is picked up from
//! within 25 units; ground items are not saved yet.

use crate::sim::Sim;
use kenshi_char::inventory::Item;
use kenshi_world::navmesh::METRES;
use kenshi_world::{WORLD_SIZE, ZONE_SIZE};

/// An item on the ground.
#[derive(Debug, Clone)]
pub struct GroundItem {
    pub id: u64,
    pub item: Item,
    /// World units.
    pub at: [f32; 3],
    /// Its turn about the vertical (radians).
    pub yaw: f32,
}

/// An item spot in a building (F037792, ITEM_PLACEMENT_GROUP): the group, where it lies, the
/// faction that owns it (a town's: taking from it is stealing), the ground item now on it and,
/// once taken, when it comes back (game hours: "min respawn time" .. "max respawn time").
/// A loaded game's zones make them again: each takes the saved item lying on it, and one saved
/// empty keeps waiting till it was due.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSpot {
    pub group: String,
    pub at: [f32; 3],
    pub yaw: f32,
    pub owner: Option<String>,
    pub item: Option<u64>,
    pub due: f64,
}

/// How far round a town its loot lies (units), and how near to pick an item up.
const SPREAD: f32 = 400.0;
const REACH: f32 = 25.0;

fn zone_of(p: [f32; 3]) -> (i32, i32) {
    let k = |v: f32| ((v + WORLD_SIZE / 2.0) / ZONE_SIZE).floor() as i32;
    (k(p[0]), k(p[2]))
}

impl Sim {
    /// @fn F048909
    ///
    /// Every second: the towns of the loaded zones not yet spawned put their "loot spawn" on
    /// the ground round them.
    pub fn loot_tick(&mut self) {
        if std::env::var("KENSHI_LOOT_LIST").is_ok() && self.next_ground == 0 && self.looted.is_empty() {
            eprintln!("loot: {} towns, e.g. {:?}", self.towns.len(), self.towns.iter().take(8).map(|t| t.name.clone()).collect::<Vec<_>>());
            for t in &self.towns {
                if let Some(r) = self.data.get(&t.record)
                    && r.ints.get("gear artifacts max value").copied().unwrap_or(0) > 0
                {
                    eprintln!("artifact town {} at zone {:?}", t.name, zone_of(t.position));
                }
                if self.data.get(&t.record).is_some_and(|r| !r.refs("loot spawn").is_empty()) {
                    eprintln!("loot town {} at zone {:?}", t.name, zone_of(t.position));
                }
            }
        }
        self.spots_tick();
        let due: Vec<usize> = (0..self.towns.len())
            .filter(|&t| !self.looted.contains(&self.towns[t].record) && self.zones.contains(&zone_of(self.towns[t].position)))
            .collect();
        for t in due {
            let (record, centre) = (self.towns[t].record.clone(), self.towns[t].position);
            self.looted.insert(record.clone());
            let Some(town) = self.data.get(&record).cloned() else { continue };
            for (list, v) in town.refs("loot spawn") {
                let Some(list) = self.data.get(list).cloned() else { continue };
                let (lo, hi) = (v[0].min(v[1]), v[0].max(v[1]));
                let n = lo + (self.rng.uniform() * (hi - lo + 1) as f32) as i32;
                let mut items = Vec::new();
                let data = self.data.clone();
                let rng = &mut self.rng;
                crate::trade::generate(&data, &list, n.min(hi), &mut items, &mut || rng.uniform());
                for item in items {
                    let (a, r) = (self.rng.uniform() * std::f32::consts::TAU, self.rng.uniform().sqrt() * SPREAD);
                    let p = [centre[0] + a.cos() * r, centre[1], centre[2] + a.sin() * r];
                    // on the ground there (the navmesh's nearest point)
                    let Some((_, q)) = self.nav.closest_point(p.map(|x| x * METRES), 60.0) else { continue };
                    let at = q.map(|x| x / METRES);
                    let yaw = self.rng.uniform() * std::f32::consts::TAU;
                    self.next_ground += 1;
                    self.ground.push(GroundItem { id: self.next_ground, item, at, yaw });
                }
            }
            if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
                eprintln!("loot: {} spawned, {} items on the ground", town.name, self.ground.len());
            }
        }
    }

    /// The ground item nearest `p` (world units) within `within`.
    pub fn ground_item_near(&self, p: [f32; 3], within: f32) -> Option<u64> {
        self.ground
            .iter()
            .map(|g| (g.id, (g.at[0] - p[0]).hypot(g.at[2] - p[2])))
            .filter(|(_, d)| *d < within)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The selection goes to ground item `id`: the first of them walks over and picks it up.
    pub fn order_pick_item(&mut self, ids: &[usize], id: u64) {
        let Some(g) = self.ground.iter().find(|g| g.id == id) else { return };
        let at = g.at;
        let Some(&who) = ids.iter().find(|&&i| !self.folk[i].down) else { return };
        self.order_move(&[who], at);
        self.folk[who].fetch = Some(id);
    }

    /// Each tick: people fetching an item that are near it take it (into their inventory if it
    /// fits; else it stays where it is).
    pub fn pickup_tick(&mut self) {
        for who in 0..self.folk.len() {
            let Some(id) = self.folk[who].fetch else { continue };
            let Some(k) = self.ground.iter().position(|g| g.id == id) else {
                self.folk[who].fetch = None;
                continue;
            };
            let (p, g) = (self.folk[who].mover.position_units(), self.ground[k].at);
            if (p[0] - g[0]).hypot(p[2] - g[2]) > REACH {
                continue;
            }
            self.folk[who].fetch = None;
            let item = self.ground[k].item.clone();
            let name = item.name.clone();
            if self.folk[who].inventory.add(item, false) {
                // an item off a town's spot is theirs: taking it is stealing (seen or not)
                if let Some(owner) = self.spots.iter().find(|s| s.item == Some(id)).and_then(|s| s.owner.clone())
                    && self.folk[who].faction.as_deref() != Some(owner.as_str())
                {
                    self.commit_crime(who, kenshi_ai::CrimeEnum::CrimeStealing, Some(&owner));
                }
                self.ground.remove(k);
                if self.folk[who].player {
                    self.messages.push(format!("{} picked up {name}", self.display_name(who)));
                }
            } else if self.folk[who].player {
                self.messages.push(format!("{} has no room for {name}", self.display_name(who)));
            }
        }
    }
}

impl Sim {
    /// Person `who` puts `item` down on the ground at their feet (a little to one side).
    pub fn drop_item(&mut self, who: usize, item: Item) {
        let p = self.folk[who].mover.position_units();
        let (a, r) = (self.rng.uniform() * std::f32::consts::TAU, 4.0 + self.rng.uniform() * 4.0);
        let near = [p[0] + a.cos() * r, p[1], p[2] + a.sin() * r];
        let at = self.nav.closest_point(near.map(|x| x * METRES), 5.0).map_or(p, |(_, q)| q.map(|x| x / METRES));
        let yaw = self.rng.uniform() * std::f32::consts::TAU;
        self.next_ground += 1;
        self.ground.push(GroundItem { id: self.next_ground, item, at, yaw });
    }
}

/// Containers as inventory owners (`Sim::inv`): BOX_OWNER + their index.
pub const BOX_OWNER: usize = 1 << 21;
/// A person's worn backpack as an inventory owner (`Sim::inv`): BAG_OWNER + the person.
pub const BAG_OWNER: usize = 1 << 22;

impl Sim {
    /// A storage's place (world units), how far out its edge is, its name and grid size: a
    /// player building's (BUILDING_OWNER + index) or a container's (BOX_OWNER + index).
    pub fn storage_info(&self, owner: usize) -> Option<([f32; 3], f32, String, (u32, u32))> {
        if let Some(p) = owner.checked_sub(BAG_OWNER) {
            let wearer = self.folk.get(p)?;
            let worn = wearer.inventory.sections.iter().find(|s| s.name == "backpack_attach")?.items.first()?;
            let grid = worn.item.contents.as_ref()?.sections.first()?;
            return Some((wearer.mover.position_units(), 10.0, worn.item.name.clone(), (grid.w, grid.h)));
        }
        if let Some(k) = owner.checked_sub(BOX_OWNER) {
            let b = self.boxes.get(k)?;
            // a counter shows its trader's stock grid
            let size = self
                .counter_trader(k)
                .and_then(|t| self.folk[t].shop.as_ref())
                .and_then(|s| s.stock.sections.first())
                .map_or(b.at.size, |s| (s.w, s.h));
            return Some((b.at.position, 10.0, b.at.name.clone(), size));
        }
        let pb = self.buildings.get(owner.checked_sub(crate::base::BUILDING_OWNER)?)?;
        let size = pb.storage.as_ref().and_then(|s| s.sections.first()).map_or((8, 8), |s| (s.w, s.h));
        let name = self.data.get(&pb.record).map_or("Storage".to_string(), |r| r.name.clone());
        Some((pb.at, pb.radius, name, size))
    }

    /// The trader whose stock container `k` shows: a shop counter in the building a trader
    /// keeps (their home). (Kenshi keeps a shop's goods in its counters, where they are
    /// stolen from.)
    pub fn counter_trader(&self, k: usize) -> Option<usize> {
        let b = self.boxes.get(k)?;
        if !b.at.name.to_lowercase().contains("counter") {
            return None;
        }
        self.folk.iter().position(|p| p.shop.is_some() && !p.dead && p.home == Some(b.at.placement))
    }

    /// The faction a container belongs to: its town's (the player's own storage none).
    /// (assumption) The town within whose radius it stands; ruins' (no faction) none.
    pub fn box_faction(&self, k: usize) -> Option<String> {
        let p = self.boxes.get(k)?.at.position;
        if let Some(t) = self.counter_trader(k) {
            return self.folk[t].faction.clone();
        }
        self.towns
            .iter()
            .filter(|t| (t.position[0] - p[0]).hypot(t.position[2] - p[2]) < t.radius())
            .find_map(|t| t.faction.clone())
    }

    /// The container nearest `p` (world units) within `within`, as an owner (BOX_OWNER + index).
    pub fn box_near(&self, p: [f32; 3], within: f32) -> Option<usize> {
        self.boxes
            .iter()
            .enumerate()
            .map(|(k, b)| (k, (b.at.position[0] - p[0]).hypot(b.at.position[2] - p[2]), (b.at.position[1] - p[1]).abs()))
            .filter(|(_, d, dy)| *d < within && *dy < 40.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, ..)| BOX_OWNER + k)
    }
}

impl Sim {
    /// The saved contents put back into the containers from `first` on that match them (the
    /// same record within 2 units).
    pub fn refill_boxes(&mut self, first: usize) {
        if self.saved_boxes.is_empty() {
            return;
        }
        let data = self.data.clone();
        for k in first..self.boxes.len() {
            let (rec, p) = (self.boxes[k].at.record.clone(), self.boxes[k].at.position);
            let Some(i) = self.saved_boxes.iter().position(|(r, q, _)| *r == rec && (q[0] - p[0]).hypot(q[2] - p[2]) < 2.0 && (q[1] - p[1]).abs() < 2.0) else { continue };
            let (_, _, items) = self.saved_boxes.remove(i);
            crate::save::restore_items(&data, &mut self.boxes[k].inventory, &items);
        }
    }
}

/// A container in a town or ruin (`kenshi_world::furniture::containers`) and what is in it.
#[derive(Debug, Clone)]
pub struct WorldBox {
    pub at: kenshi_world::furniture::Container,
    pub inventory: kenshi_char::inventory::Inventory,
}

impl Sim {
    /// The containers of placements `placed` (their indices from `off`), each empty.
    /// A zone's buildings' item spots (`ItemSpot`), each with an item to start with; a town's
    /// belong to its faction.
    pub fn add_item_spots(&mut self, placed: &[kenshi_world::Placement]) {
        if self.layouts.is_none() {
            self.layouts = Some(kenshi_world::nodes::Layouts::load(&self.install));
        }
        let Some(layouts) = self.layouts.as_ref() else { return };
        let found = kenshi_world::furniture::item_spots(&self.data, layouts, placed);
        let first = self.spots.len();
        for s in found {
            let town = kenshi_world::town::current_town(&self.towns, s.position, None);
            let owner = town.filter(|&t| self.towns[t].kind != 4).and_then(|t| self.towns[t].faction.clone());
            let w = s.rotation[0].clamp(-1.0, 1.0);
            let yaw = 2.0 * w.acos() * if s.rotation[2] < 0.0 { -1.0 } else { 1.0 };
            self.spots.push(ItemSpot { group: s.group, at: s.position, yaw, owner, item: None, due: 0.0 });
        }
        self.apply_spot_dues(first);
        if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
            eprintln!("item spots: {} new, {} in all", self.spots.len() - first, self.spots.len());
        }
    }

    /// The spots from `first` on that a loaded game had empty: they wait till they were due.
    pub fn apply_spot_dues(&mut self, first: usize) {
        if self.saved_spot_dues.is_empty() {
            return;
        }
        for s in self.spots.iter_mut().skip(first) {
            if let Some(&(_, due)) = self.saved_spot_dues.iter().find(|(at, _)| (at[0] - s.at[0]).abs() < 1.0 && (at[1] - s.at[1]).abs() < 1.0 && (at[2] - s.at[2]).abs() < 1.0) {
                s.item = None;
                s.due = due;
            }
        }
    }

    /// One item of an ITEM_PLACEMENT_GROUP: of its "items", "weapons", "clothing", "armour
    /// blueprints", "blueprints" and "maps", one by their chances (the refs' first values).
    /// (assumption) Picked among them all by chance; weapons of no particular maker, gear of
    /// the record's own quality.
    fn spot_item(&mut self, group: &str) -> Option<kenshi_char::inventory::Item> {
        let g = self.data.get(group)?;
        let all: Vec<(String, f32)> = ["items", "weapons", "clothing", "armour blueprints", "blueprints", "maps"]
            .iter()
            .flat_map(|k| g.refs(k).iter().map(|(s, v)| (s.clone(), v[0].max(0) as f32)))
            .filter(|(s, w)| *w > 0.0 && self.data.get(s).is_some())
            .collect();
        let total: f32 = all.iter().map(|x| x.1).sum();
        if total <= 0.0 {
            return None;
        }
        let mut r = self.rng.uniform() * total;
        let pick = all.iter().find(|(_, w)| {
            r -= w;
            r < 0.0
        })?;
        let rec = self.data.get(&pick.0)?;
        Some(kenshi_char::inventory::Item::from_record(rec, 1))
    }

    /// Every second: item spots empty and due get an item; spots whose item has gone wait
    /// "min respawn time" .. "max respawn time" hours (F037792).
    fn spots_tick(&mut self) {
        let now = self.hours_gone();
        for k in 0..self.spots.len() {
            if let Some(id) = self.spots[k].item {
                if !self.ground.iter().any(|g| g.id == id) {
                    let g = self.data.get(&self.spots[k].group);
                    let lo = g.and_then(|g| g.ints.get("min respawn time").copied()).unwrap_or(24) as f64;
                    let hi = g.and_then(|g| g.ints.get("max respawn time").copied()).unwrap_or(48).max(lo as i32) as f64;
                    self.spots[k].item = None;
                    self.spots[k].due = now + lo + (hi - lo) * self.rng.uniform() as f64;
                }
                continue;
            }
            if now < self.spots[k].due {
                continue;
            }
            // an item already lying on it (a loaded game's) is its own
            let at = self.spots[k].at;
            let taken: Vec<u64> = self.spots.iter().filter_map(|s| s.item).collect();
            if let Some(g) = self.ground.iter().find(|g| !taken.contains(&g.id) && (g.at[0] - at[0]).hypot(g.at[2] - at[2]) < 3.0 && (g.at[1] - at[1]).abs() < 10.0) {
                self.spots[k].item = Some(g.id);
                continue;
            }
            let group = self.spots[k].group.clone();
            let Some(item) = self.spot_item(&group) else {
                self.spots[k].due = f64::INFINITY;
                continue;
            };
            self.next_ground += 1;
            let (at, yaw) = (self.spots[k].at, self.spots[k].yaw);
            self.ground.push(GroundItem { id: self.next_ground, item, at, yaw });
            self.spots[k].item = Some(self.next_ground);
        }
    }

    pub fn add_containers(&mut self, placed: &[kenshi_world::Placement], off: usize) {
        if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
            eprintln!("containers: {} placements from {off}, layouts {}", placed.len(), self.layouts.is_some());
        }
        if self.layouts.is_none() {
            self.layouts = Some(kenshi_world::nodes::Layouts::load(&self.install));
        }
        let Some(layouts) = self.layouts.as_ref() else { return };
        let found = kenshi_world::furniture::containers(&self.data, layouts, placed);
        if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
            eprintln!("containers: {} found, e.g. {:?}", found.len(), found.iter().take(5).map(|c| c.name.clone()).collect::<Vec<_>>());
        }
        let first = self.boxes.len();
        for mut c in found {
            c.placement += off;
            let (w, h) = c.size;
            let inventory = kenshi_char::inventory::Inventory { sections: vec![kenshi_char::inventory::Section::new("main", w, h, kenshi_char::inventory::ANY)] };
            self.boxes.push(WorldBox { at: c, inventory });
        }
        self.refill_boxes(first);
        self.add_item_spots(placed);
    }

    /// F048891 Town::distributeArtifacts: when a town's zone first loads, the artifacts the
    /// world gave it (`artifacts::distribute`, F022241) go into its buildings' storage: each
    /// into a container whose building's residents ("squad template" / "residents template")
    /// have an "item artifacts base value" (an item) or "gear artifacts base value" (gear),
    /// picked weighted by it (BuildingFinderForArtifacts, F047554: storage with room).
    /// (assumption) The pick is weighted by the base value, one artifact at a time; with no
    /// such building, any of the town's containers; with none, the ground by the town's
    /// middle.
    pub fn artifacts_tick(&mut self) {
        if self.artifacts.is_none() {
            self.artifacts = Some(crate::artifacts::distribute(&self.data, &self.towns));
            if std::env::var("KENSHI_LOOT_DEBUG").is_ok()
                && let Some(h) = &self.artifacts
            {
                let total: i32 = h.values().flatten().map(|x| x.2).sum();
                eprintln!("artifacts handed out: {total} to {} towns", h.len());
                for (t, list) in h.iter().take(6) {
                    let name = self.data.get(t).map_or("?", |r| r.name.as_str());
                    eprintln!("  {name}: {:?}", list.iter().map(|(r, _, n, _)| (self.data.get(r).map_or("?".into(), |x| x.name.clone()), *n)).collect::<Vec<_>>());
                }
            }
        }
        let due: Vec<usize> = (0..self.towns.len())
            .filter(|&t| {
                let key = format!("art:{}", self.towns[t].record);
                !self.looted.contains(&key) && self.zones.contains(&zone_of(self.towns[t].position))
            })
            .collect();
        for t in due {
            let (record, centre, radius) = (self.towns[t].record.clone(), self.towns[t].position, self.towns[t].radius());
            self.looted.insert(format!("art:{record}"));
            let given = self.artifacts.as_ref().and_then(|a| a.get(&record)).cloned().unwrap_or_default();
            if given.is_empty() {
                continue;
            }
            let here: Vec<usize> = (0..self.boxes.len())
                .filter(|&b| {
                    let p = self.boxes[b].at.position;
                    (p[0] - centre[0]).hypot(p[2] - centre[2]) < radius
                })
                .collect();
            // each container's weights from its building's residents
            let weight = |sim: &Sim, b: usize, key: &str| -> f32 {
                let pl = sim.placed.get(sim.boxes[b].at.placement);
                pl.into_iter()
                    .flat_map(|p| p.squad.iter().chain(p.residents.iter()))
                    .filter_map(|sq| sim.data.get(sq))
                    .map(|r| r.ints.get(key).copied().unwrap_or(0) as f32)
                    .fold(0.0, f32::max)
            };
            let mut put = (0, 0);
            for (sid, gear, n, quality) in given {
                let key = if gear { "gear artifacts base value" } else { "item artifacts base value" };
                let weights: Vec<f32> = here.iter().map(|&b| weight(self, b, key)).collect();
                for _ in 0..n {
                    let data = self.data.clone();
                    let rng = &mut self.rng;
                    let Some(item) = crate::artifacts::artifact_item(&data, &sid, quality, &mut || rng.uniform()) else { continue };
                    let total: f32 = weights.iter().sum();
                    let pick = if total > 0.0 {
                        let mut x = self.rng.uniform() * total;
                        here.iter().zip(&weights).find(|(_, w)| {
                            x -= **w;
                            x <= 0.0
                        }).map(|(b, _)| *b)
                    } else if !here.is_empty() {
                        Some(here[((self.rng.uniform() * here.len() as f32) as usize).min(here.len() - 1)])
                    } else {
                        None
                    };
                    match pick {
                        Some(b) if self.boxes[b].inventory.add(item.clone(), false) => put.0 += 1,
                        _ => {
                            // no room or no container: on the ground by the town's middle
                            let (a, d) = (self.rng.uniform() * std::f32::consts::TAU, self.rng.uniform() * 150.0);
                            let p = [centre[0] + a.cos() * d, centre[1], centre[2] + a.sin() * d];
                            let at = self.nav.closest_point(p.map(|x| x * METRES), 80.0).map_or(p, |(_, q)| q.map(|x| x / METRES));
                            self.next_ground += 1;
                            let yaw = self.rng.uniform() * std::f32::consts::TAU;
                            self.ground.push(GroundItem { id: self.next_ground, item, at, yaw });
                            put.1 += 1;
                        }
                    }
                }
            }
            if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
                eprintln!("artifacts: {} put {} in its {} containers, {} on the ground", self.towns[t].name, put.0, here.len(), put.1);
            }
        }
    }
}
