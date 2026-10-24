//! A farm of the player's as Kenshi's (`kenshi_game::production`, FarmBuilding F022906 /
//! F022907, its containers F029392): its operator (Operating machine, as a job) fills its
//! water container from a storage of water while the crop grows ("It's empty.", F022884: only
//! while growing), the crop ripens, and the operator harvests it plant by plant into its output
//! container (F022891's chance for each of a plant's "output per plant").
//! (testing) The plants start near ripe, so the run is short.
//! `KENSHI_BUILD_DEBUG=1 cargo run --release -p kenshi-game --example farm_trip -- <install> X Y ["<farm>"] [seconds]`
use kenshi_char::inventory::Item;
use kenshi_game::base::{BUILDING_OWNER, PlayerBuilding};
use kenshi_game::sim::Sim;
use std::path::Path;

fn held(sim: &Sim, b: usize, section: &str) -> u32 {
    sim.buildings[b].storage.as_ref().and_then(|i| i.section(section).map(|k| i.sections[k].count())).unwrap_or(0)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let farm_name = a.get(3).cloned().unwrap_or_else(|| "Hemp Farm M".to_string());
    let seconds: f32 = a.get(4).map_or(900.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let data = sim.data.clone();
    let op = sim.folk.iter().find(|p| p.player).map(|p| p.id).unwrap();
    let at = sim.folk[op].mover.position_units();
    sim.ai_options.sit_idle = false;
    sim.ai_options.sleep_injured = false;
    sim.ai_options.ditch_items = false;
    sim.rain = 0.0;
    let rec = |name: &str| data.of_type(kenshi_data::ItemType::Building).find(|r| r.name == name).cloned().unwrap_or_else(|| panic!("no {name}"));
    let built = |r: &kenshi_data::GameData, dx: f32, dz: f32| {
        let mut pb = PlayerBuilding::new(r, [at[0] + dx, at[1], at[2] + dz], 0.0);
        let t = pb.total();
        (pb.confirmed, pb.done, pb.progress, pb.used) = (true, true, t as f32, t);
        pb
    };
    let farm_rec = rec(&farm_name);
    let water = data.of_type(kenshi_data::ItemType::Item).find(|i| i.name == "Water").map(|i| i.string_id.clone()).expect("Water");
    let tank_rec = data
        .of_type(kenshi_data::ItemType::Building)
        .filter(|r| kenshi_game::base::BuildingClass::of(&data, r) == kenshi_game::base::BuildingClass::Storage)
        .find(|r| r.refs("limit inventory").iter().map(|(s, _)| s.as_str()).eq([water.as_str()]))
        .cloned()
        .expect("a storage of water");
    sim.buildings.push(built(&farm_rec, 300.0, 0.0));
    sim.buildings.push(built(&tank_rec, -150.0, 0.0));
    let (f, t) = (0, 1);
    sim.setup_inventories();
    for _ in 0..30 {
        sim.buildings[t].storage.as_mut().unwrap().add(Item::from_record(data.get(&water).unwrap(), 1), false);
    }
    // (testing) the plants near ripe
    sim.step();
    for g in sim.buildings[f].plants.iter_mut() {
        *g = 0.98;
    }
    sim.buildings[f].stage = 0.98;
    println!("fertility {:.2}, the crop's fit {:.2}", sim.farm_fertility(f), sim.crop_fit(f));
    println!(
        "{} ({} plants), containers {:?}; {} holds {} Water",
        farm_rec.name,
        sim.buildings[f].plants.len(),
        sim.buildings[f].storage.as_ref().map(|i| i.sections.iter().map(|s| (s.name, s.stack)).collect::<Vec<_>>()),
        tank_rec.name,
        held(&sim, t, "main")
    );
    sim.order_stop(&[op]);
    sim.add_job(&[op], BUILDING_OWNER + f);
    println!("operator's jobs {:?}", sim.folk[op].jobs.iter().map(|j| j.task).collect::<Vec<_>>());
    // the most taken off at once (a crop taken off whole is replanted, F022904: its count 0)
    let (mut fed, mut ripe, mut most) = (0, false, 0);
    while sim.t < seconds {
        sim.step();
        fed = fed.max(held(&sim, f, "in1"));
        ripe |= sim.buildings[f].stage >= 1.0;
        most = most.max(sim.buildings[f].harvested);
        if sim.tick.is_multiple_of(1800) {
            let pb = &sim.buildings[f];
            println!(
                "t {:.0}: queue {:?}; water in {} (level {:.2}), stage {:.3}, harvested {}/{}, output {}",
                sim.t,
                sim.folk[op].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>(),
                held(&sim, f, "in1"),
                pb.taken.first().copied().unwrap_or(0.0),
                pb.stage,
                pb.harvested,
                pb.plants.len(),
                held(&sim, f, "out")
            );
        }
    }
    let pb = &sim.buildings[f];
    println!(
        "check: the operator watered the farm from the storage {} (water in at most {fed}, the tank {}), the crop ripened {ripe}, harvested {} {} (output {})",
        fed > 0 && held(&sim, t, "main") < 30,
        held(&sim, t, "main"),
        most > 0,
        most,
        held(&sim, f, "out")
    );
}
