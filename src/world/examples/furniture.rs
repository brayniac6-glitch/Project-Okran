//! The furniture a zone's buildings place, by function and building:
//! `cargo run -p kenshi-world --example furniture -- <install> X Y`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use kenshi_world::{furniture::furniture, nodes::Layouts, placements, zone_file};
use std::collections::BTreeMap;
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
    let f = furniture(&data, &Layouts::load(install), &placed);
    let mut by: BTreeMap<(String, i32), Vec<String>> = BTreeMap::new();
    for i in &f {
        let b = data
            .get(&placed[i.placement].building)
            .map_or("?".into(), |b| b.name.clone());
        by.entry((b, i.function)).or_default().push(i.name.clone());
    }
    println!("{} pieces", f.len());
    for ((b, func), v) in by {
        let mut n = v.clone();
        n.sort();
        n.dedup();
        println!(
            "{b:28} function {func:2}: {} ({:?})",
            v.len(),
            &n[..n.len().min(4)]
        );
    }
}
