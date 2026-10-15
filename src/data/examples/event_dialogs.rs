//! The dialogues the base game's packages give for chosen events (EventTriggerEnum), with each
//! one's lines, conditions and effects:
//! `cargo run -p kenshi-data --example event_dialogs -- <install> <event,...>`
use kenshi_data::{GameData, GameDataContainer};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let events: Vec<i32> = a.get(1).map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let text = |l: &GameData| l.strings.get("text0").cloned().unwrap_or_default().chars().take(90).collect::<String>();
    let conds = |l: &GameData| -> Vec<String> {
        l.refs("conditions")
            .iter()
            .filter_map(|(s, v)| c.get(s).map(|x| format!("c{}:{}{}:{v:?}", x.ints.get("condition name").copied().unwrap_or(-1), x.ints.get("compare by").copied().unwrap_or(0), x.ints.get("who").copied().unwrap_or(0))))
            .collect()
    };
    let effects = |l: &GameData| -> Vec<String> {
        l.refs("effects").iter().filter_map(|(s, v)| c.get(s).map(|x| format!("a{}:{v:?}", x.ints.get("action name").copied().unwrap_or(-1)))).collect()
    };
    for ev in events {
        println!("== event {ev}");
        let mut seen = std::collections::HashSet::new();
        for pkg in c.of_type(kenshi_data::ItemType::DialoguePackage) {
            for (sid, v) in pkg.refs("dialogs") {
                if v[0] != ev || !seen.insert(sid.clone()) {
                    continue;
                }
                let Some(d) = c.get(sid) else { continue };
                println!("{} ({}) chance {:?} in {} {:?} {:?}", d.name, sid, v, pkg.name, conds(d), effects(d));
                for (l, _) in d.refs("lines") {
                    if let Some(l) = c.get(l) {
                        println!("    {:?} {:?} {:?}", text(l), conds(l), effects(l));
                        for (r, _) in l.refs("lines") {
                            if let Some(r) = c.get(r) {
                                println!("        > {:?} {:?} {:?}", text(r), conds(r), effects(r));
                            }
                        }
                    }
                }
            }
        }
    }
}
