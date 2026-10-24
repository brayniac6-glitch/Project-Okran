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
    let mut last: HashMap<usize, [f32; 3]> = HashMap::new();
    let mut stalled = 0usize;
    while sim.t < seconds {
        sim.step();
        if sim.tick.is_multiple_of(600) {
            for p in &sim.folk {
                let at = p.mover.position_units();
                if p.pathing.follow.is_some() && let Some(b) = last.get(&p.id) && (b[0] - at[0]).hypot(b[2] - at[2]) < 5.0 {
                    stalled += 1;
                    // CENSUS_STALLED: who, where, heading for what, and who stands near
                    if std::env::var("CENSUS_STALLED").is_ok() {
                        let f = p.pathing.follow.as_ref().unwrap();
                        let near: Vec<String> = sim
                            .folk
                            .iter()
                            .filter(|o| o.id != p.id && !o.away)
                            .map(|o| (o, (o.mover.position[0] - p.mover.position[0]).hypot(o.mover.position[2] - p.mover.position[2])))
                            .filter(|(_, d)| *d < 1.2)
                            .map(|(o, d)| format!("{}@{d:.2}{}{}{}{}{} t{:?}", o.id, if o.claimed.is_some() { " claimed" } else { "" }, if o.pathing.follow.is_some() { " path" } else { "" }, if o.operating.is_some() { " operating" } else { "" }, if o.bed.is_some() { " bed" } else { "" }, if o.prison.is_some() { " prison" } else { "" }, o.ai.current.as_ref().map(|c| c.task)))
                            .collect();
                        println!(
                            "stalled t {:.0} {} at {:?} task {:?} goal {:?} gate {}/{} speed {:.1} claimed {:?} near [{}]",
                            sim.t,
                            p.id,
                            at.map(|v| v as i32),
                            p.ai.current.as_ref().map(|c| c.task),
                            f.path.goal.map(|v| (v * 10.0) as i32),
                            f.path.next,
                            f.path.gates.len(),
                            p.speed,
                            p.claimed,
                            near.join(", ")
                        );
                        // CENSUS_STALLED=2: the walls round it and a step toward its heading
                        if std::env::var("CENSUS_STALLED").is_ok_and(|v| v == "2") {
                            let mut walls = Vec::new();
                            let pos = p.mover.position;
                            if let Some(face) = p.pathing.face {
                                sim.nav.walls_near(face, pos, 1.5, &mut walls);
                            }
                            let mut path = f.path.clone();
                            let h = path.heading(pos, p.mover.velocity, p.mover.radius, false, None);
                            let target = [pos[0] + h.dir[0], pos[1] + h.dir[1], pos[2] + h.dir[2]];
                            let to = [target[0] - pos[0], 0.0, target[2] - pos[2]];
                            let l = to[0].hypot(to[2]).max(1e-6);
                            let d = [to[0] / l * 0.12, 0.0, to[2] / l * 0.12];
                            for &(a, b, n) in &walls {
                                let (fa, fb) = ([a[0], 0.0, a[2]], [b[0], 0.0, b[2]]);
                                let ab = [fb[0] - fa[0], fb[2] - fa[2]];
                                let t = (((pos[0] - fa[0]) * ab[0] + (pos[2] - fa[2]) * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-9)).clamp(0.0, 1.0);
                                let q = [fa[0] + ab[0] * t, fa[2] + ab[1] * t];
                                println!(
                                    "    wall {:?}-{:?} dist {:.3} n.dir {:.2} dy {:.2}",
                                    a.map(|v| (v * 10.0) as i32),
                                    b.map(|v| (v * 10.0) as i32),
                                    (pos[0] - q[0]).hypot(pos[2] - q[1]),
                                    (n[0] * d[0] + n[2] * d[2]) / 0.12,
                                    a[1] - pos[1]
                                );
                            }
                            let (t, hit) = kenshi_world::navmesh::NavWorld::sweep_wall(pos, d, p.mover.radius, &walls);
                            println!("    target {:?} sweep t {t:.3} hit {hit:?} r {:.2}", target.map(|v| (v * 10.0) as i32), p.mover.radius);
                        }
                    }
                }
                last.insert(p.id, at);
            }
        }
        // CENSUS_OFFMESH: who stands off the navmesh (no face) on the first ticks and every 10 s
        if std::env::var("CENSUS_OFFMESH").is_ok() && (sim.tick <= 3 || sim.tick.is_multiple_of(60) && sim.tick <= 300 || sim.tick.is_multiple_of(600)) {
            for p in sim.folk.iter().filter(|p| !p.away && p.pathing.face.is_none()) {
                let pos = p.mover.position;
                // the nearest face: how far off (m, on the ground plane and up), its kind, cut
                let closest = sim.nav.closest_point(pos, 5.0).map(|(f, q)| (f, (q[0] - pos[0]).hypot(q[2] - pos[2]), q[1] - pos[1], sim.nav.face(f).kind, sim.nav.face_cut(f)));
                println!("offmesh t {:.2} {} at {:?} inside {:?} home {:?} bed {:?} op {:?} near {:?} closest {closest:?}", sim.t, p.id, p.mover.position_units().map(|v| v as i32), p.inside, p.home, p.bed, p.operating, p.near);
            }
        }
        // CENSUS_TRACE=<id>: that one's place and plan every 5 s (CENSUS_TRACE_TICKS: every tick)
        if (sim.tick.is_multiple_of(300) || std::env::var("CENSUS_TRACE_TICKS").is_ok())
            && let Some(w) = std::env::var("CENSUS_TRACE").ok().and_then(|v| v.parse::<usize>().ok())
        {
            let p = &sim.folk[w];
            println!("t {:.2} {w} face {:?} op {:?} bed {:?} at {:?} town {:?} inside {:?} home {:?} speed {:.1} queue {:?}", sim.t, p.pathing.face, p.operating, p.bed, p.mover.position_units().map(|v| v as i32), p.town.map(|t| sim.towns[t].name.clone()), p.inside.map(|b| sim.data.get(&sim.placed[b].building).map_or(sim.placed[b].building.clone(), |r| r.name.clone())), p.home.map(|b| (b, sim.data.get(&sim.placed[b].building).map_or(sim.placed[b].building.clone(), |r| r.name.clone()))), p.speed, p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>());
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
    eprintln!("stalled on a path (10 s samples) {stalled}");
    eprintln!("paths found {} failed {}", kenshi_world::navmesh::PATHS[0].load(std::sync::atomic::Ordering::Relaxed), kenshi_world::navmesh::PATHS[1].load(std::sync::atomic::Ordering::Relaxed));
    eprintln!("wall sweeps {} stuck {}", kenshi_world::navmesh::SWEEPS.load(std::sync::atomic::Ordering::Relaxed), kenshi_world::navmesh::STUCK.load(std::sync::atomic::Ordering::Relaxed));
}
