//! Each BIOME_GROUP's "homeless spawn amount" and how many entries its homeless list has:
//! `cargo run --release -p kenshi-game --example biome_amounts -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = kenshi_io::mods::Install::new(&a[0]).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    for b in kenshi_world::biome::biomes(&data) {
        println!("{:>5.2}  {:3} entries  {:3} nests  {} ({})", b.homeless_spawn_amount, b.homeless.len(), b.num_nests, b.name, b.index);
    }
}
