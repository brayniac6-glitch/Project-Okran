//! BUILDING_FUNCTIONALITY records: "function" and "is production building": `cargo run --release -p kenshi-game --example prod_flags -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut by = std::collections::BTreeMap::new();
    for r in data.of_type(kenshi_data::ItemType::BuildingFunctionality) {
        let e = by.entry((r.ints.get("function").copied().unwrap_or(-1), r.bools.get("is production building").copied())).or_insert((0, Vec::new()));
        e.0 += 1;
        if e.1.len() < 4 {
            e.1.push(r.name.clone());
        }
    }
    for (k, v) in by {
        println!("{k:?}: {v:?}");
    }
}
