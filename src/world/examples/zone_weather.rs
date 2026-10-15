//! A zone's weather region (its biome group, by the biome map) and the weathers its seasons
//! bring, each with its fog and effects:
//! `cargo run -p kenshi-world --example zone_weather -- <install> X Y`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::biome::{BiomeMap, biome_of_zone, biomes};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let order = Install::new(install).load_order().unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    let map = BiomeMap::load(install).expect("biome map");
    let bs = biomes(&data);
    let Some(b) = biome_of_zone(&map, &bs, x, y) else {
        println!("zone {x}.{y}: no biome");
        return;
    };
    let group = data.get(&bs[b].record).unwrap();
    println!("zone {x}.{y}: biome group {} ({})", group.name, group.string_id);
    for (s, _) in group.refs("seasons") {
        let Some(season) = data.get(s) else { continue };
        println!("  season {}", season.name);
        for (w, v) in season.refs("weathers") {
            let Some(w) = data.get(w) else { continue };
            let f = |k: &str| w.floats.get(k).copied().or_else(|| w.ints.get(k).map(|v| *v as f32)).unwrap_or(0.0);
            println!(
                "    {} {:?}: fog enabled {} distance {}..{} colour {:#x}; effects {:?}",
                w.name,
                v,
                w.bools.get("fog enabled").copied().unwrap_or(false),
                f("fog distance min"),
                f("fog distance max"),
                w.ints.get("fog color").copied().unwrap_or(0),
                w.refs("effects").iter().filter_map(|(e, _)| data.get(e).map(|e| e.name.clone())).collect::<Vec<_>>()
            );
        }
    }
}
