//! Each FACTION_CAMPAIGN's phase AI packages ("pt1/pt2/retreat/victory AI leader/others", with
//! "inherits from") and the tasks of each package:
//! `cargo run -p kenshi-data --example campaign_phases -- <install>`
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn refs<'a>(c: &'a GameDataContainer, r: &'a GameData, cat: &str, depth: u32) -> Vec<String> {
    let own: Vec<String> = r.refs(cat).iter().map(|(s, _)| s.clone()).collect();
    if !own.is_empty() || depth > 4 {
        return own;
    }
    for (p, _) in r.refs("inherits from") {
        if let Some(pr) = c.get(p) {
            let v = refs(c, pr, cat, depth + 1);
            if !v.is_empty() {
                return v;
            }
        }
    }
    Vec::new()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in c.of_type(ItemType::FactionCampaign) {
        if !r.bools.get("is hostile").copied().unwrap_or(false) {
            continue;
        }
        let v = refs(&c, r, "victory AI leader", 0);
        let name = |s: &String| c.get(s).map_or(s.clone(), |p| p.name.clone());
        let key = v.iter().map(name).collect::<Vec<_>>().join(", ");
        let by: Vec<String> = c.of_type(ItemType::Faction).filter(|f| f.refs("campaigns").iter().any(|(x, _)| *x == r.string_id)).map(|f| f.name.clone()).collect();
        eprintln!("campaign {} [{key}] listed by {by:?}, characters {}", r.name, r.bools.get("target characters").copied().unwrap_or(false));
        packages.entry(key).or_default().push(r.name.clone());
        for s in refs(&c, r, "pt2 AI leader", 0).iter().chain(&v) {
            if let Some(p) = c.get(s) {
                let tasks: Vec<String> = p.refs("Leader AI Goals").iter().chain(p.refs("Squad AI Goals")).filter_map(|(t, _)| c.get(t)).map(|t| format!("{} ({})", t.name, t.ints.get("enum").copied().unwrap_or(-1))).collect();
                eprintln!("package {}: {:?}", p.name, tasks);
            }
        }
    }
    for (k, v) in packages {
        println!("victory [{k}]: {} campaigns, e.g. {:?}", v.len(), &v[..v.len().min(4)]);
    }
}
