//! How many DIALOGUE_LINEs use each DialogActionEnum (DIALOG_ACTION "action name"), with an
//! example line's dialogue: `cargo run -p kenshi-data --example dialog_actions -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let mut n: HashMap<i32, (usize, String)> = HashMap::new();
    for line in c.of_type(ItemType::DialogueLine) {
        for (sid, _) in line.refs("effects") {
            let Some(act) = c.get(sid) else { continue };
            let k = act.ints.get("action name").copied().unwrap_or(0);
            let e = n.entry(k).or_insert((0, String::new()));
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = line.strings.get("text0").cloned().unwrap_or_default().chars().take(60).collect();
            }
        }
    }
    let mut v: Vec<_> = n.into_iter().collect();
    v.sort_by_key(|(_, (k, _))| std::cmp::Reverse(*k));
    for (k, (count, eg)) in v {
        println!("{count:5} {k:3} {eg:?}");
    }
}
