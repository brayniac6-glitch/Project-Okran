//! Item spots across a save (headless): an item taken from a spot, the game saved and restored
//! (in memory), and whether the spot still waits to fill again:
//! `cargo run --release -p kenshi-game --example spot_save -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    while sim.t < 3.0 {
        sim.step();
    }
    let Some(k) = sim.spots.iter().position(|s| s.item.is_some()) else {
        println!("no filled item spot here");
        return;
    };
    // the item taken off it: the spot waits its respawn time
    let id = sim.spots[k].item.unwrap();
    sim.ground.retain(|g| g.id != id);
    let end = sim.t + 2.0;
    while sim.t < end {
        sim.step();
    }
    let at = sim.spots[k].at;
    println!("spot {k} at {:?}: item {:?}, due {:.1} h (now {:.1})", at.map(|v| v as i32), sim.spots[k].item, sim.spots[k].due, sim.hours_gone());
    let save = sim.snapshot((zx, zy), |_| None);
    let mut again = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    again.no_spawns();
    again.restore(&save);
    let end = again.t + 3.0;
    while again.t < end {
        again.step();
    }
    let s = again.spots.iter().find(|s| s.at == at).expect("the spot again");
    println!("after the save: item {:?}, due {:.1} h (now {:.1})", s.item, s.due, again.hours_gone());
}
