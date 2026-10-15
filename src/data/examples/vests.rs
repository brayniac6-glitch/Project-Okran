//! Which armour slots carry painted-on clothing ("vest texture"): slot -> count, with examples:
//! `cargo run -p kenshi-data --example vests -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let mut by: std::collections::BTreeMap<i32, Vec<String>> = Default::default();
    for r in c.of_type(ItemType::Armour) {
        if r.files.get("vest texture").is_some_and(|f| !f.is_empty()) {
            by.entry(r.ints.get("slot").copied().unwrap_or(-1)).or_default().push(r.name.clone());
        }
    }
    for (slot, names) in by {
        println!("slot {slot}: {} e.g. {:?}", names.len(), names.iter().take(4).collect::<Vec<_>>());
    }
}
