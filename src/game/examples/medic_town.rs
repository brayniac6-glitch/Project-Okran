//! A town's medics (headless): a townsperson cut, then who treats them through the "Medic"
//! goal (58) and "First aid" (25) on the planner, and how their first aid need goes:
//! `KENSHI_FIRST_AID_DEBUG=1 cargo run --release -p kenshi-game --example medic_town -- <install> 24 27`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    for _ in 0..300 {
        sim.step();
    }
    let has = |sim: &Sim, i: usize| sim.folk[i].ai.jobs.iter().flatten().any(|j| j.task == 58);
    let medics: Vec<usize> = (0..sim.folk.len()).filter(|&i| !sim.folk[i].dead && has(&sim, i)).collect();
    // the patient: someone outdoors near a medic, of its squad
    let Some(&m) = medics.iter().find(|&&m| sim.folk[m].inside.is_none()) else {
        println!("no medic outdoors ({} medics)", medics.len());
        return;
    };
    let p = sim.folk.iter().position(|q| q.id != m && !q.dead && q.platoon == sim.folk[m].platoon).unwrap_or(m);
    let parts = sim.arena.f[p].medical.parts.len();
    for k in 0..parts.min(3) {
        sim.arena.f[p].medical.parts[k].damage(kenshi_char::medical::Damage { cut: 25.0, ..Default::default() }, 0.0);
    }
    println!("{} medics; medic {m}, patient {p}, need {:.2}", medics.len(), {
        let n = sim.arena.f[p].medical.aid_need(false);
        n.flesh + n.robotic
    });
    for k in 0..60 {
        for _ in 0..60 {
            sim.step();
        }
        let n = sim.arena.f[p].medical.aid_need(false);
        let treating: Vec<usize> = sim.folk.iter().filter(|q| q.first_aid == Some(p)).map(|q| q.id).collect();
        if k % 5 == 0 || n.flesh + n.robotic <= 0.0 {
            println!("t {:.0}: need {:.2}, treated by {treating:?}, medic {m} current {:?} queue {:?}", sim.t, n.flesh + n.robotic, sim.folk[m].ai.current.as_ref().map(|c| c.task), sim.folk[m].ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>());
        }
        if n.flesh + n.robotic <= 0.0 {
            break;
        }
    }
}
