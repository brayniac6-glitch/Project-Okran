//! "Ditching loot" (headless): one of a zone's people with the goal is handed loot (an item
//! of another faction's), and whether it takes it to a storage of its town:
//! `cargo run --release -p kenshi-game --example ditch_loot -- <install> X Y [seconds]`
use kenshi_game::sim::{DITCHING_LOOT, Sim};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    sim.step();
    let Some(id) = (0..sim.folk.len()).find(|&i| !sim.folk[i].dead && sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == DITCHING_LOOT)) else {
        println!("no one here has Ditching loot");
        return;
    };
    // loot: a weapon of another faction's
    let Some(rec) = sim.data.records.values().find(|r| r.name == "Iron Club") else { return };
    let mut item = kenshi_char::inventory::Item::from_record(rec, 1);
    item.owner = Some("loot-test-faction".to_string());
    if let Some(s) = sim.folk[id].inventory.sections.iter_mut().find(|s| s.name == "main") {
        s.items.clear();
    }
    sim.folk[id].inventory.add(item, false);
    sim.folk[id].rethink = true;
    let name = |sim: &Sim, f: &Option<String>| f.as_deref().and_then(|f| sim.data.get(f)).map_or("-".to_string(), |r| r.name.clone());
    println!("{id} ({}) given loot at {:.0?}, town {:?}, home {:?}", name(&sim, &sim.folk[id].faction), sim.folk[id].mover.position_units(), sim.folk[id].town, sim.folk[id].home);
    let holds = |sim: &Sim| sim.folk[id].inventory.sections.iter().any(|s| s.items.iter().any(|i| i.item.owner.as_deref() == Some("loot-test-faction")));
    let in_box = |sim: &Sim| (0..sim.boxes.len()).find(|&k| sim.boxes[k].inventory.sections.iter().any(|s| s.items.iter().any(|i| i.item.owner.as_deref() == Some("loot-test-faction"))));
    let end = sim.t + seconds;
    let mut last = None;
    while sim.t < end {
        sim.step();
        let now = (sim.folk[id].ai.current.as_ref().map(|c| c.task), sim.folk[id].ai.queue.first().map(|s| (s.task, s.target)));
        if Some(now) != last {
            println!("t {:.1}: goal {:?}, step {:?}", sim.t, now.0, now.1);
            last = Some(now);
        }
        if !holds(&sim) {
            break;
        }
    }
    println!("after {:.1} s: still carried {}, in container {:?}", sim.t, holds(&sim), in_box(&sim).map(|k| (k, sim.boxes[k].at.name.clone())));
}
