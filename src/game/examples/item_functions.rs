//! Items by "item function" (which are tools, splint kits, ...) and the shackles' lock fields:
//! `cargo run --release -p kenshi-game --example item_functions -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut by: std::collections::BTreeMap<i32, Vec<String>> = Default::default();
    for t in [kenshi_data::ItemType::Item, kenshi_data::ItemType::Armour] {
        for r in data.of_type(t) {
            let f = r.ints.get("item function").copied().unwrap_or(-1);
            by.entry(f).or_default().push(format!("{} [{}] q{:?} c{:?}", r.name, r.string_id, r.floats.get("quality").or(r.ints.get("quality").map(|_| &0.0)), r.ints.get("charges")));
            if r.bools.get("is locked").copied().unwrap_or(false) {
                println!("locked: {} [{}] ints {:?} floats {:?}", r.name, r.string_id, r.ints, r.floats);
            }
        }
    }
    for (k, v) in by {
        if k > 0 {
            println!("function {k}: {} e.g. {:?}", v.len(), &v[..v.len().min(6)]);
        }
    }
}
