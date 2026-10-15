//! A long-distance route between two towns, as a squad away from the player takes it:
//! `cargo run -p kenshi-world --example route -- <install> <town> <town> [road preference]`
use kenshi_world::longpath::{DEFAULT_ROAD_PREFERENCE, WorldGrid};
use kenshi_world::{Terrain, town::towns};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let mut order = kenshi_io::mods::Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let ts = towns(install, &data);
    let find = |n: &str| ts.iter().find(|t| t.name == n).unwrap_or_else(|| panic!("no town {n}"));
    let (from, to) = (find(&a[1]), find(&a[2]));
    let pref: f32 = a.get(3).map_or(DEFAULT_ROAD_PREFERENCE, |v| v.parse().unwrap());
    let t0 = std::time::Instant::now();
    let mut terrain = Terrain::open(install).unwrap();
    let grid = WorldGrid::build(&mut terrain).unwrap();
    let hidden = grid.roads.roads.iter().filter(|r| r.hidden).count();
    println!(
        "{} roads ({hidden} hidden), {} junctions, loaded in {:?}",
        grid.roads.roads.len(),
        grid.roads.junctions.len(),
        t0.elapsed()
    );
    let t1 = std::time::Instant::now();
    let r = grid.route_with(from.position, to.position, pref);
    println!("route in {:?}", t1.elapsed());
    println!(
        "{} waypoints, {:.0} units (straight {:.0})",
        r.len(),
        WorldGrid::length(from.position, &r),
        (to.position[0] - from.position[0]).hypot(to.position[2] - from.position[2])
    );
    for p in r.iter().take(12) {
        println!("  {:.0}, {:.0}, {:.0}", p[0], p[1], p[2]);
    }
    if r.len() > 12 {
        println!("  ... {:.0}, {:.0}, {:.0}", r[r.len() - 1][0], r[r.len() - 1][1], r[r.len() - 1][2]);
    }
}
