//! Hauling on the AI planner (`kenshi_game::jobs`): a full machine of the player's emptied into a
//! storage that keeps its output, by its operator (Operating machine 87 -> Collecting resource
//! 92 -> Try emptying machine 113 -> Filling machine 93 at the storage), then by an Auto-Haul
//! worker (Operating automatic machine 152, its output taken to the storage whenever there is
//! any). `--survey` lists the machines and the storages that keep each one's output:
//! `KENSHI_JOBS_DEBUG=1 cargo run --release -p kenshi-game --example haul_trip -- <install> X Y ["<machine>"] [--survey]`
use kenshi_char::inventory::{Inventory, Item};
use kenshi_game::base::PlayerBuilding;
use kenshi_game::production::function_of;
use kenshi_game::sim::Sim;
use std::path::Path;

fn count(inv: Option<&Inventory>, record: &str) -> u32 {
    inv.map_or(0, |i| i.sections.iter().flat_map(|s| s.items.iter()).filter(|p| p.item.record == record).map(|p| p.item.quantity).sum())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let survey = a.iter().any(|s| s == "--survey");
    let machine_name = a.get(3).filter(|s| !s.starts_with("--")).cloned().unwrap_or_else(|| "Well".to_string());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string(), "player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let data = sim.data.clone();
    let buildings: Vec<&kenshi_data::GameData> = data.of_type(kenshi_data::ItemType::Building).collect();
    let keeps = |r: &kenshi_data::GameData| r.refs("limit inventory").iter().map(|(s, _)| s.clone()).collect::<Vec<_>>();
    if survey {
        for r in &buildings {
            let Some((f, function)) = function_of(&data, r) else { continue };
            let produces: Vec<String> = f.refs("produces").iter().map(|(s, _)| data.get(s).map_or(s.clone(), |i| i.name.clone())).collect();
            if produces.is_empty() || !sim.is_production_record(&r.string_id) {
                continue;
            }
            let consumes: Vec<String> = f.refs("consumes").iter().map(|(s, _)| data.get(s).map_or(s.clone(), |i| i.name.clone())).collect();
            let stores: Vec<String> = buildings
                .iter()
                .filter(|s| f.refs("produces").iter().any(|(p, _)| keeps(s).contains(p)))
                .map(|s| s.name.clone())
                .collect();
            println!(
                "{} (function {function}, max operators {:?}/{:?}, storage {}x{}): produces {produces:?} consumes {consumes:?}; kept by {stores:?}",
                r.name,
                r.ints.get("max operators"),
                f.ints.get("max operators"),
                r.ints.get("storage size width").unwrap_or(&0),
                r.ints.get("storage size height").unwrap_or(&0)
            );
        }
        return;
    }
    let mut people: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    people.truncate(2);
    let (op, hauler) = (people[0], people[1]);
    let at = sim.folk[op].mover.position_units();
    // the machine, beside the squad, built
    let m_rec = buildings.iter().find(|r| r.name == machine_name).copied().expect("machine");
    let (f, _) = function_of(&data, m_rec).expect("functionality");
    let product = f.refs("produces").first().map(|(s, _)| s.clone()).expect("a product");
    let p_name = data.get(&product).map_or(product.clone(), |r| r.name.clone());
    // (built buildings: their work done, as building leaves them)
    let finish = |pb: &mut PlayerBuilding| {
        let t = pb.total();
        (pb.confirmed, pb.done, pb.progress, pb.used) = (true, true, t as f32, t);
    };
    let mut machine = PlayerBuilding::new(m_rec, [at[0] + 200.0, at[1], at[2]], 0.0);
    finish(&mut machine);
    // the storage keeping the product, the other side
    let s_rec = buildings.iter().find(|s| keeps(s).contains(&product)).copied().expect("a storage keeping the product");
    let mut store = PlayerBuilding::new(s_rec, [at[0] - 200.0, at[1], at[2]], 0.0);
    finish(&mut store);
    // the machine filled: its output container (F029392) up to its limit, as production
    // leaves it
    let item = data.get(&product).unwrap();
    sim.buildings.push(machine);
    let m = sim.buildings.len() - 1;
    sim.setup_inventories();
    {
        let inv = sim.buildings[m].storage.as_mut().expect("machine storage");
        while inv.add(Item::from_record(item, 1), false) {}
    }
    sim.buildings[m].full = true;
    let start = count(sim.buildings[m].storage.as_ref(), &product);
    sim.buildings.push(store);
    let s = sim.buildings.len() - 1;
    println!("{} full of {start} {p_name}; storage {} beside it", m_rec.name, s_rec.name);
    for &id in &people {
        sim.order_stop(&[id]);
    }
    // the operator: Operating machine as a job, JOBS on
    sim.folk[op].jobs.push(kenshi_game::orders::Job { task: kenshi_game::jobs::OPERATING_MACHINE, target: kenshi_game::base::BUILDING_OWNER + m, label: "Operate".into() });
    sim.toggle_jobs(&[op]);
    let status = |sim: &Sim| (count(sim.buildings[m].storage.as_ref(), &product), count(sim.buildings[s].storage.as_ref(), &product), sim.buildings[m].full);
    let mut emptied_at = None;
    let mut trips = 0;
    let mut last = 0;
    while sim.t < 600.0 {
        sim.step();
        if sim.tick.is_multiple_of(600) {
            let q = sim.folk[op].mover.position_units();
            println!("t {:.0}: operator at [{:.0}, {:.0}] queue {:?}; machine, storage, full {:?}", sim.t, q[0] - at[0], q[2] - at[2], sim.folk[op].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(), status(&sim));
        }
        let now = status(&sim).1;
        if now > last {
            trips += 1;
            last = now;
            emptied_at.get_or_insert(sim.t);
        }
    }
    let (left, stored, _) = status(&sim);
    println!(
        "check: the operator emptied the full machine into the storage {} ({trips} trips, the first at {emptied_at:?}; machine {start} -> {left}, storage {stored}; back at work {})",
        trips >= 2,
        // (on the way back counts: its plan leads back to Operating machine)
        sim.folk[op].ai.queue.iter().any(|s| s.task == kenshi_game::jobs::OPERATING_MACHINE)
    );
    // Auto-Haul: the second one ordered to keep an automatic machine of the same product (no
    // operators: F044603's menu gives Operating automatic machine) emptied; the operator's job
    // taken away
    sim.folk[op].jobs.clear();
    sim.order_stop(&[op]);
    let auto_rec = buildings
        .iter()
        .find(|r| r.name != machine_name && r.ints.get("max operators") == Some(&0) && function_of(&data, r).is_some_and(|(f, _)| f.refs("produces").first().is_some_and(|(p, _)| *p == product)))
        .copied()
        .expect("an automatic machine of the product");
    let mut auto = PlayerBuilding::new(auto_rec, [at[0], at[1], at[2] + 200.0], 0.0);
    finish(&mut auto);
    sim.buildings.push(auto);
    let m = sim.buildings.len() - 1;
    sim.setup_inventories();
    {
        let inv = sim.buildings[m].storage.as_mut().expect("machine storage");
        while inv.add(Item::from_record(item, 1), false) {}
    }
    sim.buildings[m].full = true;
    let status = |sim: &Sim| (count(sim.buildings[m].storage.as_ref(), &product), count(sim.buildings[s].storage.as_ref(), &product), sim.buildings[m].full);
    let before = status(&sim);
    println!("{} full of {} {p_name}", auto_rec.name, before.0);
    // the Auto-Haul job (F044603's order for a machine of no operators, 152, and its job's
    // follow-up Hauling to, 124; F036821), JOBS on: what it collects once the machine is full
    // (Method 21, F038636) put away by the ditch habit (F036915) once 152 has nothing more
    sim.add_job(&[hauler], kenshi_game::base::BUILDING_OWNER + m);
    println!("hauler's jobs {:?}", sim.folk[hauler].jobs.iter().map(|j| j.task).collect::<Vec<_>>());
    let t0 = sim.t;
    while sim.t < t0 + 240.0 {
        sim.step();
        if sim.tick.is_multiple_of(600) {
            let q = sim.folk[hauler].mover.position_units();
            println!("t {:.0}: hauler at [{:.0}, {:.0}] queue {:?}; machine, storage, full {:?}", sim.t, q[0] - at[0], q[2] - at[2], sim.folk[hauler].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(), status(&sim));
        }
    }
    let after = status(&sim);
    println!("check: Auto-Haul took the output to the storage {} (machine {} -> {}, storage {} -> {}, full {})", after.1 > before.1 && !after.2, before.0, after.0, before.1, after.1, after.2);
}
