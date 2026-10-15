//! A campaign's raid played out headless (set KENSHI_CAMPAIGN=<name>, KENSHI_CAMPAIGN_NEAR):
//! what becomes of the player's people.
//! `cargo run --release -p kenshi-game --example raid_trip -- <install> X Y <seconds> [extras]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a[3].parse().unwrap();
    let extras: Vec<String> = a.iter().skip(4).cloned().collect();
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    let players: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    // (testing) RAID_FOOD_STORE: a storage of 10 dried meat beside the squad
    if std::env::var("RAID_FOOD_STORE").is_ok() {
        let data = sim.data.clone();
        let store = data.records.values().find(|r| r.name.starts_with("Storage Crate")).expect("storage");
        let meat = data.records.values().find(|r| r.name == "Dried Meat").expect("food");
        let at = sim.folk[players[0]].mover.position_units();
        let mut b = kenshi_game::base::PlayerBuilding::new(store, [at[0] + 100.0, at[1], at[2]], 0.0);
        b.done = true;
        b.confirmed = true;
        if b.storage.is_none() {
            b.storage = Some(kenshi_char::inventory::Inventory { sections: vec![kenshi_char::inventory::Section::new("main", 8, 8, kenshi_char::inventory::ANY)] });
        }
        b.storage.as_mut().unwrap().add(kenshi_char::inventory::Item::from_record(meat, 10), false);
        sim.buildings.push(b);
    }
    let mut last = String::new();
    let mut was_down: Vec<bool> = Vec::new();
    let mut knocked = false;
    while sim.t < seconds {
        sim.step();
        // (testing) the player's people go down once raiders are near: the victory plays out
        // (RAID_NO_KO: they stay up, to see the raiders' announcement)
        if !knocked && std::env::var("RAID_NO_KO").is_err() && sim.tick.is_multiple_of(60) {
            let near = sim.folk.iter().any(|p| {
                p.world_squad.is_some() && !p.away && players.iter().any(|&i| {
                    let (a, b) = (p.mover.position_units(), sim.folk[i].mover.position_units());
                    (a[0] - b[0]).hypot(a[2] - b[2]) < 300.0
                })
            });
            if near {
                knocked = true;
                for &i in &players {
                    sim.knock_out(i);
                    sim.arena.f[i].medical.knockout_time = 600.0;
                }
                println!("t {:.0}: the player's people knocked out", sim.t);
            }
        }
        // RAID_SQUADS: each campaign squad's place, route left and people every 30 s
        if std::env::var("RAID_SQUADS").is_ok() && sim.tick.is_multiple_of(1800) {
            for (k, q) in sim.world.squads.iter().enumerate().filter(|(_, q)| q.alive && matches!(q.goal, kenshi_game::world::Goal::Campaign { .. })) {
                let people: Vec<String> = q.members.iter().map(|&m| {
                    let p = &sim.folk[m];
                    format!("{m}@{:?} {:?} q{:?}", p.mover.position_units().map(|v| v as i32), p.ai.current.as_ref().map(|c| c.task), p.ai.queue.iter().map(|s| s.task).collect::<Vec<_>>())
                }).collect();
                println!("squad {k} t {:.0}: at {:?} route {} won {:?} people {}", sim.t, q.pos.map(|v| v as i32), q.route.len(), q.won, people.join(", "));
            }
        }
        // RAID_DOWN: each one who goes down, and why it might be
        if std::env::var("RAID_DOWN").is_ok() {
            was_down.resize(sim.folk.len(), false);
            for i in 0..sim.folk.len() {
                if sim.folk[i].down && !was_down[i] {
                    let m = &sim.arena.f[i].medical;
                    let worst = m.parts.iter().map(|q| (q.health(), q.id.clone())).min_by(|a, b| a.0.total_cmp(&b.0));
                    println!("down {i} {} t {:.1}: hunger {:.0} blood {:.0}/{:.0} worst {:?} fighting {} at {:?}", sim.display_name(i), sim.t, m.hunger.level, m.blood, m.max_blood, worst, sim.arena.f[i].active, sim.folk[i].mover.position_units().map(|v| v as i32));
                }
                was_down[i] = sim.folk[i].down;
            }
        }
        // RAID_WATCH=<id>: that person's body every 5 s (why it goes down)
        if let Some(w) = std::env::var("RAID_WATCH").ok().and_then(|v| v.parse::<usize>().ok())
            && sim.tick.is_multiple_of(300)
            && w < sim.folk.len()
        {
            let f = &sim.arena.f[w];
            let m = &f.medical;
            let worst = m.parts.iter().map(|q| (q.health(), q.id.clone())).min_by(|a, b| a.0.total_cmp(&b.0));
            println!("watch {w} t {:.0}: down {} away {} hunger {:.0} stomach {:.0} blood {:.0}/{:.0} worst {:?} active {} at {:?}", sim.t, sim.folk[w].down, sim.folk[w].away, m.hunger.level, m.hunger.stomach, m.blood, m.max_blood, worst, f.active, sim.folk[w].mover.position_units().map(|v| v as i32));
        }
        if sim.tick.is_multiple_of(600) {
            let state: Vec<String> = players
                .iter()
                .map(|&i| {
                    let p = &sim.folk[i];
                    let q = p.mover.position_units();
                    format!(
                        "{i}: down {} away {} carried {:?} slave {:?} shackled {} at [{:.0}, {:.0}]",
                        p.down,
                        p.away,
                        p.carried_by,
                        p.slave.as_ref().map(|s| s.faction.clone()),
                        p.shackled,
                        q[0],
                        q[2]
                    )
                })
                .collect();
            let s = state.join("; ");
            if s != last {
                println!("t {:.0}: {s}", sim.t);
                last = s;
            }
        }
    }
    let food: u32 = sim.buildings.iter().filter_map(|b| b.storage.as_ref()).flat_map(|s| s.sections.iter()).flat_map(|s| s.items.iter()).filter(|i| i.item.food.is_some()).map(|i| i.item.quantity).sum();
    println!("food left in storage: {food}");
    for (conv, fx) in &sim.npc_talks {
        println!("conversation: {} says {:?}, replies {:?}, effects {fx:?}", sim.display_name(conv.npc), conv.npc_text, conv.choices.iter().map(|c| c.1.clone()).collect::<Vec<_>>());
    }
    for (who, text) in &sim.said {
        println!("said: {}: {text}", sim.display_name(*who));
    }
    for m in &sim.messages {
        println!("message: {m}");
    }
}
