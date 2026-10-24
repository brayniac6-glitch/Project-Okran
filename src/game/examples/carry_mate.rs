//! Carrying one's own knocked-out squad mate past a town's guards (headless): no crime (it is
//! picked up, not kidnapped: F046519 counts only one being kidnapped, F044460):
//! `cargo run --release -p kenshi-game --example carry_mate -- <install> 24 27`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:2".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let mine: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    if mine.len() < 2 {
        println!("need two player characters, have {}", mine.len());
        return;
    }
    let (me, mate) = (mine[0], mine[1]);
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
    // by a guard
    let g = (0..sim.folk.len()).find(|&g| !sim.folk[g].player && sim.guards_town(g)).expect("a guard");
    let m = kenshi_world::navmesh::METRES;
    let at = sim.folk[g].mover.position;
    let f = if sim.folk[g].facing == [0.0; 3] { [0.0, 0.0, 1.0] } else { sim.folk[g].facing };
    for (k, &i) in [me, mate].iter().enumerate() {
        sim.folk[i].mover.position = [at[0] + f[0] * (8.0 + k as f32) * m, at[1], at[2] + f[2] * (8.0 + k as f32) * m];
        sim.arena.f[i].mover.position = sim.folk[i].mover.position;
    }
    step(&mut sim, 1.0);
    sim.knock_out(mate);
    sim.arena.f[mate].medical.knockout_time = 300.0;
    step(&mut sim, 1.0);
    sim.order_pickup(&[me], mate);
    for _ in 0..10 {
        step(&mut sim, 1.0);
        if sim.folk[me].carrying == Some(mate) {
            break;
        }
    }
    println!("carrying {:?}", sim.folk[me].carrying);
    step(&mut sim, 10.0);
    println!("after 10 s by guard {g}: crime {:?}, bounties {:?}", sim.folk[me].crime.as_ref().map(|c| c.kind), sim.folk[me].bounties.iter().map(|b| b.amount).collect::<Vec<_>>());
}
