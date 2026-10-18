//! "Corpse disposal" (headless): the zone's disposal machines and who has the goal; a stranger
//! killed in the town, and whether someone finds the body, lifts it and feeds it into a
//! machine:
//! `cargo run --release -p kenshi-game --example corpse_disposal -- <install> X Y [seconds]`
use kenshi_game::sim::{BF_CORPSE_DISPOSAL, CORPSE_DISPOSAL, FIND_CORPSES, Sim};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(300.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    sim.step();
    let machines: Vec<usize> = (0..sim.furn.len()).filter(|&k| sim.furn[k].function == BF_CORPSE_DISPOSAL).collect();
    for &k in &machines {
        println!("machine {k}: {} at {:.0?}", sim.furn[k].name, sim.furn[k].position);
    }
    // those with Corpse disposal or Find corpses, the finders first
    let has = |sim: &Sim, i: usize, t: u16| sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == t);
    let mut disposers: Vec<usize> = (0..sim.folk.len())
        .filter(|&i| !sim.folk[i].dead && (has(&sim, i, CORPSE_DISPOSAL) || has(&sim, i, FIND_CORPSES)))
        .collect();
    disposers.sort_by_key(|&i| !has(&sim, i, FIND_CORPSES));
    println!("finders: {:?}", disposers.iter().filter(|&&i| has(&sim, i, FIND_CORPSES)).collect::<Vec<_>>());
    let name = |sim: &Sim, f: &Option<String>| f.as_deref().and_then(|f| sim.data.get(f)).map_or("-".to_string(), |r| r.name.clone());
    println!("{} with Corpse disposal or Find corpses: {:?}", disposers.len(), disposers.iter().map(|&i| (i, name(&sim, &sim.folk[i].faction))).collect::<Vec<_>>());
    if let Some(&d) = disposers.first() {
        println!("jobs of {d}: {:?}", sim.folk[d].ai.jobs.iter().map(|l| l.iter().map(|j| (j.task, j.priority, j.hours)).collect::<Vec<_>>()).collect::<Vec<_>>());
    }
    let (Some(&m), Some(&d)) = (machines.first(), disposers.first()) else {
        println!("nothing to test here");
        return;
    };
    // a body: one of another faction (else anyone not a disposer), killed near the first disposer
    let body = (0..sim.folk.len())
        .find(|&i| !sim.folk[i].player && !sim.folk[i].dead && !disposers.contains(&i) && sim.folk[i].faction != sim.folk[d].faction)
        .or_else(|| (0..sim.folk.len()).find(|&i| !sim.folk[i].player && !sim.folk[i].dead && !disposers.contains(&i)))
        .expect("a body");
    // (CORPSE_OPEN) in the open: the finder stood 300 units from the machine, the body by it
    let at = if std::env::var("CORPSE_OPEN").is_ok() {
        let q = sim.furn[m].position;
        let metres = kenshi_world::navmesh::METRES;
        let s = sim.nav.closest_point([q[0] + 300.0, q[1], q[2]].map(|v| v * metres), 20.0).map(|(_, r)| r).unwrap_or(q.map(|v| v * metres));
        sim.folk[d].mover.position = s;
        sim.arena.f[d].mover.position = s;
        s.map(|v| v / metres)
    } else {
        sim.folk[d].mover.position_units()
    };
    // on the walking mesh 60 units off (where the first finds a spot)
    let metres = kenshi_world::navmesh::METRES;
    let spot = [[60.0f32, 0.0], [-60.0, 0.0], [0.0, 60.0], [0.0, -60.0], [0.0, 0.0]]
        .iter()
        .find_map(|o| sim.nav.closest_point([at[0] + o[0], at[1], at[2] + o[1]].map(|v| v * metres), 2.0).map(|(_, q)| q.map(|v| v / metres)))
        .unwrap_or(at);
    sim.folk[body].mover.position = spot.map(|v| v * kenshi_world::navmesh::METRES);
    sim.arena.f[body].mover.position = sim.folk[body].mover.position;
    sim.knock_out(body);
    sim.folk[body].dead = true;
    println!("body {body} ({}) killed at {:.0?}, machine {m} {:.0} units away", name(&sim, &sim.folk[body].faction), spot, {
        let q = sim.furn[m].position;
        (q[0] - spot[0]).hypot(q[2] - spot[2])
    });
    let end = sim.t + seconds;
    let mut last = String::new();
    while sim.t < end {
        sim.step();
        let carrier = sim.folk[body].carried_by;
        let state = format!(
            "carried by {carrier:?}, gone {}, at {:.0?}; carrier's goal {:?} step {:?}",
            sim.folk[body].away,
            sim.folk[body].mover.position_units(),
            carrier.and_then(|c| sim.folk[c].ai.current.as_ref().map(|g| g.task)),
            carrier.and_then(|c| sim.folk[c].ai.queue.first().map(|s| s.task)),
        );
        let short = format!("{carrier:?} {}", sim.folk[body].away);
        if short != last {
            println!("t {:.1}: {state}", sim.t);
            last = short;
        }
        if sim.folk[body].away {
            break;
        }
        if std::env::var("CORPSE_TRACE").is_ok() && sim.tick % 300 == 0 {
            let c = &sim.folk[d];
            println!("  t {:.0}: finder at {:.0?} speed {:.1} own {:.1} pending {} follow {}", sim.t, c.mover.position_units(), c.speed, c.own_speed, c.pathing.pending, c.pathing.follow.is_some());
        }
    }
    // who went for it
    let lifting: Vec<(usize, Option<u16>, Vec<u16>)> = disposers.iter().map(|&i| (i, sim.folk[i].ai.current.as_ref().map(|g| g.task), sim.folk[i].ai.queue.iter().map(|s| s.task).collect())).collect();
    println!("disposers now: {lifting:?}");
    println!("body gone: {}", sim.folk[body].away);
}
