//! Which navmesh building sections of a zone match its building placements:
//! `cargo run -p kenshi-world --example navbuildings -- <install> X Y`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use kenshi_world::navmesh::{METRES, NavWorld, tile_of};
use kenshi_world::{placements, zone_file};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let zone = fcs::read(&zone_file(install, x, y).expect("zone file")).unwrap();
    let placed = placements(&zone);
    let p0 = placed[0].position.map(|v| v * METRES);
    let (tx, ty) = tile_of(p0);
    let nav = NavWorld::load_around(install, tx, ty, 1).unwrap();
    let mut matched = 0;
    let mut total = 0;
    for s in nav.sections.values() {
        let Some(bp) = s.building_position() else {
            continue;
        };
        if total < 4 {
            println!("info {:?} -> {:?}", s.info, bp);
        }
        total += 1;
        let best = placed
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (p.position[0] - bp[0]).hypot(p.position[2] - bp[2])))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, d)) = best {
            println!(
                "  nearest {} at {d:.0}",
                data.get(&placed[i].building)
                    .map_or("?", |b| b.name.as_str())
            );
        }
        if let Some((i, d)) = best
            && d < 1.0
        {
            matched += 1;
            let name = data
                .get(&placed[i].building)
                .map_or("?", |b| b.name.as_str());
            println!(
                "section {} -> {name} ({d:.2} away, y {} vs {:?})",
                s.uid, bp[1], placed[i].world_y
            );
        }
    }
    println!("{matched} of {total} building sections match a placement");
}
