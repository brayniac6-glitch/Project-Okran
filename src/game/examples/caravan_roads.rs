//! Wandering traders on the roads (headless):
//! `cargo run --release -p kenshi-game --example caravan_roads -- <install> X Y [seconds]`
//! Loads the world at zone X.Y with a player squad and prints each caravan leader every 20 s:
//! where it is, the town it heads for, and its road way (state, junctions left, next marker).
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(300.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    println!(
        "roads: {}",
        sim.world.grid.as_ref().map_or(0, |g| g.roads.roads.len())
    );
    let mut took = 0;
    while sim.t < seconds {
        sim.step();
        if sim.tick.is_multiple_of(60 * 20) {
            for p in sim.folk.iter().filter(|p| p.caravan.is_some() && !p.down) {
                let q = p.mover.position_units();
                let to = p.caravan.as_ref().and_then(|c| c.target).map_or("-".to_string(), |t| sim.towns[t].name.clone());
                let road = match (&p.roads.path, sim.world.grid.as_ref()) {
                    (Some(r), Some(g)) if r.is_active() => {
                        took += 1;
                        let m = r.target(&g.roads);
                        format!(
                            "road state {} junctions {} next ({:.0}, {:.0}) goal ({:.0}, {:.0})",
                            r.state, r.junctions.len(), m[0], m[2], p.roads.goal[0], p.roads.goal[2]
                        )
                    }
                    _ => "off road".to_string(),
                };
                println!(
                    "t {:5.0} {:3} {:20} at ({:.0}, {:.0}) to {to:16} factor {:.2} {road}",
                    sim.t, p.id, p.name, q[0], q[2], p.roads.road_factor
                );
            }
        }
    }
    println!("samples on a road: {took}");
}
