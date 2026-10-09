//! Two file fields of every record of a type, tab separated:
//! `cargo run -p kenshi-data --example pairs -- <install> <type id> <file key> <file key>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    for r in c.records.values().filter(|r| r.kind == kind) {
        let f = |k: &str| r.files.get(k).cloned().unwrap_or_default();
        let (x, y) = (f(&a[2]), f(&a[3]));
        if !x.is_empty() && !y.is_empty() {
            println!("{}\t{x}\t{y}", r.name);
        }
    }
}
