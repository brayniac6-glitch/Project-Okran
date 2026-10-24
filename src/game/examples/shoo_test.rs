//! Shooing (headless): the player's character stands inside the home of a resident with
//! "Shoo strangers out of my building" (Shooing, 158 / 283); the resident walks up to it
//! (Talking, 161: Task_TalktoPlayer) and raises EV_SHOO_FROM_MY_BUILDING at it:
//! `cargo run --release -p kenshi-game --example shoo_test -- <install> X Y [seconds]`
//! (KENSHI_TALK_DEBUG=1 traces the talk task.)
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(60.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).expect("a player character");
    let step = |sim: &mut Sim, secs: f32| {
        let end = sim.t + secs;
        while sim.t < end {
            sim.step();
            sim.arena.sh.hits.clear();
            for m in sim.messages.drain(..) {
                println!("t {:.0}: {m}", sim.t);
            }
            for (who, text) in sim.said.drain(..) {
                println!("t {:.1}: {who} says \"{text}\"", sim.t);
            }
            for (c, _) in sim.npc_talks.drain(..) {
                println!("t {:.1}: {} talks to {}: \"{}\" choices {:?}", sim.t, c.npc, c.player, c.npc_text, c.choices.iter().map(|x| x.1.clone()).collect::<Vec<_>>());
            }
        }
    };
    step(&mut sim, 1.0);
    let shoos = |sim: &Sim, i: usize| sim.folk[i].ai.jobs.iter().flatten().filter(|j| j.task == 158 || j.task == 283).map(|j| (j.task, j.priority, j.hours)).collect::<Vec<_>>();
    let with: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].player && !shoos(&sim, i).is_empty()).collect();
    println!("{} with Shooing: {:?}", with.len(), with.iter().map(|&i| (i, shoos(&sim, i))).collect::<Vec<_>>());
    // a resident at home whose home is not public now (Building::isPublic)
    step(&mut sim, 10.0);
    let pick = std::env::var("SHOO_WHO").ok().and_then(|v| v.parse::<usize>().ok());
    let resident = pick.or_else(|| {
        with.iter().copied().find(|&i| {
            let p = &sim.folk[i];
            !p.dead && p.home.is_some() && p.inside == p.home && !sim.building_public(p.home.unwrap()) && sim.npc_class(i) != 1 && sim.npc_class(i) != 2
        })
    });
    let Some(r) = resident else {
        println!("no resident with Shooing at home in a private home");
        return;
    };
    let home = sim.folk[r].home.unwrap();
    println!(
        "resident {r} ({}, class {}), home {home} ({}), public {}, locked up {}; has EV 9 dialogue {}; me {me}",
        sim.display_name(r),
        sim.npc_class(r),
        sim.data.get(&sim.placed[home].building).map_or(sim.placed[home].building.clone(), |b| b.name.clone()),
        sim.building_public(home),
        sim.locked_up(home),
        sim.has_event(r, 9)
    );
    // the player put inside the home, a metre or two from the resident
    let at = sim.folk[r].mover.position;
    let m = kenshi_world::navmesh::METRES;
    let reach: f32 = std::env::var("SHOO_REACH").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let mut placed = false;
    'out: for dist in [reach, reach * 0.75, reach * 0.5] {
        // behind it first (out of its view cone), then round it
        let f = if sim.folk[r].facing == [0.0; 3] { [0.0, 0.0, 1.0] } else { sim.folk[r].facing };
        let back = (-f[2]).atan2(-f[0]);
        for k in 0..8 {
            let ang = back + k as f32 * std::f32::consts::FRAC_PI_4;
            let spot = [at[0] + ang.cos() * dist * m, at[1], at[2] + ang.sin() * dist * m];
            sim.folk[me].mover.position = spot;
            sim.arena.f[me].mover.position = spot;
            step(&mut sim, 0.1);
            if sim.folk[me].inside == Some(home) {
                placed = true;
                break 'out;
            }
        }
    }
    let (pa, pb) = (sim.folk[r].mover.position_units(), sim.folk[me].mover.position_units());
    println!("player placed inside the home: {placed}, {:.0} units from the resident", (pa[0] - pb[0]).hypot(pa[2] - pb[2]));
    {
        let p = &sim.folk[r];
        println!(
            "{r}: class {} until {:.2} now {:.2}; jobs {:?}; has-event view {}",
            p.ai.classification,
            p.ai.until,
            sim.hours_gone(),
            p.ai.jobs.iter().enumerate().map(|(c, l)| (c, l.iter().map(|j| (j.task, j.priority, j.hours)).collect::<Vec<_>>())).collect::<Vec<_>>(),
            sim.talk_view.has_event.contains(&(r, 9))
        );
    }
    let start = sim.t;
    while sim.t < start + seconds {
        step(&mut sim, 1.0);
        let p = &sim.folk[r];
        let (a, b) = (p.mover.position_units(), sim.folk[me].mover.position_units());
        {
            println!(
                "t {:.0}: me inside {:?} | {r}: goal {:?} queue {:?} dist {:.0} talk {:?} shoo target {:?} strangers {} delivered {}",
                sim.t,
                sim.folk[me].inside,
                p.ai.current.as_ref().map(|c| c.task),
                p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                (a[0] - b[0]).hypot(a[2] - b[2]),
                sim.talk_state.running.get(&r).map(|t| t.event),
                sim.talk_view.shoo.get(&r),
                sim.talk_view.shoo.contains_key(&r),
                sim.dialog_memory.delivered.contains(&(r, 9))
            );
        }
    }
    let mut v: Vec<_> = sim.dropped_steps.iter().collect();
    v.sort();
    println!("dropped steps: {v:?}");
}
