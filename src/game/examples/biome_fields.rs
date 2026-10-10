//! A BIOME_GROUP record's and its ENVIRONMENT_RESOURCES' fields (the prospecting window's
//! sources): `cargo run --release -p kenshi-game --example biome_fields -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    for t in [kenshi_data::ItemType::BiomeGroup, kenshi_data::ItemType::EnvironmentResources] {
        let Some(r) = data.of_type(t).next() else { continue };
        println!("{t:?} {} ints {:?}", r.name, r.ints);
        println!("  floats {:?}", r.floats);
        println!("  bools {:?}", r.bools);
        println!("  strings {:?}", r.strings);

    }
}
