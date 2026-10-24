//! A builder's round trips (`kenshi_game::base`, `kenshi_game::jobs`): a storage of building
//! materials and a building site either side of the squad; the first character ordered to build
//! plans fetching the materials from the storage, adding them and building:
//! `KENSHI_BUILD_DEBUG=1 cargo run --release -p kenshi-game --example build_trip -- <install> X Y "<building>" [seconds]`
use kenshi_game::base::PlayerBuilding;
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(4).map_or(600.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no homeless squads: one spawned nearby (since the 304-town list moved the spawn
    // points) knocks the builder out partway; and the zone's own people away: since main's
    // b4d1dc4 a squad of the zone's bar wanders over and knocks the builder out at t 1175
    sim.population = None;
    for p in sim.folk.iter_mut().filter(|p| !p.player) {
        p.away = true;
    }
    let data = sim.data.clone();
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).unwrap();
    let at = sim.folk[me].mover.position_units();
    // the storage keeping the site's material (a StorageBuilding, F029410: the builders' item
    // search, F038632, finds only those and the machines' outputs), its materials in it
    let site_rec = data.records.values().find(|r| r.name == a[3]).expect("building");
    let site = PlayerBuilding::new(site_rec, [at[0] - 250.0, at[1], at[2]], 0.0);
    let material = site.materials.first().map(|m| m.0.clone()).unwrap_or_else(|| kenshi_game::base::BUILDING_MATERIAL.to_string());
    let crate_rec = data
        .of_type(kenshi_data::ItemType::Building)
        .filter(|r| kenshi_game::base::BuildingClass::of(&data, r) == kenshi_game::base::BuildingClass::Storage)
        .find(|r| r.refs("limit inventory").iter().map(|(s, _)| s.as_str()).eq([material.as_str()]))
        .expect("a storage keeping the material");
    let mut store = PlayerBuilding::new(crate_rec, [at[0] + 250.0, at[1], at[2]], 0.0);
    store.done = true;
    store.confirmed = true;
    sim.buildings.push(store);
    sim.setup_inventories();
    for (m, n) in &site.materials {
        let r = data.get(m).unwrap();
        for _ in 0..*n {
            sim.buildings[0].storage.as_mut().unwrap().add(kenshi_char::inventory::Item::from_record(r, 1), false);
        }
        println!("site needs {n} {} (in {})", r.name, crate_rec.name);
    }
    let mut site = site;
    site.confirmed = true;
    sim.buildings.push(site);
    let b = sim.buildings.len() - 1;
    sim.order_build(&[me], b);
    while sim.t < seconds && !sim.buildings[b].done {
        sim.step();
        if sim.tick.is_multiple_of(600) {
            let q = sim.folk[me].mover.position_units();
            println!("t {:.0}: builder at [{:.0}, {:.0}], used {} progress {:.2}, plan {:?}", sim.t, q[0] - at[0], q[2] - at[2], sim.buildings[b].used, sim.buildings[b].progress, sim.folk[me].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>());
        }
    }
    println!("built: {} at t {:.0}", sim.buildings[b].done, sim.t);
    let q = sim.folk[me].mover.position_units();
    println!("outposts {:?}, the buildings' {:?}; the builder in one: {}", sim.outposts, sim.buildings.iter().map(|b| b.outpost).collect::<Vec<_>>(), sim.in_outpost(q));
}
