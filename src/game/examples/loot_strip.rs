//! Task_Loot_AI on a body (headless): one of a zone's people loots another, killed beside it,
//! and what each holds and wears afterwards:
//! `cargo run --release -p kenshi-game --example loot_strip -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    sim.step();
    // a looter and a body of another faction, the body worn and armed
    let worn = |sim: &Sim, i: usize| sim.folk[i].inventory.sections.iter().filter(|s| s.is_equipment()).map(|s| s.items.len()).sum::<usize>();
    // a well-fed looter (a squad's hunger cap 2.8 or more strips the body), else any
    let fed = |sim: &Sim, i: usize| sim.folk[i].platoon.and_then(|pl| sim.squad_hunger.get(&pl)).copied().unwrap_or(3.0) >= 2.8;
    let looter = (0..sim.folk.len())
        .find(|&i| !sim.folk[i].player && sim.folk[i].race.is_none() && fed(&sim, i))
        .or_else(|| (0..sim.folk.len()).find(|&i| !sim.folk[i].player && sim.folk[i].race.is_none()))
        .expect("a looter");
    let body = (0..sim.folk.len())
        .filter(|&i| i != looter && !sim.folk[i].player && sim.folk[i].race.is_none() && sim.folk[i].faction != sim.folk[looter].faction)
        .max_by_key(|&i| worn(&sim, i))
        .or_else(|| (0..sim.folk.len()).find(|&i| i != looter && !sim.folk[i].player && sim.folk[i].race.is_none()))
        .expect("a body");
    let names = |sim: &Sim, i: usize| sim.folk[i].inventory.sections.iter().flat_map(|s| s.items.iter().map(move |p| format!("{}:{}", s.name, p.item.name))).collect::<Vec<_>>();
    // the looter's main grid emptied so the body's things fit
    if let Some(s) = sim.folk[looter].inventory.sections.iter_mut().find(|s| s.name == "main") {
        s.items.clear();
    }
    println!("body {body} ({:?}) before: {:?}", sim.folk[body].faction, names(&sim, body));
    let at = sim.folk[body].mover.position_units();
    sim.folk[looter].mover.position = at.map(|v| v * kenshi_world::navmesh::METRES);
    sim.knock_out(body);
    sim.folk[body].dead = true;
    sim.folk[looter].loot = Some((body, 0.0));
    let end = sim.t + 25.0;
    while sim.t < end && sim.folk[looter].loot.is_some() {
        sim.step();
    }
    println!("after {:.1} s, body: {:?}", sim.t, names(&sim, body));
    println!("looter {looter} ({:?}): {:?}", sim.folk[looter].faction, names(&sim, looter));
}
