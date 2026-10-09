//! Zones whose ground dips under the water (height 100, F050961), with how much of the zone does
//! and whether a zone file places buildings there:
//! `cargo run -p kenshi-world --example water_zones -- <install>`
use kenshi_world::{Terrain, zone_file};
use std::path::Path;

fn main() {
    let install = std::env::args().nth(1).expect("install dir");
    let mut t = Terrain::open(Path::new(&install)).expect("heightmap");
    for zy in 0..64u32 {
        for zx in 0..64u32 {
            let p = t.patch(zx * 256, zy * 256, 17, 17, 16).expect("patch");
            let under = p.heights.iter().filter(|&&h| h < 100.0).count();
            if under > 0 {
                let town = zone_file(Path::new(&install), zx as i32, zy as i32).is_some();
                println!("{zx}.{zy} {:.0}% under{}", under as f32 / 2.89, if town { " (zone file)" } else { "" });
            }
        }
    }
}
