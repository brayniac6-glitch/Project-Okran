//! The training dummies' records (BUILDING and their BUILDING_FUNCTIONALITY): `cargo run
//! --release -p kenshi-game --example dummies -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    for r in data.of_type(kenshi_data::ItemType::Building) {
        let n = r.name.to_lowercase();
        if !(n.contains("dummy") || n.contains("training") || n.contains("punching") || n.contains("target")) {
            continue;
        }
        println!("{} [{}] floats {:?} ints {:?}", r.name, r.string_id, r.floats.iter().filter(|(k, _)| k.contains("output") || k.contains("rate")).collect::<Vec<_>>(), r.ints.iter().filter(|(k, _)| k.contains("function") || k.contains("stat") || k.contains("category")).collect::<Vec<_>>());
        for (f, _) in r.refs("functionality") {
            if let Some(f) = data.get(f) {
                println!("   function {} ints {:?} floats {:?} bools {:?}", f.name, f.ints, f.floats, f.bools);
                for k in ["animation", "animation events", "animation KO"] {
                    let names: Vec<String> = f.refs(k).iter().filter_map(|(s, _)| data.get(s).map(|r| r.name.clone())).collect();
                    println!("   {k}: {names:?}");
                }
            }
        }
    }
}
