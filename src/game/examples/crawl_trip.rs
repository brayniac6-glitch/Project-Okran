//! A character whose legs have collapsed ordered to move (headless): whether it stays prone and
//! crawls until its legs bear it again:
//! `cargo run --release -p kenshi-game --example crawl_trip -- <install> X Y [seconds] [extras]`
//! Its legs are put just under the KO point (as the game's KENSHI_LEGLESS), then it is ordered
//! 300 units off; every second: prone, speed, its own speed cap, the legs' health, the KO point.
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(60.0, |s| s.parse().unwrap());
    let mut extras: Vec<String> = a.iter().skip(4).cloned().collect();
    if !extras.iter().any(|e| e.starts_with("player:")) {
        extras.push("player:1".to_string());
    }
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    sim.step();
    let Some(who) = sim.folk.iter().find(|p| p.player && p.race.is_none()).map(|p| p.id) else {
        println!("no player character");
        return;
    };
    let legs: Vec<bool> = sim.arena.f[who].medical.parts.iter().map(|q| q.kind == 1 && q.side != 0).collect();
    let ko = sim.arena.f[who].ko_point();
    for (q, leg) in sim.arena.f[who].medical.parts.iter_mut().zip(&legs) {
        if *leg {
            q.flesh = ko - 5.0;
            q.update_frac();
        }
    }
    sim.step();
    let at = sim.folk[who].mover.position_units();
    sim.order_move(&[who], [at[0] + 300.0, at[1], at[2]]);
    let mut next = sim.t;
    let mut stood = 0;
    let mut crawled = 0;
    let end = sim.t + seconds;
    while sim.t < end {
        sim.step();
        let f = &sim.arena.f[who];
        let p = &sim.folk[who];
        if p.speed > 0.0 {
            if f.prone() { crawled += 1 } else { stood += 1 }
        }
        if sim.t >= next {
            next += 1.0;
            let leg_hp: Vec<String> = f.medical.parts.iter().zip(&legs).filter(|(_, l)| **l).map(|(q, _)| format!("{:.1}", q.health())).collect();
            let q = p.mover.position_units();
            println!(
                "t {:.0}: prone {} down {} getting up {:.1} speed {:.1} own {:.1} legs {} ko {:.1} moved {:.0} goal {:?}",
                sim.t,
                f.prone(),
                p.down,
                p.getting_up,
                p.speed,
                p.own_speed,
                leg_hp.join("/"),
                f.ko_point(),
                (q[0] - at[0]).hypot(q[2] - at[2]),
                p.ai.current.as_ref().map(|c| c.task)
            );
        }
    }
    println!("ticks moving: {crawled} prone, {stood} standing");
    eprintln!("wall sweeps {} stuck {}", kenshi_world::navmesh::SWEEPS.load(std::sync::atomic::Ordering::Relaxed), kenshi_world::navmesh::STUCK.load(std::sync::atomic::Ordering::Relaxed));
}
