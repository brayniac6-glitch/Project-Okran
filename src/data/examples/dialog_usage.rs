//! Where the base game's dialogue uses chosen DialogActionEnum / DialogConditionEnum values and
//! line fields, with an example of each:
//! `cargo run -p kenshi-data --example dialog_usage -- <install> [action,...] [condition,...]`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let list = |k: usize| -> Vec<i32> { a.get(k).map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default() };
    let (actions, conds) = (list(1), list(2));
    let text = |l: &kenshi_data::GameData| l.strings.get("text0").cloned().unwrap_or_default().chars().take(70).collect::<String>();
    let mut lines: Vec<&kenshi_data::GameData> = c.of_type(ItemType::DialogueLine).collect();
    lines.extend(c.of_type(ItemType::Dialogue));
    for want in &actions {
        let hits: Vec<_> = lines.iter().filter(|l| l.refs("effects").iter().any(|(s, _)| c.get(s).and_then(|x| x.ints.get("action name")) == Some(want))).collect();
        println!("action {want}: {} lines", hits.len());
        for l in hits.iter().take(4) {
            println!("    {} {:?}", l.string_id, text(l));
        }
    }
    for want in &conds {
        let hits: Vec<_> = lines.iter().filter(|l| l.refs("conditions").iter().any(|(s, _)| c.get(s).and_then(|x| x.ints.get("condition name")) == Some(want))).collect();
        println!("condition {want}: {} lines", hits.len());
        for l in hits.iter().take(4) {
            let vals: Vec<String> = l.refs("conditions").iter().filter(|(s, _)| c.get(s).and_then(|x| x.ints.get("condition name")) == Some(want)).map(|(s, v)| format!("{:?} {v:?}", c.get(s).map(|x| (x.ints.get("compare by").copied(), x.ints.get("who").copied())))).collect();
            println!("    {} {:?} {vals:?}", l.string_id, text(l));
        }
    }
    for field in ["crowd trigger", "interrupt", "AI contract", "change AI"] {
        let hits: Vec<_> = lines.iter().filter(|l| !l.refs(field).is_empty()).collect();
        println!("field {field:?}: {} lines", hits.len());
        for l in hits.iter().take(4) {
            println!("    {} {:?} -> {:?}", l.string_id, text(l), l.refs(field).iter().map(|(s, v)| (c.get(s).map(|x| x.name.clone()), *v)).collect::<Vec<_>>());
        }
    }
}
