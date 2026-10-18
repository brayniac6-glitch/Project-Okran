//! A town's ground loot and its override town (F051436 / F051437 / F048909): the town's zone
//! loaded, its loot put down once; its uniques then killed so the world states call for its
//! override, and the zone loaded afresh: the zone's items cleared and the override's loot put
//! down. `cargo run --release -p kenshi-game --example override_loot -- <install> [town name]`
use kenshi_data::GameDataContainer;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let mut order = kenshi_io::mods::Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let towns = kenshi_world::town::towns(install, &data);
    let k = |v: f32| ((v + kenshi_world::WORLD_SIZE / 2.0) / kenshi_world::ZONE_SIZE).floor() as i32;
    // a town whose override town has a loot spawn (Admag's once Esata is dead)
    let pick = towns.iter().find(|t| {
        let Some(r) = data.get(&t.record) else { return false };
        if a.get(1).is_some_and(|n| !t.name.contains(n.as_str())) {
            return false;
        }
        r.refs("override town").iter().any(|(o, _)| data.get(o).is_some_and(|o| !o.refs("loot spawn").is_empty()))
    });
    let Some(town) = pick else {
        println!("no town with loot and an override for dead uniques; overrides with loot:");
        for t in &towns {
            let Some(r) = data.get(&t.record) else { continue };
            for (o, _) in r.refs("override town") {
                let Some(o) = data.get(o) else { continue };
                let states: Vec<String> = o.refs("world state").iter().map(|(s, v)| format!("{}={:?}", data.get(s).map_or(s.clone(), |s| s.name.clone()), v)).collect();
                println!("  {} (loot {}) -> {} (loot {}) when {:?}", t.name, r.refs("loot spawn").len(), o.name, o.refs("loot spawn").len(), states);
            }
        }
        return;
    };
    let (zx, zy) = (k(town.position[0]), k(town.position[2]));
    println!("{} ({}) at zone {zx}.{zy}", town.name, town.record);
    let mut sim = kenshi_game::sim::Sim::load(install, zx, zy, 0, &["player:1".to_string()]);
    let count = |sim: &kenshi_game::sim::Sim| sim.ground.iter().filter(|g| (k(g.at[0]), k(g.at[2])) == (zx, zy)).count();
    let t0 = sim.t;
    while sim.t < t0 + 3.0 {
        sim.step();
    }
    let t = sim.towns.iter().position(|t| t.record == town.record).expect("the town in the sim");
    println!("before: record now {}, {} items lying in the zone, looted {:?}", sim.town_record(t), count(&sim), sim.looted.contains(&town.record));
    // a marked item: one put down there (on no item spot)
    let first = sim.ground.iter().find(|g| (k(g.at[0]), k(g.at[2])) == (zx, zy)).cloned().map(|mut g| {
        sim.next_ground += 1;
        g.id = sim.next_ground;
        g.at[0] += 3.0;
        sim.ground.push(g.clone());
        g.id
    });
    let loose = |sim: &kenshi_game::sim::Sim| {
        sim.ground.iter().filter(|g| (k(g.at[0]), k(g.at[2])) == (zx, zy) && !sim.spots.iter().any(|s| s.item == Some(g.id))).count()
    };
    println!("{} of them on no item spot", loose(&sim));
    // the override's world state: its "NPC is" uniques dead
    let r = data.get(&town.record).unwrap();
    for (o, _) in r.refs("override town") {
        let Some(o) = data.get(o) else { continue };
        if o.refs("loot spawn").is_empty() {
            continue;
        }
        // each "NPC is" the state needs (or, one that must not hold, the uniques it needs alive)
        // dead
        for (s, v) in o.refs("world state") {
            let must = v.first().copied().unwrap_or(1) != 0;
            for (c, w) in data.get(s).map(|s| s.refs("NPC is").to_vec()).unwrap_or_default() {
                if (w[0] == 0) == must {
                    sim.world.dead_uniques.insert(c.clone());
                }
            }
        }
        break;
    }
    let now = sim.town_record(t);
    println!("override now {now} ({})", data.get(&now).map_or("?".to_string(), |r| r.name.clone()));
    sim.step_for(2.0);
    println!("still loaded: {} items in the zone (the override waits for the zone to load)", count(&sim));
    // the zone loaded afresh
    sim.loot_zones.clear();
    sim.step_for(2.0);
    println!(
        "zone loaded again: {} items lying in the zone ({} on no spot), the one put down still there {:?}, override looted {:?}",
        count(&sim),
        loose(&sim),
        first.is_some_and(|id| sim.ground.iter().any(|g| g.id == id)),
        sim.looted.contains(&now)
    );
}

trait StepFor {
    fn step_for(&mut self, secs: f32);
}

impl StepFor for kenshi_game::sim::Sim {
    fn step_for(&mut self, secs: f32) {
        let t0 = self.t;
        while self.t < t0 + secs {
            self.step();
        }
    }
}
