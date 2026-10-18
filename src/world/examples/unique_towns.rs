//! Where the unique characters live: each TOWN's residents and bar squads with unique
//! characters, and the town's zone:
//! `cargo run --release -p kenshi-world --example unique_towns -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let towns = kenshi_world::town::towns(install, &c);
    for t in &towns {
        let Some(rec) = c.get(&t.record) else { continue };
        let mut found = Vec::new();
        for cat in ["residents", "bar squads", "roaming squads"] {
            for (sq, _) in rec.refs(cat) {
                let Some(s) = c.get(sq) else { continue };
                for k in ["leader", "squad", "choosefrom list"] {
                    for (ch, _) in s.refs(k) {
                        if let Some(x) = c.get(ch)
                            && x.item_type() == Some(ItemType::Character)
                            && x.bools.get("unique").copied().unwrap_or(false)
                        {
                            found.push(format!("{} ({cat})", x.name));
                        }
                    }
                }
            }
        }
        if !found.is_empty() {
            let z = kenshi_world::zone_of(t.position[0], t.position[2]);
            // the zone state's resident squads in the town (none: Town::setupResidents's to
            // hand out, F048905 / F048907)
            let placed = kenshi_world::read_zone(install, z.0, z.1).map(|f| kenshi_world::placements(&f)).unwrap_or_default();
            let set: usize = placed.iter().filter(|p| t.contains(p.position, 1.0) && (p.squad.is_some() || p.residents.is_some())).count();
            println!("{} zone {}.{}: {} [zone state residents: {set}]", t.name, z.0, z.1, found.join(", "));
        }
    }
}
