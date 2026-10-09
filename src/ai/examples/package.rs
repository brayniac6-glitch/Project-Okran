//! The goals an AI package gives each role: `cargo run -p kenshi-ai --example package -- <install> <package name>`
//! (`--task N` lists the packages giving task N to squad members, with their leaders' tasks;
//! `--users <package name>` the squad templates that use a package, with their factions)
use kenshi_ai::package::{Role, package_goals};
use kenshi_ai::task_def::def;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    if a[1] == "--task" {
        let want: u16 = a[2].parse().unwrap();
        for p in data.of_type(ItemType::AiPackage) {
            let squad = package_goals(&data, p, Role::Squad);
            if squad.iter().any(|g| g.task == want) {
                let leader: Vec<u16> = package_goals(&data, p, Role::Leader)
                    .iter()
                    .map(|g| g.task)
                    .collect();
                println!("{:50} leader {:?}", p.name, leader);
            }
        }
        return;
    }
    if a[1] == "--users" {
        let p = data
            .of_type(ItemType::AiPackage)
            .find(|r| r.name == a[2])
            .expect("package");
        for t in data.of_type(ItemType::SquadTemplate) {
            if t.refs("AI packages").iter().any(|(s, _)| *s == p.string_id) {
                let f = t.refs("faction").first().and_then(|(s, _)| data.get(s));
                println!(
                    "{:40} {:28} faction {:24} formation {:?}",
                    t.name,
                    t.string_id,
                    f.map_or("-", |f| f.name.as_str()),
                    f.and_then(|f| f.ints.get("squad formation"))
                );
            }
        }
        return;
    }
    let p = data
        .of_type(ItemType::AiPackage)
        .find(|r| r.name == a[1])
        .expect("package");
    for role in [Role::Leader, Role::Squad, Role::Squad2, Role::Slave] {
        let g = package_goals(&data, p, role);
        if g.is_empty() {
            continue;
        }
        println!("{role:?}:");
        for x in g {
            let d = def(x.task).map_or("?", |d| d.name);
            println!(
                "  {:40} task {:3} {:28} class {} hours {:?} priority {:.2} targeting {}",
                x.name, x.task, d, x.classification, x.hours, x.priority, x.targeting
            );
        }
    }
}
