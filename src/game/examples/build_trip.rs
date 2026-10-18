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
    let data = sim.data.clone();
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).unwrap();
    let at = sim.folk[me].mover.position_units();
    // the storage, its materials in it
    let crate_rec = data.records.values().find(|r| r.name.starts_with("Storage Crate")).expect("storage");
    let mut store = PlayerBuilding::new(crate_rec, [at[0] + 250.0, at[1], at[2]], 0.0);
    store.done = true;
    store.confirmed = true;
    if store.storage.is_none() {
        store.storage = Some(kenshi_char::inventory::Inventory { sections: vec![kenshi_char::inventory::Section::new("main", 10, 10, kenshi_char::inventory::ANY)] });
    }
    let site_rec = data.records.values().find(|r| r.name == a[3]).expect("building");
    let site = PlayerBuilding::new(site_rec, [at[0] - 250.0, at[1], at[2]], 0.0);
    for (m, n) in &site.materials {
        let r = data.get(m).unwrap();
        store.storage.as_mut().unwrap().add(kenshi_char::inventory::Item::from_record(r, *n), false);
        println!("site needs {n} {}", r.name);
    }
    sim.buildings.push(store);
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
