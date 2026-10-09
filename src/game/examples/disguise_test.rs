//! Disguises (`kenshi_game::disguise`): a player character put in a uniformed NPC's outfit,
//! beside that NPC's faction, for a while; prints the messages and the disguise's state:
//! `cargo run --release -p kenshi-game --example disguise_test -- <install> X Y [seconds] [stealth]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |n| n.parse().unwrap());
    let stealth: f32 = a.get(4).map_or(0.0, |n| n.parse().unwrap());
    let mut sim = Sim::load(install, zx, zy, 0, &["player:1".to_string()]);
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).expect("a player character");
    let Some(npc) = sim.folk.iter().find(|p| !p.player && p.inventory.equipped().filter(|i| i.uniform.is_some()).count() >= 2).map(|p| p.id) else {
        println!("no uniformed NPC in zone {zx}.{zy}");
        return;
    };
    let faction = sim.folk[npc].faction.clone().unwrap_or_default();
    println!("uniform of {} ({}): {:?}", sim.display_name(npc), faction, sim.folk[npc].inventory.equipped().filter(|i| i.uniform.is_some()).map(|i| i.name.clone()).collect::<Vec<_>>());
    // the outfit: the NPC's worn armour, on the player's character
    let outfit: Vec<_> = sim.folk[npc].inventory.equipped().filter(|i| i.kind == Some(kenshi_data::ItemType::Armour)).cloned().collect();
    sim.folk[me].inventory = kenshi_char::inventory::Inventory::character();
    for it in outfit {
        let _ = sim.folk[me].inventory.add(it, true);
    }
    sim.equip(me);
    let mut skills = sim.arena.f[me].skills.clone();
    skills.levels.insert(kenshi_char::StatsEnumerated::StatStealth, stealth);
    sim.arena.f[me].skills = skills;
    // beside the NPC
    // in front of it, 30 units off
    let at = sim.folk[npc].mover.position;
    let f = if sim.folk[npc].facing == [0.0; 3] { [0.0, 0.0, 1.0] } else { sim.folk[npc].facing };
    let m = kenshi_world::navmesh::METRES;
    sim.folk[me].mover.position = [at[0] + f[0] * 20.0 * m, at[1], at[2] + f[2] * 20.0 * m];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    println!("uniform_of: {:?}", sim.uniform_of(me));
    let mut last = None;
    while sim.t < seconds {
        sim.step();
        // (the test) the NPC watching the player
        let (r, q) = (sim.folk[npc].mover.position_units(), sim.folk[me].mover.position_units());
        let (dx, dz) = (q[0] - r[0], q[2] - r[2]);
        let l = dx.hypot(dz).max(1e-3);
        sim.folk[npc].facing = [dx / l, 0.0, dz / l];
        for m in sim.messages.drain(..) {
            println!("t {:.0}: {m}", sim.t);
        }
        if sim.tick % 1200 == 0 {
            let at = sim.folk[me].mover.position_units();
            let near: Vec<(usize, i32, bool, bool)> = (0..sim.folk.len())
                .filter(|&w| sim.folk[w].faction.as_deref() == Some(faction.as_str()) && w != me)
                .map(|w| {
                    let r = sim.folk[w].mover.position_units();
                    (w, (r[0] - at[0]).hypot(r[2] - at[2]) as i32, sim.can_see(w, me), sim.folk[w].down)
                })
                .filter(|x| x.1 < 600)
                .collect();
            let r = sim.folk[npc].mover.position_units();
            println!("t {:.0}: me {me} at {:?} npc {npc} ({:?}) at {:?} sees {} inside {:?}/{:?} facing {:?}; near {:?}", sim.t, at.map(|v| v as i32), sim.folk[npc].faction, r.map(|v| v as i32), sim.can_see(npc, me), sim.folk[npc].inside, sim.folk[me].inside, sim.folk[npc].facing, near);
        }
        let now = (sim.folk[me].disguised.clone(), sim.folk[me].disguise_blown > 0.0);
        if Some(&now) != last.as_ref() {
            println!("t {:.0}: disguised {:?}, blown {}", sim.t, now.0, now.1);
            last = Some(now);
        }
    }
}
