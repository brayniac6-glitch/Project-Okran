//! Which AI goals the shipped data uses: every AI_TASK reference in "AI Goals" lists (AI
//! packages, characters, factions, squads), counted by task type:
//! `cargo run -p kenshi-ai --example goals -- <install>`
use kenshi_ai::task_def::def;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::HashMap;

fn main() {
    let install = std::env::args().nth(1).expect("install");
    let mut order = Install::new(&install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let mut count: HashMap<i32, (usize, Vec<String>)> = HashMap::new();
    for r in data.records.values() {
        for (cat, refs) in &r.references {
            if !cat.contains("AI Goals") {
                continue;
            }
            for e in refs {
                let Some(t) = data.get(&e.0) else { continue };
                if t.item_type() != Some(ItemType::AiTask) {
                    continue;
                }
                let n = *t.ints.get("enum").unwrap_or(&-1);
                let c = count.entry(n).or_default();
                c.0 += 1;
                if c.1.len() < 3 {
                    c.1.push(format!("{} ({:?})", r.name, r.item_type()));
                }
            }
        }
    }
    let mut v: Vec<_> = count.into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1.0));
    for (task, (n, eg)) in v {
        let name = def(task as u16).map_or("?", |d| d.name);
        println!("{n:4}  {task:3} {name:32} e.g. {}", eg.join("; "));
    }
}
