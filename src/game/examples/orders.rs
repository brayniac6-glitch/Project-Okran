//! A player squad taking orders in a zone (headless check of `Sim::order_move` /
//! `Sim::order_attack`): `cargo run --release -p kenshi-game --example orders -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:3".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let squad: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let start = sim.folk[squad[0]].mover.position_units();
    println!("squad {squad:?} at {start:?}");
    // a resident to walk to and then attack
    let victim = sim.folk.iter().find(|p| !p.player).map(|p| p.id).unwrap();
    let goal = sim.folk[victim].mover.position_units();
    let show = |sim: &Sim, what: &str| {
        for &i in &squad {
            let p = &sim.folk[i];
            let q = p.mover.position_units();
            println!(
                "{what} t {:.1}: {i} at ({:.0}, {:.0}) queue {:?} fighting {}",
                sim.t,
                q[0],
                q[2],
                p.ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(),
                sim.arena.f[i].active
            );
        }
    };
    sim.order_move_squad(&squad, [goal[0] + 80.0, goal[1], goal[2]]);
    while sim.t < 20.0 {
        sim.step();
        if sim.tick.is_multiple_of(300) {
            show(&sim, "move");
        }
    }
    sim.order_attack(&squad, victim);
    while sim.t < 60.0 {
        sim.step();
        if sim.tick.is_multiple_of(300) {
            show(&sim, "attack");
        }
    }
    println!("victim down: {}", sim.folk[victim].down);
}
