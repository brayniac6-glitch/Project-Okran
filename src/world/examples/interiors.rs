//! Node placements in interiors.level and how they point at their building:
//! `cargo run -p kenshi-world --example interiors -- <install> <file>`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let z = fcs::read(std::path::Path::new(&a[1])).unwrap();
    let mut shown = 0;
    for r in &z.records {
        let nodes: Vec<_> = r
            .instances
            .iter()
            .filter(|i| {
                data.get(&i.target)
                    .is_some_and(|t| t.bools.get("is node").copied().unwrap_or(false))
            })
            .collect();
        if nodes.is_empty() || shown > 2 {
            continue;
        }
        shown += 1;
        println!(
            "record kind {} '{}' {} ({} instances, {} nodes)",
            r.kind,
            r.name,
            r.string_id,
            r.instances.len(),
            nodes.len()
        );
        println!("  ints {:?}", r.ints);
        println!("  strings {:?}", r.strings);
        for i in r.instances.iter().take(6) {
            let t = data.get(&i.target).map_or("?".into(), |t| t.name.clone());
            println!(
                "  inst {} -> {} {:?} states {:?}",
                i.id, t, i.position, i.states
            );
        }
    }
}
