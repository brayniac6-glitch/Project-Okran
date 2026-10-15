//! The NODE buildings ("is node") and what their functionality says about animation:
//! `cargo run --release -p kenshi-world --example node_anims -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    for r in data.of_type(kenshi_data::ItemType::Building) {
        if !r.bools.get("is node").copied().unwrap_or(false) {
            continue;
        }
        let mut ints: Vec<String> = r.ints.iter().map(|(k, v)| format!("{k}={v}")).collect();
        ints.sort();
        let refs: Vec<String> = r.references.iter().map(|(k, v)| format!("{k}={:?}", v.iter().filter_map(|(s, _)| data.get(s).map(|x| x.name.clone())).collect::<Vec<_>>())).collect();
        println!("{} ({}): {:?} refs {:?}", r.name, r.string_id, ints, refs);
    }
}
