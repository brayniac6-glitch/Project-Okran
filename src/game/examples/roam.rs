//! The roaming population round a waiting player squad (headless):
//! `cargo run --release -p kenshi-game --example roam -- <install> X Y [seconds]`
//! Prints each homeless squad's leader every 30 s: where it is, its distance to the player's
//! squad, its plan, and who is fighting.
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(300.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:3".to_string()]);
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).unwrap();
    while sim.t < seconds {
        sim.step();
        if sim.tick.is_multiple_of(60 * 30) {
            let home = sim.folk[me].mover.position_units();
            let squads = sim.population.as_ref().map_or(vec![], |p| p.squads.clone());
            for (members, zone, template) in &squads {
                let l = members[0];
                let q = sim.folk[l].mover.position_units();
                let name = sim.data.get(template).map_or("?", |t| t.name.as_str());
                let fighting = members.iter().filter(|&&i| sim.arena.f[i].active).count();
                let down = members.iter().filter(|&&i| sim.folk[i].down).count();
                println!(
                    "t {:5.0} {name:24} from {}.{} at ({:.0}, {:.0}) {:5.0} from player, plan {:?}, fighting {fighting}, down {down}",
                    sim.t,
                    zone.0,
                    zone.1,
                    q[0],
                    q[2],
                    (q[0] - home[0]).hypot(q[2] - home[2]),
                    sim.folk[l]
                        .ai
                        .queue
                        .iter()
                        .map(|s| s.task)
                        .collect::<Vec<_>>()
                );
            }
            let players_down = sim.folk.iter().filter(|p| p.player && p.down).count();
            println!("t {:5.0} player squad: {players_down} down", sim.t);
        }
    }
}
