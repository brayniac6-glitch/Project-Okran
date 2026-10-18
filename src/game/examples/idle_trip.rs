//! The AI options' idle habits as built-in goals (`kenshi_game::jobs`, F036915), with JOBS on:
//! at home (a building of the player's about), one with nothing to do sits (Sitting around),
//! then given a job leaves the seat for it; one hurt but bandaged goes to bed (Going to bed ->
//! Sleeping) and, healed, gets up once the goal's hours are out (Getting out of bed):
//! `KENSHI_JOBS_DEBUG=1 cargo run --release -p kenshi-game --example idle_trip -- <install> X Y`
use kenshi_game::base::{BUILDING_OWNER, PlayerBuilding};
use kenshi_game::orders::Job;
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:2".to_string()]);
    let data = sim.data.clone();
    let squad: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let (sitter, hurt) = (squad[0], squad[1]);
    let at = sim.folk[sitter].mover.position_units();
    // a training dummy of the player's: home
    let rec = data.records.values().find(|r| r.name == "Training Dummy MkI").unwrap();
    let mut pb = PlayerBuilding::new(rec, [at[0], at[1], at[2] - 200.0], 0.0);
    let t = pb.total();
    (pb.confirmed, pb.done, pb.progress, pb.used) = (true, true, t as f32, t);
    sim.buildings.push(pb);
    let dummy = sim.buildings.len() - 1;
    // (testing) no squads spawned about: none to fight the squad
    sim.population = None;
    sim.order_stop(&squad);
    sim.toggle_jobs(&squad);
    // hurt, bandaged: every part at half, its treatment the rest
    for q in sim.arena.f[hurt].medical.parts.iter_mut() {
        let most = q.max_health();
        q.flesh = most * 0.5;
        q.treatment = most * 0.5;
        q.update_frac();
    }
    println!("hour {:.0} s", sim.hour_secs);
    let show = |sim: &Sim, what: &str| {
        for &i in &squad {
            let p = &sim.folk[i];
            println!(
                "{what} t {:.0}: {i} goal {:?} queue {:?} operating {:?} bed {:?}",
                sim.t,
                p.ai.current.as_ref().map(|c| c.task),
                p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                p.operating,
                p.bed
            );
        }
    };
    let run = |sim: &mut Sim, secs: f32| {
        let until = sim.t + secs;
        while sim.t < until {
            sim.step();
        }
    };
    run(&mut sim, 60.0);
    show(&sim, "1");
    let sits = sim.folk[sitter].ai.current.as_ref().is_some_and(|c| c.task == kenshi_game::jobs::SITTING_AROUND) && sim.folk[sitter].operating.is_some();
    let in_bed = sim.folk[hurt].bed.is_some();
    println!("check: one sits {sits}; the hurt one in bed {in_bed}");
    // the sitter given the dummy's Training job: it gets up for it
    let target = BUILDING_OWNER + dummy;
    let (task, _) = sim.order_task_of(target).and_then(kenshi_game::orders::job_form).unwrap();
    let label = sim.job_label(task, target);
    sim.folk[sitter].jobs = vec![Job { task, target, label }];
    // healed
    for q in sim.arena.f[hurt].medical.parts.iter_mut() {
        q.flesh = q.max_health();
        q.treatment = 0.0;
        q.update_frac();
    }
    run(&mut sim, 60.0);
    show(&sim, "2");
    println!("check: the sitter trains {}", sim.folk[sitter].operate == Some(dummy));
    let secs = (sim.hour_secs * 8.5) as f32;
    run(&mut sim, secs);
    show(&sim, "3");
    println!("check: the healed one up {}", sim.folk[hurt].bed.is_none());
}
