//! Placements of one zone file and whether their building resolves in the merged core data:
//! `cargo run -p kenshi-world --example placed -- <install> <zone file> [n]`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let f = fcs::read(std::path::Path::new(&a[1])).unwrap();
    let n: usize = a.get(2).map_or(12, |x| x.parse().unwrap());
    let ps = kenshi_world::placements(&f);
    let found = ps
        .iter()
        .filter(|p| data.get(&p.building).is_some())
        .count();
    println!("{} placements, {found} resolve", ps.len());
    for p in ps.iter().take(n) {
        let b = data.get(&p.building);
        println!(
            "{:<28} {:<26} {:?} pos {:?} world_y {:?}",
            p.building,
            b.map_or("?", |b| b.name.as_str()),
            b.and_then(|b| b.item_type()),
            p.position,
            p.world_y
        );
    }
}
