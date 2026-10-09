//! Every field name used by records of one type, with how many records use it:
//! `cargo run -p kenshi-data --example keys -- <install> <type id> [field]`; with a field, each
//! record's name and value of it instead.
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    if let Some(field) = a.get(2) {
        for r in c.records.values().filter(|r| r.kind == kind) {
            let v = r
                .ints
                .get(field)
                .map(|v| v.to_string())
                .or_else(|| r.floats.get(field).map(|v| v.to_string()))
                .or_else(|| r.bools.get(field).map(|v| v.to_string()))
                .or_else(|| r.strings.get(field).cloned())
                .or_else(|| r.files.get(field).cloned());
            println!("{} = {:?}", r.name, v);
        }
        return;
    }
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    for r in c.records.values().filter(|r| r.kind == kind) {
        for k in r.ints.keys().chain(r.floats.keys()).chain(r.bools.keys()).chain(r.strings.keys()) {
            *keys.entry(k.clone()).or_default() += 1;
        }
        for k in r.files.keys() {
            *keys.entry(format!("file {k}")).or_default() += 1;
        }
        for (k, _) in &r.references {
            *keys.entry(format!("ref {k}")).or_default() += 1;
        }
    }
    for (k, n) in keys {
        println!("{n:6} {k}");
    }
}
