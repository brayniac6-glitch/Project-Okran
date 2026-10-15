//! How many towns, buildings and cages a world load holds (raids' captives need cages):
//! `cargo run --release -p kenshi-game --example cage_count -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    let cages = (0..sim.furn.len()).filter(|&k| sim.furn[k].function == kenshi_world::furniture::BF_CAGE).count();
    let names: Vec<&str> = sim.towns.iter().map(|t| t.name.as_str()).collect();
    println!("{} towns, {} placed, {} furniture, {cages} cages; towns {names:?}", sim.towns.len(), sim.placed.len(), sim.furn.len());
}
