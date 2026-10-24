//! Each person with a home: the home building, where it stands and how far from them they were
//! put down: `cargo run --release -p kenshi-game --example homes -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let seconds: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    while sim.t < seconds {
        sim.step();
    }
    for p in &sim.folk {
        let me = p.mover.position_units();
        let Some(h) = p.home else {
            println!(
                "{} {} ({:?}): no home, at {:?}; town {:?}; task {:?} queue {:?} goals {:?}",
                p.id,
                sim.display_name(p.id),
                p.faction.as_deref().and_then(|f| sim.data.get(f)).map(|r| r.name.clone()),
                me.map(|v| v as i32),
                p.town.map(|t| sim.towns[t].name.clone()),
                p.ai.current.as_ref().map(|c| (c.task, c.target)),
                p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                p.ai.goals.iter().map(|g| g.task).collect::<Vec<_>>()
            );
            continue;
        };
        let pl = &sim.placed[h];
        let name = sim.data.get(&pl.building).map_or("?", |b| b.name.as_str());
        let d = (pl.position[0] - me[0]).hypot(pl.position[2] - me[2]);
        println!("{} {}: home {h} {name} at {:?}, {d:.0} away (me {:?})", p.id, sim.display_name(p.id), pl.position.map(|v| v as i32), me.map(|v| v as i32));
    }
    for &b in &sim.bars {
        let pl = &sim.placed[b];
        println!("bar {b}: {} at {:?}", sim.data.get(&pl.building).map_or("?", |r| r.name.as_str()), pl.position.map(|v| v as i32));
    }
    for k in [80usize, 272] {
        if let Some(f) = sim.furn.get(k) {
            let pl = &sim.placed[f.placement];
            println!("furniture {k}: {} at {:?} in placement {} {} at {:?}", f.name, f.position.map(|v| v as i32), f.placement, sim.data.get(&pl.building).map_or("?", |r| r.name.as_str()), pl.position.map(|v| v as i32));
        }
    }
    for (k, n) in sim.general.iter().enumerate() {
        let pl = &sim.placed[n.placement];
        if k >= 50 && k <= 56 {
            println!("node {k}: kind {} placement {} {} at {:?}", n.kind, n.placement, sim.data.get(&pl.building).map_or("?", |b| b.name.as_str()), sim.approach[k].map(|v| v as i32));
        }
    }
}
