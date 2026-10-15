//! Nests in the simulation (kenshi_world::nest): BiomeManager keeps each biome's camps and dens
//! up to its "num nests" (F048131, a nest a population tick), each at a random point of the
//! biome away from towns and the player (F048115). A nest whose ground is loaded is built
//! (F048909) and its "roaming squads" come to live there; one whose people are all dead is
//! wiped out (F048878) and no longer counts, so another rises elsewhere in time.
//!
//! A biome whose nests stand at fixed markers only takes the level's nest markers (towns of
//! kind 8). A nest's people are made once, when it is first built.

use crate::sim::Sim;
use kenshi_world::biome::{biome_of_zone, zone_at, zone_centre};
use kenshi_world::nest::{CampPiece, nest_def};
use kenshi_world::ZONE_SIZE;
use serde::{Deserialize, Serialize};


/// A camp or den in the world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nest {
    /// The TOWN record and its name.
    pub record: String,
    pub name: String,
    /// The biome (index into the population's biomes) and the centre (world units).
    pub biome: usize,
    pub at: [f32; 3],
    /// Its buildings once built, and its people.
    #[serde(default)]
    pub pieces: Vec<CampPiece>,
    /// Its loose debris (kenshi_world::nest::DebrisPiece): NEST_ITEM, mesh, place, turn, size.
    #[serde(default)]
    pub loose: Vec<(String, i32, [f32; 3], f32, f32)>,
    #[serde(default)]
    pub members: Vec<usize>,
    #[serde(default)]
    pub built: bool,
    #[serde(default)]
    pub wiped: bool,
}

impl Sim {
    /// @fn F048131
    ///
    /// One nest for the first biome below its "num nests" x the multiplier: one of its "nests"
    /// (F038262) at a spawn point of the biome (F048115).
    pub fn spawn_nests(&mut self) {
        let nests_mult = self.advanced.nests;
        let Some(pop) = self.population.as_ref() else { return };
        let n = pop.biomes.len();
        let mut counts = vec![0usize; n];
        for k in self.nests.iter().filter(|k| !k.wiped) {
            if k.biome < n {
                counts[k.biome] += 1;
            }
        }
        let Some(b) = (0..n).find(|&b| {
            let bio = &pop.biomes[b];
            0 < bio.num_nests && (counts[b] as f32) < bio.num_nests as f32 * nests_mult
        }) else {
            return;
        };
        let data = self.data.clone();
        let biome_name = pop.biomes[b].name.clone();
        let Some(rec) = data.get(&pop.biomes[b].record) else { return };
        let roll = self.rng.uniform();
        let Some(nest) = self.pick_nest(rec, roll) else {
            // a biome with nests to make but none to make them from: counted as made
            self.nests.push(Nest { record: String::new(), name: String::new(), biome: b, at: [0.0; 3], pieces: vec![], loose: vec![], members: vec![], built: true, wiped: false });
            return;
        };
        let Some(at) = self.homeless_spawn_point(b) else { return };
        let name = data.get(&nest).map_or(String::new(), |r| r.name.clone());
        if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
            eprintln!("nest {name} ({nest}) in {biome_name} at {:?}", at.map(|v| v as i32));
        }
        self.nests.push(Nest { record: nest, name, biome: b, at, pieces: vec![], loose: vec![], members: vec![], built: false, wiped: false });
    }

    /// @fn F048115
    ///
    /// A spawn point for biome `b`: up to 10 tries of a random zone of the biome that is not
    /// one a player character stands in and holds no town, at a random point of its middle 80%;
    /// else a random zone of the biome.
    /// (stand-in) The point's placement, F048102 (pushed off towns, tried again on bad ground,
    /// over water), is not ported.
    ///
    /// A biome with "nests at fixed markers only" (+0x04): one of its nest markers (F048808:
    /// the towns of kind 8 in the biome, (assumption) by the zone they stand in) at random,
    /// leaving out those with a town (other than a marker) or a nest within 2500 units
    /// (F048793; (assumption) its second list, +0x88, the nests); none left: no nest.
    fn homeless_spawn_point(&mut self, b: usize) -> Option<[f32; 3]> {
        let at_markers = self.population.as_ref().and_then(|p| p.biomes.get(b)).is_some_and(|bio| bio.nests_at_markers);
        if at_markers {
            let pop = self.population.as_ref()?;
            let near = |p: [f32; 3], q: [f32; 3]| (p[0] - q[0]).powi(2) + (p[2] - q[2]).powi(2) < 2500.0 * 2500.0;
            if self.nest_markers.is_none() {
                self.nest_markers = Some(kenshi_world::town::nest_markers(&self.install, &self.data));
            }
            let all = self.nest_markers.as_deref().unwrap_or_default();
            let markers: Vec<[f32; 3]> = all
                .iter()
                .copied()
                .filter(|&m| {
                    let (zx, zy) = zone_at(m[0], m[2]);
                    biome_of_zone(&pop.map, &pop.biomes, zx, zy) == Some(b)
                })
                .filter(|&m| {
                    let towns = self.towns.iter().any(|o| o.kind != 8 && near(o.position, m));
                    let nests = self.nests.iter().any(|k| !k.wiped && near(k.at, m));
                    !towns && !nests
                })
                .collect();
            if markers.is_empty() {
                return None;
            }
            let k = ((self.rng.uniform() * markers.len() as f32) as usize).min(markers.len() - 1);
            return Some(markers[k]);
        }
        let zones = self.biome_zones(b);
        if zones.is_empty() {
            return None;
        }
        let players: Vec<(i32, i32)> = self.folk.iter().filter(|p| p.player).map(|p| {
            let q = p.mover.position_units();
            zone_at(q[0], q[2])
        }).collect();
        let towns: Vec<(i32, i32)> = self.towns.iter().map(|t| zone_at(t.position[0], t.position[2])).collect();
        let mut pick = None;
        for _ in 0..10 {
            let z = zones[((self.rng.uniform() * zones.len() as f32) as usize).min(zones.len() - 1)];
            if !players.contains(&z) && !towns.contains(&z) {
                pick = Some(z);
                break;
            }
        }
        let z = pick.unwrap_or(zones[((self.rng.uniform() * zones.len() as f32) as usize).min(zones.len() - 1)]);
        let (cx, cz) = zone_centre(z.0, z.1);
        let (dx, dz) = ((self.rng.uniform() - 0.5) * 0.8 * ZONE_SIZE, (self.rng.uniform() - 0.5) * 0.8 * ZONE_SIZE);
        let (x, zz) = (cx + dx, cz + dz);
        let y = self.terrain.height_at(x, zz).unwrap_or(0.0);
        Some([x, y, zz])
    }

    /// The zones of biome `b` (the 64 x 64 grid), worked out once.
    fn biome_zones(&mut self, b: usize) -> Vec<(i32, i32)> {
        let Some(pop) = self.population.as_mut() else { return vec![] };
        if pop.zones.is_empty() {
            let mut z: Vec<Vec<(i32, i32)>> = vec![Vec::new(); pop.biomes.len()];
            for x in 0..64 {
                for y in 0..64 {
                    if let Some(k) = biome_of_zone(&pop.map, &pop.biomes, x, y) {
                        z[k].push((x, y));
                    }
                }
            }
            pop.zones = z;
        }
        pop.zones.get(b).cloned().unwrap_or_default()
    }

    /// @fn F038262
    ///
    /// One of a biome's "nests" (TOWN records) by the refs' first values as weights, of those
    /// whose "world state" refs all stand (F050367: the "Invasion" camps only once their world
    /// has changed).
    fn pick_nest(&self, biome: &kenshi_data::GameData, roll: f32) -> Option<String> {
        let refs: Vec<(String, f32)> = biome
            .refs("nests")
            .iter()
            .filter(|(sid, _)| self.data.get(sid).is_some_and(|t| t.item_type() == Some(kenshi_data::ItemType::Town) && self.template_states_hold(t)))
            .map(|(sid, v)| (sid.clone(), v[0].max(0) as f32))
            .collect();
        if refs.is_empty() {
            return None;
        }
        let total: f32 = refs.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            let k = ((roll * refs.len() as f32) as usize).min(refs.len() - 1);
            return Some(refs[k].0.clone());
        }
        let mut r = roll * total;
        for (sid, w) in &refs {
            if r < *w {
                return Some(sid.clone());
            }
            r -= w;
        }
        refs.last().map(|(s, _)| s.clone())
    }

    /// The nests whose ground is loaded built and peopled (F048909); those whose people are all
    /// dead wiped out (F048878). A nest's people: its population manager (F048875's
    /// BasePopulationManager, its target the "nest resident population", F048832 topping
    /// squads up within it) fills it from the "roaming squads", their first values the
    /// weights; (assumption) at once, squads picked by weight until the people reach the
    /// target (one of each when the record has none).
    pub fn nests_tick(&mut self) {
        let data = self.data.clone();
        for k in 0..self.nests.len() {
            let n = &self.nests[k];
            if n.wiped || n.record.is_empty() {
                continue;
            }
            if n.built {
                if !n.members.is_empty() && n.members.iter().all(|&i| self.folk.get(i).is_none_or(|p| p.dead)) {
                    let name = n.name.clone();
                    self.nests[k].wiped = true;
                    eprintln!("{name} has been wiped out");
                    self.messages.push(format!("{name} has been wiped out"));
                }
                continue;
            }
            let at = n.at;
            if !self.nav.tiles.contains(&zone_at(at[0], at[2])) {
                continue;
            }
            let Some(rec) = data.get(&n.record) else { continue };
            let def = nest_def(rec);
            let terrain = &mut self.terrain;
            let nav = &self.nav;
            let m = kenshi_world::navmesh::METRES;
            let (pieces, loose) = kenshi_world::nest::camp(&data, &def, at, &mut |x, z| terrain.height_at(x, z).unwrap_or(at[1]), &mut |x, y, z| nav.closest_point([x * m, y * m, z * m], 6.0).is_some());
            if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
                eprintln!("nest {}: {} loose debris", def.name, loose.len());
            }
            let mut members = Vec::new();
            let total: f32 = def.squads.iter().map(|(_, w)| *w as f32).sum();
            let mut picks: Vec<String> = Vec::new();
            if def.population <= 0 || total <= 0.0 {
                picks = def.squads.iter().map(|(t, _)| t.clone()).collect();
            }
            let mut tries = 0;
            while (def.population > 0 && total > 0.0 && (members.len() as i32) < def.population && tries < 64) || !picks.is_empty() {
                tries += 1;
                let t = match picks.pop() {
                    Some(t) => t,
                    None => {
                        let mut r = self.rng.uniform() * total;
                        def.squads.iter().find(|(_, w)| {
                            r -= *w as f32;
                            r < 0.0
                        }).or(def.squads.last()).map(|(t, _)| t.clone()).unwrap_or_default()
                    }
                };
                let a = self.rng.uniform() * std::f32::consts::TAU;
                let d = self.rng.uniform() * def.radius * 0.3;
                let (x, z) = (at[0] + a.cos() * d, at[2] + a.sin() * d);
                let y = self.terrain.height_at(x, z).unwrap_or(at[1]);
                if let Some(m) = self.spawn_template(&t, [x, y, z]) {
                    members.extend(m);
                }
            }
            if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
                eprintln!("nest {} built: {} buildings, {} people", def.name, pieces.len(), members.len());
            }
            let n = &mut self.nests[k];
            n.pieces = pieces;
            n.loose = loose.into_iter().map(|d| (d.record, d.mesh, d.at, d.yaw, d.scale)).collect();
            n.members = members;
            n.built = true;
        }
    }
}

impl Sim {
    /// (testing) A nest of the biome at `at` placed there (its first "nests" pick) and built as
    /// soon as its ground is loaded.
    pub fn spawn_nest_at(&mut self, at: [f32; 3]) -> Option<String> {
        let pop = self.population.as_ref()?;
        let z = zone_at(at[0], at[2]);
        let b = biome_of_zone(&pop.map, &pop.biomes, z.0, z.1)?;
        let data = self.data.clone();
        let rec = data.get(&pop.biomes[b].record)?;
        let roll = self.rng.uniform();
        // (testing) KENSHI_NEST_RECORD=<TOWN name>: that one
        let forced = std::env::var("KENSHI_NEST_RECORD").ok().and_then(|n| data.of_type(kenshi_data::ItemType::Town).find(|t| t.name == n).map(|t| t.string_id.clone()));
        let nest = match forced {
            Some(n) => n,
            None => self.pick_nest(rec, roll)?,
        };
        let name = data.get(&nest).map_or(String::new(), |r| r.name.clone());
        self.nests.push(Nest { record: nest, name: name.clone(), biome: b, at, pieces: vec![], loose: vec![], members: vec![], built: false, wiped: false });
        Some(name)
    }
}
