//! The slavers' talk tasks (headless): those with Working the slaves (188) or Selling slaves
//! (194 / 218) among their goals, their squad's slaves, and what they do: a slaver walks up to
//! one of its slaves (Moving, Talking 159) and raises EV_SHOUT_AT_SLAVE_WORKER, or to the buyer
//! with EV_SLAVE_DELIVERY:
//! `cargo run --release -p kenshi-game --example slaver_talk -- <install> X Y [seconds]`
//! (SLAVER_GIVE=<n>: n of the zone's free townsfolk made the first slaver's squad's slaves;
//! SLAVER_PACKAGE=<string id>: the first slaver put on that AI package.)
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let step = |sim: &mut Sim, secs: f32| {
        let end = sim.t + secs;
        while sim.t < end {
            sim.step();
            sim.arena.sh.hits.clear();
            for m in sim.messages.drain(..) {
                println!("t {:.0}: {m}", sim.t);
            }
            for (who, text) in sim.said.drain(..) {
                println!("t {:.1}: {who} says \"{text}\"", sim.t);
            }
            sim.npc_talks.clear();
        }
    };
    step(&mut sim, 2.0);
    let tasks = |sim: &Sim, i: usize| sim.folk[i].ai.jobs.iter().flatten().filter(|j| [188, 194, 218].contains(&j.task)).map(|j| (j.task, j.priority, j.hours)).collect::<Vec<_>>();
    let mut with: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && !tasks(&sim, i).is_empty()).collect();
    // SLAVER_PACKAGE=<string id>: the first of them put on that AI package instead (e.g. 3396-gamedata.base,
    // "Slaver deliver to work camp": Selling slaves 218)
    if let Ok(pkg) = std::env::var("SLAVER_PACKAGE")
        && let Some(&s) = with.first()
    {
        // (its squad's leader: the package's Leader AI Goals hold the slaver tasks)
        sim.folk[s].role = 5;
        sim.set_package(s, &pkg);
        println!("{s} (role {}) put on package {pkg}: jobs {:?}", sim.folk[s].role, sim.folk[s].ai.jobs.iter().flatten().map(|j| j.task).collect::<Vec<_>>());
        with = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && !tasks(&sim, i).is_empty()).collect();
    }
    if let Some(n) = std::env::var("SLAVER_GIVE").ok().and_then(|v| v.parse::<usize>().ok())
        && let Some(&s) = with.first()
    {
        let f = sim.folk[s].faction.clone().unwrap_or_default();
        let owner = sim.folk[s].platoon;
        let at = sim.folk[s].mover.position_units();
        let mut free: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && !sim.folk[i].player && sim.folk[i].slave.is_none() && sim.folk[i].faction != sim.folk[s].faction && sim.folk[i].race.is_none()).collect();
        free.sort_by(|&x, &y| {
            let d = |i: usize| (sim.folk[i].mover.position_units()[0] - at[0]).hypot(sim.folk[i].mover.position_units()[2] - at[2]);
            d(x).total_cmp(&d(y))
        });
        for &c in free.iter().take(n) {
            sim.make_slave(c, owner, &f);
            sim.folk[c].shackled = true;
            println!("made {c} ({}) a slave of {s}'s squad", sim.display_name(c));
        }
    }
    for &i in &with {
        let p = &sim.folk[i];
        let owned = (0..sim.folk.len()).filter(|&k| p.platoon.is_some() && sim.folk[k].slave.as_ref().is_some_and(|s| s.owner == p.platoon)).count();
        println!(
            "{i} ({}, {:?}) in {:?}: tasks {:?}, squad's slaves {owned}, has EV 38 {} EV 39 {}",
            sim.display_name(i),
            p.faction,
            p.town.map(|t| sim.towns[t].name.clone()),
            tasks(&sim, i),
            sim.has_event(i, 38),
            sim.has_event(i, 39)
        );
    }
    let start = sim.t;
    while sim.t < start + seconds {
        step(&mut sim, 2.0);
        for &i in &with {
            let p = &sim.folk[i];
            let g = p.ai.current.as_ref().map(|c| c.task);
            if g.is_some_and(|g| [188, 194, 218].contains(&g)) || p.ai.queue.first().is_some_and(|s| s.task == 159 || s.task == 161) {
                let target = p.ai.queue.iter().find_map(|s| match s.target {
                    kenshi_game::sim::Target::Person(x) => Some(x),
                    _ => None,
                });
                let d = target.map(|x| {
                    let (u, w) = (p.mover.position_units(), sim.folk[x].mover.position_units());
                    (u[0] - w[0]).hypot(u[2] - w[2])
                });
                println!(
                    "t {:.0}: {i}: goal {g:?} queue {:?} dist {:?} slaves {:?} work {:?} buyer {:?}",
                    sim.t,
                    p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                    d.map(|v| v as i32),
                    sim.talk_view.slaves.get(&i),
                    sim.talk_view.work_slave.get(&i),
                    sim.talk_view.buyer.get(&i)
                );
            }
        }
    }
    for &i in &with {
        let p = &sim.folk[i];
        let owned: Vec<usize> = (0..sim.folk.len()).filter(|&k| p.platoon.is_some() && sim.folk[k].slave.as_ref().is_some_and(|s| s.owner == p.platoon)).collect();
        println!("end: {i}: goal {:?}, its squad's slaves {owned:?}", p.ai.current.as_ref().map(|c| c.task));
    }
    for k in 0..sim.folk.len() {
        if let Some(s) = sim.folk[k].slave.as_ref() {
            println!("end: slave {k} owned by squad {:?} of {:?}, state {}", s.owner, s.faction, s.state);
        }
    }
    let mut v: Vec<_> = sim.dropped_steps.iter().collect();
    v.sort();
    println!("dropped steps: {v:?}");
}
