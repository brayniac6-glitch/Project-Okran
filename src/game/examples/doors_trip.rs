//! A zone's doors as they start and after a while of people walking through them:
//! `cargo run --release -p kenshi-game --example doors_trip -- <install> X Y [seconds]`
use kenshi_game::sim::Sim;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(std::path::Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let count = |sim: &Sim| {
        let mut c = [0usize; 6];
        for d in &sim.door_states {
            c[d.phase as usize] += 1;
            if d.locked && d.level >= 1 {
                c[4] += 1;
            }
            if d.broken {
                c[5] += 1;
            }
        }
        format!("closed {} open {} opening {} closing {} | locked {} broken {}", c[0], c[1], c[2], c[3], c[4], c[5])
    };
    println!("{} doors at the start: {}", sim.door_states.len(), count(&sim));
    if let Some(p) = sim.folk.iter().find(|p| p.player) {
        println!("player at {:?}", p.mover.position_units());
    }
    for d in sim.door_states.iter().filter(|d| d.door.always_locks || d.door.dist > 100.0).take(12) {
        let name = sim.data.get(&d.door.record).map_or(d.door.record.clone(), |r| r.name.clone());
        let b = sim.data.get(&sim.placed[d.door.placement].building).map_or(String::new(), |r| r.name.clone());
        println!("  big door {name} of {b}: phase {} open {:.2} locked {} dist {} speed {} axis {}", d.phase, d.open, d.locked, d.door.dist, d.door.speed, d.door.axis);
    }
    while sim.t < seconds {
        sim.step();
    }
    println!("after {seconds} s: {}", count(&sim));
}
