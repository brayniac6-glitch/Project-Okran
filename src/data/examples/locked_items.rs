//! Items with "is locked" (shackles): `cargo run -p kenshi-data --example locked_items -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for r in c.records.values() {
        if r.bools.get("is locked").copied().unwrap_or(false) {
            println!("{} {} {:?} slot {:?} lock {:?}", r.string_id, r.name, r.item_type(), r.ints.get("slot"), r.ints.get("lock level").or(r.ints.get("lock")));
        }
    }
}
