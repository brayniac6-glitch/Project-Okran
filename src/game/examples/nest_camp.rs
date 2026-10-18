//! A nest's people at their camp (headless): a nest of the zone's biome placed by the player
//! (KENSHI_NEST_RECORD=<TOWN name> picks it) and built, then every 20 s what its people do (their
//! current goals, how many sit, how far they have moved) and its camp's furniture:
//! `KENSHI_NEST_RECORD="Dust Bandit Camp" cargo run --release -p kenshi-game --example nest_camp -- <install> X Y [seconds]`
use kenshi_game::sim::Sim;
use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(180.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).expect("a player character");
    sim.step();
    let p = sim.folk[me].mover.position_units();
    let at = [p[0] + 5000.0, p[1], p[2]];
    let at = [at[0], sim.terrain.height_at(at[0], at[2]).unwrap_or(at[1]), at[2]];
    let Some(name) = sim.spawn_nest_at(at) else {
        println!("no nest for this biome");
        return;
    };
    println!("nest {name} at {at:?}");
    let k = sim.nests.len() - 1;
    let mut first: BTreeMap<usize, [f32; 3]> = BTreeMap::new();
    let mut last = -100.0f32;
    while sim.t < seconds {
        sim.step();
        let n = &sim.nests[k];
        if !n.built {
            continue;
        }
        for &m in &n.members {
            first.entry(m).or_insert(sim.folk[m].mover.position_units());
        }
        if sim.t - last >= 20.0 {
            last = sim.t;
            let mut goals: BTreeMap<String, usize> = BTreeMap::new();
            let (mut sitting, mut moved) = (0, 0);
            for &m in &n.members {
                let q = &sim.folk[m];
                if q.dead {
                    continue;
                }
                let g = q.ai.current.as_ref().map_or("-".to_string(), |c| c.task.to_string());
                *goals.entry(g).or_default() += 1;
                if q.operating.is_some() {
                    sitting += 1;
                }
                let (s, now) = (first[&m], q.mover.position_units());
                if (s[0] - now[0]).hypot(s[2] - now[2]) > 50.0 {
                    moved += 1;
                }
            }
            let camp: Vec<_> = sim.furn.iter().filter(|f| (f.position[0] - at[0]).hypot(f.position[2] - at[2]) < 2500.0).map(|f| f.function).collect();
            println!("t {:.0}: {} people, goals {goals:?}, sitting {sitting}, moved {moved}; camp furniture {}", sim.t, n.members.len(), camp.len());
            if std::env::var("NEST_DEBUG_LEADERS").is_ok() {
                for &m in &n.members {
                    let q = &sim.folk[m];
                    if q.ai.jobs.iter().flatten().any(|j| j.task == 214) {
                        println!("  {m}: current {:?} queue {:?} operating {:?} at {:?}", q.ai.current.as_ref().map(|c| (c.task, c.target)), q.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(), q.operating, q.mover.position_units().map(|v| v as i32));
                    }
                }
            }
        }
    }
}
