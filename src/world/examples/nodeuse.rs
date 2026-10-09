//! Which records place node buildings, by node type: `cargo run -p kenshi-world --example nodeuse -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let install = std::env::args().nth(1).unwrap();
    let mut order = Install::new(&install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let mut by: BTreeMap<i32, BTreeMap<String, usize>> = BTreeMap::new();
    for r in data.records.values() {
        for i in &r.instances {
            if let Some(t) = data.get(&i.target)
                && t.item_type() == Some(ItemType::Building)
                && t.bools.get("is node").copied().unwrap_or(false)
            {
                let k = *t.ints.get("node type").unwrap_or(&1);
                *by.entry(k)
                    .or_default()
                    .entry(format!("{:?} {}", r.item_type(), r.name))
                    .or_default() += 1;
            }
        }
    }
    for (k, m) in by {
        let total: usize = m.values().sum();
        let eg: Vec<String> = m.iter().take(8).map(|(n, c)| format!("{n} x{c}")).collect();
        println!("type {k}: {total} in {} records, e.g. {eg:?}", m.len());
    }
}
