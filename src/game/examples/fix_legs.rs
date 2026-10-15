//! "Fixing Legs" (headless): one of a zone's people with the goal, a first aid kit and a hurt
//! leg, and whether it treats the leg itself:
//! `cargo run --release -p kenshi-game --example fix_legs -- <install> X Y`
use kenshi_game::sim::{FIXING_LEGS, Sim};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    sim.step();
    let Some(id) = (0..sim.folk.len()).find(|&i| sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == FIXING_LEGS)) else {
        println!("no one here has Fixing Legs");
        return;
    };
    // a kit, and a leg cut to half
    if let Some(kit) = sim.data.records.values().find(|r| r.name == "Basic First Aid Kit") {
        let item = kenshi_char::inventory::Item::from_record(kit, 1);
        let inv = &mut sim.folk[id].inventory;
        if !inv.add(item.clone(), false) {
            if let Some(s) = inv.sections.iter_mut().find(|s| s.name == "main") {
                s.items.clear();
            }
            inv.add(item, false);
        }
    }
    let leg = sim.arena.f[id].medical.parts.iter().position(|q| q.kind == 1).expect("a leg");
    let max = sim.arena.f[id].medical.parts[leg].max_health();
    sim.arena.f[id].medical.parts[leg].flesh = max * 0.3;
    println!("{id} {:?}: leg {:.0} of {:.0}", sim.folk[id].name, sim.arena.f[id].medical.parts[leg].health(), max);
    let end = sim.t + 60.0;
    let mut treated = false;
    while sim.t < end {
        sim.step();
        treated |= sim.folk[id].first_aid == Some(id);
    }
    let q = &sim.arena.f[id].medical.parts[leg];
    println!("after 60 s: treating itself {treated}, leg {:.0} (treatment {:.1}), goal {:?}", q.health(), q.treatment, sim.folk[id].ai.current.as_ref().map(|c| c.task));
}
