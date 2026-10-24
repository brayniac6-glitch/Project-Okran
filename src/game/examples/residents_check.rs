//! A zone's people as loaded: each one's name, character record and the building it lives in
//! (its home placement), then the count:
//! `cargo run --release -p kenshi-game --example residents_check -- <install> X Y [around]`
//! (`around` 1: the eight zones about it loaded too)
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    if a.get(3).is_some_and(|n| n == "1") {
        let res = sim.res.clone();
        for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
            sim.add_zone(zx + dx, zy + dy, &res);
        }
    }
    for p in sim.folk.iter().filter(|p| !p.player) {
        let home = p.home.and_then(|h| sim.placed.get(h)).and_then(|pl| sim.data.get(&pl.building)).map_or("-", |b| b.name.as_str());
        let ch = p.character.as_deref().and_then(|c| sim.data.get(c)).map_or("-", |c| c.name.as_str());
        println!("{} ({ch}) home {home}", sim.display_name(p.id));
    }
    for pl in sim.placed.iter().filter(|pl| pl.squad.is_some() || pl.residents.is_some()) {
        let n = |s: &Option<String>| s.as_deref().and_then(|s| sim.data.get(s)).map_or("-", |r| r.name.as_str());
        println!("placement {}: squad {} residents {}", sim.data.get(&pl.building).map_or("?", |b| b.name.as_str()), n(&pl.squad), n(&pl.residents));
    }
    println!("{} people", sim.folk.iter().filter(|p| !p.player).count());
}
