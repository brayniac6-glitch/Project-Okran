//! Nests (port_map `zones`): the bandit camps, animal dens and nomad camps that BiomeManager
//! places in the wilds (TOWN records named in a biome's "nests"), laid out round their centre
//! and home to their "roaming squads".
//!
//! Specs: F048131 BiomeManager::spawnNests, F038262 a record's weighted reference, F048115
//! Biome::homelessSpawnPoint, F048141 / F048145 the nest count, F048875 Nest (its resident
//! population), F048909 TownBase's camp layout (debris buildings round "num centrepoints"
//! centres), F048878 a nest wiped out.

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

/// A small deterministic generator seeded from a camp's centre, so a camp is laid out the same
/// each time it is built (F048909 seeds the game's generator from the centre: x ^ 0xdeadbeef,
/// z ^ 0xdeadbeef).
/// (stand-in) Kenshi's own generator (F0086ed, F051d66) is not ported: the layouts differ from
/// the game's while following its rules.
pub struct CampRng(u64);

impl CampRng {
    pub fn new(centre: [f32; 3]) -> CampRng {
        let x = (centre[0] as i64 as u32) ^ 0xdead_beef;
        let z = (centre[2] as i64 as u32) ^ 0xdead_beef;
        CampRng(((x as u64) << 32 | z as u64) ^ 0x9e37_79b9_7f4a_7c15)
    }
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    /// Uniform integer in [a, b].
    pub fn int(&mut self, a: i32, b: i32) -> i32 {
        if b <= a {
            return a;
        }
        a + ((self.next() * (b - a + 1) as f32) as i32).min(b - a)
    }
}

/// A building of a camp: record, world position (y from the ground) and turn about y.
pub type CampPiece = (String, [f32; 3], f32);

/// @fn F048909
///
/// A camp's debris buildings: "num centrepoints" centres at random within its radius, each kept
/// 140 units from the ones before (moved out along the line from the nearer); at each centre
/// every debris building its count of times (0: one on a coin toss; above 1, the count give or
/// take a half), each at a random distance in its range and a random bearing, facing the centre.
/// `ground(x, z)` is the terrain's height.
/// (assumption) F029f32's placing (count, distance range) is read from the references' three
/// values; the pieces face their centre; the loose "debris" (NEST_ITEM trash, lootable) and the
/// "loot spawn" are not placed.
pub fn camp_layout(def: &NestDef, centre: [f32; 3], ground: &mut dyn FnMut(f32, f32) -> f32) -> Vec<CampPiece> {
    const APART: f32 = 140.0;
    let mut rng = CampRng::new(centre);
    let mut centres: Vec<[f32; 2]> = Vec::new();
    let mut out = Vec::new();
    for _ in 0..def.centres {
        let a = rng.next() * std::f32::consts::TAU;
        let d = rng.next().sqrt() * def.radius * 0.5;
        let mut p = [centre[0] + a.cos() * d, centre[2] + a.sin() * d];
        for q in &centres {
            let (dx, dz) = (p[0] - q[0], p[1] - q[1]);
            let l = (dx * dx + dz * dz).sqrt();
            if l < APART {
                let (nx, nz) = if l < 1e-3 { (1.0, 0.0) } else { (dx / l, dz / l) };
                p = [q[0] + nx * APART, q[1] + nz * APART];
            }
        }
        centres.push(p);
        for (b, count, lo, hi) in &def.buildings {
            let n = match *count {
                0 => i32::from(rng.next() < 0.5),
                1 => 1,
                c => c + rng.int(-(c as f32 * 0.5) as i32, (c as f32 * 0.5) as i32),
            };
            for _ in 0..n.max(0) {
                let (x, z) = if *hi <= 0.0 {
                    (p[0], p[1])
                } else {
                    let a = rng.next() * std::f32::consts::TAU;
                    let d = lo + (hi - lo).max(0.0) * rng.next();
                    (p[0] + a.cos() * d, p[1] + a.sin() * d)
                };
                let yaw = (p[0] - x).atan2(p[1] - z);
                out.push((b.clone(), [x, ground(x, z), z], if *hi <= 0.0 { rng.next() * std::f32::consts::TAU } else { yaw }));
            }
        }
    }
    out
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
            squads: vec![],
            population: 10,
        };
        let a = camp_layout(&def, [1000.0, 0.0, 2000.0], &mut |_, _| 5.0);
        let b = camp_layout(&def, [1000.0, 0.0, 2000.0], &mut |_, _| 5.0);
        assert_eq!(a, b);
        assert_eq!(a.iter().filter(|p| p.0 == "fire").count(), 3);
        let beds = a.iter().filter(|p| p.0 == "bed").count();
        assert!((6..=12).contains(&beds), "{beds}");
        assert!(a.iter().all(|p| p.1[1] == 5.0));
    }
}
