//! Each walled town's gates in Kenshi's order (Town +0x390, `town::hand_map_order` over the
//! gates' handles in the order they are made), the first being the one its town point is 200
//! units out of (F048824): `cargo run --release -p kenshi-world --example town_gates -- <install> [name]`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::town::{building_town, hand_map_order, towns};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let all = towns(install, &data);
    let player = |_: usize| false;
    // every wall gate of every zone, in zone order then file order: (town, handle, position)
    let mut gates: Vec<(usize, [i32; 5], [f32; 3])> = Vec::new();
    let mut no_handle = 0;
    for zx in 0..64 {
        for zy in 0..64 {
            let Some(zone) = kenshi_world::read_zone(install, zx, zy) else { continue };
            for pl in kenshi_world::placements(&zone) {
                let Some(b) = data.get(&pl.building) else { continue };
                if !b.bools.get("is wall gate").copied().unwrap_or(false) {
                    continue;
                }
                let interior = !b.refs("interior").is_empty();
                let at = [pl.position[0], pl.world_y.unwrap_or(pl.position[1]), pl.position[2]];
                let Some(t) = building_town(&all, at, interior, pl.inside, player) else { continue };
                match pl.handle {
                    Some(h) => gates.push((t, h, at)),
                    None => no_handle += 1,
                }
            }
        }
    }
    println!("{} wall gates in towns, {no_handle} without a handle", gates.len());
    let mut ts: Vec<usize> = gates.iter().map(|g| g.0).collect();
    ts.sort();
    ts.dedup();
    let mut most = 0;
    for t in ts {
        if a.get(1).is_some_and(|n| !all[t].name.contains(n.as_str())) {
            continue;
        }
        let mine: Vec<&(usize, [i32; 5], [f32; 3])> = gates.iter().filter(|g| g.0 == t).collect();
        most = most.max(mine.len());
        let keys: Vec<[i32; 5]> = mine.iter().map(|g| g.1).collect();
        let order = hand_map_order(&keys);
        println!("{} ({}): {} gates; first made {:?}, Kenshi's first {:?}", all[t].name, all[t].id, mine.len(), mine[0].2.map(|v| v as i32), mine[order[0]].2.map(|v| v as i32));
    }
    println!("most gates in a town: {most}");
}
