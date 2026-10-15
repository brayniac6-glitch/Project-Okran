//! The "path mode" of each furniture record a zone places (and how many), the navmesh treatment
//! Kenshi gives it (0 ignored, 1 projected, 2 obstacle, 3 walkable):
//! `cargo run --release -p kenshi-world --example furniture_modes -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut by: std::collections::BTreeMap<(i32, i32), Vec<String>> = Default::default();
    for r in data.of_type(kenshi_data::ItemType::Building) {
        let Some((function, _, _)) = kenshi_world::furniture::functionality(&data, r) else { continue };
        let mode = r.ints.get("path mode").copied().unwrap_or(-1);
        by.entry((mode, function)).or_default().push(r.name.clone());
    }
    for ((mode, function), names) in by {
        println!("mode {mode} function {function}: {} e.g. {:?}", names.len(), &names[..names.len().min(6)]);
    }
}
