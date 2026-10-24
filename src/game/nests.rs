//! Nests in the simulation (kenshi_world::nest): BiomeManager keeps each biome's camps and dens
//! up to its "num nests" (F048131, a nest a population tick), each at a random point of the
//! biome away from towns and the player (F048115). A nest whose ground is loaded is built
//! (F048909); its population manager (`nests_step`, one nest a step) brings its "roaming
//! squads" to live there a squad at a time as its budget regrows, and finds it wiped out
//! (F048779 / F048878) once it has none it can make and none left able to fight; a wiped out
//! nest no longer counts, so another rises elsewhere in time.
//!
//! A biome whose nests stand at fixed markers only takes the level's nest markers (towns of
//! kind 8).
//!
//! A nest is a town of its own (TownBase): its camp's buildings with a functionality (sitting
//! spots and stools, camp beds, the campfire, torches) are its furniture (`register_camp`, as
//! a zone's are, when built and again after a load), and its squads' package handler runs at
//! it (Package_HangAroundBar +0x10, F029191): "Patrol hometown" goes round the nest and an
//! outdoors-only handler (signal func 26 / 27, the nests' "At home" packages) seats them on its
//! sitting spots (`outdoor_chair`).

use crate::sim::Sim;
use kenshi_world::biome::{biome_of_zone, zone_at, zone_centre};
use kenshi_world::nest::{CampPiece, nest_def};
use kenshi_world::ZONE_SIZE;
use serde::{Deserialize, Serialize};

/// A nest's furniture stands in no zone placement: its `placement` is this plus the nest's
/// index.
pub const NEST_PLACEMENT: usize = 1 << 40;

/// A nest's radius (Nest vtable 0x2a0, F048767: 625 units; none for a type 8 one).
pub const NEST_RADIUS: f32 = 625.0;

/// The height Kenshi gives where it finds no ground (F050534: -99).
pub const NO_HEIGHT: f32 = -99.0;


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
    /// Its camp's furniture (indices into `Sim::furn`), registered once built (not saved: again
    /// after a load).
    #[serde(skip)]
    pub furniture: Vec<usize>,
    #[serde(skip)]
    pub registered: bool,
    /// Its population manager (Nest +0xe0).
    #[serde(default)]
    pub pop: NestPop,
}

/// @fn F048875
/// @fn F048143
///
/// A nest's population manager (NestPopulationManager, BasePopulationManager's lists 1 and 2,
/// saved as "pop" / "popd", "pop2" / "popd2", "ded", "started", F048149 / F048150): each list's
/// target (+0x08) and budget (+0x0c) and when its budget last grew (its +0x00, game hours). The
/// first list 0 and 0 for a nest; the second the record's "nest resident population", its
/// budget 0.7 of that (F048875 over F048143's faction "roaming population").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NestPop {
    pub target: [i32; 2],
    pub budget: [f32; 2],
    pub stamp: [f64; 2],
    pub dead: bool,
    pub started: bool,
    /// Made from the record (an older save's nests are made as they come up).
    pub made: bool,
}

impl NestPop {
    pub fn for_record(r: &kenshi_data::GameData) -> NestPop {
        let n = nest_def(r).population;
        NestPop { target: [0, n], budget: [0.0, n as f32 * 0.7], stamp: [0.0; 2], dead: false, started: false, made: true }
    }

    /// @fn F048096
    ///
    /// List `l`'s budget regrown: the game hours since its stamp x 3600 / 33 x 0.5, x 0.004,
    /// added while under the target and held to it; the stamp now.
    pub fn regrow(&mut self, l: usize, now: f64) {
        let units = ((now - self.stamp[l]) * 3600.0 / 33.0 * 0.5) as f32;
        self.stamp[l] = now;
        if 0.0 <= units {
            let target = self.target[l] as f32;
            if self.budget[l] < target {
                self.budget[l] = (units * 0.004 + self.budget[l]).min(target);
            }
        }
    }
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
            self.nests.push(Nest { record: String::new(), name: String::new(), biome: b, at: [0.0; 3], pieces: vec![], loose: vec![], members: vec![], built: true, wiped: false, furniture: vec![], registered: false, pop: NestPop::default() });
            return;
        };
        let Some(mut at) = self.homeless_spawn_point(b) else { return };
        // F048858: the nest's place on the ground there, when the ground is known
        if let Some(h) = self.terrain.height_at(at[0], at[2]).ok().filter(|&h| h != NO_HEIGHT) {
            at[1] = h;
        }
        let name = data.get(&nest).map_or(String::new(), |r| r.name.clone());
        if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
            eprintln!("nest {name} ({nest}) in {biome_name} at {:?}", at.map(|v| v as i32));
        }
        self.nests.push(Nest { record: nest, name, biome: b, at, pieces: vec![], loose: vec![], members: vec![], built: false, wiped: false, furniture: vec![], registered: false, pop: NestPop::default() });
        // F048805: a nest joins its faction's war manager list as it is made
        self.war_town_moved(crate::world::Place::Nest(self.nests.len() - 1));
    }

    /// @fn F048115
    ///
    /// A spawn point for biome `b`: up to 10 tries of a random zone of the biome (randInt over
    /// its zones) that is not the camera's zone, not flagged as a town's (`zone_flagged`) and
    /// with no town of its own (F048114), at a spawn point of it (F048102 with the camera kept
    /// clear, towns' radius x 1.4 and other factions' nests' x 3); else a random zone of the
    /// biome and its spawn point (towns' and nests' radius x 1), or its centre when it has none.
    ///
    /// A biome with "nests at fixed markers only" (+0x04): one of its nest markers at random
    /// (F048808: the towns of kind 8 whose biome, Town +0x1a0, is it: the biome of the zone the
    /// town stands in, set with its position, F048858), leaving out those with a town (other
    /// than a marker) or a nest within 2500 units (F048793: TownList's towns, then its second
    /// list, the TownBases that are no Town, F048868: the biomes' nests); none left: no nest.
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
            let k = self.rand_int(0, markers.len() as i32 - 1) as usize;
            return Some(markers[k]);
        }
        let zones = self.biome_zones(b);
        if zones.is_empty() {
            return None;
        }
        let camera = self.camera_at().map(|c| zone_at(c[0], c[2]));
        for _ in 0..10 {
            let z = zones[self.rand_int(0, zones.len() as i32 - 1) as usize];
            if Some(z) != camera
                && !self.zone_flagged(z)
                && self.zone_town(z).is_none()
                && let Some(p) = self.zone_spawn_point(z, true, None, 1.4, 3.0)
            {
                return Some(p);
            }
        }
        let z = zones[self.rand_int(0, zones.len() as i32 - 1) as usize];
        let (cx, cz) = zone_centre(z.0, z.1);
        Some(self.zone_spawn_point(z, true, None, 1.0, 1.0).unwrap_or([cx, 0.0, cz]))
    }

    /// @fn F050548
    ///
    /// randInt: a whole number from `lo` to `hi`, both in (clamped to them).
    pub(crate) fn rand_int(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        (lo + (self.rng.uniform() * (hi - lo + 1) as f32) as i32).clamp(lo, hi)
    }

    /// Where the camera looks (the game's camera focus, world units). (harness) Run headless,
    /// with no camera, the first player character stands for it.
    pub(crate) fn camera_at(&self) -> Option<[f32; 3]> {
        self.camera_focus.or_else(|| self.folk.iter().find(|p| p.player && !p.dead).map(|p| p.mover.position_units()))
    }

    /// @fn F048904
    ///
    /// A zone flagged as a town's (Zone +0x30): Town::setup flags the zone each town not of
    /// type 4 or 9 stands in (its zone list is empty then: the zone at its position).
    pub(crate) fn zone_flagged(&self, z: (i32, i32)) -> bool {
        self.towns.iter().any(|t| t.kind != 4 && t.kind != 9 && zone_at(t.position[0], t.position[2]) == z)
    }

    /// @fn F048114
    ///
    /// A zone's own town: the town nearest its centre that stands in it, not a marker (type 8).
    pub(crate) fn zone_town(&self, z: (i32, i32)) -> Option<usize> {
        let (cx, cz) = zone_centre(z.0, z.1);
        (0..self.towns.len())
            .filter(|&t| self.towns[t].kind != 8 && zone_at(self.towns[t].position[0], self.towns[t].position[2]) == z)
            .min_by(|&a, &b| {
                let d = |t: usize| (self.towns[t].position[0] - cx).powi(2) + (self.towns[t].position[2] - cz).powi(2);
                d(a).total_cmp(&d(b))
            })
    }

    /// @fn F048099
    ///
    /// Whether a spawn point is good ground: the terrain there not below 95 (its height unknown,
    /// -99, passes) and the areas map coloured there (ZoneManager's map, the pixel at the point
    /// not black).
    fn spawn_ground_ok(&mut self, p: [f32; 3]) -> bool {
        let h = self.terrain.height_at(p[0], p[2]).unwrap_or(NO_HEIGHT);
        if h < 95.0 && h != NO_HEIGHT {
            return false;
        }
        self.population.as_ref().is_some_and(|pop| pop.map.colour_at(p[0], p[2]) != 0)
    }

    /// @fn F048792
    ///
    /// The nearest nest to `p` (TownList's second list: the biomes' nests, F048868), of
    /// `faction` when given.
    pub(crate) fn nearest_nest(&self, p: [f32; 3], faction: Option<&str>) -> Option<usize> {
        (0..self.nests.len())
            .filter(|&k| !self.nests[k].wiped && !self.nests[k].record.is_empty())
            .filter(|&k| faction.is_none_or(|f| self.nest_faction(k).as_deref() == Some(f)))
            .min_by(|&a, &b| {
                let d = |k: usize| (self.nests[k].at[0] - p[0]).powi(2) + (self.nests[k].at[2] - p[2]).powi(2);
                d(a).total_cmp(&d(b))
            })
    }

    /// A nest's faction (its TOWN record's "faction", the Nest's +0x10).
    pub(crate) fn nest_faction(&self, k: usize) -> Option<String> {
        self.data.get(&self.nests[k].record).and_then(|r| r.refs("faction").first().map(|(f, _)| f.clone()))
    }

    /// Whether nest `k` holds `p` within its radius x `mult` (F048764 with the Nest's radius:
    /// 625, none for a type 8 one, F048767).
    fn nest_contains(&self, k: usize, p: [f32; 3], mult: f32) -> bool {
        let kind = self.data.get(&self.nests[k].record).and_then(|r| r.ints.get("type").copied()).unwrap_or(0);
        let r = if kind == 8 { 0.0 } else { NEST_RADIUS } * mult;
        (self.nests[k].at[0] - p[0]).powi(2) + (self.nests[k].at[2] - p[2]).powi(2) < r * r
    }

    /// @fn F048102
    /// @fn F048091
    ///
    /// Zone::spawnPoint: up to 4 random points of zone `z` (randRange over its square, z then
    /// x, F050502), each tried 10 times: inside the nearest town (any but a marker, F048796)
    /// within its radius x `town_mult` it is pushed out to that (F048091); else inside the
    /// nearest nest (F048792) of another faction than `faction` it is pushed out to the nest's
    /// radius x `nest_mult` (both only with `town_mult` over 0.01); else, with `avoid_camera`,
    /// within 1382.4 units of the camera pushed out to that; else taken if on good ground
    /// (F048099). The zone loaded: the point on the ground, out of a town it stands in (to the
    /// town's radius), on the navmesh within 600 units (F032179); then, with `avoid_camera`,
    /// none in a town (other than type 4); nor one whose gate code (F032176, `NavWorld::code_at`)
    /// is above 0: inside a building's region.
    pub(crate) fn zone_spawn_point(&mut self, z: (i32, i32), avoid_camera: bool, faction: Option<&str>, town_mult: f32, nest_mult: f32) -> Option<[f32; 3]> {
        const CAMERA_CLEAR: f32 = 1382.4;
        let lo = |k: i32| k as f32 * ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
        let (x0, z0, x1, z1) = (lo(z.0), lo(z.1), lo(z.0 + 1), lo(z.1 + 1));
        let camera = self.camera_at();
        let push = |p: &mut [f32; 3], c: [f32; 3], d: f32| {
            let (dx, dz) = (p[0] - c[0], p[2] - c[2]);
            let l = dx.hypot(dz);
            let (ux, uz) = if l > 0.0 { (dx / l, dz / l) } else { (0.0, 0.0) };
            *p = [c[0] + ux * d, c[1], c[2] + uz * d];
        };
        let mut found = None;
        'outer: for _ in 0..4 {
            let pz = z0 + self.rng.uniform() * (z1 - z0);
            let px = x0 + self.rng.uniform() * (x1 - x0);
            let mut p = [px, NO_HEIGHT, pz];
            for _ in 0..10 {
                if town_mult > 0.01 {
                    if let Some(t) = kenshi_world::town::nearest(&self.towns, p, 10, None)
                        && self.towns[t].contains(p, town_mult)
                    {
                        let (c, r) = (self.towns[t].position, self.towns[t].radius() * town_mult);
                        push(&mut p, c, r);
                        continue;
                    }
                    if let Some(k) = self.nearest_nest(p, None)
                        && (faction.is_none() || self.nest_faction(k).as_deref() != faction)
                        && self.nest_contains(k, p, 1.0)
                    {
                        push(&mut p, self.nests[k].at, NEST_RADIUS * nest_mult);
                        continue;
                    }
                }
                if avoid_camera
                    && let Some(c) = camera
                    && (c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2) + (c[2] - p[2]).powi(2) < CAMERA_CLEAR * CAMERA_CLEAR
                {
                    push(&mut p, c, CAMERA_CLEAR);
                    continue;
                }
                if self.spawn_ground_ok(p) {
                    found = Some(p);
                    break 'outer;
                }
            }
        }
        let mut p = found?;
        // (Zone +0xb0: the zone loaded, `live_zones`)
        if self.live_zones.contains_key(&z) {
            p[1] = self.terrain.height_at(p[0], p[2]).unwrap_or(NO_HEIGHT);
            if town_mult > 0.01
                && let Some(t) = kenshi_world::town::nearest(&self.towns, p, 10, None)
                && self.towns[t].contains(p, 1.0)
            {
                let (c, r) = (self.towns[t].position, self.towns[t].radius());
                push(&mut p, c, r);
            }
            if !self.live_zones.contains_key(&zone_at(p[0], p[2])) {
                return None;
            }
            if let Some((_, q)) = self.nav.closest_point(p.map(|v| v * kenshi_world::navmesh::METRES), 600.0 * kenshi_world::navmesh::METRES) {
                p = q.map(|v| v / kenshi_world::navmesh::METRES);
            }
        }
        if avoid_camera
            && let Some(t) = kenshi_world::town::nearest(&self.towns, p, 10, None)
            && self.towns[t].kind != 4
            && self.towns[t].contains(p, 1.0)
        {
            return None;
        }
        // F032176: none behind a door (the gate code there above 0: a building's region)
        if 0 < self.nav.code_at(p.map(|v| v * kenshi_world::navmesh::METRES)) {
            return None;
        }
        Some(p)
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

    /// The nests whose ground is loaded built (F048909). Their people come from each nest's
    /// population manager (`nests_step`), which also finds a nest wiped out.
    pub fn nests_tick(&mut self) {
        let data = self.data.clone();
        for k in 0..self.nests.len() {
            let n = &self.nests[k];
            if n.wiped || n.record.is_empty() {
                continue;
            }
            if n.built {
                if !n.registered {
                    self.register_camp(k);
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
                eprintln!("nest {} built: {} buildings", def.name, pieces.len());
            }
            let n = &mut self.nests[k];
            n.pieces = pieces;
            n.loose = loose.into_iter().map(|d| (d.record, d.mesh, d.at, d.yaw, d.scale)).collect();
            n.built = true;
            self.register_camp(k);
        }
    }

    /// @fn F048870
    ///
    /// TownList::update's nests (its second list): one a step, in turn (0x142135438), has its
    /// update (Nest vtable +0xe8, F048778): its population manager's (`nest_update`).
    pub fn nests_step(&mut self) {
        let list: Vec<usize> = (0..self.nests.len()).filter(|&k| !self.nests[k].record.is_empty()).collect();
        if list.is_empty() {
            return;
        }
        if list.len() <= self.nest_turn {
            self.nest_turn = 0;
        }
        let k = list[self.nest_turn];
        self.nest_turn += 1;
        if !self.nests[k].pop.made
            && let Some(r) = self.data.get(&self.nests[k].record)
        {
            let mut pop = NestPop::for_record(r);
            // (an older save's nest: its people made at once as it was built)
            pop.started = !self.nests[k].members.is_empty();
            self.nests[k].pop = pop;
        }
        self.nest_update(k);
    }

    /// @fn F048117
    /// @fn F048139
    ///
    /// A nest's population manager's update: its first list's budget regrown (F048096) and,
    /// that one within 1 of its target, its second's; then with the nest's zone loaded (Zone
    /// +0xb1, F051553: the port's zone in and its navmesh with it) and the nest of type 0 not
    /// dead (+0x08), its second list topped up (F048138).
    fn nest_update(&mut self, k: usize) {
        let now = self.hours_gone();
        let pop = &mut self.nests[k].pop;
        pop.regrow(0, now);
        if pop.budget[0] >= pop.target[0] as f32 - 1.0 {
            pop.regrow(1, now);
        }
        let at = self.nests[k].at;
        let z = zone_at(at[0], at[2]);
        if !self.live_zones.contains_key(&z) || !self.nav.tiles.contains(&z) {
            return;
        }
        let kind = self.data.get(&self.nests[k].record).and_then(|r| r.ints.get("type").copied()).unwrap_or(0);
        if kind == 0 && !self.nests[k].pop.dead {
            self.nest_top_up(k);
        }
    }

    /// @fn F048138
    /// @fn F048134
    ///
    /// A nest's second list topped up: its "roaming squads" (SpawnInfoList::load, F048166: the
    /// ref's first value the weight, kept above 0; its second the least room it needs; the
    /// template's faction, else 3943-gamedata.base) weighed (F048134, ratio and factor 1): one
    /// whose world states hold, when the room (the budget less the people standing, F048121:
    /// each squad's living, F044491) is at least its least, or over 4 with nobody left; its
    /// weight less the squads of it its faction has loaded here not all down (F041595,
    /// F044493), times the faction's spawn factor (1, F030216). None: unless one of its squads
    /// can still fight (F044612) or one is gone, the nest is dead (F048779) and wiped out
    /// (vtable +0x338, F048878). Else while the budget (floored) is above the people standing:
    /// not with the camera within 1250 (squared, as Kenshi compares it) of the nest nor any of
    /// the player's within 1250 units (F044287), one drawn by weight (randRange(0, total) along
    /// the running sums) is made at the nest's centre, homed there (F038259, list 2).
    fn nest_top_up(&mut self, k: usize) {
        let data = self.data.clone();
        let Some(rec) = data.get(&self.nests[k].record) else { return };
        let entries: Vec<(String, i32, i32, String)> = rec
            .refs("roaming squads")
            .iter()
            .filter(|(sq, v)| v[0] > 0 && data.get(sq).is_some_and(|t| t.item_type() == Some(kenshi_data::ItemType::SquadTemplate)))
            .map(|(sq, v)| {
                let faction = data.get(sq).and_then(|t| t.refs("faction").first().map(|(f, _)| f.clone())).unwrap_or_else(|| kenshi_world::biome::DEFAULT_SPAWN_FACTION.to_string());
                (sq.clone(), v[0], v[1], faction)
            })
            .collect();
        let members = self.nests[k].members.clone();
        let mut platoons: Vec<usize> = members.iter().filter_map(|&m| self.folk.get(m).and_then(|p| p.platoon)).collect();
        platoons.sort_unstable();
        platoons.dedup();
        let standing = members.iter().filter(|&&m| self.folk.get(m).is_some_and(|p| !p.dead)).count() as i32;
        let budget = self.nests[k].pop.budget[1];
        let room = (budget - standing as f32) as i32;
        let first = standing == 0;
        let mut candidates: Vec<(String, f32)> = Vec::new();
        for (template, weight, least, faction) in &entries {
            let Some(t) = data.get(template) else { continue };
            if !self.template_states_hold(t) {
                continue;
            }
            if !(*least <= room || (4 < room && first)) {
                continue;
            }
            let folk = &self.folk;
            let out = platoons
                .iter()
                .filter(|&&pid| {
                    let of: Vec<usize> = members.iter().copied().filter(|&m| folk[m].platoon == Some(pid)).collect();
                    self.squad_template.get(&pid) == Some(template)
                        && of.iter().any(|&m| folk[m].faction.as_deref() == Some(faction.as_str()) && !folk[m].away)
                        && of.iter().any(|&m| !folk[m].dead && !folk[m].down)
                })
                .count();
            let w = (*weight as f32 - out as f32).max(0.0);
            if 0.0 < w {
                candidates.push((template.clone(), w));
            }
        }
        if candidates.is_empty() {
            if platoons.iter().any(|pid| self.cells.of.get(pid).is_some_and(|e| e.able)) {
                return;
            }
            let name = self.nests[k].name.clone();
            self.nests[k].pop.dead = true;
            self.nests[k].wiped = true;
            eprintln!("{name} has been wiped out");
            self.messages.push(format!("{name} has been wiped out"));
            return;
        }
        if budget.floor() <= standing as f32 {
            return;
        }
        let at = self.nests[k].at;
        if let Some(c) = self.camera_at()
            && (c[0] - at[0]).powi(2) + (c[1] - at[1]).powi(2) + (c[2] - at[2]).powi(2) < 1250.0
        {
            return;
        }
        if self.folk.iter().any(|p| {
            let q = p.mover.position_units();
            p.player && !p.away && (q[0] - at[0]).hypot(q[2] - at[2]) <= 1250.0
        }) {
            return;
        }
        let pick = if candidates.len() == 1 {
            0
        } else {
            let total: f32 = candidates.iter().map(|(_, w)| w).sum();
            let r = self.rng.next() * total;
            let mut run = 0.0;
            candidates
                .iter()
                .position(|(_, w)| {
                    run += w;
                    r < run
                })
                .unwrap_or(candidates.len() - 1)
        };
        let template = candidates[pick].0.clone();
        if let Some(made) = self.spawn_template(&template, at) {
            if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
                eprintln!("nest {}: squad {template} ({} people)", self.nests[k].name, made.len());
            }
            let radius = nest_def(rec).radius;
            let platoons: Vec<usize> = made.iter().filter_map(|&i| self.folk[i].platoon).collect();
            for pl in platoons {
                if let Some(Some(b)) = self.bar_states.get_mut(pl) {
                    b.town = Some((at, radius));
                }
            }
            self.nests[k].members.extend(made);
            self.nests[k].pop.started = true;
        }
    }

    /// Nest `k`'s camp as a town's: its buildings with a functionality become furniture (their
    /// function, operators and use range, F029399; an approach point on the navmesh), and its
    /// squads' package handlers run at it (+0x10: the nest's centre and radius).
    fn register_camp(&mut self, k: usize) {
        let data = self.data.clone();
        let m = kenshi_world::navmesh::METRES;
        let pieces = self.nests[k].pieces.clone();
        let mut added = Vec::new();
        for (record, at, yaw) in &pieces {
            let Some(b) = data.get(record) else { continue };
            let Some((function, max_operators, use_range)) = kenshi_world::furniture::functionality(&data, b) else { continue };
            let (s, c) = (yaw * 0.5).sin_cos();
            let approach = self.nav.closest_point(at.map(|v| v * m), 50.0).map_or(*at, |(_, q)| q.map(|v| v / m));
            self.furniture_approach.push(approach);
            self.operators.push(None);
            self.bed_taken.push(None);
            added.push(self.furn.len());
            self.furn.push(kenshi_world::furniture::Furniture {
                function,
                max_operators,
                use_range,
                position: *at,
                rotation: [c, 0.0, s, 0.0],
                record: record.clone(),
                name: b.name.clone(),
                placement: NEST_PLACEMENT + k,
            });
        }
        let radius = data.get(&self.nests[k].record).map_or(1000.0, |r| nest_def(r).radius);
        let at = self.nests[k].at;
        let platoons: Vec<usize> = self.nests[k].members.iter().filter_map(|&i| self.folk.get(i).and_then(|p| p.platoon)).collect();
        for pl in platoons {
            if let Some(Some(b)) = self.bar_states.get_mut(pl) {
                b.town = Some((at, radius));
            }
        }
        if std::env::var("KENSHI_NEST_DEBUG").is_ok() {
            eprintln!("nest {}: {} pieces of furniture", self.nests[k].name, added.len());
        }
        self.nests[k].furniture = added;
        self.nests[k].registered = true;
    }
}

/// @fn F029228
///
/// An outdoors-only handler's chair for `me` (Package_HangAroundBar +0x151: no bar, F029219):
/// the one it holds while nobody else sits there; else of the sitting furniture (function 17)
/// out of doors of the town or nest it runs at (F029215's search drops those inside a
/// building: here a nest's, or one standing alone as its own placement), within its radius,
/// those no other member holds and nobody else uses, the nearest to its centre.
/// (assumption) With no town or nest known, the chairs within 2000 units of `me` (`at`).
pub fn outdoor_chair(state: &crate::sim::BarState, me: usize, furniture: &[kenshi_world::furniture::Furniture], operators: &[Option<usize>], placed_at: &[[f32; 3]], at: [f32; 3]) -> Option<usize> {
    let (centre, radius) = state.town.unwrap_or((at, 2000.0));
    let outdoors = |f: &kenshi_world::furniture::Furniture| {
        f.placement >= NEST_PLACEMENT || placed_at.get(f.placement).is_some_and(|p| (p[0] - f.position[0]).hypot(p[2] - f.position[2]) < 1.0)
    };
    let free = |i: usize| operators.get(i).copied().flatten().is_none_or(|o| o == me);
    if let Some(&h) = state.held.get(&me)
        && h < furniture.len()
        && free(h)
    {
        return Some(h);
    }
    let others: Vec<usize> = state.held.iter().filter(|(m, _)| **m != me).map(|(_, c)| *c).collect();
    (0..furniture.len())
        .filter(|&i| {
            let f = &furniture[i];
            f.function == kenshi_world::furniture::BF_CHAIR && outdoors(f) && free(i) && !others.contains(&i) && (f.position[0] - centre[0]).hypot(f.position[2] - centre[2]) <= radius
        })
        .min_by(|&a, &b| {
            let d = |i: usize| (furniture[i].position[0] - centre[0]).powi(2) + (furniture[i].position[2] - centre[2]).powi(2);
            d(a).total_cmp(&d(b))
        })
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
        self.nests.push(Nest { record: nest, name: name.clone(), biome: b, at, pieces: vec![], loose: vec![], members: vec![], built: false, wiped: false, furniture: vec![], registered: false, pop: NestPop::default() });
        // F048805: a nest joins its faction's war manager list as it is made
        self.war_town_moved(crate::world::Place::Nest(self.nests.len() - 1));
        Some(name)
    }
}
