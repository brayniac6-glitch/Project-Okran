//! Who brings each named character into the world: every record referencing it, and every
//! record referencing a squad template that has it (by category):
//! `cargo run -p kenshi-data --example who_spawns -- <install> <name>...`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for name in &a[1..] {
        let Some(ch) = c.records.values().find(|r| r.name == *name) else {
            println!("{name}: no such record");
            continue;
        };
        println!("{name} [{}]:", ch.string_id);
        let refs_to = |sid: &str| -> Vec<String> {
            let mut out = Vec::new();
            for r in c.records.values() {
                for (cat, list) in &r.references {
                    if list.iter().any(|(s, _)| s == sid) {
                        out.push(format!("{:?} {} . {cat}", r.item_type(), r.name));
                    }
                }
            }
            out
        };
        for l in refs_to(&ch.string_id) {
            println!("    {l}");
        }
        for t in c.of_type(kenshi_data::ItemType::SquadTemplate) {
            if t.references.iter().any(|(_, list)| list.iter().any(|(s, _)| *s == ch.string_id)) {
                for m in refs_to(&t.string_id) {
                    println!("      via {}: {m}", t.name);
                }
            }
        }
    }
}
