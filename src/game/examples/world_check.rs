//! The world's people in a zone over a while (headless): who spawns, whether they move, and
//! whether those who are enemies fight when they meet:
//! `cargo run --release -p kenshi-game --example world_check -- <install> X Y [seconds] [extras...]`
//! Every 60 s: people alive, squads, how many have moved, fighting; at the end each pair of
//! enemy factions that came within 300 units, how long they stood that near and how long they
//! fought, and the people who never moved.
use kenshi_game::sim::Sim;
use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(900.0, |s| s.parse().unwrap());
    let mut extras: Vec<String> = a.iter().skip(4).cloned().collect();
    if !extras.iter().any(|e| e.starts_with("player:")) {
        extras.push("player:1".to_string());
    }
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    let name = |sim: &Sim, f: &Option<String>| f.as_deref().and_then(|f| sim.data.get(f)).map_or("-".to_string(), |r| r.name.clone());
    let mut first_at: Vec<Option<[f32; 3]>> = Vec::new();
    let mut moved: Vec<f32> = Vec::new();
    // (faction a, faction b) -> (seconds near, seconds fighting each other)
    let mut meetings: BTreeMap<(String, String), (f32, f32)> = BTreeMap::new();
    let mut last_report = 0.0;
    let mut last_check = 0.0;
    while sim.t < seconds {
        sim.step();
        // each one's farthest from where it was first seen
        first_at.resize(sim.folk.len(), None);
        moved.resize(sim.folk.len(), 0.0);
        for p in &sim.folk {
            let q = p.mover.position_units();
            let s = *first_at[p.id].get_or_insert(q);
            moved[p.id] = moved[p.id].max((q[0] - s[0]).hypot(q[2] - s[2]));
        }
        if sim.t - last_check >= 1.0 {
            last_check = sim.t;
            // enemy factions standing within 300 units of each other, and whether they fight
            let live: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && !sim.folk[i].down && !sim.folk[i].away && sim.folk[i].prison.is_none()).collect();
            let mut near_pairs: BTreeMap<(String, String), bool> = BTreeMap::new();
            for &i in &live {
                for &j in &live {
                    if i >= j {
                        continue;
                    }
                    let (fi, fj) = (sim.folk[i].faction.clone(), sim.folk[j].faction.clone());
                    if fi == fj {
                        continue;
                    }
                    let enemy = fi.as_deref().and_then(|f| sim.relations.get(f)).is_some_and(|r| r.is_enemy(fj.as_deref()));
                    if !enemy {
                        continue;
                    }
                    let (p, q) = (sim.folk[i].mover.position_units(), sim.folk[j].mover.position_units());
                    if (p[0] - q[0]).hypot(p[2] - q[2]) > 300.0 {
                        continue;
                    }
                    let mut key = (name(&sim, &fi), name(&sim, &fj));
                    if key.0 > key.1 {
                        key = (key.1, key.0);
                    }
                    let fight = |x: usize, y: usize| {
                        let f = &sim.arena.f[x];
                        f.active && (f.focus == Some(y) || f.combat.target == Some(y))
                    };
                    let e = near_pairs.entry(key).or_insert(false);
                    *e |= fight(i, j) || fight(j, i);
                }
            }
            for (k, fought) in near_pairs {
                let e = meetings.entry(k).or_insert((0.0, 0.0));
                e.0 += 1.0;
                if fought {
                    e.1 += 1.0;
                }
            }
        }
        if sim.t - last_report >= 60.0 {
            last_report = sim.t;
            let alive = sim.folk.iter().filter(|p| !p.dead && !p.away).count();
            let squads = sim.population.as_ref().map_or(0, |p| p.squads.len());
            let moving = sim.folk.iter().filter(|p| !p.dead && moved[p.id] > 50.0).count();
            let fighting = (0..sim.folk.len()).filter(|&i| sim.arena.f[i].active).count();
            let down = sim.folk.iter().filter(|p| p.down && !p.dead).count();
            let dead = sim.folk.iter().filter(|p| p.dead).count();
            println!("t {:5.0}: {alive} alive ({} spawned), {squads} roaming squads, {moving} have moved 50+, {fighting} fighting, {down} down, {dead} dead", sim.t, sim.folk.len());
            // who fights whom now (attacker's faction -> target's)
            let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
            for i in 0..sim.folk.len() {
                let f = &sim.arena.f[i];
                if !f.active {
                    continue;
                }
                if let Some(t) = f.focus.or(f.combat.target) {
                    if sim.folk[i].faction == sim.folk[t].faction && std::env::var("WORLD_CHECK_SAME").is_ok() {
                        println!("    same side: {i} (team {}, platoon {:?}, foes {:?}) -> {t} (team {}, platoon {:?}); focus {:?} target {:?}", f.team, sim.folk[i].platoon, f.foes, sim.arena.f[t].team, sim.folk[t].platoon, f.focus, f.combat.target);
                    }
                    *pairs.entry((name(&sim, &sim.folk[i].faction), name(&sim, &sim.folk[t].faction))).or_default() += 1;
                }
            }
            if !pairs.is_empty() {
                println!("    fights: {pairs:?}");
            }
            // WORLD_CHECK_FACTIONS: the people by faction
            if std::env::var("WORLD_CHECK_FACTIONS").is_ok() {
                let mut by: BTreeMap<String, usize> = BTreeMap::new();
                for p in sim.folk.iter().filter(|p| !p.dead && !p.away) {
                    *by.entry(name(&sim, &p.faction)).or_default() += 1;
                }
                println!("    {by:?}");
            }
        }
    }
    println!("enemies meeting within 300 units (seconds near / seconds fighting each other):");
    for ((a, b), (near, fought)) in &meetings {
        println!("  {a} vs {b}: {near:.0} / {fought:.0}");
    }
    let still: Vec<String> = sim
        .folk
        .iter()
        .filter(|p| !p.player && !p.dead && !p.down && p.prison.is_none() && moved[p.id] < 5.0)
        .map(|p| format!("{} ({}, goal {:?})", p.id, name(&sim, &p.faction), p.ai.current.as_ref().map(|c| c.task)))
        .collect();
    println!("{} never moved: {}", still.len(), still.iter().take(25).cloned().collect::<Vec<_>>().join(", "));
}
