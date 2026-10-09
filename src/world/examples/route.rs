//! A long-distance route between two towns: `cargo run -p kenshi-world --example route -- <install> <town> <town>`
use kenshi_world::longpath::WorldGrid;
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
    let t0 = std::time::Instant::now();
    let mut terrain = Terrain::open(install).unwrap();
    let grid = WorldGrid::build(&mut terrain).unwrap();
    println!("grid built in {:?}", t0.elapsed());
    let t1 = std::time::Instant::now();
    let r = grid.route(from.position, to.position);
    println!("route in {:?}", t1.elapsed());
    match r {
        Some(r) => {
            println!("{} waypoints, {:.0} units (straight {:.0})", r.len(), WorldGrid::length(from.position, &r), (to.position[0] - from.position[0]).hypot(to.position[2] - from.position[2]));
            for p in r.iter().take(12) {
                println!("  {:.0}, {:.0}, {:.0}", p[0], p[1], p[2]);
            }
        }
        None => println!("no route"),
    }
}
