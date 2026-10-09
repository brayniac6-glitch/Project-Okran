//! A zone's placed buildings: name, position, destroyed flag, whether it has a "destroyed mesh":
//! `cargo run -p kenshi-world --example zone_buildings -- <install> X Y [filter]`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let filter = a.get(3).map(|s| s.to_ascii_lowercase());
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let Some(zone) = kenshi_world::read_zone(install, x, y) else { return };
    for p in kenshi_world::placements(&zone) {
        let Some(b) = data.get(&p.building) else {
            println!("? {} at {:.0?}", p.building, p.position);
            continue;
        };
        if filter.as_ref().is_some_and(|f| !b.name.to_ascii_lowercase().contains(f)) {
            continue;
        }
        let ruin = b
            .refs("parts")
            .iter()
            .filter_map(|(s, _)| data.get(s))
            .any(|r| r.files.get("destroyed mesh").is_some_and(|m| !m.is_empty()));
        println!(
            "{:40} {} at {:.0?} destroyed {} has-ruin-mesh {}",
            b.name, p.building, p.position, p.destroyed, ruin
        );
    }
}
