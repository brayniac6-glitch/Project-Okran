//! Each bank's events (HIRC kind 4) by id, named where a candidate name (one per line in a file)
//! hashes to it: `cargo run -p kenshi-audio --example events -- <install> <names file>`
use kenshi_audio::bnk::{name_id, Bank};
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::Path::new(&a[0]).join("data/audio");
    let names: Vec<String> = std::fs::read_to_string(&a[1]).unwrap().lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let mut by: HashMap<u32, String> = HashMap::new();
    for n in &names {
        by.entry(name_id(n)).or_insert(n.clone());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "bnk")).collect();
    entries.sort();
    let (mut total, mut named) = (0, 0);
    for p in entries {
        let bank = Bank::parse(std::fs::read(&p).unwrap()).unwrap();
        let ev: Vec<u32> = bank.objects.iter().filter(|o| o.kind == 4).map(|o| o.id).collect();
        let known: Vec<String> = ev.iter().filter_map(|i| by.get(i).cloned()).collect();
        total += ev.len();
        named += known.len();
        println!("{} ({} events, {} named): {}", p.file_name().unwrap().to_string_lossy(), ev.len(), known.len(), known.join(", "));
        let unknown: Vec<String> = ev.iter().filter(|i| !by.contains_key(i)).map(|i| i.to_string()).collect();
        if !unknown.is_empty() {
            println!("   unknown: {}", unknown.join(" "));
        }
    }
    println!("{named} of {total} events named");
}
