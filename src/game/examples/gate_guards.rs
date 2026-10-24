//! "Man the gate" (headless): a zone's people with the Gate guard goal (180), their home, the
//! gate they guard and how far from it they stand over a while:
//! `cargo run --release -p kenshi-game --example gate_guards -- <install> X Y [seconds]`
use kenshi_game::sim::{GATE_GUARD, Sim};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:3".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    sim.step();
    let guards: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == GATE_GUARD)).collect();
    let gates: Vec<usize> = (0..sim.door_states.len()).filter(|&d| sim.is_gate(d)).collect();
    println!("{} gates: {:?}", gates.len(), gates.iter().map(|&d| (d, sim.door_states[d].door.placement, sim.door_states[d].phase)).collect::<Vec<_>>());
    let near_gate = |sim: &Sim, i: usize| {
        let at = sim.folk[i].mover.position_units();
        gates
            .iter()
            .map(|&d| (d, (sim.door_states[d].door.position[0] - at[0]).hypot(sim.door_states[d].door.position[2] - at[2])))
            .min_by(|x, y| x.1.total_cmp(&y.1))
    };
    for &i in &guards {
        println!("guard {i}: home {:?}, nearest gate {:?}", sim.folk[i].home, near_gate(&sim, i));
    }
    let end = sim.t + seconds;
    while sim.t < end {
        sim.step();
    }
    for &i in &guards {
        println!(
            "after {:.0} s guard {i}: goal {:?}, queue {:?}, nearest gate {:?}",
            sim.t,
            sim.folk[i].ai.current.as_ref().map(|c| c.task),
            sim.folk[i].ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
            near_gate(&sim, i)
        );
    }
    for &d in &gates {
        println!("gate {d} at {:.0?}", sim.door_states[d].door.position);
    }
    let players: Vec<[f32; 3]> = sim.folk.iter().filter(|p| p.player).map(|p| p.mover.position_units()).collect();
    println!("players at {players:.0?}");
    println!("gates now: {:?}", gates.iter().map(|&d| (d, sim.door_states[d].phase)).collect::<Vec<_>>());
}
