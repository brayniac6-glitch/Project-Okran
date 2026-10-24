//! "Opening up" (headless): the homes of a zone's people with the goal (86) and their doors,
//! locked and open, over the hours (TOWN_START_HOUR and TOWN_HOUR_SECONDS set the clock):
//! `cargo run --release -p kenshi-game --example shop_doors -- <install> X Y [seconds]`
use kenshi_game::sim::{DOOR_OPEN, OPENING_UP, Sim};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(300.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    sim.step();
    let openers: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == OPENING_UP)).collect();
    let mut homes: Vec<usize> = openers.iter().filter_map(|&i| sim.folk[i].home).collect();
    homes.sort();
    homes.dedup();
    println!("{} with Opening up, homes {homes:?}", openers.len());
    for &i in openers.iter().take(3) {
        println!("opener {i} jobs {:?}", sim.folk[i].ai.jobs.iter().map(|l| l.iter().map(|j| (j.task, j.priority, j.hours)).collect::<Vec<_>>()).collect::<Vec<_>>());
    }
    // (SHOP_LOCK) every door of those homes shut and locked to start with
    if std::env::var("SHOP_LOCK").is_ok() {
        for s in sim.door_states.iter_mut().filter(|s| homes.contains(&s.door.placement)) {
            s.phase = kenshi_game::sim::DOOR_CLOSED;
            s.open = 0.0;
            s.locked = true;
            s.level = s.level.max(1);
        }
    }
    let state = |sim: &Sim| {
        homes
            .iter()
            .map(|&h| {
                let doors: Vec<String> = (0..sim.door_states.len())
                    .filter(|&d| sim.door_states[d].door.placement == h)
                    .map(|d| format!("{}{}", if sim.door_states[d].locked { "L" } else { "-" }, if sim.door_states[d].phase == DOOR_OPEN { "o" } else { "s" }))
                    .collect();
                format!("{h}:{}", doors.join(""))
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let end = sim.t + seconds;
    let mut last_hour = -1;
    while sim.t < end {
        sim.step();
        let h = sim.hour() as i32;
        if h != last_hour {
            last_hour = h;
            let goals: Vec<Option<u16>> = openers.iter().map(|&i| sim.folk[i].ai.current.as_ref().map(|c| c.task)).collect();
            println!("hour {h}: doors {} | openers' goals {goals:?}", state(&sim));
        }
    }
}
