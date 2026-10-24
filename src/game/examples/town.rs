//! Townsfolk of a zone on Kenshi's AI (`kenshi_game::sim`), printed for `town_map`:
//! `cargo run --release -p kenshi-game --example town -- <install> X Y [people] [seconds] [squads] > town.csv`
//!
//! Prints `t,id,x,y,z,speed,task,role,state` rows at 20 Hz (see `kenshi_game::sim`).
use kenshi_game::sim::Sim;
use kenshi_world::navmesh::METRES;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let people: usize = a.get(3).map_or(8, |n| n.parse().unwrap());
    let seconds: f32 = a.get(4).map_or(60.0, |n| n.parse().unwrap());
    let extras: Vec<String> = a.iter().skip(5).cloned().collect();
    let mut sim = Sim::load(install, zx, zy, people, &extras);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    while sim.t < seconds {
        let (t, tick) = (sim.t, sim.tick);
        sim.step();
        if tick.is_multiple_of(3) {
            for p in &sim.folk {
                let q = p.mover.position_units();
                let v = (p.mover.velocity[0].powi(2) + p.mover.velocity[2].powi(2)).sqrt() / METRES;
                let task = p.ai.queue.first().map_or(0, |s| s.task);
                let state = if p.down {
                    2
                } else if sim.arena.f[p.id].active {
                    1
                } else {
                    0
                };
                println!(
                    "{t:.3},{},{:.1},{:.1},{:.1},{v:.1},{task},{},{state}",
                    p.id, q[0], q[1], q[2], p.role
                );
            }
        }
    }
    for p in &sim.folk {
        eprintln!(
            "person {:2} home {:?}: goal {:?}, node {:?}",
            p.id,
            p.home.map(|h| sim
                .data
                .get(&sim.placed[h].building)
                .map_or("?", |b| b.name.as_str())),
            p.ai.current
                .as_ref()
                .map(|c| kenshi_ai::task_def::def(c.task).map_or("?", |d| d.name)),
            p.claimed.map(|n| sim.general[n].kind)
        );
    }
}
