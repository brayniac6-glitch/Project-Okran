//! Which AI_PACKAGEs (and their goal categories) use an AI_TASK (by name):
//! `cargo run -p kenshi-data --example task_users -- <install> <task name>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let Some(task) = c.of_type(ItemType::AiTask).find(|t| t.name == a[1]) else { return println!("no task {:?}", a[1]) };
    println!("task {} ({}) {:?}", task.name, task.string_id, task.ints);
    for p in c.of_type(ItemType::AiPackage) {
        for (cat, refs) in &p.references {
            if let Some((_, v)) = refs.iter().find(|(s, _)| *s == task.string_id) {
                println!("  {} ({}): {cat} {v:?}", p.name, p.string_id);
            }
        }
    }
}
