//! The SQUAD_TEMPLATEs that start malnourished (F045433):
//! `cargo run -p kenshi-data --example malnourished -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for r in c.of_type(ItemType::SquadTemplate) {
        if r.bools.get("malnourished").copied().unwrap_or(false) {
            println!("{} {}", r.string_id, r.name);
        }
    }
}
