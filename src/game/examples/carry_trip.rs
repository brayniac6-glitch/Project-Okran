//! Picking someone up and putting them down (headless): one of the player's people knocked out,
//! another ordered to pick them up; when the body is taken onto the shoulder (Task_LiftSomething:
//! at 0.4 of "shoulder lift left") and when the lift is done (0.95), then the put-down (let go
//! once the backwards clip is under 0.2):
//! `cargo run --release -p kenshi-game --example carry_trip -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:2".to_string()]);
    sim.step();
    let squad: Vec<usize> = sim.folk.iter().filter(|p| p.player && p.race.is_none()).map(|p| p.id).collect();
    let [carrier, body] = squad[..2] else {
        println!("needs two of the player's people");
        return;
    };
    let len = sim.arena.sh.lengths.get("shoulder lift left").copied().unwrap_or(0.0);
    println!("\"shoulder lift left\" lasts {len:.2} s: taken up at {:.2}, done at {:.2}", len * 0.4, len * 0.95);
    sim.knock_out(body);
    sim.arena.f[body].medical.knockout_time = 600.0;
    sim.order_pickup(&[carrier], body);
    let start = sim.t;
    let (mut began, mut taken, mut done) = (None, None, None);
    while sim.t < start + 30.0 && done.is_none() {
        sim.step();
        let p = &sim.folk[carrier];
        if began.is_none() && p.lifting > 0.0 {
            began = Some(sim.t);
        }
        if taken.is_none() && p.carrying == Some(body) {
            taken = Some((sim.t, p.lifting));
        }
        if taken.is_some() && p.lifting == 0.0 {
            done = Some(sim.t);
        }
    }
    let rel = |t: Option<f32>| t.zip(began).map_or("never".to_string(), |(t, b)| format!("{:.2} s into the lift", t - b));
    println!("lift began {:?}; body taken {}; lift done {}", began.map(|b| b - start), rel(taken.map(|x| x.0)), rel(done));
    // Task_PutDownObject: let go once the backwards lift is under 0.2 (0.8 of the clip in)
    sim.order_put_down(&[carrier]);
    let put = sim.t;
    while sim.folk[carrier].carrying.is_some() && sim.t < put + 10.0 {
        sim.step();
    }
    println!("put down after {:.2} s (0.8 of the clip: {:.2}): carrying {:?}, the body carried by {:?}, down {}", sim.t - put, len * 0.8, sim.folk[carrier].carrying, sim.folk[body].carried_by, sim.folk[body].down);
}
