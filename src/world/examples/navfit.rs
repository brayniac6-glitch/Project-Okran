//! How well navmesh tiles sit on the terrain: for the ground mesh of tile X.Y, the spread of
//! (vertex height - terrain height) in units, and the same for its building meshes.
//! `cargo run -p kenshi-world --example navfit -- <install> X Y`
use kenshi_world::Terrain;
use kenshi_world::navmesh::{METRES, load_tile};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut t = Terrain::open(install).unwrap();
    for s in load_tile(install, x, y).unwrap() {
        let mut err: Vec<f32> = s
            .vertices
            .iter()
            .step_by(7)
            .map(|v| v[1] / METRES - t.height_at(v[0] / METRES, v[2] / METRES).unwrap())
            .collect();
        err.sort_by(f32::total_cmp);
        println!(
            "section {} ({} faces{}): median {:.1} p10 {:.1} p90 {:.1}",
            s.uid,
            s.faces.len(),
            if s.info.is_some() { ", building" } else { "" },
            err[err.len() / 2],
            err[err.len() / 10],
            err[err.len() * 9 / 10]
        );
    }
}
