//! Each hostile FACTION_CAMPAIGN's "retreat AI leader" and "victory AI leader" packages (through
//! "inherits from") and their leader and squad tasks, grouped:
//! `cargo run --release -p kenshi-data --example campaign_retreats -- <install>`
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
    let order = Install::new(&a[0]).game_order().unwrap();
    let c = GameDataContainer::load(&order).unwrap();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in c.of_type(ItemType::FactionCampaign) {
        if !r.bools.get("is hostile").copied().unwrap_or(false) {
            continue;
        }
        let name = |s: &String| c.get(s).map_or(s.clone(), |p| p.name.clone());
        let ret: Vec<String> = refs(&c, r, "retreat AI leader", 0).iter().map(name).collect();
        let vic: Vec<String> = refs(&c, r, "victory AI leader", 0).iter().map(name).collect();
        let pt2: Vec<String> = refs(&c, r, "pt2 AI leader", 0).iter().map(name).collect();
        groups.entry(format!("retreat {ret:?} / victory {vic:?} / pt2 {pt2:?}")).or_default().push(r.name.clone());
    }
    for (k, v) in &groups {
        println!("{k}: {} e.g. {:?}", v.len(), &v[..v.len().min(3)]);
    }
    for p in c.of_type(ItemType::AiPackage) {
        if !groups.keys().any(|k| k.contains(&format!("\"{}\"", p.name))) {
            continue;
        }
        let tasks = |cat: &str| -> Vec<String> { p.refs(cat).iter().filter_map(|(t, v)| c.get(t).map(|t| format!("{}:{}@{}", t.ints.get("enum").copied().unwrap_or(-1), v[0], v[1]))).collect() };
        println!("package {}: leader {:?} squad {:?} inherits {:?}", p.name, tasks("Leader AI Goals"), tasks("Squad AI Goals"), p.refs("inherits from").iter().map(|(s, _)| name_of(&c, s)).collect::<Vec<_>>());
    }
}

fn name_of(c: &GameDataContainer, s: &str) -> String {
    c.get(s).map_or(s.to_string(), |p| p.name.clone())
}
