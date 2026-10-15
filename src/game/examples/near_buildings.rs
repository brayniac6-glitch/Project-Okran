//! The placed buildings within a radius of a point, with their furniture count:
//! `cargo run --release -p kenshi-game --example near_buildings -- <install> X Y x,z radius`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let (x, z) = a[3].split_once(',').map(|(x, z)| (x.parse::<f32>().unwrap(), z.parse::<f32>().unwrap())).unwrap();
    let r: f32 = a[4].parse().unwrap();
    let sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    for (k, p) in sim.placed.iter().enumerate() {
        let d = (p.position[0] - x).hypot(p.position[2] - z);
        if d > r {
            continue;
        }
        let name = sim.data.get(&p.building).map_or("?", |b| b.name.as_str());
        let furn: Vec<&str> = sim.furn.iter().filter(|f| f.placement == k).map(|f| f.name.as_str()).collect();
        println!("{k} {name} at ({:.0},{:.0},{:.0}) d {d:.0}: {} furniture {:?}", p.position[0], p.position[1], p.position[2], furn.len(), &furn[..furn.len().min(8)]);
    }
}
