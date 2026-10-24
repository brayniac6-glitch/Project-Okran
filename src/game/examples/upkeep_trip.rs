//! The player's wall and gate: the gate's door, a wall damaged and repaired, then dismantled:
//! `cargo run --release -p kenshi-game --example upkeep_trip -- <install> X Y`
use kenshi_data::ItemType;
use kenshi_game::base::{BUILDING_MATERIAL, PlayerBuilding};
use kenshi_game::sim::Sim;

fn count(sim: &Sim, id: usize, record: &str) -> u32 {
    sim.folk[id].inventory.sections.iter().flat_map(|s| &s.items).filter(|i| i.item.record == record).map(|i| i.item.quantity).sum()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(std::path::Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).unwrap();
    let at = sim.folk[me].mover.position_units();
    let rec = |name: &str| sim.data.of_type(ItemType::Building).find(|r| r.name == name).cloned().unwrap();
    let (wall, gate) = (rec("Concrete Wall"), rec("Concrete Gate"));
    for (r, dx) in [(&wall, 200.0), (&gate, 400.0)] {
        let mut pb = PlayerBuilding::new(r, [at[0] + dx, at[1], at[2]], 0.0);
        let t = pb.total();
        (pb.confirmed, pb.done, pb.progress, pb.used) = (true, true, t as f32, t);
        // its reach as build mode gives it (half its length, about)
        pb.radius = r.floats.get("link length").copied().unwrap_or(30.0) * 0.5;
        sim.buildings.push(pb);
    }
    sim.step();
    let doors: Vec<usize> = (0..sim.door_states.len()).filter(|&d| sim.door_states[d].player_building == Some(1)).collect();
    println!("wall {} ({} materials), gate {}: {} door(s), health {:?}, a gate {:?}", wall.name, sim.buildings[0].total(), gate.name, doors.len(), doors.iter().map(|&d| sim.door_states[d].health).collect::<Vec<_>>(), doors.iter().map(|&d| sim.is_gate(d)).collect::<Vec<_>>());
    // the wall hit hard
    let mut hits = 0;
    while sim.buildings[0].progress > sim.buildings[0].total() as f32 * 0.5 {
        sim.damage_building(0, 60.0);
        hits += 1;
    }
    println!("wall at half after {hits} heavy blows: progress {:.2}, needs repair {}", sim.buildings[0].progress, sim.needs_repair(0));
    // repaired with building materials
    let mat = sim.data.get(BUILDING_MATERIAL).cloned().unwrap();
    for sec in &mut sim.folk[me].inventory.sections {
        sec.items.clear();
    }
    for _ in 0..20 {
        let ok = sim.folk[me].inventory.add(kenshi_char::inventory::Item::from_record(&mat, 1), false);
        if !ok {
            println!("no room for building material");
            break;
        }
    }
    let before = count(&sim, me, BUILDING_MATERIAL);
    sim.order_repair(&[me], 0);
    let t0 = sim.t;
    while sim.needs_repair(0) && sim.t - t0 < 600.0 {
        sim.step();
        if (sim.t - t0) as i32 % 60 == 0 && sim.tick % 60 == 0 {
            let p = &sim.folk[me];
            let b = &sim.buildings[0];
            println!("  t {:.0}: build {:?}, {:.0} from it (radius {:.0}), progress {:.2}, missing {:?}", sim.t - t0, p.build, (p.mover.position_units()[0] - b.at[0]).hypot(p.mover.position_units()[2] - b.at[2]), b.radius, b.progress, sim.missing_material);
        }
    }
    println!("repaired in {:.0} s, progress {:.2}/{}, materials {} -> {}", sim.t - t0, sim.buildings[0].progress, sim.buildings[0].total(), before, count(&sim, me, BUILDING_MATERIAL));
    // dismantled
    let before = count(&sim, me, BUILDING_MATERIAL);
    sim.order_dismantle(&[me], 0);
    let t0 = sim.t;
    let n = sim.buildings.len();
    while sim.buildings.len() == n && sim.t - t0 < 600.0 {
        sim.step();
        if (sim.t - t0) as i32 % 60 == 0 && sim.tick % 60 == 0 {
            let p = &sim.folk[me];
            let b = &sim.buildings[0];
            println!("  t {:.0}: dismantle {:?}, {:.0} from it, progress {:.2}", sim.t - t0, p.dismantle, (p.mover.position_units()[0] - b.at[0]).hypot(p.mover.position_units()[2] - b.at[2]), b.progress);
        }
    }
    println!(
        "dismantled in {:.0} s: {} buildings left, materials {} -> {}, gate's door now of building {:?}",
        sim.t - t0,
        sim.buildings.len(),
        before,
        count(&sim, me, BUILDING_MATERIAL),
        doors.first().map(|&d| sim.door_states[d].player_building)
    );
}
