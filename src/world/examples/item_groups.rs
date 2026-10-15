//! Where ITEM_PLACEMENT_GROUPs (items placed to respawn, F037792) are put: zone placements and
//! the records that reference them, with a few examples:
//! `cargo run -p kenshi-world --example item_groups -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::path::Path;

fn main() {
    let install = std::env::args().nth(1).expect("install dir");
    let mut order = Install::new(&install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let groups: Vec<&kenshi_data::GameData> = data.of_type(ItemType::ItemPlacementGroup).collect();
    println!("{} ITEM_PLACEMENT_GROUP records", groups.len());
    for g in groups.iter().take(5) {
        let refs: Vec<String> = ["items", "weapons", "clothing", "containers", "blueprints", "maps"]
            .iter()
            .flat_map(|k| g.refs(k).iter().map(move |(s, v)| format!("{k}:{s}:{}", v[0])))
            .collect();
        println!("  {} {:?} respawn {:?}..{:?} {:?}", g.string_id, g.name, g.ints.get("min respawn time"), g.ints.get("max respawn time"), refs);
    }
    // records referencing them
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    for r in data.records.values() {
        for (cat, list) in &r.references {
            if list.iter().any(|x| data.get(&x.0).is_some_and(|t| t.item_type() == Some(ItemType::ItemPlacementGroup))) {
                *by_kind.entry(format!("{} / {cat}", r.item_type().map_or("?", |t| t.name()))).or_default() += 1;
            }
        }
    }
    println!("referenced from {by_kind:?}");
    // zone placements
    let mut n = 0;
    let mut zones = 0;
    for zx in 0..64 {
        for zy in 0..64 {
            let Some(z) = kenshi_world::read_zone(Path::new(&install), zx, zy) else { continue };
            let k = kenshi_world::placements(&z).iter().filter(|p| data.get(&p.building).is_some_and(|t| t.item_type() == Some(ItemType::ItemPlacementGroup))).count();
            if k > 0 {
                zones += 1;
                n += k;
            }
        }
    }
    println!("{n} placed in {zones} zones");
    // building layouts (interiors.level)
    let layouts = kenshi_world::nodes::Layouts::load(Path::new(&install));
    let mut m = 0;
    let mut eg = Vec::new();
    for ((b, l), insts) in &layouts.0 {
        let k = insts.iter().filter(|i| data.get(&i.target).is_some_and(|t| t.item_type() == Some(ItemType::ItemPlacementGroup))).count();
        if k > 0 {
            m += k;
            if eg.len() < 5 {
                eg.push(format!("{} / {l}: {k}", data.get(b).map_or(b.as_str(), |r| r.name.as_str())));
            }
        }
    }
    println!("{m} in {} layouts, e.g. {eg:?}", layouts.0.len());
}
