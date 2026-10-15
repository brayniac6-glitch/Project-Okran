//! Every AI_TASK the game's goal lists use (AI packages' "Leader AI Goals", "Squad AI Goals",
//! "Slave AI Goals"; characters' and factions' "AI Goals"), by its enum, with how many records
//! use it: `cargo run --release -p kenshi-ai --example package_tasks -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(&a[0]).load_order().unwrap();
    let c = GameDataContainer::load(&order).unwrap();
    let mut uses: BTreeMap<i32, (String, usize, Vec<String>)> = BTreeMap::new();
    for r in c.records.values() {
        for cat in ["Leader AI Goals", "Squad AI Goals", "Slave AI Goals", "AI Goals"] {
            for (sid, _) in r.refs(cat) {
                let Some(t) = c.get(sid) else { continue };
                let Some(&e) = t.ints.get("enum") else { continue };
                let entry = uses.entry(e).or_insert_with(|| (t.name.clone(), 0, Vec::new()));
                entry.1 += 1;
                if entry.2.len() < 3 && !entry.2.contains(&r.name) {
                    entry.2.push(r.name.clone());
                }
            }
        }
    }
    for (e, (name, n, eg)) in &uses {
        let def = kenshi_ai::task_def::def(*e as u16).map_or("?".to_string(), |d| d.name.to_string());
        println!("{e:>4} {def:<36} {n:>4}  e.g. {name:?} in {eg:?}");
    }
}
