//! Machines fed by their workers (`kenshi_game::jobs`, `kenshi_game::production`): a machine's
//! inputs are its containers (F029392), filled by those who work it, not drawn from storage.
//! - An operator of a Manual Stone Processor (Operating machine, 87, as a job): its input empty
//!   ("It's empty.", 62), it plans Filling machine (93) at the machine, Try filling machine (94,
//!   finder F038633 -> the item search F038632: the "Storage: Stones" beside it), Collecting
//!   resource (92, Task_EmptyMachine F031048, as many as the input has room for, F031335) at
//!   the storage; then works the machine: Building Material made into its output container.
//! - An automatic Iron Refinery III (no operators: the "Auto-Haul" job, Operating automatic
//!   machine 152 and its follow-up Hauling to 124): the hauler fetches Raw Iron from a
//!   "Storage: Ore" and fills the refinery's input (Task_FillMachine F031179).
//! `--survey` lists the machines' containers (F029392):
//! `KENSHI_JOBS_DEBUG=1 cargo run --release -p kenshi-game --example feed_trip -- <install> X Y [--survey]`
use kenshi_char::inventory::Item;
use kenshi_game::base::{BUILDING_OWNER, PlayerBuilding};
use kenshi_game::sim::Sim;
use std::path::Path;

fn held(sim: &Sim, b: usize, section: &str) -> u32 {
    sim.buildings[b].storage.as_ref().and_then(|i| i.section(section).map(|k| i.sections[k].count())).unwrap_or(0)
}

fn count(sim: &Sim, b: usize, record: &str) -> u32 {
    sim.buildings[b].storage.as_ref().map_or(0, |i| i.sections.iter().flat_map(|s| s.items.iter()).filter(|p| p.item.record == record).map(|p| p.item.quantity).sum())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string(), "player:1".to_string()]);
    let data = sim.data.clone();
    let rec = |name: &str| data.of_type(kenshi_data::ItemType::Building).find(|r| r.name == name).cloned().unwrap_or_else(|| panic!("no {name}"));
    if a.iter().any(|s| s == "--survey") {
        for r in data.of_type(kenshi_data::ItemType::Building) {
            if let Some(inv) = kenshi_game::production::machine_inventory(&data, r) {
                let secs: Vec<String> = inv.sections.iter().map(|s| format!("{} {}x{} {} x{}", s.name, s.w, s.h, s.only.first().and_then(|o| data.get(o)).map_or("?".to_string(), |i| i.name.clone()), s.stack)).collect();
                println!("{}: {secs:?}", r.name);
            }
        }
        return;
    }
    let people: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).take(2).collect();
    let (op, hauler) = (people[0], people[1]);
    let at = sim.folk[op].mover.position_units();
    // (testing) no squads spawned about, no idle habits
    sim.population = None;
    sim.ai_options.sit_idle = false;
    sim.ai_options.sleep_injured = false;
    sim.ai_options.ditch_items = false;
    let built = |r: &kenshi_data::GameData, dx: f32, dz: f32| {
        let mut pb = PlayerBuilding::new(r, [at[0] + dx, at[1], at[2] + dz], 0.0);
        let t = pb.total();
        (pb.confirmed, pb.done, pb.progress, pb.used) = (true, true, t as f32, t);
        pb
    };
    // the stone processor and a storage of stones, either side of the operator
    sim.buildings.push(built(&rec("Manual Stone Processor"), 200.0, 0.0));
    sim.buildings.push(built(&rec("Storage: Stones"), -200.0, 0.0));
    let (m, s) = (0, 1);
    sim.setup_inventories();
    let stone = "1515-gamedata.base";
    let bm = kenshi_game::base::BUILDING_MATERIAL;
    for _ in 0..40 {
        sim.buildings[s].storage.as_mut().unwrap().add(Item::from_record(data.get(stone).unwrap(), 1), false);
    }
    println!("processor containers {:?}; storage holds {} Raw Stone", sim.buildings[m].storage.as_ref().map(|i| i.sections.iter().map(|s| (s.name, s.w, s.h, s.stack)).collect::<Vec<_>>()), count(&sim, s, stone));
    for &id in &people {
        sim.order_stop(&[id]);
    }
    sim.add_job(&[op], BUILDING_OWNER + m);
    println!("operator's jobs {:?}", sim.folk[op].jobs.iter().map(|j| j.task).collect::<Vec<_>>());
    let mut fed_at = None;
    while sim.t < 900.0 {
        sim.step();
        if fed_at.is_none() && held(&sim, m, "in1") > 0 {
            fed_at = Some(sim.t);
        }
        if sim.tick.is_multiple_of(1800) {
            let q = sim.folk[op].mover.position_units();
            println!(
                "t {:.0}: operator at [{:.0}, {:.0}] queue {:?}; input {}, output {}, storage {}",
                sim.t,
                q[0] - at[0],
                q[2] - at[2],
                sim.folk[op].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(),
                held(&sim, m, "in1"),
                held(&sim, m, "out"),
                count(&sim, s, stone)
            );
        }
    }
    let made = count(&sim, m, bm);
    let progress = sim.buildings[m].made.first().copied().unwrap_or(0.0);
    println!(
        "check: the operator fed the processor from the storage {} (first fed at {fed_at:?}; storage {} Raw Stone left, input {}, made {made} Building Material, the next {progress:.2} made) and worked it {}",
        fed_at.is_some() && count(&sim, s, stone) < 40,
        count(&sim, s, stone),
        held(&sim, m, "in1"),
        made > 0 || progress > 0.0
    );
    // the automatic refinery fed by the Auto-Haul job (152, its follow-up 124)
    sim.folk[op].jobs.clear();
    sim.order_stop(&[op]);
    sim.buildings.push(built(&rec("Iron Refinery III"), 0.0, 250.0));
    sim.buildings.push(built(&rec("Storage: Ore"), 0.0, -250.0));
    let (r, o) = (2, 3);
    sim.setup_inventories();
    let iron = data.of_type(kenshi_data::ItemType::Item).find(|i| i.name == "Raw Iron").map(|i| i.string_id.clone()).expect("Raw Iron");
    for _ in 0..30 {
        sim.buildings[o].storage.as_mut().unwrap().add(Item::from_record(data.get(&iron).unwrap(), 1), false);
    }
    sim.add_job(&[hauler], BUILDING_OWNER + r);
    println!("hauler's jobs {:?}; ore storage holds {}", sim.folk[hauler].jobs.iter().map(|j| j.task).collect::<Vec<_>>(), count(&sim, o, &iron));
    let t0 = sim.t;
    while sim.t < t0 + 300.0 {
        sim.step();
        if std::env::var("FEED_DEBUG").is_ok() && sim.tick.is_multiple_of(600) {
            let v = &sim.jobs_state.view;
            let pos = sim.folk[hauler].mover.position_units();
            let t = kenshi_game::sim::Target::Building(r);
            println!(
                "  goals {:?} current {:?}; auto score {} fill source {:?} search {:?} wants {:?} 67 {:?}",
                sim.folk[hauler].ai.goals.iter().map(|g| (g.task, g.target)).collect::<Vec<_>>(),
                sim.folk[hauler].ai.current.as_ref().map(|c| c.task),
                v.auto_score(hauler, pos, &t),
                v.fill_source(hauler, pos, &t),
                v.item_search(hauler, &iron, r, pos),
                v.wants(r),
                v.check(hauler, pos, 67, &t)
            );
        }
        if sim.tick.is_multiple_of(600) {
            let q = sim.folk[hauler].mover.position_units();
            println!(
                "t {:.0}: hauler at [{:.0}, {:.0}] queue {:?}; refinery input {}, ore storage {}",
                sim.t,
                q[0] - at[0],
                q[2] - at[2],
                sim.folk[hauler].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(),
                held(&sim, r, "in1"),
                count(&sim, o, &iron)
            );
        }
    }
    println!("check: the Auto-Haul job fed the refinery from the ore storage {} (input {}, storage {})", held(&sim, r, "in1") > 0 && count(&sim, o, &iron) < 30, held(&sim, r, "in1"), count(&sim, o, &iron));
    // loose Raw Stone the item search finds first (F038632's items lying about, within 1200):
    // the operator picks it up (Collecting resource on it, Task_Pickup F031289) to feed the
    // processor, its input and the storage emptied
    sim.folk[hauler].jobs.clear();
    sim.order_stop(&[hauler]);
    for b in [m, s] {
        if let Some(inv) = sim.buildings[b].storage.as_mut() {
            for sec in &mut inv.sections {
                sec.items.retain(|p| p.item.record != stone);
            }
        }
    }
    sim.buildings[m].taken.iter_mut().for_each(|t| *t = 0.0);
    // (put down by the hauler, at its feet on the navmesh)
    let first = sim.next_ground;
    for _ in 0..2 {
        sim.drop_item(hauler, Item::from_record(data.get(stone).unwrap(), 1));
    }
    let loose = |sim: &Sim| sim.ground.iter().filter(|g| g.id > first && g.item.record == stone).count();
    println!("{} Raw Stone lying loose; the processor's input {}", loose(&sim), held(&sim, m, "in1"));
    sim.add_job(&[op], BUILDING_OWNER + m);
    let t1 = sim.t;
    let mut fed = 0;
    while sim.t < t1 + 200.0 {
        sim.step();
        fed = fed.max(held(&sim, m, "in1"));
        if sim.tick.is_multiple_of(600) {
            println!("t {:.0}: operator queue {:?}; loose {}, input {}", sim.t, sim.folk[op].ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(), loose(&sim), held(&sim, m, "in1"));
        }
    }
    // (one picked up feeds it: the input no longer empty, 62, the other is left until it is)
    println!("check: the operator picked the loose Raw Stone up and fed the processor {} (loose {} of 2, input at most {fed})", loose(&sim) < 2 && fed > 0, loose(&sim));
}
