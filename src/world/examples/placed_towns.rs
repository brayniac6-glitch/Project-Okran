//! Towns the level files place, with their radius:
//! `cargo run -p kenshi-world --example placed_towns -- <install> [name]`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::{town::towns, zone_of};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let all = towns(install, &data);
    println!("{} towns", all.len());
    for t in all.iter().filter(|t| a.get(1).is_none_or(|w| *w == t.name)) {
        println!(
            "{:32} {:24} zone {:?} at {:?} type {} radius {}",
            t.name,
            t.record,
            zone_of(t.position[0], t.position[2]),
            t.position.map(|v| v.round()),
            t.kind,
            t.radius()
        );
    }
}
