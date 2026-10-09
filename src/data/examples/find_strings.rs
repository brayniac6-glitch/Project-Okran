//! Which record fields hold one of the given strings (a file of words, one per line), by type
//! and field: `cargo run -p kenshi-data --example find_strings -- <install> <words file>`.
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use std::collections::{BTreeMap, HashSet};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let words: HashSet<String> = std::fs::read_to_string(&a[1])
        .unwrap()
        .lines()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    let mut hits: BTreeMap<(i32, String), Vec<String>> = BTreeMap::new();
    for r in c.records.values() {
        for (k, v) in r.strings.iter().chain(r.files.iter()) {
            if words.contains(&v.trim().to_ascii_lowercase()) {
                hits.entry((r.kind, k.clone()))
                    .or_default()
                    .push(format!("{} = {v}", r.name));
            }
        }
    }
    for ((kind, field), list) in hits {
        println!("type {kind} field {field:?}: {} records", list.len());
        for l in list.iter().take(6) {
            println!("    {l}");
        }
    }
}
