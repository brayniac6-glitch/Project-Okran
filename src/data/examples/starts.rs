//! Every game start (NEW_GAME_STARTOFF) with its fields and references, and whether the base
//! game's files define it: `cargo run -p kenshi-data --example starts -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(&a[0]).game_order().unwrap();
    let mut core_order = order.clone();
    core_order.files.retain(|f| f.core);
    let core = GameDataContainer::load(&core_order).unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    for s in data.of_type(ItemType::NewGameStartoff) {
        let base = core.get(&s.string_id).is_some();
        println!("{} [{}] {}", s.name, s.string_id, if base { "BASE" } else { "MOD" });
        let mut f: Vec<String> = s.ints.iter().map(|(k, v)| format!("{k}={v}")).chain(s.floats.iter().map(|(k, v)| format!("{k}={v}"))).chain(s.bools.iter().map(|(k, v)| format!("{k}={v}"))).collect();
        f.sort();
        println!("   {}", f.join(", "));
        for (cat, refs) in &s.references {
            for (sid, v) in refs {
                println!("   {cat}: {} {v:?}", data.get(sid).map_or(sid.as_str(), |r| r.name.as_str()));
            }
        }
    }
}
