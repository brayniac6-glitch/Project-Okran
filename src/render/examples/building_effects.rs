//! Which records place EFFECT instances (buildings, parts), with their type and functionality:
//! `cargo run --release -p kenshi-render --example building_effects -- <install>`
use kenshi_data::{GameDataContainer, ItemType};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    for r in data.records.values() {
        let fx: Vec<String> = r
            .instances
            .iter()
            .filter(|i| data.get(&i.target).and_then(|t| t.item_type()) == Some(ItemType::Effect))
            .map(|i| data.get(&i.target).map_or(i.target.clone(), |t| t.name.clone()))
            .collect();
        if !fx.is_empty() {
            println!(
                "{:?} {:40} functionality {:?} -> {:?}",
                r.item_type(),
                r.name,
                r.ints.get("functionality"),
                fx
            );
        }
    }
}
