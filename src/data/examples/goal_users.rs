//! The AI packages (AI_PACKAGE) whose goals or slave goals use an AI task of a given enum, with
//! the reference's values: `cargo run --release -p kenshi-data --example goal_users -- <install> <enum>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(std::path::Path::new(&a[0])).game_order().unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    let want: i32 = a[1].parse().unwrap();
    for r in data.records.values() {
        for (key, refs) in &r.references {
            for (sid, v) in refs {
                if let Some(t) = data.get(sid)
                    && t.ints.get("enum").copied() == Some(want)
                    && t.ints.contains_key("enum")
                {
                    println!("{} [{:?}] {key} -> {} {:?}", r.name, r.item_type(), t.name, v);
                }
            }
        }
    }
}
