//! Which of the given names are values a switch group's containers map, in the banks of an install:
//! `cargo run -p kenshi-audio --example switch_values -- <install> <group> <name> ...`
use kenshi_audio::{bnk::{name_id, Bank}, tree::{Kind, Library}};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::Path::new(&a[0]).join("data/audio");
    let mut lib = Library::default();
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = f.path();
        if p.extension().is_some_and(|e| e == "bnk") {
            if let Some(b) = Bank::parse(std::fs::read(&p).unwrap()) {
                lib.add(&b);
            }
        }
    }
    let group = name_id(&a[1]);
    let mut values = std::collections::HashSet::new();
    let mut containers = 0;
    for n in lib.nodes.values() {
        if let Kind::Switch { group: g, map, .. } = &n.kind
            && *g == group
        {
            containers += 1;
            values.extend(map.iter().map(|(v, _)| *v));
        }
    }
    println!("{} containers on {}, {} values", containers, a[1], values.len());
    for name in &a[2..] {
        println!("  {name}: {}", values.contains(&name_id(name)));
    }
}
