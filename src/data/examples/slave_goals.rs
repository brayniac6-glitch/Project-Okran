//! AI packages' "Slave AI Goals": `cargo run -p kenshi-data --example slave_goals -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for p in c.of_type(ItemType::AiPackage) {
        let g = p.refs("Slave AI Goals");
        if g.is_empty() {
            continue;
        }
        let names: Vec<String> = g.iter().map(|(s, v)| format!("{} {:?} enum {:?}", c.get(s).map_or("?", |r| r.name.as_str()), v, c.get(s).and_then(|r| r.ints.get("enum").copied()))).collect();
        println!("{}: {names:?}", p.name);
    }
}
