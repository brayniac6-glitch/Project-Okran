//! Towns with a "loot spawn": `cargo run --release -p kenshi-game --example loot_towns -- <install>`
use kenshi_data::GameDataContainer;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let towns = kenshi_world::town::towns(std::path::Path::new(&a[0]), &data);
    let mut n = 0;
    for t in &towns {
        let Some(r) = data.get(&t.record) else { continue };
        if !r.refs("loot spawn").is_empty() && n < 12 {
            n += 1;
            let k = |v: f32| ((v + kenshi_world::WORLD_SIZE / 2.0) / kenshi_world::ZONE_SIZE).floor() as i32;
            println!("{} zone {}.{} loot {:?}", t.name, k(t.position[0]), k(t.position[2]), r.refs("loot spawn").len());
        }
    }
}
