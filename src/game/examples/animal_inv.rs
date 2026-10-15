//! An animal's inventory (`Sim::make_animal`): its sections and its backpack's grid:
//! `cargo run --release -p kenshi-game --example animal_inv -- <install> X Y "<animal name>"`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    let animal = sim.data.records.values().find(|r| r.name == a[3] && r.item_type() == Some(kenshi_data::ItemType::AnimalCharacter)).map(|r| r.string_id.clone()).expect("animal");
    sim.make_animal(0, &animal);
    let inv = &sim.folk[0].inventory;
    for s in &inv.sections {
        let names: Vec<&str> = s.items.iter().map(|p| p.item.name.as_str()).collect();
        println!("{} {}x{}: {names:?}", s.name, s.w, s.h);
    }
    match inv.bag() {
        Some(b) => println!("bag {}x{}, weight now {:.1}", b.sections[0].w, b.sections[0].h, inv.weight()),
        None => println!("no bag"),
    }
}
