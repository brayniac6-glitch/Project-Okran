//! A shut gate keeps out someone hostile to its town, who bashes it until it breaks:
//! `cargo run --release -p kenshi-game --example bash_gate -- <install> X Y [seconds]`
use kenshi_game::sim::{DOOR_CLOSED, Sim};
use kenshi_world::navmesh::METRES;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(240.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(std::path::Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    // every gate shut and locked
    sim.step();
    let gates: Vec<usize> = (0..sim.door_states.len()).filter(|&d| sim.is_gate(d)).collect();
    for &d in &gates {
        let s = &mut sim.door_states[d];
        (s.phase, s.open, s.locked, s.level) = (DOOR_CLOSED, 0.0, true, 1);
    }
    sim.step();
    println!("{} gates, {} barriers", gates.len(), sim.nav.barriers.len());
    let Some(&g) = gates.first() else { return };
    let door = sim.door_states[g].door.clone();
    let owner = sim.door_owner(g);
    println!("gate at {:?}, owner {owner:?}, health {}", door.position.map(|v| v as i32), sim.door_states[g].health);
    // the town's middle, to tell in from out
    let town = kenshi_world::town::current_town(&sim.towns, door.position, None).map(|t| sim.towns[t].position).unwrap_or(door.position);
    let (dx, dz) = (door.position[0] - town[0], door.position[2] - town[2]);
    let l = dx.hypot(dz).max(1.0);
    let outside = [door.position[0] + dx / l * 700.0, door.position[1], door.position[2] + dz / l * 700.0];
    let inside = [door.position[0] - dx / l * 400.0, door.position[1], door.position[2] - dz / l * 400.0];
    // a raider: someone of the zone made a Dust Bandit, hostile to the town both ways
    let id = sim.folk.iter().find(|p| !p.player && !p.dead).map(|p| p.id).unwrap();
    let raiders = "Dust Bandits".to_string();
    let bandit_faction = sim.data.of_type(kenshi_data::ItemType::Faction).find(|f| f.name == raiders).map(|f| f.string_id.clone()).unwrap();
    if let Some(o) = &owner {
        for (x, y) in [(bandit_faction.clone(), o.clone()), (o.clone(), bandit_faction.clone())] {
            let r = sim.relations.entry(x.clone()).or_insert_with(|| kenshi_ai::faction::FactionRelations { faction: x.clone(), ..Default::default() });
            r.relations.insert(y, kenshi_ai::faction::Relation { forced_ally: false, relation: -100.0 });
        }
    }
    {
        let p = &mut sim.folk[id];
        p.faction = Some(bandit_faction);
        // (testing) as the player's, without a will of its own, ordered to bash the gate
        if std::env::var("BASH_ORDER").is_ok() {
            p.player = true;
            p.bash_order = Some(g);
        }
        p.home = None;
        p.mover.position = outside.map(|v| v * METRES);
        p.ai.queue.clear();
    }
    sim.arena.f[id].mover.position = outside.map(|v| v * METRES);
    // the raider's pass taken up before they ask for a way
    sim.step();
    sim.folk[id].pathing = Default::default();
    sim.step();
    let b = sim.nav.barriers[0];
    println!("barrier {:?} -> {:?} (units), heights {:.0}..{:.0}, owner {}, shut {} locked {}; raider pass {:?}", b.a.map(|v| v / METRES), b.b.map(|v| v / METRES), b.lo / METRES, b.hi / METRES, b.owner, b.shut, b.locked, sim.folk[id].pathing.pass);
    {
        let pass = sim.folk[id].pathing.pass;
        let (o, i) = (outside.map(|v| v * METRES), inside.map(|v| v * METRES));
        for (name, pass) in [("raider", pass), ("anyone", kenshi_world::navmesh::Pass::ANYONE)] {
            match sim.nav.find_path_as(o, i, 0.4, 3.0, pass) {
                kenshi_world::navmesh::PathResult::Found(f) => {
                    let len: f32 = f.points.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum();
                    let crosses = f.points.windows(2).any(|w| sim.nav.barriers.iter().any(|b| { let mut b = *b; b.owner = u32::MAX - 7; b.shut = true; b.stops(pass, w[0], w[1]) || b.stops(kenshi_world::navmesh::Pass { faction: 99, player: false, ignore: false }, w[0], w[1]) }));
                    println!("  {name}: path {} points, {len:.0} m, crosses a gate line {crosses}", f.points.len());
                }
                other => println!("  {name}: {other:?}"),
            }
        }
    }
    println!("raider {} from {:?} to {:?}", sim.folk[id].name, outside.map(|v| v as i32), inside.map(|v| v as i32));
    let mut last = 0.0;
    while sim.t < seconds {
        if sim.folk[id].gate_in_way.is_none() && !sim.door_states[g].broken {
            let p = &mut sim.folk[id];
            let manager = &mut sim.manager;
            if !p.pathing.pending && p.pathing.follow.is_none() {
                p.pathing.go_to(id, &p.mover, inside, false, manager);
            }
        }
        sim.step();
        if sim.t - last >= 10.0 {
            last = sim.t;
            let p = &sim.folk[id];
            let s = &sim.door_states[g];
            println!(
                "t {:.0}: raider at {:?} (gate {:.0} away), gate in way {:?}, health {:.1}, broken {}, open {:.2}",
                sim.t,
                p.mover.position_units().map(|v| v as i32),
                (p.mover.position_units()[0] - door.position[0]).hypot(p.mover.position_units()[2] - door.position[2]),
                p.gate_in_way,
                s.health,
                s.broken,
                s.open
            );
        }
    }
}
