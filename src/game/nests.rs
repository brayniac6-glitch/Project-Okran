//! Nests in the simulation (kenshi_world::nest): BiomeManager keeps each biome's camps and dens
//! up to its "num nests" (F048131, a nest a population tick), each at a random point of the
//! biome away from towns and the player (F048115). A nest whose ground is loaded is built
//! (F048909) and its "roaming squads" come to live there; one whose people are all dead is
//! wiped out (F048878) and no longer counts, so another rises elsewhere in time.
//!
//! (stand-in) Biomes whose nests stand at fixed markers only (the level's nest markers) get
//! none: the markers are not read. A nest's people are made once, when it is first built.

use crate::sim::Sim;
use kenshi_world::biome::{biome_of_zone, zone_at, zone_centre};
use kenshi_world::nest::{CampPiece, camp_layout, nest_def, pick_nest};
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
            !bio.nests_at_markers && 0 < bio.num_nests && (counts[b] as f32) < bio.num_nests as f32 * nests_mult
        }) else {
            return;
        };
        let data = self.data.clone();
        let biome_name = pop.biomes[b].name.clone();
        let Some(rec) = data.get(&pop.biomes[b].record) else { return };
        let roll = self.rng.uniform();
        let Some(nest) = pick_nest(&data, rec, roll) else {
            // a biome with nests to make but none to make them from: counted as made
            self.nests.push(Nest { record: String::new(), name: String::new(), biome: b, at: [0.0; 3], pieces: vec![], members: vec![], built: true, wiped: false });
            return;
        };
        let Some(at) = self.homeless_spawn_point(b) else { return };
        let name = data.get(&nest).map_or(String::new(), |r| r.name.clone());
        if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
            eprintln!("nest {name} ({nest}) in {biome_name} at {:?}", at.map(|v| v as i32));
        }
        self.nests.push(Nest { record: nest, name, biome: b, at, pieces: vec![], members: vec![], built: false, wiped: false });
    }

    /// @fn F048115
    ///
    /// A spawn point for biome `b`: up to 10 tries of a random zone of the biome that is not
    /// one a player character stands in and holds no town, at a random point of its middle 80%;
    /// else a random zone of the biome.
    /// (stand-in) F039310's height and slope test is not ported.
    fn homeless_spawn_point(&mut self, b: usize) -> Option<[f32; 3]> {
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

    /// The nests whose ground is loaded built and peopled (F048909, the nest's "roaming
    /// squads" spawned at its centre); those whose people are all dead wiped out (F048878).
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
            let pieces = camp_layout(&def, at, &mut |x, z| terrain.height_at(x, z).unwrap_or(at[1]));
            let mut members = Vec::new();
            for (t, count) in &def.squads {
                for _ in 0..*count {
                    let a = self.rng.uniform() * std::f32::consts::TAU;
                    let d = self.rng.uniform() * def.radius * 0.3;
                    let (x, z) = (at[0] + a.cos() * d, at[2] + a.sin() * d);
                    let y = self.terrain.height_at(x, z).unwrap_or(at[1]);
                    if let Some(m) = self.spawn_template(t, [x, y, z]) {
                        members.extend(m);
                    }
                }
            }
            if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
                eprintln!("nest {} built: {} buildings, {} people", def.name, pieces.len(), members.len());
            }
            let n = &mut self.nests[k];
            n.pieces = pieces;
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
        let nest = pick_nest(&data, rec, roll)?;
        let name = data.get(&nest).map_or(String::new(), |r| r.name.clone());
        self.nests.push(Nest { record: nest, name: name.clone(), biome: b, at, pieces: vec![], members: vec![], built: false, wiped: false });
        Some(name)
    }
}
