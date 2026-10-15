//! The hostile campaigns (through "inherits from") and whether their "dialog announcement" offers
//! the player replies or phase effects (DA_ASSAULT_PHASE 37, DA_RETREAT_PHASE 38):
//! `cargo run -p kenshi-data --example campaign_talks -- <install>`
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn field<'a>(c: &'a GameDataContainer, r: &'a GameData, k: &str) -> Option<bool> {
    r.bools.get(k).copied().or_else(|| r.refs("inherits from").iter().find_map(|(b, _)| c.get(b).and_then(|b| field(c, b, k))))
}

fn refs(c: &GameDataContainer, r: &GameData, k: &str) -> Vec<String> {
    let mut out: Vec<String> = r.refs(k).iter().map(|(s, _)| s.clone()).collect();
    for (b, _) in r.refs("inherits from") {
        if let Some(b) = c.get(b) {
            out.extend(refs(c, b, k));
        }
    }
    out
}

fn walk(c: &GameDataContainer, l: &GameData, depth: u32, replies: &mut bool, phases: &mut Vec<i32>) {
    if depth > 6 {
        return;
    }
    for (s, _) in l.refs("effects") {
        if let Some(a) = c.get(s).and_then(|x| x.ints.get("action name").copied())
            && (a == 37 || a == 38 || a == 6)
        {
            phases.push(a);
        }
    }
    for (s, _) in l.refs("lines") {
        if let Some(n) = c.get(s) {
            if n.ints.get("speaker").copied().unwrap_or(0) == 1 {
                *replies = true;
            }
            walk(c, n, depth + 1, replies, phases);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for camp in c.of_type(ItemType::FactionCampaign) {
        if !field(&c, camp, "is hostile").unwrap_or(true) || camp.name.starts_with("BASE") || camp.name.starts_with("FORCES") {
            continue;
        }
        let ann = refs(&c, camp, "dialog announcement");
        let mut replies = false;
        let mut phases = Vec::new();
        for d in &ann {
            if let Some(d) = c.get(d) {
                walk(&c, d, 0, &mut replies, &mut phases);
            }
        }
        phases.sort();
        phases.dedup();
        println!("{:60} replies {replies:5} effects {phases:?}", camp.name);
    }
}
