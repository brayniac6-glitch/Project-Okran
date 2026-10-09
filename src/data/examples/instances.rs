//! Print a record's placed instances (what they place and where):
//! `cargo run -p kenshi-data --example instances -- <install> <name or string_id>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(&a[0]).load_order().unwrap();
    let c = GameDataContainer::load(&order).unwrap();
    for r in c.records.values().filter(|r| r.name == a[1] || r.string_id == a[1]) {
        println!("{} '{}' [{:?}]", r.string_id, r.name, r.item_type());
        for i in &r.instances {
            let t = c.get(&i.target);
            println!(
                "  {} -> {} '{}' [{:?}] at {:?} rot {:?}",
                i.id,
                i.target,
                t.map_or("?", |t| t.name.as_str()),
                t.and_then(|t| t.item_type()),
                i.position,
                i.rotation
            );
        }
    }
}
