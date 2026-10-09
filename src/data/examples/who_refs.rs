//! Which record types and reference categories point at records of a type:
//! `cargo run -p kenshi-data --example who_refs -- <install> <target type id>`.
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    let mut hits: BTreeMap<(i32, String), Vec<String>> = BTreeMap::new();
    for r in c.records.values() {
        for (cat, entries) in &r.references {
            for e in entries {
                if let Some(t) = c.get(&e.0)
                    && t.kind == kind
                {
                    hits.entry((r.kind, cat.clone()))
                        .or_default()
                        .push(format!("{} -> {} {:?}", r.name, t.name, e.1));
                }
            }
        }
    }
    for ((k, cat), list) in hits {
        println!("type {k} ref {cat:?}: {}", list.len());
        for l in list.iter().take(5) {
            println!("    {l}");
        }
    }
}
