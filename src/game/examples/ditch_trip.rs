//! Ditch items (`kenshi_game::routines`): an idle character carrying building materials beside a
//! "Storage: Building Materials" takes them there:
//! `KENSHI_BUILD_DEBUG=1 cargo run --release -p kenshi-game --example ditch_trip -- <install> X Y`
use kenshi_game::base::PlayerBuilding;
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    let data = sim.data.clone();
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).unwrap();
    let at = sim.folk[me].mover.position_units();
    let store_rec = data.records.values().find(|r| r.name == "Storage: Building Materials").expect("storage");
    let mut store = PlayerBuilding::new(store_rec, [at[0] + 300.0, at[1], at[2]], 0.0);
    store.done = true;
    store.confirmed = true;
    if store.storage.is_none() {
        store.storage = Some(kenshi_char::inventory::Inventory { sections: vec![kenshi_char::inventory::Section::new("main", 8, 8, kenshi_char::inventory::ANY)] });
    }
    sim.buildings.push(store);
    let mat = data.get(kenshi_game::base::BUILDING_MATERIAL).unwrap();
    sim.folk[me].inventory.add(kenshi_char::inventory::Item::from_record(mat, 5), false);
    sim.ai_options.ditch_items = true;
    let count = |sim: &Sim| -> (u32, u32) {
        let c = |inv: &kenshi_char::inventory::Inventory| inv.sections.iter().flat_map(|s| s.items.iter()).filter(|p| p.item.record == kenshi_game::base::BUILDING_MATERIAL).map(|p| p.item.quantity).sum::<u32>();
        (c(&sim.folk[me].inventory), sim.buildings.last().and_then(|b| b.storage.as_ref()).map_or(0, c))
    };
    println!("start: carried, stored {:?}", count(&sim));
    while sim.t < 120.0 {
        sim.step();
    }
    println!("after 120 s: carried, stored {:?}", count(&sim));
}
