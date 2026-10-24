//! NPC shopping (headless): a zone's shop counters with what they sell, each resident's shopping
//! list, and the purchases over a run (KENSHI_SHOP_DEBUG prints them as they happen):
//! `cargo run --release -p kenshi-game --example shopping -- <install> X Y [seconds] [start hour]`
use kenshi_game::sim::{SHOPPING, SHOPPING_VISIT, Sim};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(300.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    if let Some(h) = a.get(4).and_then(|s| s.parse::<f64>().ok()) {
        sim.start_hour = h;
    }
    sim.step();
    for c in sim.counters() {
        let b = &sim.boxes[c.k];
        println!("counter {} {} (placement {}, town {:?}): item functions {:?}", c.k, b.at.name, b.at.placement, c.town.map(|t| sim.towns[t].name.clone()), c.functions);
    }
    println!("{} public buildings", sim.public_buildings().len());
    let mut shopping = 0;
    while sim.t < seconds {
        sim.step();
        if sim.tick.is_multiple_of(60) {
            shopping += sim.folk.iter().filter(|p| p.ai.current.as_ref().is_some_and(|c| c.task == SHOPPING) || p.ai.queue.first().is_some_and(|s| s.task == SHOPPING_VISIT)).count();
        }
    }
    for p in &sim.folk {
        let Some(l) = &p.shopping else { continue };
        if l.lines.is_empty() {
            continue;
        }
        println!("{} {:?} town {:?}: {:?} (inventory {} items)", p.id, p.name, p.town, l.lines.iter().map(|x| (x.function, x.wanted, x.bought)).collect::<Vec<_>>(), p.inventory.sections.iter().map(|s| s.items.len()).sum::<usize>());
    }
    println!("person-seconds shopping: {shopping}");
    for p in &sim.folk {
        let has = p.ai.jobs.iter().flatten().any(|j| j.task == SHOPPING) || p.ai.goals.iter().any(|g| g.task == SHOPPING);
        if has {
            println!("{} has Shopping: current {:?} queue {:?}", p.id, p.ai.current.as_ref().map(|c| c.task), p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>());
        }
    }
}
