//! Which CHARACTER and SQUAD_TEMPLATE records use a dialogue package (by name):
//! `cargo run -p kenshi-data --example package_users -- <install> <package name>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let Some(pkg) = c.of_type(ItemType::DialoguePackage).find(|p| p.name == a[1]) else { return println!("no package {:?}", a[1]) };
    let chars: Vec<&kenshi_data::GameData> = c.of_type(ItemType::Character).filter(|ch| ch.refs("dialogue package").iter().any(|(s, _)| *s == pkg.string_id)).collect();
    for ch in &chars {
        println!("character {} ({})", ch.name, ch.string_id);
    }
    for t in c.of_type(ItemType::SquadTemplate) {
        let uses = t.refs("dialogue package").iter().any(|(s, _)| *s == pkg.string_id)
            || ["leader", "squad", "squad2", "choosefrom list"].iter().any(|k| t.refs(k).iter().any(|(s, _)| chars.iter().any(|ch| ch.string_id == *s)));
        if uses {
            println!("squad {} ({})", t.name, t.string_id);
        }
    }
}
