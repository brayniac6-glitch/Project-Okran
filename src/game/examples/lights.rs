//! The game data's lights: LIGHT records, and the buildings, parts and effects that place them
//! (as instances): `cargo run --release -p kenshi-game --example lights -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let lights: Vec<_> = data.of_type(ItemType::Light).collect();
    println!("{} LIGHT records", lights.len());
    for l in lights.iter().take(8) {
        println!("  {} [{}] ints {:?} floats {:?}", l.name, l.string_id, l.ints, l.floats);
    }
    let mut by_type = std::collections::BTreeMap::new();
    for r in data.records.values() {
        let n = r.instances.iter().filter(|i| data.get(&i.target).is_some_and(|t| t.item_type() == Some(ItemType::Light))).count();
        if n > 0 {
            let used: Vec<String> = r.instances.iter().filter_map(|i| data.get(&i.target).filter(|t| t.item_type() == Some(ItemType::Light)).map(|t| format!("{} at {:?}", t.name, i.position.map(|v| v as i32)))).collect();
            println!("  {:?} {} [{}]: {used:?}", r.item_type(), r.name, r.string_id);
            let e = by_type.entry(format!("{:?}", r.item_type())).or_insert((0, 0, String::new()));
            e.0 += 1;
            e.1 += n;
            if e.2.is_empty() {
                e.2 = r.name.clone();
            }
        }
    }
    println!("records placing lights (type: records, lights, e.g.): {by_type:?}");
}
