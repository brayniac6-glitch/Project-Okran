//! The ANIMATION records by their "category", with the names in each:
//! `cargo run --release -p kenshi-data --example anim_categories -- <install> [category]`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(std::path::Path::new(&a[0])).game_order().unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    let want: Option<i32> = a.get(1).and_then(|v| v.parse().ok());
    let mut by: BTreeMap<i32, Vec<String>> = BTreeMap::new();
    for r in data.of_type(ItemType::Animation) {
        let c = r.ints.get("category").copied().unwrap_or(0);
        by.entry(c).or_default().push(r.name.clone());
    }
    for (c, names) in by {
        if want.is_some_and(|w| w != c) {
            continue;
        }
        println!("{c}: {} {:?}", names.len(), &names[..names.len().min(if want.is_some() { 200 } else { 6 })]);
    }
}
