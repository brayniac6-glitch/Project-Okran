//! Intruders carried out (headless): the player's character knocked out inside the home of
//! residents with "pickup intruder" (Remove Home Intruder, 223) and "take intruder outside"
//! (Intruder out, 224): a resident lifts it (Lifting, 68) and carries it out of the door
//! (Task_TakeIntruderOutside): `cargo run --release -p kenshi-game --example intruder_out -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).expect("a player character");
    let step = |sim: &mut Sim, secs: f32| {
        let end = sim.t + secs;
        while sim.t < end {
            sim.step();
            sim.arena.sh.hits.clear();
            for m in sim.messages.drain(..) {
                println!("t {:.0}: {m}", sim.t);
            }
            sim.said.clear();
        }
    };
    step(&mut sim, 1.0);
    let has = |sim: &Sim, i: usize, t: u16| sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == t);
    let resident = (0..sim.folk.len()).find(|&i| !sim.folk[i].player && !sim.folk[i].dead && has(&sim, i, 223) && sim.folk[i].home.is_some() && sim.folk[i].inside == sim.folk[i].home && sim.folk[i].mover.velocity == [0.0; 3]);
    let Some(r) = resident else {
        println!("no resident with goal 223 at home");
        return;
    };
    let home = sim.folk[r].home.unwrap();
    println!("resident {r} ({}), home {home}, goals 223 {} 224 {}; me {me}", sim.display_name(r), has(&sim, r, 223), has(&sim, r, 224));
    // the player knocked out by the resident inside its home, once it has settled
    step(&mut sim, 15.0);
    let at = sim.folk[r].mover.position;
    let m = kenshi_world::navmesh::METRES;
    // in front of it (its view cone, F046498)
    let f = if sim.folk[r].facing == [0.0; 3] { [0.0, 0.0, 1.0] } else { sim.folk[r].facing };
    sim.folk[me].mover.position = [at[0] + f[0] * 1.5 * m, at[1], at[2] + f[2] * 1.5 * m];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    step(&mut sim, 0.5);
    sim.knock_out(me);
    sim.arena.f[me].medical.knockout_time = 300.0;
    for k in 0..90 {
        step(&mut sim, 1.0);
        let p = &sim.folk[r];
        if k % 5 == 0 || sim.folk[me].carried_by.is_none() && k > 3 && sim.folk[me].inside.is_none() {
            println!(
                "t {:.0}: me inside {:?} carried by {:?} | {r}: current {:?} queue {:?} carrying {:?} inside {:?} target {:?} senses me {} jobs {:?}",
                sim.t,
                sim.folk[me].inside,
                sim.folk[me].carried_by,
                p.ai.current.as_ref().map(|c| c.task),
                p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                p.carrying,
                p.inside,
                sim.law_state.view.targets.get(&(r, 223)),
                p.sensed.iter().any(|e| e.0 == me),
                p.ai.jobs.iter().flatten().filter(|j| j.task == 223 || j.task == 224).map(|j| (j.task, j.priority, j.hours)).collect::<Vec<_>>()
            );
        }
        if k > 3 && sim.folk[me].carried_by.is_none() && sim.folk[me].inside.is_none() {
            println!("put outside at t {:.0}; {r} ignores {:?}", sim.t, sim.folk[r].ignored);
            break;
        }
    }
}
