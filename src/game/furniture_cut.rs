//! The navmesh cut round the towns' furniture: Kenshi gives each furniture BUILDING a "path
//! mode" (0 NAVMESH_IGNORE, 1 NAVMESH_PROJECTED, 2 NAVMESH_OBSTACLE, 3 NAVMESH_WALKABLE); those of
//! 1 and 2 (beds, tables, counters, storage, machines) are cut out of the navmesh, so characters
//! walk round them, inside buildings too ([`kenshi_world::navcut`]).
//! (stand-in) Cut by their parts' collision as [`kenshi_render::zone::building_footprints`]
//! gives it (the meshes' bounds for a part without one), not Havok's projection.

use crate::sim::Sim;
use bevy::math::{Quat, Vec3};
use kenshi_io::resources::Resources;
use kenshi_world::furniture::BF_CAGE;
use kenshi_world::navcut::{Cutter, PAD};
use kenshi_world::navmesh::METRES;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The cutters of the sim's furniture (each piece a group of its own, numbered on from `next`);
/// a cage's half size is kept for its door (`Sim::cage_reach`). `only_cages`: (testing) the
/// cages alone.
pub fn furniture_cutters(sim: &mut Sim, res: &Resources, root: &Path, next: &mut u32, only_cages: bool) -> Vec<Cutter> {
    furniture_cutters_each(sim, res, root, next, only_cages).into_iter().flat_map(|(_, c)| c).collect()
}

/// The key a piece of furniture's cutters are given (`NavWorld::add_cutters`).
pub fn furniture_key(k: usize) -> (i64, i64) {
    (-3, k as i64)
}

/// How many faces a spot must reach to count as open (not shut in).
const OPEN_FACES: usize = 300;

/// Cut the navmesh round the sim's furniture, each piece under its own key
/// ([`furniture_key`]). (stand-in for the Havok character squeezing through) A spot people go to
/// (a node, a piece of furniture's place to use it) that the cut shuts in (tightly packed
/// furniture: a shopkeeper behind the counter) has the furniture round it left uncut. Returns
/// (pieces cut, pieces left uncut).
pub fn cut_furniture(sim: &mut Sim, res: &Resources, root: &Path, next: &mut u32, only_cages: bool) -> (usize, usize) {
    let t0 = std::time::Instant::now();
    let each = furniture_cutters_each(sim, res, root, next, only_cages);
    let t_shapes = t0.elapsed().as_secs_f64();
    // the spots, and whether each was open before
    let spots: Vec<[f32; 3]> = sim.approach.iter().chain(sim.furniture_approach.iter()).map(|p| p.map(|v| v * METRES)).collect();
    // (reached by a person: no gap narrower than one, 0.4 m of radius, the path search's rule)
    let open = |nav: &kenshi_world::navmesh::NavWorld, p: [f32; 3]| nav.closest_point(p, 1.0).is_none_or(|(f, _)| nav.flood_wide(f, OPEN_FACES, 0.4) >= OPEN_FACES);
    let before: Vec<bool> = spots.iter().map(|p| open(&sim.nav, *p)).collect();
    let t_before = t0.elapsed().as_secs_f64();
    // each piece's box (x, z metres) for finding those round a spot
    let boxes: Vec<(usize, [f32; 4])> = each
        .iter()
        .map(|(k, cs)| {
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for c in cs {
                for p in &c.outline {
                    b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
                }
            }
            (*k, b)
        })
        .collect();
    let n = each.len();
    sim.nav.add_cutters_many(each.into_iter().map(|(k, c)| (furniture_key(k), c)).collect());
    let t_cut = t0.elapsed().as_secs_f64();
    let mut freed: HashSet<usize> = HashSet::new();
    let gap = |b: &[f32; 4], p: &[f32; 3]| {
        let dx = (b[0] - p[0]).max(p[0] - b[2]).max(0.0);
        let dz = (b[1] - p[2]).max(p[2] - b[3]).max(0.0);
        dx.hypot(dz)
    };
    // a few rounds freeing the piece nearest each spot still shut in, then (the last) every
    // piece within 2 m of one
    for round in 0..5 {
        let mut free = Vec::new();
        for (i, p) in spots.iter().enumerate() {
            if !before[i] || open(&sim.nav, *p) {
                continue;
            }
            let mut near: Vec<(f32, usize)> = boxes.iter().filter(|(k, _)| !freed.contains(k)).map(|(k, b)| (gap(b, p), *k)).filter(|(d, _)| *d < 2.0).collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            let take = if round < 4 { 1 } else { near.len() };
            for (_, k) in near.into_iter().take(take) {
                if freed.insert(k) {
                    free.push(furniture_key(k));
                }
            }
        }
        if free.is_empty() {
            break;
        }
        sim.nav.remove_cutters_many(&free);
    }
    if std::env::var("KENSHI_NAV_DEBUG").is_ok() {
        eprintln!(
            "furniture cut: {n} pieces, {} left uncut to keep spots open; {} spots; shapes {t_shapes:.1} s, spots {:.1} s, cut {:.1} s, freeing {:.1} s",
            freed.len(),
            spots.len(),
            t_before - t_shapes,
            t_cut - t_before,
            t0.elapsed().as_secs_f64() - t_cut
        );
    }
    (n, freed.len())
}

/// Each piece of furniture's cutters (`furniture_cutters`), by its index.
fn furniture_cutters_each(sim: &mut Sim, res: &Resources, root: &Path, next: &mut u32, only_cages: bool) -> Vec<(usize, Vec<Cutter>)> {
    let mut out = Vec::new();
    let mut shapes: HashMap<String, Vec<Vec<Vec3>>> = HashMap::new();
    for k in 0..sim.furn.len() {
        let f = sim.furn[k].clone();
        if only_cages && f.function != BF_CAGE {
            continue;
        }
        let Some(b) = sim.data.get(&f.record) else { continue };
        let mode = b.ints.get("path mode").copied().unwrap_or(0);
        if !(mode == 1 || mode == 2) {
            continue;
        }
        let feet = shapes.entry(f.record.clone()).or_insert_with(|| kenshi_render::zone::building_footprints(&sim.data, res, root, b));
        if feet.is_empty() {
            continue;
        }
        let rot = Quat::from_xyzw(f.rotation[1], f.rotation[2], f.rotation[3], f.rotation[0]);
        let pos = Vec3::from(f.position);
        if f.function == BF_CAGE {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for p in feet.iter().flatten() {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            sim.cage_reach.insert(k, (hi.x - lo.x).max(hi.z - lo.z) * 0.5);
        }
        *next += 1;
        if std::env::var("KENSHI_FURNITURE_CUT_DEBUG").is_ok() {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for p in feet.iter().flatten() {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            eprintln!("furniture cut {k} {} ({} shapes): size {:.0} x {:.0} x {:.0} at {:?}", f.name, feet.len(), hi.x - lo.x, hi.y - lo.y, hi.z - lo.z, f.position.map(|v| v as i32));
        }
        // (one outline round all its shapes: cut once, not once a shape, a face is cut into far
        // fewer pieces)
        let m: Vec<[f32; 3]> = feet.iter().flatten().map(|p| ((rot * *p + pos) * METRES).to_array()).collect();
        let cutters: Vec<Cutter> = Cutter::from_points(&m, PAD)
            .map(|mut c| {
                c.group = *next;
                c.furniture = true;
                c
            })
            .into_iter()
            .collect();
        out.push((k, cutters));
    }
    out
}
