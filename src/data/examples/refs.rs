//! Each record of a type with its references (category, target name, values):
//! `cargo run -p kenshi-data --example refs -- <install> <type id> [category]`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    for sid in &c.order {
        let Some(r) = c.records.get(sid) else { continue };
        if r.kind != kind {
            continue;
        }
        println!("{} [{}]", r.name, r.string_id);
        for (cat, entries) in &r.references {
            if a.get(2).is_some_and(|w| w != cat) {
                continue;
            }
            for e in entries {
                let name = c.get(&e.0).map_or("?", |t| t.name.as_str());
                println!("    {cat}: {name} {:?}", e.1);
            }
        }
    }
}
