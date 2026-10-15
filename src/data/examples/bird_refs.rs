//! What spawns and attracts the WILDLIFE_BIRDS: biome groups' "birds" (average per zone) and the
//! "bird attractor"s of buildings, animals and features:
//! `cargo run -p kenshi-data --example bird_refs -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let birds: Vec<&kenshi_data::GameData> = c.of_type(ItemType::WildlifeBirds).collect();
    for r in c.records.values() {
        for cat in ["birds", "bird attractor"] {
            for (s, v) in r.refs(cat) {
                if let Some(b) = birds.iter().find(|b| b.string_id == *s) {
                    println!("{:?} {} -> {cat} {} {v:?}", r.item_type(), r.name, b.name);
                }
            }
        }
    }
}
