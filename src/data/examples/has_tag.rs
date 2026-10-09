//! The values DIALOG_ACTION conditions of one kind are compared with, by use:
//! `cargo run --release -p kenshi-data --example has_tag -- <install> <condition number>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let want: i32 = a[1].parse().unwrap();
    let mut by: HashMap<(i32, i32, i32), usize> = HashMap::new();
    for r in c.of_type(ItemType::DialogueLine).chain(c.of_type(ItemType::Dialogue)) {
        for (sid, v) in r.refs("conditions") {
            if let Some(d) = c.get(sid)
                && d.ints.get("condition name").copied().unwrap_or(0) == want
            {
                let tag = d.ints.get("tag").copied().unwrap_or(-1);
                let who = d.ints.get("who").copied().unwrap_or(0);
                *by.entry((v[0], tag, who)).or_default() += 1;
            }
        }
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    for ((val, tag, who), n) in v {
        println!("value {val} tag {tag} who {who}: {n}");
    }
    if let Some(d) = c.of_type(ItemType::DialogAction).find(|d| d.ints.get("condition name").copied() == Some(want)) {
        println!("e.g. {:?} {:?}", d.ints, d.strings);
    }
}
