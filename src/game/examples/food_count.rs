//! How many of a zone's people carry food (NPC hunger's footing):
//! `cargo run --release -p kenshi-game --example food_count -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    let npcs: Vec<_> = sim.folk.iter().filter(|p| !p.player && p.race.is_none()).collect();
    let fed = npcs.iter().filter(|p| p.inventory.sections.iter().any(|s| s.items.iter().any(|i| i.item.food.is_some()))).count();
    let animals = sim.folk.iter().filter(|p| !p.player && p.race.is_some()).count();
    println!("{} people, {fed} with food; {animals} animals", npcs.len());
}
