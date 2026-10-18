//! Crime and law (`kenshi_game::law`): a player character stealing in front of a town's
//! guards, then caged and picking its way out (an escaped prisoner until out of town), then
//! enslaved and escaping its cage; prints what the law makes of it:
//! `KENSHI_LAW_DEBUG=1 cargo run --release -p kenshi-game --example law_test -- <install> X Y [skill]`
use kenshi_ai::CrimeEnum;
use kenshi_game::sim::Sim;
use std::path::Path;

fn run(sim: &mut Sim, seconds: f32, what: &str) {
    let end = sim.t + seconds;
    while sim.t < end {
        sim.step();
        // (as the game's damage floaters take them each frame)
        sim.arena.sh.hits.clear();
        for m in sim.messages.drain(..) {
            println!("[{what}] t {:.0}: {m}", sim.t);
        }
        for (who, text) in sim.said.drain(..) {
            println!("[{what}] t {:.0}: {who} says \"{text}\"", sim.t);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let skill: f32 = a.get(3).map_or(80.0, |n| n.parse().unwrap());
    let mut sim = Sim::load(install, zx, zy, 0, &["player:1".to_string()]);
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).expect("a player character");
    run(&mut sim, 2.0, "load");
    let guards: Vec<usize> = (0..sim.folk.len()).filter(|&g| !sim.folk[g].player && sim.guards_town(g)).collect();
    println!("{} guards of the town: {:?}", guards.len(), guards.iter().map(|&g| (g, sim.npc_class(g), sim.folk[g].faction.clone())).take(8).collect::<Vec<_>>());
    let Some(&g) = guards.first() else { return };
    let faction = sim.folk[g].faction.clone().unwrap();
    println!("faction {faction}: offers bounties {}, anti slavery {}", sim.offers_bounties(&faction), sim.anti_slavery(&faction));
    let mut skills = sim.arena.f[me].skills.clone();
    skills.levels.insert(kenshi_char::StatsEnumerated::StatLockpicking, skill);
    sim.arena.f[me].skills = skills;
    // A: stealing in front of a guard
    let at = sim.folk[g].mover.position;
    let f = if sim.folk[g].facing == [0.0; 3] { [0.0, 0.0, 1.0] } else { sim.folk[g].facing };
    let m = kenshi_world::navmesh::METRES;
    sim.folk[me].mover.position = [at[0] + f[0] * 10.0 * m, at[1], at[2] + f[2] * 10.0 * m];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    run(&mut sim, 1.0, "A");
    sim.commit_crime(me, CrimeEnum::CrimeStealing, Some(&faction));
    println!("[A] crime {:?}", sim.folk[me].crime.as_ref().map(|c| (c.kind, c.left)));
    run(&mut sim, 4.0, "A");
    println!("[A] bounty with {faction}: {}, crime {:?}, blown {:.0}", sim.bounty_with(me, &faction), sim.folk[me].crime.as_ref().map(|c| (c.kind, c.caught)), sim.folk[me].disguise_blown);
    // B: caged, picking the cage
    sim.folk[me].crime = None;
    sim.folk[me].bounties.clear();
    let here = sim.folk[me].mover.position_units();
    let Some(cage) = sim.free_cage(&faction, here, true) else {
        println!("no cage");
        return;
    };
    sim.imprison(me, cage, &faction, false);
    sim.order_escape(me);
    for _ in 0..120 {
        run(&mut sim, 1.0, "B");
        if sim.folk[me].prison.is_none() {
            break;
        }
    }
    println!("[B] prison {:?}, escaped {:?}, crime {:?}, town {:?}", sim.folk[me].prison.as_ref().map(|p| p.cage), sim.folk[me].escaped, sim.folk[me].crime.as_ref().map(|c| (c.kind, c.faction.clone())), sim.folk[me].town);
    run(&mut sim, 3.0, "B");
    // out of town: no longer an escaped prisoner
    let far = sim.folk[me].mover.position;
    sim.folk[me].mover.position = [far[0] + 6000.0 * m, far[1], far[2]];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    run(&mut sim, 2.0, "B");
    println!("[B] away: town {:?}, escaped {:?}", sim.folk[me].town, sim.folk[me].escaped);
    // C: a slave of the faction, caged and shackled, picking its way out
    sim.folk[me].mover.position = far;
    sim.arena.f[me].mover.position = far;
    run(&mut sim, 1.0, "C");
    let owner = sim.folk[g].platoon;
    // one of the owners close by processes the new slave
    let near = owner.and_then(|o| sim.folk.iter().find(|q| q.platoon == Some(o) && !q.dead && !q.down && q.slave.is_none()).map(|q| q.id));
    if let Some(k) = near {
        let at = sim.folk[me].mover.position;
        sim.folk[k].mover.position = [at[0] + 2.0 * m, at[1], at[2]];
        sim.arena.f[k].mover.position = sim.folk[k].mover.position;
    }
    sim.enslave(me, owner, &faction);
    println!("[C] state {}, prison {:?}, shackled {}", sim.slave_state(me), sim.folk[me].prison.as_ref().map(|p| p.cage), sim.folk[me].shackled);
    // the processor's clip (Task_ProcessNewSlave: "medic" for 3 s)
    let takers: Vec<usize> = sim.law_state.processing.iter().map(|e| e.taker).collect();
    println!("[C] processing by {takers:?}: clip {:?}", takers.first().and_then(|&t| sim.law_task_clip(t)));
    run(&mut sim, 3.5, "C");
    println!("[C] processed: clip {:?}", takers.first().and_then(|&t| sim.law_task_clip(t)));
    sim.order_escape(me);
    for _ in 0..240 {
        run(&mut sim, 1.0, "C");
        if sim.folk[me].prison.is_none() {
            break;
        }
    }
    println!("[C] state {}, prison {:?}, shackled {}, escaped {:?}, crime {:?}", sim.slave_state(me), sim.folk[me].prison.as_ref().map(|p| p.cage), sim.folk[me].shackled, sim.folk[me].escaped, sim.folk[me].crime.as_ref().map(|c| (c.kind, c.faction.clone())));
    run(&mut sim, 5.0, "C");
    println!("[C] after: state {}, blown {:.0}, bounty {}", sim.slave_state(me), sim.folk[me].disguise_blown, sim.bounty_with(me, &faction));
    // D: a fight with a guard (no longer a slave, no bounty)
    sim.folk[me].slave = None;
    sim.folk[me].bounties.clear();
    sim.folk[me].crime = None;
    let g2 = guards.iter().copied().find(|&k| !sim.folk[k].down && !sim.arena.f[k].active).unwrap_or(g);
    let at = sim.folk[g2].mover.position;
    sim.folk[me].mover.position = [at[0] + 5.0 * m, at[1], at[2]];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    let mut skills = sim.arena.f[me].skills.clone();
    for k in [kenshi_char::StatsEnumerated::StatMeleeAttack, kenshi_char::StatsEnumerated::StatStrength, kenshi_char::StatsEnumerated::StatDexterity] {
        skills.levels.insert(k, 90.0);
    }
    sim.arena.f[me].skills = skills;
    sim.order_attack(&[me], g2);
    for k in 0..60 {
        run(&mut sim, 1.0, "D");
        if k % 10 == 0 {
            let (a, b) = (sim.folk[me].mover.position_units(), sim.folk[g2].mover.position_units());
            println!("[D] t {:.0}: apart {:.0}, me {:?} {:?}, guard {:?}", sim.t, (a[0] - b[0]).hypot(a[2] - b[2]), sim.arena.f[me].combat.state, sim.folk[me].ai.current.as_ref().map(|t| t.task), sim.arena.f[g2].combat.state);
        }
        if sim.bounty_with(me, &faction) > 0 {
            break;
        }
    }
    // (the arena's hits are not always landing in a short run) a melee hit on the guard as the
    // arena reports it, and a guard's hit on a free character
    if sim.bounty_with(me, &faction) == 0 {
        let n = sim.arena.sh.hits.len();
        sim.arena.sh.hits.push((me, g2, 0, Default::default()));
        sim.law_state.melee_hits = n..n + 1;
        sim.law_tick(0.0);
        println!("[D] after a hit on the guard: bounty {}, crime {:?}", sim.bounty_with(me, &faction), sim.folk[me].crime.as_ref().map(|c| (c.kind, c.left)));
    }
    println!("[D] me active {} focus {:?}, guard {g2} active {} down {}", sim.arena.f[me].active, sim.arena.f[me].focus, sim.arena.f[g2].active, sim.folk[g2].down);
    println!("[D] bounty {}, crime {:?}, down {}", sim.bounty_with(me, &faction), sim.folk[me].crime.as_ref().map(|c| (c.kind, c.left)), sim.folk[me].down);
    // E: a sentence served
    sim.folk[me].crime = None;
    sim.folk[me].bounties.clear();
    let here = sim.folk[me].mover.position_units();
    if let Some(cage) = sim.free_cage(&faction, here, true) {
        sim.imprison(me, cage, &faction, false);
        run(&mut sim, 1.0, "E");
        if let Some(pr) = sim.folk[me].prison.as_mut() {
            pr.until = 0.0;
        }
        run(&mut sim, 1.0, "E");
        println!("[E] prison {:?}, lately released {}", sim.folk[me].prison.is_some(), sim.lately_released(me));
    }
    // F: an alarm raised about a thief (DA_CRIME_ALARM): who investigates it
    sim.folk[me].prison = None;
    sim.folk[me].bounties.clear();
    let at = sim.folk[g].mover.position;
    sim.folk[me].mover.position = [at[0] + 30.0 * m, at[1], at[2]];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    run(&mut sim, 1.0, "F");
    sim.commit_crime(me, CrimeEnum::CrimeStealing, Some(&faction));
    let raised = sim.crime_alarm(g, me);
    println!("[F] alarm raised {raised}: {:?}", sim.law_state.alarms.values().map(|v| v.len()).collect::<Vec<_>>());
    for k in 0..20 {
        run(&mut sim, 1.0, "F");
        let inv: Vec<usize> = (0..sim.folk.len()).filter(|&i| sim.folk[i].ai.queue.first().is_some_and(|s| s.task == 209 || s.task == 210)).collect();
        if k % 4 == 0 {
            println!("[F] t {:.0}: investigating {:?}, alarms {:?}, near {}", sim.t, inv, sim.law_state.alarms.values().map(|v| v.iter().map(|a| (a.crime, a.left as i32)).collect::<Vec<_>>()).collect::<Vec<_>>(), sim.law_state.alarm_near.iter().filter(|&&b| b).count());
        }
    }
    // G: a wanted one down near the guards: arrested on the planner (Bounty hunting 204:
    // stripped, lifted, put in a cage)
    sim.folk[me].prison = None;
    sim.folk[me].crime = None;
    sim.add_bounty_amount(me, &faction, 3000);
    sim.folk[me].disguise_blown = 250.0;
    let at = sim.folk[g].mover.position;
    sim.folk[me].mover.position = [at[0] + 20.0 * m, at[1], at[2]];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    sim.knock_out(me);
    sim.arena.f[me].medical.knockout_time = 200.0;
    for k in 0..90 {
        run(&mut sim, 1.0, "G");
        if sim.folk[me].prison.is_some() {
            println!("[G] t {:.0}: caged {:?}", sim.t, sim.folk[me].prison.as_ref().map(|p| p.cage));
            break;
        }
        if k % 10 == 0 {
            println!("[G] t {:.0}: carried by {:?}, down {}, weapons {}", sim.t, sim.folk[me].carried_by, sim.folk[me].down, sim.folk[me].inventory.equipped().filter(|i| i.kind == Some(kenshi_data::ItemType::Weapon)).count());
            if let Some(c) = sim.folk[me].carried_by {
                let p = &sim.folk[c];
                println!("[G]   carrier {c}: current {:?}, queue {:?}, orders {:?}, cage {:?}", p.ai.current.as_ref().map(|t| (t.task, t.target)), p.ai.queue.iter().map(|t| (t.task, t.target)).collect::<Vec<_>>(), p.ai.orders.iter().map(|t| t.task).collect::<Vec<_>>(), sim.law_state.view.cages.get(&c));
            }
        }
    }
    // H: kidnapping a townsperson knocked out (Task_Kidnapping::start as it is lifted: the
    // crime, seen by the town's people about; the victim, out cold, notices nothing)
    sim.folk[me].prison = None;
    sim.folk[me].bounties.clear();
    sim.folk[me].crime = None;
    sim.folk[me].carried_by = None;
    sim.folk[me].disguise_blown = 0.0;
    sim.arena.f[me].medical.knockout_time = 0.0;
    // (healed and up again whatever G's fight left)
    let fresh: Vec<f32> = sim.arena.f[me].medical.parts.iter().map(|p| p.max_health()).collect();
    for (k, part) in sim.arena.f[me].medical.parts.iter_mut().enumerate() {
        part.flesh = fresh[k].max(part.flesh);
    }
    sim.wake(me);
    // (no one still after it from G)
    for k in 0..sim.folk.len() {
        if sim.folk[k].arrest.as_ref().is_some_and(|a| a.target == me) {
            sim.folk[k].arrest = None;
        }
        if sim.folk[k].carrying == Some(me) {
            sim.folk[k].carrying = None;
        }
        if sim.folk[k].pickup == Some(me) {
            sim.folk[k].pickup = None;
        }
        sim.folk[k].ai.queue.retain(|s| s.target != kenshi_game::sim::Target::Person(me));
    }
    run(&mut sim, 5.0, "H");
    let civ = (0..sim.folk.len()).find(|&c| !sim.folk[c].player && !sim.folk[c].dead && !sim.folk[c].down && sim.folk[c].faction.as_deref() == Some(faction.as_str()) && !sim.guards_town(c) && sim.folk[c].race.is_none());
    if let Some(v) = civ {
        let at = sim.folk[v].mover.position;
        sim.folk[me].mover.position = [at[0] + 3.0 * m, at[1], at[2]];
        sim.arena.f[me].mover.position = sim.folk[me].mover.position;
        sim.knock_out(v);
        sim.arena.f[v].medical.knockout_time = 200.0;
        run(&mut sim, 1.0, "H");
        println!("[H] kidnap {v}: is kidnap {}, down {}; me down {} carrying {:?} prison {}", sim.is_kidnap(me, v), sim.folk[v].down, sim.folk[me].down, sim.folk[me].carrying, sim.folk[me].prison.is_some());
        sim.order_kidnap(&[me], v);
        println!("[H] pending {:?}", sim.law_state.kidnaps);
        for _ in 0..20 {
            run(&mut sim, 1.0, "H");
            if sim.folk[me].carrying == Some(v) {
                break;
            }
        }
        println!("[H] carrying {:?}, pending {:?}, crime {:?}", sim.folk[me].carrying, sim.law_state.kidnaps, sim.folk[me].crime.as_ref().map(|c| (c.kind, c.faction.clone(), c.victim)));
        run(&mut sim, 5.0, "H");
        println!("[H] bounty {}, alarms {:?}", sim.bounty_with(me, &faction), sim.law_state.alarms.values().map(|v| v.iter().map(|a| a.crime).collect::<Vec<_>>()).collect::<Vec<_>>());
    }
}
