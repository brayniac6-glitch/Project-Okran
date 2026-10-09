//! The zones of biomes whose name holds a word, with their land share above the water:
//! `cargo run -p kenshi-world --example biome_zones -- <install> <word>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::biome::{BiomeMap, biome_of_zone, biomes};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let word = a[1].to_ascii_lowercase();
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let map = BiomeMap::load(install).expect("biome map");
    let all = biomes(&data);
    for (i, b) in all.iter().enumerate() {
        let Some(r) = data.get(&b.record) else { continue };
        if !r.name.to_ascii_lowercase().contains(&word) {
            continue;
        }
        let mut zones = vec![];
        for y in 0..64 {
            for x in 0..64 {
                if biome_of_zone(&map, &all, x, y) == Some(i) {
                    zones.push(format!("{x}.{y}"));
                }
            }
        }
        println!("{} colour {:06x} ({} zones): {}", r.name, b.index, zones.len(), zones.join(" "));
    }
}
