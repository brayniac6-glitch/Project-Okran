//! Nests (port_map `zones`): the bandit camps, animal dens and nomad camps that BiomeManager
//! places in the wilds (TOWN records named in a biome's "nests"), laid out round their centre
//! and home to their "roaming squads".
//!
//! Specs: F048131 BiomeManager::spawnNests, F038262 a record's weighted reference, F048115
//! Biome::homelessSpawnPoint, F048141 / F048145 the nest count, F048875 Nest (its resident
//! population), F048909 TownBase's camp layout (its loose debris, F048908, then debris buildings
//! round "num centrepoints" centres, F048784 / F048884) on the CRT's rand seeded from the centre
//! (F032248, F050499 srand; F050500..F050502, F050548 the game's random helpers), F048878 a nest
//! wiped out.

use kenshi_data::{GameData, GameDataContainer, ItemType};

/// A nest record as its camp is built from it.
#[derive(Debug, Clone, PartialEq)]
pub struct NestDef {
    /// The TOWN record.
    pub record: String,
    pub name: String,
    pub faction: Option<String>,
    /// "size radius" (units).
    pub radius: f32,
    /// "num centrepoints".
    pub centres: i32,
    /// "debris building": BUILDING, its count (0: a coin toss for one; above 1, give or take a
    /// half), and the distance range from its centre (units).
    pub buildings: Vec<(String, i32, f32, f32)>,
    /// "debris": NEST_ITEM, its count range and size (%; 0: 100).
    pub debris: Vec<(String, i32, i32, i32)>,
    /// "roaming squads": SQUAD_TEMPLATE and how many.
    pub squads: Vec<(String, i32)>,
    /// "nest resident population" (F048875).
    pub population: i32,
}

/// @fn F048909
///
/// A nest's definition from its TOWN record.
pub fn nest_def(r: &GameData) -> NestDef {
    let int = |k: &str| r.ints.get(k).copied().or_else(|| r.floats.get(k).map(|f| *f as i32)).unwrap_or(0);
    let float = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|i| *i as f32)).unwrap_or(0.0);
    NestDef {
        record: r.string_id.clone(),
        name: r.name.clone(),
        faction: r.refs("faction").first().map(|(f, _)| f.clone()),
        radius: float("size radius").max(50.0),
        centres: int("num centrepoints").max(1),
        buildings: r.refs("debris building").iter().map(|(b, v)| (b.clone(), v[0], v[1] as f32, v[2] as f32)).collect(),
        debris: r.refs("debris").iter().map(|(b, v)| (b.clone(), v[0], v[1], v[2])).collect(),
        squads: r.refs("roaming squads").iter().map(|(s, v)| (s.clone(), v[0].max(1))).collect(),
        population: int("nest resident population"),
    }
}

/// @fn F038262
///
/// A biome's nest to spawn: one of its "nests" TOWN references, weighted by the first value
/// (each counted once when none is weighted). `roll` is uniform in [0, 1).
pub fn pick_nest(data: &GameDataContainer, biome: &GameData, roll: f32) -> Option<String> {
    let refs: Vec<(&String, f32)> = biome
        .refs("nests")
        .iter()
        .filter(|(sid, _)| data.get(sid).is_some_and(|t| t.item_type() == Some(ItemType::Town)))
        .map(|(sid, v)| (sid, v[0].max(0) as f32))
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
            return Some((*sid).clone());
        }
        r -= w;
    }
    refs.last().map(|(s, _)| (*s).clone())
}

/// The C runtime's rand() (MSVC: seed x 214013 + 2531011, bits 16..30) as the camp generator
/// uses it, seeded by srand (F050499), with the game's helpers over it.
pub struct CampRng(u32);

impl CampRng {
    /// @fn F032248
    ///
    /// The seed of a camp at `centre`: its x and z truncated to integers, each xor 0xdeadbeef,
    /// combined as boost's hash_combine does (F048909).
    pub fn new(centre: [f32; 3]) -> CampRng {
        let x = (centre[0] as i64 as u32) ^ 0xdead_beef;
        let z = (centre[2] as i64 as u32) ^ 0xdead_beef;
        CampRng(x ^ (x << 6).wrapping_add(z).wrapping_add(x >> 2).wrapping_add(0x9e37_79b9))
    }

    /// rand(): 0 .. 32767.
    pub fn rand(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.0 >> 16) & 0x7fff) as i32
    }

    /// @fn F050500
    ///
    /// rand() / 32768.
    pub fn next(&mut self) -> f32 {
        (self.rand() as f64 * (1.0 / 32768.0)) as f32
    }

    /// @fn F050502
    ///
    /// randRange(a, b): a + (b - a) x next.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        let u = self.next();
        u * (b - a) + a
    }

    /// @fn F050501
    pub fn coin(&mut self) -> bool {
        self.next() > 0.5
    }

    /// @fn F050548
    ///
    /// randInt(a, b): a + the unit (as the game rounds it) times b - a + 1, kept within a .. b.
    pub fn int(&mut self, a: i32, b: i32) -> i32 {
        let u = (self.rand() as f64 * (-1.0 / 32768.0)) as f32;
        let r = a - (u * (b - a + 1) as f32) as i32;
        r.min(b).max(a)
    }
}

/// A building of a camp: record, world position (y from the ground) and turn about y.
pub type CampPiece = (String, [f32; 3], f32);

/// A loose piece of a camp's debris (NEST_ITEM): record, which of its meshes (1 .. "number of
/// meshes"; 0 the plain one), world position, turn about y and size.
#[derive(Debug, Clone, PartialEq)]
pub struct DebrisPiece {
    pub record: String,
    pub mesh: i32,
    pub at: [f32; 3],
    pub yaw: f32,
    pub scale: f32,
}

/// A unit vector on the ground from two randRange(-1, 1) (the first z, the second x), times
/// randRange(0, `reach`).
fn scatter(rng: &mut CampRng, reach: f32) -> [f32; 2] {
    let z = rng.range(-1.0, 1.0);
    let x = rng.range(-1.0, 1.0);
    let l = (x * x + z * z).sqrt();
    let (x, z) = if l > 1e-6 { (x / l, z / l) } else { (0.0, 0.0) };
    let d = rng.range(0.0, reach);
    [x * d, z * d]
}

/// @fn F048908
///
/// `count` of a NEST_ITEM about `at` within `range`, each a cluster: with no "physics file", the
/// cluster's first at a scattered point, then "cluster min" .. "cluster max" of its meshes
/// (each turned by next x 360 degrees, mesh randInt(1, "number of meshes") when more than one,
/// size `scale` + randRange(-0.1, 0.1)), each the next step scattered within "cluster range" x
/// `scale` of the cluster's start; with one, its pieces scattered within "cluster range" and
/// turned. Then its "cluster" refs about the same point: randInt(first, second) of each within
/// its third value.
#[allow(clippy::too_many_arguments)]
fn debris(data: &GameDataContainer, item: &GameData, at: [f32; 3], range: f32, count: i32, scale: f32, rng: &mut CampRng, ground: &mut dyn FnMut(f32, f32) -> f32, out: &mut Vec<DebrisPiece>) {
    let int = |k: &str| item.ints.get(k).copied().unwrap_or(0);
    let physics = item.files.get("physics file").is_some_and(|f| !f.is_empty());
    let meshes = int("number of meshes");
    let (lo, hi, reach) = (int("cluster min"), int("cluster max"), int("cluster range"));
    let mut here = at;
    for _ in 0..count.max(0) {
        let start = here;
        if !physics {
            let d = scatter(rng, range);
            let (x, z) = (start[0] + d[0], start[2] + d[1]);
            here = [x, ground(x, z), z];
            let base = here;
            let n = rng.int(lo, hi);
            for _ in 0..n.max(0) {
                let yaw = (rng.next() * 360.0).to_radians();
                let mesh = if meshes < 2 { 0 } else { rng.int(1, meshes) };
                let size = rng.range(-0.1, 0.1) + scale;
                out.push(DebrisPiece { record: item.string_id.clone(), mesh, at: here, yaw, scale: size });
                let d = scatter(rng, reach as f32 * scale);
                let (x, z) = (base[0] + d[0], base[2] + d[1]);
                here = [x, ground(x, z), z];
            }
        } else {
            let n = rng.int(lo, hi);
            for _ in 0..n.max(0) {
                let d = scatter(rng, reach as f32);
                let (x, z) = (start[0] + d[0], start[2] + d[1]);
                here = [x, ground(x, z), z];
                let yaw = (rng.next() * 360.0).to_radians();
                out.push(DebrisPiece { record: item.string_id.clone(), mesh: 0, at: here, yaw, scale: 1.0 });
            }
        }
        for (c, v) in item.refs("cluster") {
            let n = rng.int(v[0], v[1]);
            if let Some(c) = data.get(c) {
                debris(data, c, here, v[2] as f32, n, scale, rng, ground, out);
            }
        }
    }
}

/// @fn F048884
///
/// `count` of a debris building about `centre`, each a scattered point between `lo` and `hi`
/// out, moved to 25 from any piece placed before within 17; on the walking mesh (`on_mesh`) and
/// no nearer than 12 to another, it stands on the ground turned by next x 360 degrees (the
/// game also tilts it to the ground up to its "max slope"); else it is not built.
#[allow(clippy::too_many_arguments)]
fn place_building(b: &str, centre: [f32; 2], lo: f32, hi: f32, count: i32, rng: &mut CampRng, ground: &mut dyn FnMut(f32, f32) -> f32, on_mesh: &mut dyn FnMut(f32, f32, f32) -> bool, out: &mut Vec<CampPiece>) {
    let mut placed: Vec<[f32; 2]> = Vec::new();
    for _ in 0..count.max(0) {
        let z = rng.range(-1.0, 1.0);
        let x = rng.range(-1.0, 1.0);
        let l = (x * x + z * z).sqrt();
        let (x, z) = if l > 1e-6 { (x / l, z / l) } else { (0.0, 0.0) };
        let d = rng.range(lo, hi);
        let mut p = [centre[0] + x * d, centre[1] + z * d];
        for q in &placed {
            let (dx, dz) = (p[0] - q[0], p[1] - q[1]);
            if dx * dx + dz * dz < 289.0 {
                let l = (dx * dx + dz * dz).sqrt().max(1e-6);
                p = [q[0] + dx / l * 25.0, q[1] + dz / l * 25.0];
            }
        }
        let y = ground(p[0], p[1]);
        if !on_mesh(p[0], y, p[1]) || placed.iter().any(|q| (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) < 144.0) {
            continue;
        }
        let yaw = (rng.next() * 360.0).to_radians();
        out.push((b.to_string(), [p[0], y, p[1]], yaw));
        placed.push(p);
    }
}

/// @fn F048909
///
/// A camp: the generator seeded from its centre; its "debris" (NEST_ITEMs: randInt(first,
/// second) of each within its "size radius", sized its third value %); then "num centrepoints"
/// centres, each scattered within the radius (F048784) and moved to 140 from a centre before
/// within 140, at each every debris building its count of times (0: a coin toss for one; above
/// 1, give or take a half: randInt) placed by F048884. `ground(x, z)` is the terrain's height,
/// `on_mesh(x, y, z)` whether the walking mesh is there. Returns the buildings and the loose
/// debris.
pub fn camp(data: &GameDataContainer, def: &NestDef, centre: [f32; 3], ground: &mut dyn FnMut(f32, f32) -> f32, on_mesh: &mut dyn FnMut(f32, f32, f32) -> bool) -> (Vec<CampPiece>, Vec<DebrisPiece>) {
    const APART: f32 = 140.0;
    let mut rng = CampRng::new(centre);
    let mut loose = Vec::new();
    for (item, lo, hi, size) in &def.debris {
        let n = rng.int(*lo, *hi);
        let scale = if *size == 0 { 1.0 } else { *size as f32 * 0.01 };
        if let Some(r) = data.get(item) {
            debris(data, r, centre, def.radius, n, scale, &mut rng, ground, &mut loose);
        }
    }
    let mut centres: Vec<[f32; 2]> = Vec::new();
    let mut out = Vec::new();
    for _ in 0..def.centres {
        let d = scatter(&mut rng, def.radius);
        let mut p = [centre[0] + d[0], centre[2] + d[1]];
        for q in &centres {
            let (dx, dz) = (p[0] - q[0], p[1] - q[1]);
            if dx * dx + dz * dz < APART * APART {
                let l = (dx * dx + dz * dz).sqrt().max(1e-6);
                p = [q[0] + dx / l * APART, q[1] + dz / l * APART];
            }
        }
        for (b, count, lo, hi) in &def.buildings {
            let n = match *count {
                0 => i32::from(rng.coin()),
                c if c > 1 => c + rng.int(-((c as f32 * 0.5) as i32), (c as f32 * 0.5) as i32),
                c => c,
            };
            centres.push(p);
            place_building(b, p, *lo, *hi, n, &mut rng, ground, on_mesh, &mut out);
        }
    }
    (out, loose)
}

/// The buildings of a camp (`camp`) with every point taken as on the walking mesh.
pub fn camp_layout(data: &GameDataContainer, def: &NestDef, centre: [f32; 3], ground: &mut dyn FnMut(f32, f32) -> f32) -> Vec<CampPiece> {
    camp(data, def, centre, ground, &mut |_, _, _| true).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camp_is_the_same_each_time() {
        let def = NestDef {
            record: "n".into(),
            name: "camp".into(),
            faction: None,
            radius: 400.0,
            centres: 3,
            buildings: vec![("fire".into(), 1, 0.0, 0.0), ("bed".into(), 3, 40.0, 70.0)],
            debris: vec![],
            squads: vec![],
            population: 10,
        };
        let data = GameDataContainer::default();
        let a = camp_layout(&data, &def, [1000.0, 0.0, 2000.0], &mut |_, _| 5.0);
        let b = camp_layout(&data, &def, [1000.0, 0.0, 2000.0], &mut |_, _| 5.0);
        assert_eq!(a, b);
        assert_eq!(a.iter().filter(|p| p.0 == "fire").count(), 3);
        let beds = a.iter().filter(|p| p.0 == "bed").count();
        assert!((6..=12).contains(&beds), "{beds}");
        assert!(a.iter().all(|p| p.1[1] == 5.0));
    }
}
