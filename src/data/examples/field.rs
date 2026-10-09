//! Print one field across all records of a type, with how often each value occurs:
//! `cargo run -p kenshi-data --example field -- <install> <type id> <key> [examples per value]`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    let key = &a[2];
    let n: usize = a.get(3).map_or(3, |x| x.parse().unwrap());
    let mut by: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for r in c.records.values().filter(|r| r.kind == kind) {
        let v = if let Some(v) = r.ints.get(key) {
            v.to_string()
        } else if let Some(v) = r.floats.get(key) {
            v.to_string()
        } else if let Some(v) = r.bools.get(key) {
            v.to_string()
        } else if let Some(v) = r.files.get(key) {
            format!("file {v:?}")
        } else if let Some(v) = r.strings.get(key) {
            format!("{v:?}")
        } else {
            "-".into()
        };
        by.entry(v).or_default().push(&r.name);
    }
    for (v, mut names) in by {
        names.sort();
        println!(
            "{v}: {} e.g. {:?}",
            names.len(),
            &names[..names.len().min(n)]
        );
    }
}
