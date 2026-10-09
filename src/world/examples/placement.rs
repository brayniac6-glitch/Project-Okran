//! A zone's placements of one building with their state records' fields:
//! `cargo run -p kenshi-world --example placement -- <install> X Y <building name>`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use kenshi_world::zone_file;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let zone = fcs::read(&zone_file(install, x, y).expect("zone file")).unwrap();
    for r in &zone.records {
        for i in &r.instances {
            if data.get(&i.target).is_none_or(|b| b.name != a[3]) {
                continue;
            }
            println!("{} at {:?}", a[3], i.position);
            for s in &i.states {
                if let Some(st) = zone.records.iter().find(|r| r.string_id == *s) {
                    println!(
                        "  state kind {} {:?}\n    ints {:?}\n    floats {:?}\n    bools {:?}",
                        st.kind, st.strings, st.ints, st.floats, st.bools
                    );
                }
            }
        }
    }
}
