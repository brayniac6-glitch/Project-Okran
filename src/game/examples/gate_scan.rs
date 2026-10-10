//! Every zone's gate-like doors (a door record named "...Gate..." or sliding over 100 units)
//! and their saved state: `cargo run --release -p kenshi-game --example gate_scan -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let mut states = std::collections::BTreeMap::new();
    let mut shown = 0;
    for zx in 0..64 {
        for zy in 0..64 {
            let Some(zone) = kenshi_world::read_zone(install, zx, zy) else { continue };
            for p in kenshi_world::placements(&zone) {
                let Some(b) = data.get(&p.building) else { continue };
                for (n, (sid, _)) in b.refs("doors").iter().enumerate() {
                    let Some(d) = data.get(sid) else { continue };
                    let dist = d.floats.get("door move dist").copied().unwrap_or(30.0);
                    if !(d.name.to_lowercase().contains("gate") || dist > 100.0) {
                        continue;
                    }
                    let saved = p.door_saved.get(n).copied().flatten();
                    let key = saved.map_or("none".to_string(), |s| format!("state {} broken {}", s.state, s.broken));
                    *states.entry(key.clone()).or_insert(0) += 1;
                    if std::env::var("GATE_ZONE").is_ok_and(|z| z == format!("{zx}.{zy}")) {
                        println!("  at {:?} rot {:?}", p.position, p.rotation);
                    }
                    if shown < 25 {
                        shown += 1;
                        println!("zone {zx}.{zy} {} door {} saved {key} squad {:?} residents {:?}", b.name, d.name, p.squad, p.residents);
                    }
                }
            }
        }
    }
    println!("gates by saved state: {states:?}");
}
