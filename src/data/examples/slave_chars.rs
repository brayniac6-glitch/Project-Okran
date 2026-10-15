//! CHARACTER records with a "slave" state and the squad templates that hold them:
//! `cargo run --release -p kenshi-data --example slave_chars -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(std::path::Path::new(&a[0])).game_order().unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    for c in data.of_type(ItemType::Character) {
        let Some(&s) = c.ints.get("slave").filter(|v| **v != 0) else { continue };
        let squads: Vec<String> = data
            .of_type(ItemType::SquadTemplate)
            .filter(|t| ["squad", "leader", "squad2", "slaves"].iter().any(|k| t.refs(k).iter().any(|(sid, _)| *sid == c.string_id)))
            .map(|t| t.name.clone())
            .collect();
        println!("slave={s} {} ({}) in {:?}", c.name, c.string_id, squads);
    }
}
