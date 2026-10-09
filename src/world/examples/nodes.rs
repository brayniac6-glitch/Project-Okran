//! The AI nodes buildings place in a zone: `cargo run -p kenshi-world --example nodes -- <install> X Y`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use kenshi_world::{
    nodes::{Layouts, nodes},
    placements, zone_file,
};
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
    let layouts = Layouts::load(install);
    println!(
        "{} layouts; {} placements name one",
        layouts.0.len(),
        placed
            .iter()
            .filter(|p| p.interior.is_some() || p.exterior.is_some())
            .count()
    );
    let mut n = nodes(&data, &layouts, &placed);
    // nodes placed by the zone file itself
    let direct: Vec<_> = placed
        .iter()
        .filter(|p| {
            data.get(&p.building)
                .is_some_and(|b| b.bools.get("is node").copied().unwrap_or(false))
        })
        .collect();
    println!("{} node records placed directly", direct.len());
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for r in &zone.records {
        for i in &r.instances {
            let t = data.get(&i.target).map_or(format!("? {}", i.target), |t| {
                format!("{:?} {}", t.item_type(), t.name)
            });
            *kinds.entry(t).or_default() += 1;
        }
    }
    let mut rk: BTreeMap<String, usize> = BTreeMap::new();
    for r in &zone.records {
        let base = r
            .strings
            .iter()
            .find(|(k, _)| k == "base")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let t = data.get(&base).map_or(String::new(), |b| {
            format!("{} node={:?}", b.name, b.bools.get("is node"))
        });
        *rk.entry(format!("kind {} {}", r.kind, t)).or_default() += 1;
    }
    for (k, n) in rk
        .iter()
        .filter(|(k, _)| k.contains("node=Some(true)") || !k.contains("node="))
        .take(30)
    {
        println!("  rec {n:4} {k}");
    }
    let mut kv: Vec<_> = kinds.into_iter().collect();
    kv.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (k, n) in kv.iter().take(25) {
        println!("  {n:4} {k}");
    }
    for p in direct {
        let b = data.get(&p.building).unwrap();
        n.push(kenshi_world::nodes::Node {
            kind: *b.ints.get("node type").unwrap_or(&1),
            position: p.position,
            rotation: p.rotation,
            building: b.name.clone(),
            placement: usize::MAX,
        });
    }
    let mut by: BTreeMap<i32, Vec<&str>> = BTreeMap::new();
    for node in &n {
        by.entry(node.kind).or_default().push(&node.building);
    }
    println!("{} placements, {} nodes", placed.len(), n.len());
    let mut terrain = kenshi_world::Terrain::open(install).unwrap();
    for node in n.iter().filter(|n| n.kind <= 2).take(12) {
        let h = terrain
            .height_at(node.position[0], node.position[2])
            .unwrap();
        println!(
            "  type {} {:28} at ({:.0}, {:.0}, {:.0}) terrain {:.0} -> {:+.0}",
            node.kind,
            node.building,
            node.position[0],
            node.position[1],
            node.position[2],
            h,
            node.position[1] - h
        );
    }
    for (k, v) in by {
        let mut b: Vec<&str> = v.clone();
        b.sort();
        b.dedup();
        println!(
            "type {k}: {} nodes in {} buildings, e.g. {:?}",
            v.len(),
            b.len(),
            &b[..b.len().min(5)]
        );
    }
}
