//! Who fights whom: squads put down together (headless), each squad's people fighting and down
//! every 10 s, and how each faction stands to the others:
//! `cargo run --release -p kenshi-game --example fights -- <install> X Y seconds <extras...>`
//! (extras as `Sim::load` takes them: `player:3`, `squad:<template>[@slot]`).
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a[3].parse().unwrap();
    // `stance=<Block|Hold|Passive|Taunt|Ranged>` sets the player's people's stance
    let (stances, extras): (Vec<String>, Vec<String>) = a[4..].iter().cloned().partition(|e| e.starts_with("stance="));
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let players: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    for st in stances {
        use kenshi_game::orders::StanceOrder::*;
        let which = match &st["stance=".len()..] {
            "Block" => Block,
            "Hold" => Hold,
            "Passive" => Passive,
            "Taunt" => Taunt,
            _ => Ranged,
        };
        sim.toggle_stance(&players, which);
    }
    // the squads put down: everyone not a resident (no home) and up
    let platoons: Vec<(Option<String>, Vec<usize>)> = {
        let mut by: Vec<(Option<usize>, Option<String>, Vec<usize>)> = vec![];
        for p in sim.folk.iter().filter(|p| p.home.is_none()) {
            match by.iter_mut().find(|b| b.0 == p.platoon && p.platoon.is_some()) {
                Some(b) => b.2.push(p.id),
                None => by.push((p.platoon, p.faction.clone(), vec![p.id])),
            }
        }
        by.into_iter().map(|(_, f, m)| (f, m)).collect()
    };
    let data = sim.data.clone();
    let name = |f: &Option<String>| f.as_ref().and_then(|f| data.get(f)).map_or("-".to_string(), |r| r.name.clone());
    for (f, m) in &platoons {
        let rel = f.as_ref().and_then(|f| sim.relations.get(f));
        let stance: Vec<String> = platoons
            .iter()
            .filter(|(g, _)| g != f)
            .map(|(g, _)| format!("{}: {}", name(g), rel.map_or("?".into(), |r| if r.is_enemy(g.as_deref()) { "enemy".to_string() } else if r.is_ally(g.as_deref()) { "ally".into() } else { format!("{:.0}", r.relation(g.as_deref().unwrap_or(""))) })))
            .collect();
        println!("{} x{} {:?} [{}]", name(f), m.len(), m, stance.join(", "));
    }
    // FIGHTS_PROVOKE: at 3 s the first squad put down attacks the nearest resident (one not put
    // down), to see who comes to its aid
    let ours: Vec<usize> = platoons.iter().flat_map(|(_, m)| m.iter().copied()).collect();
    let mut provoked = false;
    // FIGHTS_RETREAT=x,z: at 6 s the player's people are ordered to run there
    let retreat: Option<[f32; 3]> = std::env::var("FIGHTS_RETREAT").ok().and_then(|v| {
        let (x, z) = v.split_once(',')?;
        Some([x.parse().ok()?, 0.0, z.parse().ok()?])
    });
    let mut retreated = false;
    // FIGHTS_PACKAGE=<package name>[@seconds]: then (12 s by default) the first squad put down
    // takes that AI_PACKAGE's goals, as a raid's retreat package is taken ("Retreat and
    // refresh", "Normal Raid Retreat [passive]"); every 10 s how far its people are from where
    // they stood then, and how many are up
    let package: Option<(String, f32)> = std::env::var("FIGHTS_PACKAGE").ok().map(|v| match v.split_once('@') {
        Some((n, s)) => (n.to_string(), s.parse().unwrap_or(12.0)),
        None => (v, 12.0),
    });
    let mut packaged: Option<Vec<[f32; 3]>> = None;
    while sim.t < seconds {
        sim.step();
        if let Some((name, at)) = &package
            && packaged.is_none()
            && sim.t > *at
        {
            let (_, m) = &platoons[0];
            let pkg = data.records.values().find(|r| r.name == *name).map(|r| r.string_id.clone()).expect("package");
            sim.take_package(m, &pkg);
            packaged = Some(m.iter().map(|&i| sim.folk[i].mover.position_units()).collect());
            println!("t {:.0}: {} take package {name}", sim.t, m.len());
        }
        if let Some(from) = &packaged
            && sim.tick.is_multiple_of(600)
        {
            let (_, m) = &platoons[0];
            let away: Vec<i32> = m.iter().zip(from).filter(|(i, _)| !sim.folk[**i].down).map(|(&i, f)| {
                let q = sim.folk[i].mover.position_units();
                (q[0] - f[0]).hypot(q[2] - f[2]) as i32
            }).collect();
            let tasks: Vec<u16> = m.iter().filter(|&&i| !sim.folk[i].down).filter_map(|&i| sim.folk[i].ai.current.as_ref().map(|c| c.task)).collect();
            println!("t {:.0}: package squad up {} away {away:?} goals {tasks:?}", sim.t, away.len());
        }
        if let Some(to) = retreat
            && !retreated
            && sim.t > 6.0
        {
            retreated = true;
            sim.player_run = true;
            sim.order_move(&players, to);
            println!("retreat to {to:?}");
        }
        if !provoked && sim.t > 3.0 && std::env::var("FIGHTS_PROVOKE").is_ok() {
            provoked = true;
            let (_, m) = &platoons[0];
            let at = sim.folk[m[0]].mover.position_units();
            let victim = (0..sim.folk.len()).filter(|i| !ours.contains(i) && !sim.folk[*i].down && sim.folk[*i].race.is_none()).min_by(|&a, &b| {
                let d = |i: usize| { let q = sim.folk[i].mover.position_units(); (q[0] - at[0]).hypot(q[2] - at[2]) };
                d(a).total_cmp(&d(b))
            });
            if let Some(v) = victim {
                println!("provoke: {} attack {v} ({})", name(&platoons[0].0), name(&sim.folk[v].faction));
                sim.order_attack(m, v);
            }
        }
        // FIGHTS_OTHERS: every 20 s, each of the others in the fight: its plan, whether it runs
        // off through it, its focus and how far that is
        if std::env::var("FIGHTS_OTHERS").is_ok() && sim.tick.is_multiple_of(1200) {
            for i in (0..sim.folk.len()).filter(|i| (std::env::var("FIGHTS_OTHERS").is_ok_and(|v| v == "all") || !ours.contains(i)) && sim.arena.f[*i].active) {
                let p = &sim.folk[i];
                let f = &sim.arena.f[i];
                let at = p.mover.position_units();
                let focus = f.focus.or(f.combat.target).map(|e| {
                    let q = sim.folk[e].mover.position_units();
                    format!("{e} at {:.0} down {}", (q[0] - at[0]).hypot(q[2] - at[2]), sim.folk[e].down)
                });
                println!(
                    "  other {i} t {:.0} down {} goal {:?} q {:?} away {} focus {focus:?} combat {:?} desired {} waiting {} act {}",
                    sim.t,
                    p.down,
                    p.ai.current.as_ref().map(|c| c.task),
                    p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                    f.ordered_away,
                    f.combat.state,
                    f.combat.desired,
                    f.combat.waiting,
                    f.act.is_some()
                );
            }
        }
        // FIGHTS_VERBOSE: each one put down every second: where, its task and plan, whether it
        // fights, its weapon, its focus and how far that is
        if std::env::var("FIGHTS_VERBOSE").is_ok() && sim.tick.is_multiple_of(60) {
            let all: Vec<usize> = if std::env::var("FIGHTS_VERBOSE").is_ok_and(|v| v == "2") { (0..sim.folk.len()).filter(|&i| sim.arena.f[i].active).collect() } else { ours.clone() };
            for &i in &all {
                let p = &sim.folk[i];
                let f = &sim.arena.f[i];
                let at = p.mover.position_units();
                let focus = f.focus.map(|e| {
                    let q = sim.folk[e].mover.position_units();
                    format!("{e} at {:.0}", (q[0] - at[0]).hypot(q[2] - at[2]))
                });
                println!(
                    "  {i} run {:.1} reach {:.1} t {:.0} at ({:.0},{:.0},{:.0}) task {:?} q {:?} active {} drawn {} down {} focus {:?} anim {:?} via {:?} path {} combat {:?} target {:?} attackers {:?} engagers {:?}",
                    f.run,
                    f.max_reach,
                    sim.t,
                    at[0],
                    at[1],
                    at[2],
                    p.ai.current.as_ref().map(|c| (c.task, c.target)),
                    p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                    f.active,
                    p.weapon_out,
                    p.down,
                    focus,
                    f.act.as_ref().map(|a| format!("{:?}", a.kind)),
                    f.via.map(|v| v.map(|c| c as i32)),
                    p.fight_path.len(),
                    f.combat.state,
                    f.combat.target,
                    f.combat.attackers,
                    f.combat.engagers,
                );
            }
        }
        if sim.tick.is_multiple_of(60 * 2) {
            let line: Vec<String> = platoons
                .iter()
                .map(|(f, m)| {
                    let fighting = m.iter().filter(|&&i| sim.arena.f[i].active).count();
                    let down = m.iter().filter(|&&i| sim.folk[i].down || sim.folk[i].dead).count();
                    let drawn = m.iter().filter(|&&i| sim.folk[i].weapon_out).count();
                    let l = m[0];
                    let at = sim.folk[l].mover.position_units();
                    format!("{} fight {fighting} drawn {drawn} down {down} task {:?} q {:?} at ({:.0},{:.0})", name(f), sim.folk[l].ai.current.as_ref().map(|c| c.task), sim.folk[l].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(), at[0], at[2])
                })
                .collect();
            let others = (0..sim.folk.len()).filter(|i| !ours.contains(i) && sim.arena.f[*i].active).count();
            let asleep = sim.folk.iter().filter(|p| p.bed.is_some()).count();
            println!("t {:4.0}  {} | others fighting {others} | in bed {asleep}", sim.t, line.join(" | "));
        }
    }
    // who of the town runs which duty (kenshi_game::sim::DUTY_*), by faction
    let mut duty: std::collections::BTreeMap<String, [usize; 5]> = Default::default();
    for p in &sim.folk {
        let f = p.faction.as_deref().and_then(|f| sim.data.get(f)).map_or("-".to_string(), |r| r.name.clone());
        let e = duty.entry(f).or_default();
        e[0] += 1;
        for (k, bit) in [kenshi_game::sim::DUTY_ARRESTS | kenshi_game::sim::DUTY_ENFORCES, kenshi_game::sim::DUTY_WATCHES_SLAVES, kenshi_game::sim::DUTY_GETS_FOOD, kenshi_game::sim::DUTY_KIDNAPS].into_iter().enumerate() {
            if p.duties & bit != 0 {
                e[k + 1] += 1;
            }
        }
    }
    for (f, [n, a, w, g, k]) in duty {
        println!("duties {f}: {n} people, arresting {a}, watching slaves {w}, getting food {g}, kidnapping {k}");
    }
    // the goals the fighting are following now
    let mut goals: std::collections::BTreeMap<u16, usize> = Default::default();
    for p in sim.folk.iter().filter(|p| sim.arena.f[p.id].active) {
        if let Some(c) = &p.ai.current {
            *goals.entry(c.task).or_default() += 1;
        }
    }
    println!("fighting by goal: {goals:?}");
}
