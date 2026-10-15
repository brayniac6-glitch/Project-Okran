//! Where the biomes' nests rise (headless): F048131 called until every biome has its nests,
//! each nest with its biome and whether it stands on a level nest marker:
//! `cargo run --release -p kenshi-game --example nest_spawns -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    for _ in 0..400 {
        sim.spawn_nests();
    }
    let Some(pop) = sim.population.as_ref() else { return };
    for n in &sim.nests {
        let bio = &pop.biomes[n.biome];
        let marker = sim.nest_markers.as_ref().is_some_and(|m| m.contains(&n.at));
        println!("{:<28} {:<24} markers only {:<5} on a marker {:<5} at {:?}", n.name, bio.name, bio.nests_at_markers, marker, n.at.map(|v| v as i32));
    }
    let markers = pop.biomes.iter().filter(|b| b.nests_at_markers).map(|b| b.name.clone()).collect::<Vec<_>>();
    println!("{} nests; marker biomes {:?}; {} nest markers", sim.nests.len(), markers, sim.nest_markers.as_ref().map_or(0, |m| m.len()));
}
