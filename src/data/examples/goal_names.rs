//! An AI package's goals by role, each with its task enum and name:
//! `cargo run --release -p kenshi-data --example goal_names -- <install> <package name>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(std::path::Path::new(&a[0])).game_order().unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    let p = data.records.values().find(|r| r.name == a[1]).expect("package");
    for (key, refs) in &p.references {
        for (sid, v) in refs {
            if let Some(t) = data.get(sid) {
                println!("{key}: {} (enum {:?}) {:?}", t.name, t.ints.get("enum"), v);
            }
        }
    }
}
