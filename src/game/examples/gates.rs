//! The game data's gates and doors: wall gates and gateways (BUILDING "is wall gate" /
//! "is gateway"), linked walls, and the squad templates whose "initial door state" is not
//! closed: `cargo run --release -p kenshi-game --example gates -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let b = |r: &kenshi_data::GameData, k: &str| r.bools.get(k).copied().unwrap_or(false);
    let mut gates = 0;
    for r in data.of_type(ItemType::Building) {
        let (gate, gateway, links) = (b(r, "is wall gate"), b(r, "is gateway"), b(r, "links together"));
        if gate || gateway || (links && a.get(1).is_some()) {
            gates += 1;
            println!(
                "{} [{}] gate {gate} gateway {gateway} links {links} type {:?} cat {:?} doors {:?} always locks {} move {:?}/{:?} axis {:?} out {:?}",
                r.name,
                r.string_id,
                r.ints.get("link type"),
                r.strings.get("building category"),
                r.refs("doors").iter().map(|(s, _)| data.get(s).map_or(s.clone(), |d| d.name.clone())).collect::<Vec<_>>(),
                b(r, "always locks"),
                r.floats.get("door move dist"),
                r.floats.get("door move speed"),
                r.ints.get("door axis"),
                r.floats.get("door comes out"),
            );
            for (sid, _) in r.refs("doors") {
                if let Some(d) = data.get(sid) {
                    println!("    door {} build materials {:?} construction {:?} hardness {:?}", d.name, d.ints.get("build materials"), d.refs("construction"), d.floats.get("hardness"));
                }
            }
            println!("    wall build materials {:?} construction {:?} hardness {:?} link length {:?} subsections {:?}", r.ints.get("build materials"), r.refs("construction"), r.floats.get("hardness"), r.floats.get("link length"), r.refs("wall subsections").len());
        }
    }
    println!("{gates} gates");
    let mut states = std::collections::BTreeMap::new();
    for s in data.of_type(ItemType::SquadTemplate) {
        let v = s.ints.get("initial door state").copied().unwrap_or(0);
        *states.entry(v).or_insert(0) += 1;
        if v != 0 && a.get(2).is_some() {
            println!("squad {} initial door state {v}", s.name);
        }
    }
    println!("squad templates by initial door state (0 closed, 1 open, 2 locked, 3 broken): {states:?}");
}
