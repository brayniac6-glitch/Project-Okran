//! Squad templates with "slaves": `cargo run -p kenshi-data --example slave_squads -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for t in c.of_type(ItemType::SquadTemplate) {
        let s = t.refs("slaves");
        if s.is_empty() {
            continue;
        }
        let names: Vec<String> = s.iter().map(|(id, v)| format!("{} {:?}", c.get(id).map_or("?", |r| r.name.as_str()), v)).collect();
        let packs: Vec<String> = t.refs("AI packages").iter().map(|(id, v)| format!("{} {:?}", c.get(id).map_or("?", |r| r.name.as_str()), v)).collect();
        println!("{}: slaves {names:?} packages {packs:?}", t.name);
    }
}
