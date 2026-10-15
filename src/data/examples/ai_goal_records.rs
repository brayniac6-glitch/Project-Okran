//! The records with an "AI Goals" list, by type, with their goals:
//! `cargo run --release -p kenshi-data --example ai_goal_records -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(std::path::Path::new(&a[0])).game_order().unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    for r in data.records.values() {
        let goals = r.refs("AI Goals");
        if goals.is_empty() {
            continue;
        }
        let names: Vec<String> = goals.iter().map(|(s, v)| format!("{} {:?}", data.get(s).map_or(s.as_str(), |t| t.name.as_str()), v)).collect();
        println!("{:?}\t{}\t{}", r.item_type(), r.name, names.join(", "));
    }
}
