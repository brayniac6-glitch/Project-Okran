//! The record types a zone file places (by the placed records' types), with an example name each:
//! `cargo run --release -p kenshi-world --example zone_types -- <install> X Y`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let z = kenshi_world::read_zone(install, x, y).expect("zone");
    let mut by: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    for p in kenshi_world::placements(&z) {
        let (t, n) = data.get(&p.building).map_or(("?".to_string(), p.building.clone()), |r| (r.item_type().map_or("?", |t| t.name()).to_string(), r.name.clone()));
        let e = by.entry(t).or_insert((0, n));
        e.0 += 1;
    }
    for (t, (n, ex)) in by {
        println!("{t:20} {n:5}  e.g. {ex}");
    }
}
