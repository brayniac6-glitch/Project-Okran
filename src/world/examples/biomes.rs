//! The biome of zones round one: `cargo run -p kenshi-world --example biomes -- <install> X Y`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::biome::{BiomeMap, biome_of_zone, biomes, zone_centre};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let map = BiomeMap::load(install).expect("biome map");
    let all = biomes(&data);
    let matched = all.iter().filter(|b| b.index != -1).count();
    println!(
        "{} biomes ({matched} with an index), map {}x{}",
        all.len(),
        map.width,
        map.height
    );
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (x, y) = (zx + dx, zy + dy);
            let (cx, cz) = zone_centre(x, y);
            let c = map.colour_at(cx, cz);
            let b = biome_of_zone(&map, &all, x, y).map(|i| &all[i]);
            println!(
                "zone {x}.{y} colour {c:06x}: {} (amount {}, {} spawns)",
                b.map_or("-", |b| b.name.as_str()),
                b.map_or(0.0, |b| b.homeless_spawn_amount),
                b.map_or(0, |b| b.homeless.len())
            );
        }
    }
}
