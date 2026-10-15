//! Where a zone's people are put down: those standing off the navmesh (more than 0.5 m from
//! its nearest point, or more than 1 m above or below it), after `seconds` of the sim:
//! `cargo run --release -p kenshi-game --example spawn_check -- <install> X Y [seconds]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:3".to_string()]);
    while sim.t < seconds {
        sim.step();
    }
    let (mut off, mut n) = (0, 0);
    for p in &sim.folk {
        if p.away || p.dead {
            continue;
        }
        n += 1;
        let at = p.mover.position;
        let Some((_, q)) = sim.nav.closest_point(at, 50.0) else {
            println!("{} {}: no navmesh within 50 m of {:?}", p.id, sim.display_name(p.id), at);
            off += 1;
            continue;
        };
        let flat = (q[0] - at[0]).hypot(q[2] - at[2]);
        let dy = at[1] - q[1];
        if flat > 0.5 || dy.abs() > 1.0 {
            off += 1;
            let ground = sim.terrain.height_at(at[0] / kenshi_world::navmesh::METRES, at[2] / kenshi_world::navmesh::METRES).map(|g| g * kenshi_world::navmesh::METRES).unwrap_or(f32::NAN);
            println!(
                "{} {} ({:?}): at {:?} navmesh {:.1} m away, {dy:+.1} m above it; ground {:+.1} m; inside {:?} down {} task {:?}",
                p.id,
                sim.display_name(p.id),
                p.faction.as_deref().and_then(|f| sim.data.get(f)).map(|r| r.name.clone()),
                at.map(|v| (v * 10.0) as i32),
                flat,
                at[1] - ground,
                p.inside,
                p.down,
                p.ai.current.as_ref().map(|c| c.task)
            );
        }
    }
    println!("{off} of {n} off the navmesh");
}
