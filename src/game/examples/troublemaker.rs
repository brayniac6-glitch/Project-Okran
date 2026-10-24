//! A troublemaker in town: the player's character attacks a townsperson unprovoked, and the
//! town's law enforcers (Attacking trouble makers, 34) take it for one (F050280 / F050279):
//! `cargo run --release -p kenshi-game --example troublemaker -- <install> 43 11` (Sho-Battai)
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
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
            sim.said.clear();
        }
    };
    step(&mut sim, 2.0);
    let Some(town) = sim.folk.iter().find(|p| !p.player && sim.guards_town(p.id)).and_then(|p| p.town) else { return };
    let v = (0..sim.folk.len()).find(|&c| {
        let q = &sim.folk[c];
        !q.player && !q.dead && !q.down && q.race.is_none() && q.town == Some(town) && !sim.guards_town(c) && sim.npc_class(c) != 1 && q.inside.is_none()
    });
    let Some(v) = v else {
        println!("no civilian");
        return;
    };
    let m = kenshi_world::navmesh::METRES;
    let at = sim.folk[v].mover.position;
    sim.folk[me].mover.position = [at[0] + 3.0 * m, at[1], at[2]];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    step(&mut sim, 2.0);
    let enforcers = |sim: &Sim| -> Vec<(usize, usize)> { sim.law_state.view.targets.iter().filter(|((_, g), _)| *g == kenshi_game::law::LAW_ENFORCER).map(|((w, _), t)| (*w, *t)).collect() };
    println!("before: enforcer targets {:?}", enforcers(&sim));
    sim.order_attack(&[me], v);
    for k in 0..20 {
        step(&mut sim, 1.0);
        let e = enforcers(&sim);
        let f = &sim.arena.f[me];
        println!("t {:.0}: crime {:?}, fighting {} {:?}, enforcer targets {:?}", sim.t, sim.folk[me].crime.as_ref().map(|c| (c.kind, c.victim)), f.active, f.focus.or(f.combat.target), e);
        if let Some(&(w, _)) = e.first() {
            step(&mut sim, 3.0);
            let p = &sim.folk[w];
            println!("enforcer {w}: current {:?}, queue {:?}", p.ai.current.as_ref().map(|t| (t.task, t.target)), p.ai.queue.iter().map(|t| (t.task, t.target)).collect::<Vec<_>>());
            break;
        }
        let _ = k;
    }
}
