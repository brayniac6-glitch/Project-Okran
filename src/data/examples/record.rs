//! Print a merged record (its fields, files and references) and the records it references:
//! `cargo run -p kenshi-data --example record -- <install> <name or string_id>`
use kenshi_data::{GameData, GameDataContainer};
use kenshi_io::mods::Install;

fn show(c: &GameDataContainer, r: &GameData, depth: usize) {
    let pad = "    ".repeat(depth);
    let t = r.item_type().map_or("?", |t| t.name());
    println!("{pad}{} '{}' [{t}]", r.string_id, r.name);
    let mut scalars: Vec<String> = Vec::new();
    scalars.extend(r.bools.iter().map(|(k, v)| format!("{k} = {v}")));
    scalars.extend(r.ints.iter().map(|(k, v)| format!("{k} = {v}")));
    scalars.extend(r.floats.iter().map(|(k, v)| format!("{k} = {v}")));
    scalars.extend(
        r.strings
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k} = {v:?}")),
    );
    scalars.sort();
    if depth == 0 {
        for x in scalars {
            println!("{pad}  {x}");
        }
    }
    let mut files: Vec<_> = r.files.iter().filter(|(_, v)| !v.is_empty()).collect();
    files.sort();
    for (k, v) in files {
        println!("{pad}  file {k} = {v}");
    }
    for (cat, refs) in &r.references {
        for (target, vals) in refs {
            println!("{pad}  ref {cat} -> {target} {vals:?}");
            if depth < 1
                && let Some(t) = c.get(target)
            {
                show(c, t, depth + 1);
            }
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let r = c
        .get(&a[1])
        .or_else(|| c.records.values().find(|r| r.name == a[1]))
        .expect("no such record");
    show(&c, r, 0);
}
