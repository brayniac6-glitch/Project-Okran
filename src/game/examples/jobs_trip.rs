//! The player's jobs on the AI planner (`kenshi_game::jobs`): a squad of three at a small base (a
//! storage crate with the materials, a building site, a research bench, a training dummy):
//! - A: Engineer (fetches the materials from the storage, puts them in, builds the site);
//! - B: Operating machine on the bench, then Engineer (the first job wins: it works the bench);
//! - C: Training on the dummy, with JOBS off at first (it stays idle), then on.
//! Then B's jobs are swapped (Engineer first: it joins the building), and A's JOBS turned off.
//! `KENSHI_JOBS_DEBUG=1 cargo run --release -p kenshi-game --example jobs_trip -- <install> X Y [seconds]`
use kenshi_game::base::{BUILDING_OWNER, PlayerBuilding};
use kenshi_game::orders::Job;
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(900.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:3".to_string()]);
    let data = sim.data.clone();
    let squad: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let (ea, eb, ec) = (squad[0], squad[1], squad[2]);
    let at = sim.folk[ea].mover.position_units();
    let rec = |name: &str| data.records.values().find(|r| r.name == name).unwrap_or_else(|| panic!("no {name}"));
    let built = |r: &kenshi_data::GameData, dx: f32, dz: f32| {
        let mut pb = PlayerBuilding::new(r, [at[0] + dx, at[1], at[2] + dz], 0.0);
        let t = pb.total();
        (pb.confirmed, pb.done, pb.progress, pb.used) = (true, true, t as f32, t);
        pb
    };
    // the storage, the materials for the site in it
    let crate_rec = data.records.values().find(|r| r.name.starts_with("Storage Crate")).expect("storage");
    let mut store = built(crate_rec, 250.0, 0.0);
    if store.storage.is_none() {
        store.storage = Some(kenshi_char::inventory::Inventory { sections: vec![kenshi_char::inventory::Section::new("main", 10, 10, kenshi_char::inventory::ANY)] });
    }
    let mut site = PlayerBuilding::new(rec("Research Bench II"), [at[0] - 250.0, at[1], at[2]], 0.0);
    site.confirmed = true;
    for (m, n) in &site.materials {
        store.storage.as_mut().unwrap().add(kenshi_char::inventory::Item::from_record(data.get(m).unwrap(), *n), false);
        println!("site needs {n} {}", data.get(m).unwrap().name);
    }
    sim.buildings.push(store);
    sim.buildings.push(site);
    sim.buildings.push(built(rec("Small Research Bench"), 0.0, 250.0));
    sim.buildings.push(built(rec("Training Dummy MkI"), 0.0, -250.0));
    let (site, bench, dummy) = (1, 2, 3);
    println!("bench operated {}, dummy trains {:?}", sim.is_operated(bench), sim.training(dummy));
    // the jobs, as a shift order gives them
    let job = |sim: &Sim, b: usize| {
        let target = BUILDING_OWNER + b;
        let (task, _) = sim.order_task_of(target).and_then(kenshi_game::orders::job_form).expect("a job");
        Job { task, target, label: sim.job_label(task, target) }
    };
    // (testing) no squads spawned about: none to fight the squad
    sim.population = None;
    // (testing) the AI options' idle habits off (JOBS_TRIP_IDLE keeps them)
    if std::env::var("JOBS_TRIP_IDLE").is_err() {
        sim.ai_options.sit_idle = false;
        sim.ai_options.sleep_injured = false;
        sim.ai_options.ditch_items = false;
    }
    let (engineer, operate, train) = (job(&sim, site), job(&sim, bench), job(&sim, dummy));
    println!("jobs: {} / {} / {}", engineer.label, operate.label, train.label);
    sim.folk[ea].jobs = vec![engineer.clone()];
    sim.folk[eb].jobs = vec![operate.clone(), engineer.clone()];
    sim.folk[ec].jobs = vec![train.clone()];
    sim.folk[ea].jobs_on = true;
    sim.folk[eb].jobs_on = true;
    sim.folk[ec].jobs_on = false;
    let skill = |sim: &Sim, id: usize| sim.training(dummy).map_or(0.0, |(s, _)| sim.arena.f[id].skills.level(s));
    let c0 = skill(&sim, ec);
    let show = |sim: &Sim, what: &str| {
        for &i in &squad {
            let p = &sim.folk[i];
            let q = p.mover.position_units();
            println!(
                "{what} t {:.0}: {i} at [{:.0}, {:.0}] goal {:?} queue {:?} operate {:?} down {} fighting {}",
                sim.t,
                q[0] - at[0],
                q[2] - at[2],
                p.ai.current.as_ref().map(|c| c.task),
                p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                p.operate,
                p.down,
                sim.arena.f[i].active
            );
        }
        if std::env::var("JOBS_TRIP_AI").is_ok() {
            for &i in &squad {
                let ai = &sim.folk[i].ai;
                println!("  {i}: orders {:?} goals {:?} jobs {:?} classified {:?} key {} class {} until {:.2}", ai.orders.iter().map(|t| (t.task, t.target)).collect::<Vec<_>>(), ai.goals.iter().map(|t| (t.task, t.target)).collect::<Vec<_>>(), ai.jobs.iter().map(|l| l.iter().map(|t| t.task).collect::<Vec<_>>()).collect::<Vec<_>>(), ai.classified.iter().map(|l| l.iter().map(|t| t.task).collect::<Vec<_>>()).collect::<Vec<_>>(), ai.key, ai.classification, ai.until);
            }
        }
        let s = &sim.buildings[site];
        println!("  site: used {} of {}, progress {:.2}, done {}", s.used, s.total(), s.progress, s.done);
    };
    let run = |sim: &mut Sim, until: f32| {
        while sim.t < until && !sim.buildings[site].done {
            sim.step();
        }
    };
    run(&mut sim, 60.0);
    show(&sim, "1");
    let b_works = sim.folk[eb].operate == Some(bench);
    let c_idle = sim.folk[ec].ai.current.as_ref().is_none_or(|c| c.task != kenshi_game::jobs::TRAINING) && sim.folk[ec].operate.is_none();
    println!("check: B works the bench (its first job) {b_works}; C not training with JOBS off {c_idle}");
    // C's JOBS on; B's jobs swapped
    sim.toggle_jobs(&[ec]);
    sim.folk[eb].jobs.swap(0, 1);
    let until = sim.t + 30.0;
    run(&mut sim, until);
    show(&sim, "2");
    let b_builds = sim.folk[eb].ai.current.as_ref().is_some_and(|c| c.task == kenshi_game::jobs::ENGINEER);
    println!("check: B on Engineer after the swap {b_builds}; C on Training {}", sim.folk[ec].ai.current.as_ref().is_some_and(|c| c.task == kenshi_game::jobs::TRAINING));
    run(&mut sim, seconds);
    show(&sim, "3");
    println!("check: built {} at t {:.0}; C trains {}, its skill {:.2} -> {:.2}", sim.buildings[site].done, sim.t, sim.folk[ec].operate == Some(dummy), c0, skill(&sim, ec));
    // A given the Training job with JOBS off: it stays idle; C's JOBS off: it lets the dummy go;
    // A's on: A takes it up
    sim.folk[ea].jobs = vec![train.clone()];
    sim.toggle_jobs(&[ea]);
    let step_for = |sim: &mut Sim, secs: f32| {
        let until = sim.t + secs;
        while sim.t < until {
            sim.step();
        }
    };
    step_for(&mut sim, 20.0);
    show(&sim, "4");
    let a_idle = sim.folk[ea].operate.is_none() && sim.folk[ea].ai.current.as_ref().is_none_or(|c| c.task != kenshi_game::jobs::TRAINING);
    sim.toggle_jobs(&[ec]);
    step_for(&mut sim, 5.0);
    let c_off = sim.folk[ec].operate.is_none();
    sim.toggle_jobs(&[ea]);
    step_for(&mut sim, 40.0);
    show(&sim, "5");
    println!("check: A idle with JOBS off {a_idle}; C lets go with JOBS off {c_off}; A trains with JOBS on {}", sim.folk[ea].operate == Some(dummy));
    // the hold stance: Attacking enemies added by classification (F036811), taken away with it
    sim.toggle_stance(&[eb], kenshi_game::orders::StanceOrder::Hold);
    step_for(&mut sim, 2.0);
    let held = sim.folk[eb].ai.classified[3].iter().any(|g| g.task == 16);
    sim.toggle_stance(&[eb], kenshi_game::orders::StanceOrder::Hold);
    step_for(&mut sim, 2.0);
    println!("check: hold gives Attacking enemies {held}, taken away {}", !sim.folk[eb].ai.has_classified(16));
    // the built site gone: the buildings after it move down, the jobs' targets with them
    sim.remove_building(site);
    step_for(&mut sim, 10.0);
    let b_jobs: Vec<(u16, usize)> = sim.folk[eb].jobs.iter().map(|j| (j.task, j.target - BUILDING_OWNER)).collect();
    println!("check: B's jobs {b_jobs:?} (Engineer kept, the bench's now {}); B works it {}", bench - 1, sim.folk[eb].operate == Some(bench - 1));
    // the Medic order: C, with a first aid kit, given the Medic job (58) treats A, cut
    if let Some(kit) = data.records.values().find(|r| r.name == "Basic First Aid Kit") {
        sim.folk[ec].inventory.add(kenshi_char::inventory::Item::from_record(kit, 1), false);
        for q in sim.arena.f[ea].medical.parts.iter_mut().take(2) {
            q.flesh = q.max_health() * 0.4;
            q.update_frac();
        }
        sim.order_medic(&[ec]);
        let mut treated = false;
        let until = sim.t + 60.0;
        while sim.t < until && !treated {
            sim.step();
            treated = sim.folk[ec].first_aid == Some(ea);
        }
        println!("check: the Medic job {:?}, C treats A {treated}", sim.folk[ec].jobs.iter().map(|j| j.task).collect::<Vec<_>>());
    }
}
