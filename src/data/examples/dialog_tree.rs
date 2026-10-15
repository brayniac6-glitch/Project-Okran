//! A dialogue's whole tree (by name): each line's speaker, text, conditions and effects:
//! `cargo run -p kenshi-data --example dialog_tree -- <install> <dialogue name> [depth]`
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn show(c: &GameDataContainer, l: &GameData, depth: usize, max: usize, seen: &mut Vec<String>) {
    let conds: Vec<String> = l
        .refs("conditions")
        .iter()
        .filter_map(|(s, v)| c.get(s).map(|x| format!("c{}:{}{}:{}", x.ints.get("condition name").copied().unwrap_or(-1), x.ints.get("compare by").copied().unwrap_or(0), x.ints.get("who").copied().unwrap_or(0), v[0])))
        .collect();
    let eff: Vec<String> = l.refs("effects").iter().filter_map(|(s, v)| c.get(s).map(|x| format!("a{}:{}", x.ints.get("action name").copied().unwrap_or(-1), v[0]))).collect();
    let text: String = l.strings.get("text0").cloned().unwrap_or_default().chars().take(80).collect();
    println!("{}[{}] {:?} {:?} {:?} {:?}", "  ".repeat(depth), l.ints.get("speaker").copied().unwrap_or(0), text, conds, eff, l.bools.iter().filter(|(_, v)| **v).map(|(k, _)| k.as_str()).collect::<Vec<_>>());
    if depth >= max || seen.contains(&l.string_id) {
        return;
    }
    seen.push(l.string_id.clone());
    for (s, _) in l.refs("lines") {
        if let Some(n) = c.get(s) {
            show(c, n, depth + 1, max, seen);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let max = a.get(2).and_then(|d| d.parse().ok()).unwrap_or(8);
    for d in c.of_type(ItemType::Dialogue).filter(|d| d.name == a[1]) {
        show(&c, d, 0, max, &mut Vec::new());
    }
}
