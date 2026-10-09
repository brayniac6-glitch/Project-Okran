//! Path between two world points (units), with the faces found:
//! `cargo run -p kenshi-world --example navprobe -- <install> x,y,z x,y,z`
use kenshi_world::navmesh::{METRES, NavWorld, tile_of};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let p = |s: &str| -> [f32; 3] {
        let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        [v[0] * METRES, v[1] * METRES, v[2] * METRES]
    };
    let (s, g) = (p(&a[1]), p(&a[2]));
    let (tx, ty) = tile_of(s);
    let nav = NavWorld::load_around(Path::new(&a[0]), tx, ty, 1).unwrap();
    let fs = nav.closest_point(s, 500.0);
    let fg = nav.closest_point(g, 500.0);
    println!("start {fs:?}\ngoal  {fg:?}");
    if let (Some((f, _)), Some((t, _))) = (fs, fg) {
        println!("reachable {}", nav.reachable(f, t));
    }
    let radius: f32 = a.get(3).map_or(0.4, |r| r.parse().unwrap());
    match nav.find_path(s, g, radius, 1.0) {
        kenshi_world::navmesh::PathResult::Found(f) => {
            println!("found {} points", f.points.len());
            for p in &f.points {
                println!("  {:?}", p.map(|v| (v * 10.0).round()));
            }
            for (l, r) in &f.follow.gates {
                println!(
                    "  gate {:?} {:?}",
                    l.map(|v| (v * 10.0).round()),
                    r.map(|v| (v * 10.0).round())
                );
            }
        }
        r => println!("{r:?}"),
    }
}
