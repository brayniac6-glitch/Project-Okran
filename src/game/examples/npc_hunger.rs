//! NPC hunger (`kenshi_game::npc_hunger`): each person's hunger and cap at the start and after
//! some game time:
//! `cargo run --release -p kenshi-game --example npc_hunger -- <install> X Y <seconds> [extras]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a[3].parse().unwrap();
    let extras: Vec<String> = a.iter().skip(4).cloned().collect();
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let show = |sim: &Sim, when: &str| {
        let mut rows: Vec<(String, f32)> = Vec::new();
        for p in &sim.folk {
            let tpl = p.platoon.and_then(|pl| sim.squad_template.get(&pl)).and_then(|t| sim.data.get(t)).map_or("-".to_string(), |t| t.name.clone());
            let who = if p.player { "player".to_string() } else { tpl };
            rows.push((who, sim.arena.f[p.id].medical.hunger.shown()));
        }
        let mut by: std::collections::BTreeMap<String, Vec<f32>> = Default::default();
        for (w, h) in rows {
            by.entry(w).or_default().push(h);
        }
        for (w, hs) in by {
            let lo = hs.iter().cloned().fold(f32::MAX, f32::min);
            let hi = hs.iter().cloned().fold(f32::MIN, f32::max);
            println!("{when} {w}: {} people, hunger {lo:.0}..{hi:.0}", hs.len());
        }
    };
    sim.step();
    show(&sim, "start");
    while sim.t < seconds {
        sim.step();
    }
    show(&sim, &format!("after {seconds} s ({:.1} game hours)", sim.t as f64 / sim.hour_secs));
}
