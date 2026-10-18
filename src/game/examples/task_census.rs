//! Which plan steps the simulation drops for want of an executor (headless): a zone with its
//! residents run for a while, then each task's dropped steps and how many people took it up:
//! `cargo run --release -p kenshi-game --example task_census -- <install> X Y [seconds]`
use kenshi_game::sim::Sim;
use std::collections::HashMap;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(180.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    let mut current: HashMap<u16, usize> = HashMap::new();
    while sim.t < seconds {
        sim.step();
        // CENSUS_TRACE=<id>: that one's place and plan every 5 s
        if sim.tick.is_multiple_of(300)
            && let Some(w) = std::env::var("CENSUS_TRACE").ok().and_then(|v| v.parse::<usize>().ok())
        {
            let p = &sim.folk[w];
            println!("t {:.0} {w} at {:?} town {:?} inside {:?} home {:?} speed {:.1} queue {:?}", sim.t, p.mover.position_units().map(|v| v as i32), p.town.map(|t| sim.towns[t].name.clone()), p.inside.map(|b| sim.data.get(&sim.placed[b].building).map_or(sim.placed[b].building.clone(), |r| r.name.clone())), p.home.map(|b| (b, sim.data.get(&sim.placed[b].building).map_or(sim.placed[b].building.clone(), |r| r.name.clone()))), p.speed, p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>());
        }
        if sim.tick.is_multiple_of(60) {
            for p in &sim.folk {
                if let Some(c) = &p.ai.current {
                    *current.entry(c.task).or_default() += 1;
                }
            }
        }
    }
    let name = |t: u16| kenshi_ai::task_def::def(t).map_or("?", |d| d.name);
    let mut v: Vec<_> = sim.dropped_steps.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1));
    println!("dropped steps (task: count):");
    for (t, n) in v {
        println!("  {t:4} {:32} {n}", name(*t));
    }
    let mut c: Vec<_> = current.into_iter().collect();
    c.sort_by(|a, b| b.1.cmp(&a.1));
    println!("goals held (task: person-seconds):");
    for (t, n) in c.iter().take(30) {
        println!("  {t:4} {:32} {n}", name(*t));
    }
    // the turrets and who mans them
    let manned = sim.arena.turrets.iter().filter(|t| t.operator.is_some()).count();
    // CENSUS_IDLE: the goals the idle people have (by task), to find those not carried out
    if std::env::var("CENSUS_IDLE").is_ok() {
        let mut idle_goals: std::collections::BTreeMap<u16, usize> = Default::default();
        let mut idle = 0;
        for p in &sim.folk {
            if p.player || p.dead || p.ai.current.as_ref().is_some_and(|c| c.task != 14) {
                continue;
            }
            idle += 1;
            if std::env::var("CENSUS_IDLE").is_ok_and(|v| v == "2") {
                println!("idle {} {:?} package {:?} at {:?} queue {:?}", p.id, p.name, p.package, p.mover.position_units().map(|v| v as i32), p.ai.queue.iter().map(|s| s.task).collect::<Vec<_>>());
            }
            for t in p.ai.jobs.iter().flatten().map(|j| j.task).collect::<std::collections::BTreeSet<_>>() {
                *idle_goals.entry(t).or_default() += 1;
            }
        }
        println!("{idle} idle; their goals: {}", idle_goals.iter().map(|(t, n)| format!("{t} {} x{n}", kenshi_ai::task_def::def(*t).map_or("?", |d| d.name))).collect::<Vec<_>>().join(", "));
    }
    println!("turrets: {} ({} manned)", sim.arena.turrets.len(), manned);
    for p in sim.folk.iter().filter(|p| p.ai.current.as_ref().is_some_and(|c| c.task == 149)).take(4) {
        println!("  {} at {:?}: queue {:?} operating {:?}", p.id, p.mover.position_units().map(|v| v as i32), p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(), p.operating);
        if let Some(kenshi_game::sim::Target::Furniture(f)) = p.ai.queue.first().map(|s| s.target) {
            println!("    turret {f} at {:?}, approach {:?}, speed {:.1}, path {}", sim.furn[f].position.map(|v| v as i32), sim.furniture_approach[f].map(|v| v as i32), p.speed, p.pathing.follow.is_some());
        }
    }
}
