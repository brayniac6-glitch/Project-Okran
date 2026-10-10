//! Storage buildings' "limit inventory": `cargo run --release -p kenshi-game --example limit_inv -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    for r in data.of_type(kenshi_data::ItemType::Building).filter(|r| r.name.starts_with("Storage: O")) {
        println!("{}: refs {:?} ints {:?}", r.name, r.references.iter().map(|(k, v)| (k, v.len())).collect::<Vec<_>>(), r.ints.iter().filter(|(k, _)| k.contains("storage") || k.contains("limit")).collect::<Vec<_>>());
    }
}
