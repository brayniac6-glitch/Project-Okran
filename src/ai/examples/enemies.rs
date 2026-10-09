//! The factions a zone's resident squads belong to and which factions each counts as an enemy:
//! `cargo run -p kenshi-ai --example enemies -- <install> X Y`
use kenshi_ai::faction::FactionRelations;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{fcs, mods::Install};
use kenshi_world::{placements, zone_file};
use std::collections::BTreeSet;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (x, y): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let zone = fcs::read(&zone_file(install, x, y).expect("zone file")).unwrap();
    let mut factions = BTreeSet::new();
    for p in placements(&zone) {
        if let Some(t) = p.squad.as_deref().and_then(|s| data.get(s))
            && let Some((f, _)) = t.refs("faction").first()
        {
            factions.insert(f.clone());
        }
    }
    let all: Vec<_> = data.of_type(ItemType::Faction).collect();
    for f in factions {
        let Some(r) = data.get(&f) else { continue };
        let rel = FactionRelations::from_data(r, &data);
        let enemies: Vec<&str> = all
            .iter()
            .filter(|o| rel.is_enemy(Some(&o.string_id)))
            .map(|o| o.name.as_str())
            .collect();
        println!(
            "{} ({f}): enemies {:?}",
            r.name,
            &enemies[..enemies.len().min(25)]
        );
    }
}
