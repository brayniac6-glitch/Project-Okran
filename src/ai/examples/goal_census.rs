//! How often each goal task appears in the running packages of the squad templates towns,
//! nests and biomes spawn (each template's highest-priority package), leaders and members:
//! `cargo run -p kenshi-ai --example goal_census -- <install>`
use kenshi_ai::package::{Role, package_goals};
use kenshi_ai::task_def::def;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let mut count: HashMap<u16, (usize, Vec<String>)> = HashMap::new();
    for t in data.of_type(ItemType::SquadTemplate) {
        let Some(p) = t.refs("AI packages").iter().filter_map(|(s, v)| Some((data.get(s)?, v[0]))).max_by_key(|(_, v)| *v).map(|(p, _)| p) else { continue };
        for role in [Role::Leader, Role::Squad] {
            for g in package_goals(&data, p, role) {
                let e = count.entry(g.task).or_default();
                e.0 += 1;
                if e.1.len() < 3 && !e.1.contains(&p.name) {
                    e.1.push(p.name.clone());
                }
            }
        }
    }
    let mut v: Vec<_> = count.into_iter().collect();
    v.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (task, (n, eg)) in v {
        let d = def(task);
        println!("{n:5} {task:4} {:40} runnable={} {:?}", d.map_or("?", |d| d.name), d.map_or(0, |d| d.f134), eg);
    }
}
