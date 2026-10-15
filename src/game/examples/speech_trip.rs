//! The lines said aloud and the conversations NPCs start (`kenshi_game::speech`) as the
//! player's squad walks up to people in a zone, headless:
//! `cargo run --release -p kenshi-game --example speech_trip -- <install> X Y [seconds] [extras]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |s| s.parse().unwrap());
    let mut extras: Vec<String> = a.iter().skip(4).cloned().collect();
    if !extras.iter().any(|e| e.starts_with("player:")) {
        extras.push("player:3".into());
    }
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    let players: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let mut next_hop = 2.0;
    // SPEECH_FREE=1: the first of the player's picks the lock of the first caged NPC's cage
    // (with none caged, a townsman put in the first cage)
    if std::env::var("SPEECH_FREE").is_ok() && !sim.folk.iter().any(|p| !p.player && p.prison.is_some()) {
        let cage = (0..sim.furn.len()).find(|&k| sim.furn[k].function == kenshi_world::furniture::BF_CAGE);
        let n = sim.folk.iter().find(|p| !p.player && !p.dead && p.race.is_none() && p.faction.is_some()).map(|p| p.id);
        if let (Some(k), Some(n)) = (cage, n) {
            let f = sim.folk[n].faction.clone().unwrap();
            sim.imprison(n, k, &f, false);
            println!("{} ({n}) caged", sim.display_name(n));
        }
    }
    if std::env::var("SPEECH_FREE").is_ok()
        && let Some(c) = sim.folk.iter().find(|p| !p.player && p.prison.is_some() && !p.dead).map(|p| p.id)
    {
        let at = sim.folk[c].mover.position;
        sim.folk[players[0]].mover.position = [at[0] + 3.0, at[1], at[2] + 3.0];
        sim.arena.f[players[0]].mover.position = sim.folk[players[0]].mover.position;
        if std::env::var("SPEECH_FREE").is_ok_and(|v| v == "now") {
            // (the lock picked at once: what crate::law does as it opens)
            sim.folk[c].prison = None;
            sim.remember(c, players[0], kenshi_game::dialogue::LT_FREED_ME);
        } else {
            sim.order_free(players[0], c);
        }
        println!("{} frees {} ({c})", sim.display_name(players[0]), sim.display_name(c));
        next_hop = f32::INFINITY;
    }
    // SPEECH_FIGHT=<name>: the squad attacks the first NPC of that name
    if let Ok(name) = std::env::var("SPEECH_FIGHT")
        && let Some(n) = sim.folk.iter().find(|p| !p.player && !p.dead && sim.display_name(p.id) == name).map(|p| p.id)
    {
        let at = sim.folk[n].mover.position;
        for (j, &p) in players.iter().enumerate() {
            sim.folk[p].mover.position = [at[0] + 4.0 + j as f32 * 1.5, at[1], at[2] + 3.0];
            sim.arena.f[p].mover.position = sim.folk[p].mover.position;
        }
        sim.order_attack(&players, n);
        println!("the squad attacks {name} ({n})");
        next_hop = f32::INFINITY;
    }
    // SPEECH_FIRSTAID=1: an NPC beaten down, the first of the player's (given a medkit) treats it
    if std::env::var("SPEECH_FIRSTAID").is_ok() {
        let data = sim.data.clone();
        let kit = data.records.values().find(|r| r.ints.get("item function") == Some(&1)).expect("a medkit");
        let n = sim.folk.iter().find(|p| !p.player && !p.dead && p.race.is_none() && p.prison.is_none()).map(|p| p.id).unwrap();
        for part in sim.arena.f[n].medical.parts.iter_mut() {
            part.flesh -= part.max_health() * 0.5;
        }
        sim.folk[players[0]].inventory.add(kenshi_char::inventory::Item::from_record(kit, 1), false);
        let at = sim.folk[n].mover.position;
        sim.folk[players[0]].mover.position = [at[0] + 2.0, at[1], at[2] + 2.0];
        sim.arena.f[players[0]].mover.position = sim.folk[players[0]].mover.position;
        sim.order_first_aid(&players[..1], n);
        println!("{} treats {} ({n})", sim.display_name(players[0]), sim.display_name(n));
        next_hop = f32::INFINITY;
    }
    let mut k = 0usize;
    while sim.t < seconds {
        sim.step();
        // every 8 s the squad stands beside the next NPC (not down, not the player's)
        if sim.t >= next_hop {
            next_hop += 8.0;
            let npcs: Vec<usize> = sim.folk.iter().filter(|p| !p.player && !p.dead && !p.down && !p.away && p.race.is_none()).map(|p| p.id).collect();
            if !npcs.is_empty() {
                let n = npcs[k % npcs.len()];
                k += 7;
                let at = sim.folk[n].mover.position;
                for (j, &p) in players.iter().enumerate() {
                    sim.folk[p].mover.position = [at[0] + 4.0 + j as f32 * 1.5, at[1], at[2] + 3.0];
                    sim.arena.f[p].mover.position = sim.folk[p].mover.position;
                    sim.folk[p].inside = sim.folk[n].inside;
                }
                println!("t {:.0}: beside {} ({})", sim.t, sim.display_name(n), n);
            }
        }
        for (who, text) in std::mem::take(&mut sim.said) {
            println!("t {:.0}: {} says: {text}", sim.t, sim.display_name(who));
        }
        for (conv, fx) in std::mem::take(&mut sim.npc_talks) {
            println!("t {:.0}: {} starts a conversation: {} {:?} {fx:?}", sim.t, sim.display_name(conv.npc), conv.npc_text, conv.choices.iter().map(|c| c.1.clone()).collect::<Vec<_>>());
        }
    }
    // SPEECH_WANTED=<character>: the first NPC made again as that CHARACTER (its bounty rolled)
    if let Ok(name) = std::env::var("SPEECH_WANTED")
        && let Some(c) = sim.data.records.values().find(|r| r.name == name && r.item_type() == Some(kenshi_data::ItemType::Character)).map(|r| r.string_id.clone())
        && let Some(n) = sim.folk.iter().find(|p| !p.player).map(|p| p.id)
    {
        sim.folk[n].character = Some(c);
        sim.roll_bounty(n);
    }
    // the wanted about (CHARACTER "bounty chance", rolled as they are made)
    let chances = sim.folk.iter().filter(|p| p.character.as_deref().and_then(|c| sim.data.get(c)).is_some_and(|c| c.ints.get("bounty chance").copied().unwrap_or(0) > 0)).count();
    println!("wanted: {chances} people with a bounty chance");
    for p in sim.folk.iter().filter(|p| p.bounties.iter().any(|b| b.amount > 0)) {
        println!("wanted: {} ({}) {:?}", sim.display_name(p.id), p.id, p.bounties.iter().map(|b| (sim.data.get(&b.faction).map_or(b.faction.clone(), |f| f.name.clone()), b.amount)).collect::<Vec<_>>());
    }
    for m in &sim.messages {
        println!("message: {m}");
    }
}
