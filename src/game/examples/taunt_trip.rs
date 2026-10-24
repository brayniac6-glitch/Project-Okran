//! Target scoring with the stances (kenshi_combat::arena::score_targets): a squad of the player's
//! beside a bandit squad, the first of the player's taunting (TAUNT=1) or not; who the bandits
//! go for, counted over the fight:
//! `TAUNT=1 cargo run --release -p kenshi-game --example taunt_trip -- <install> X Y "<squad template>" [seconds]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(4).map_or(30.0, |s| s.parse().unwrap());
    let extras = vec!["player:3".to_string(), format!("squad:{}", a[3])];
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let players: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let foes: Vec<usize> = sim.folk.iter().filter(|p| !p.player && p.faction.as_deref().is_some_and(|f| sim.data.get(f).is_some_and(|r| r.name == "Dust Bandits"))).map(|p| p.id).collect();
    println!("{} of the player's, {} bandits", players.len(), foes.len());
    if std::env::var("TAUNT").is_ok() {
        sim.toggle_stance(&players[..1], kenshi_game::orders::StanceOrder::Taunt);
        println!("{} taunts", players[0]);
    }
    // the bandits beside the squad, set on it
    let at = sim.folk[players[0]].mover.position_units();
    for (k, &b) in foes.iter().enumerate() {
        let spot = [at[0] + 60.0 + (k % 3) as f32 * 15.0, at[1], at[2] + (k / 3) as f32 * 15.0];
        sim.folk[b].mover.position = spot.map(|v| v * 0.1);
        sim.arena.f[b].mover.position = sim.folk[b].mover.position;
    }
    if std::env::var("NO_ORDER").is_err() {
        sim.order_attack(&foes, players[1]);
    }
    let mut on: std::collections::BTreeMap<usize, u32> = Default::default();
    while sim.t < seconds {
        sim.step();
        if sim.tick.is_multiple_of(30) {
            for &b in &foes {
                if let Some(t) = sim.arena.f[b].focus.filter(|t| players.contains(t)) {
                    *on.entry(t).or_default() += 1;
                }
            }
        }
    }
    println!("bandits' targets (half-seconds on each): {on:?}");
}
