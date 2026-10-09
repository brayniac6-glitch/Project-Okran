//! The DIALOGUE "chance permanent" / "chance temporary" values used, with counts:
//! `cargo run -p kenshi-data --example dialog_chances -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let mut n: BTreeMap<String, usize> = BTreeMap::new();
    for d in c.of_type(ItemType::Dialogue) {
        for k in ["chance permanent", "chance temporary"] {
            let v = d.floats.get(k).map(|v| format!("f{v}")).or_else(|| d.ints.get(k).map(|v| format!("i{v}"))).unwrap_or("-".into());
            *n.entry(format!("{k} {v}")).or_default() += 1;
        }
    }
    for (k, v) in n {
        println!("{v:5} {k}");
    }
}
