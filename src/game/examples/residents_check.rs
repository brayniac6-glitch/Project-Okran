//! A zone's people as loaded: each one's name, character record and the building it lives in
//! (its home placement), then the count:
//! `cargo run --release -p kenshi-game --example residents_check -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    for p in sim.folk.iter().filter(|p| !p.player) {
        let home = p.home.and_then(|h| sim.placed.get(h)).and_then(|pl| sim.data.get(&pl.building)).map_or("-", |b| b.name.as_str());
        let ch = p.character.as_deref().and_then(|c| sim.data.get(c)).map_or("-", |c| c.name.as_str());
        println!("{} ({ch}) home {home}", sim.display_name(p.id));
    }
    println!("{} people", sim.folk.iter().filter(|p| !p.player).count());
}
