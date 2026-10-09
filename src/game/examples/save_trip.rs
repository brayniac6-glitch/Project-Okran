//! A save's round trip (`kenshi_game::save`): a world run a while, saved, built again from the
//! save and compared person by person, then run on to see that no one is made twice:
//! `cargo run --release -p kenshi-game --example save_trip -- <install> X Y [seconds] [neighbours]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |n| n.parse().unwrap());
    let neighbours = a.get(4).is_some_and(|n| n == "1");
    let extras = vec!["player:3".to_string()];
    let mut sim = Sim::load(install, zx, zy, 0, &extras);
    if neighbours {
        let res = sim.res.clone();
        for (dx, dy) in [(1, 0), (0, 1), (-1, 0)] {
            sim.add_zone(zx + dx, zy + dy, &res);
        }
    }
    while sim.t < seconds {
        sim.step();
    }
    let save = sim.snapshot((zx, zy), |_| None);
    let path = std::env::temp_dir().join("kenshi-save-trip.ron");
    kenshi_game::save::write(&save, &path).unwrap();
    let back = kenshi_game::save::read(&path).unwrap();
    println!("saved {} people, {} zones, {} bytes", back.people.len(), back.zone_order.len(), std::fs::metadata(&path).map_or(0, |m| m.len()));
    let mut again = Sim::load(install, zx, zy, 0, &[format!("player:{}", back.players.len())]);
    again.restore(&back);
    let mut bad = 0;
    for (p, q) in sim.folk.iter().zip(&again.folk) {
        let same = p.name == q.name
            && p.character == q.character
            && p.faction == q.faction
            && p.platoon == q.platoon
            && p.home == q.home
            && p.mover.position == q.mover.position
            && p.prison.as_ref().map(|r| r.cage) == q.prison.as_ref().map(|r| r.cage)
            && p.slave.as_ref().map(|s| s.owner) == q.slave.as_ref().map(|s| s.owner)
            && p.inventory.equipped().count() == q.inventory.equipped().count()
            && p.dead == q.dead
            && sim.arena.f[p.id].medical.blood == again.arena.f[q.id].medical.blood;
        if !same {
            bad += 1;
            if bad <= 5 {
                println!("person {} differs: {:?} / {:?}", p.id, (&p.name, &p.character, p.home, p.platoon), (&q.name, &q.character, q.home, q.platoon));
            }
        }
    }
    let cages_ok = sim.furn.len() == again.furn.len() && sim.furn.iter().zip(&again.furn).all(|(x, y)| x.record == y.record && x.position == y.position);
    println!(
        "people {} -> {}, differing {bad}; furniture {} -> {} (same: {cages_ok}); prisoners {} -> {}; slaves {} -> {}",
        sim.folk.len(),
        again.folk.len(),
        sim.furn.len(),
        again.furn.len(),
        sim.folk.iter().filter(|p| p.prison.is_some()).count(),
        again.folk.iter().filter(|p| p.prison.is_some()).count(),
        sim.folk.iter().filter(|p| p.slave.is_some()).count(),
        again.folk.iter().filter(|p| p.slave.is_some()).count(),
    );
    // run on: nobody made again for the zones already in
    let before = again.folk.len();
    let until = again.t + 60.0;
    while again.t < until {
        again.step();
    }
    println!("after a minute more: {} people (was {before}); machine stock {}", again.folk.len(), again.machine_stock.len());
}
