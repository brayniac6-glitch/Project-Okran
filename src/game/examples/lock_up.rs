//! Locking up (headless): the residents with the goal "Locking up" (155) close and lock their
//! homes' doors (Task_CloseDoor 73, Task_LockDoor 162) and their squads' "locked up" flags
//! (F044498) are set; each hour the doors and flags of those homes:
//! `TOWN_START_HOUR=21 cargo run --release -p kenshi-game --example lock_up -- <install> X Y [seconds]`
use kenshi_game::sim::{DOOR_OPEN, Sim};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(300.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    sim.step();
    let lockers: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && sim.folk[i].home.is_some() && sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == 155) || sim.folk[i].ai.goals.iter().any(|g| g.task == 155)).collect();
    let mut homes: Vec<usize> = lockers.iter().filter_map(|&i| sim.folk[i].home).collect();
    homes.sort();
    homes.dedup();
    println!("{} with Locking up, homes {homes:?}", lockers.len());
    let state = |sim: &Sim| {
        homes
            .iter()
            .map(|&h| {
                let doors: String = (0..sim.door_states.len())
                    .filter(|&d| sim.door_states[d].door.placement == h)
                    .map(|d| format!("{}{}", if sim.door_states[d].locked { "L" } else { "-" }, if sim.door_states[d].phase == DOOR_OPEN { "o" } else { "s" }))
                    .collect();
                format!("{h}:{doors}{}", if sim.locked_up(h) { "*" } else { "" })
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    println!("start: {}", state(&sim));
    let end = sim.t + seconds;
    let mut last = -1.0f32;
    let mut seen = std::collections::HashSet::new();
    while sim.t < end {
        sim.step();
        for &i in &lockers {
            if let Some(s) = sim.folk[i].ai.queue.first()
                && (s.task == 73 || s.task == 162)
                && seen.insert((i, s.task))
            {
                println!("t {:.0}: {i} takes up {} on {:?}", sim.t, s.task, s.target);
            }
        }
        if std::env::var("LOCK_DEBUG").is_ok() && sim.tick % 30 == 0 && sim.t < 40.0 {
            for i in std::env::var("LOCK_DEBUG").unwrap().split(',').filter_map(|x| x.parse::<usize>().ok()) {
                let p = &sim.folk[i];
                let door = p.ai.queue.first().and_then(|s| if let kenshi_game::sim::Target::Door(d) = s.target { Some(d) } else { None });
                println!("  t {:.1}: {i} queue {:?} door {:?} phase {:?} time {:?}", sim.t, p.ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(), door, door.map(|d| (sim.door_states[d].phase, sim.door_states[d].open)), sim.law_state.door_time.get(&i));
            }
        }
        if sim.t - last >= if std::env::var("LOCK_DEBUG").is_ok() && sim.t < 60.0 { 2.0 } else { 30.0 } {
            last = sim.t;
            println!("t {:.0} (hour {:.1}): {}", sim.t, sim.hour(), state(&sim));
        }
    }
}
