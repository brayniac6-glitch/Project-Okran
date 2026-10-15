//! Each placed town's zone (X.Y), type and faction:
//! `cargo run --release -p kenshi-world --example town_zones -- <install> [name filter]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    for t in kenshi_world::town::towns(install, &data) {
        if a.get(1).is_some_and(|f| !t.name.to_lowercase().contains(&f.to_lowercase())) {
            continue;
        }
        let (x, y) = kenshi_world::biome::zone_at(t.position[0], t.position[2]);
        let faction = t.faction.as_deref().and_then(|f| data.get(f)).map_or("-".to_string(), |r| r.name.clone());
        println!("{x}.{y}  {:30} type {} {faction}", t.name, t.kind);
    }
}
