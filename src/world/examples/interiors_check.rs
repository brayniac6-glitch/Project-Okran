//! A zone's interior volumes and doors, and which building some points are inside:
//! `cargo run -p kenshi-world --example interiors_check -- <install> X Y [x,y,z ...]`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install, resources::Resources};
use kenshi_world::interior::{building_at, doors, interiors};
use kenshi_world::{placements, zone_file};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let res = Resources::index(install);
    let zone = fcs::read(&zone_file(install, x, y).expect("zone file")).unwrap();
    let placed = placements(&zone);
    let name = |i: usize| {
        data.get(&placed[i].building)
            .map_or("?".into(), |b| b.name.clone())
    };
    let ins = interiors(&data, &res, &placed);
    println!("{} interior volumes", ins.len());
    for i in &ins {
        println!("  {} ({} triangles)", name(i.placement), i.triangles.len());
    }
    let ds = doors(&data, &res, &placed);
    for d in &ds {
        println!(
            "door {} of {} at {:?}",
            d.index,
            name(d.placement),
            d.position.map(|v| v.round())
        );
    }
    for p in a.iter().skip(3) {
        let v: Vec<f32> = p.split(',').map(|s| s.parse().unwrap()).collect();
        println!(
            "{p}: inside {:?}",
            building_at(&ins, [v[0], v[1], v[2]]).map(name)
        );
    }
}
