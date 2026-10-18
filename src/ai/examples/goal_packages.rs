//! The AI packages whose goals include an AI_TASK enum, with each role's goal list (task,
//! classification, priority) so the task's rank among them shows:
//! `cargo run --release -p kenshi-ai --example goal_packages -- <install> ENUM`
use kenshi_ai::package::{Role, package_goals};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(&a[0]).load_order().unwrap();
    let c = GameDataContainer::load(&order).unwrap();
    let want: u16 = a[1].parse().unwrap();
    for r in c.of_type(ItemType::AiPackage) {
        for role in [Role::Leader, Role::Squad, Role::Squad2, Role::Slave] {
            let goals = package_goals(&c, r, role);
            if !goals.iter().any(|g| g.task == want) {
                continue;
            }
            let list: Vec<String> = goals.iter().map(|g| format!("{}{}:{}@{:.2}", if g.task == want { "*" } else { "" }, g.task, g.classification, g.priority)).collect();
            println!("{} [{role:?}]: {}", r.name, list.join(" "));
        }
        // the squads that run it
        let users: Vec<&str> = c
            .records
            .values()
            .filter(|q| matches!(q.item_type(), Some(ItemType::SquadTemplate | ItemType::UniqueSquadTemplate)))
            .filter(|q| q.refs("AI packages").iter().any(|(sid, _)| *sid == r.string_id))
            .map(|q| q.name.as_str())
            .collect();
        if !users.is_empty() && package_goals(&c, r, Role::Leader).iter().chain(package_goals(&c, r, Role::Squad).iter()).any(|g| g.task == want) {
            println!("    squads: {users:?}");
        }
    }
}
