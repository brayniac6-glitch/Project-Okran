//! gamedata <Kenshi install dir> [--warnings] [--show <string_id>]...
//!   load order, merged record counts by type, warnings; --show prints one merged record.

use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(root) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: gamedata <Kenshi install dir> [--warnings]");
        return ExitCode::FAILURE;
    };
    let install = Install::new(root);
    let order = match install.load_order() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("load order: {e}");
            return ExitCode::FAILURE;
        }
    };
    for (i, f) in order.files.iter().enumerate() {
        let tag = if f.core { "core" } else { "mod " };
        println!("{i:>3} {tag} {}", f.name);
    }
    let c = match GameDataContainer::load(&order) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("core file failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut by: BTreeMap<i32, usize> = BTreeMap::new();
    for r in c.records.values() {
        *by.entry(r.kind).or_default() += 1;
    }
    println!(
        "{} records, id counter {}, {} warnings",
        c.records.len(),
        c.id_counter,
        c.warnings.len()
    );
    for (k, n) in by {
        let name = ItemType::from_i32(k).map_or("?", ItemType::name);
        println!("  {k:>3} {name:<30} {n}");
    }
    for w in args.windows(2).filter(|w| w[0] == "--show") {
        match c.get(&w[1]) {
            Some(r) => {
                println!("{} '{}' from {}", r.string_id, r.name, c.files[r.source]);
                let mut ints: Vec<_> = r.ints.iter().collect();
                ints.sort();
                for (k, v) in ints {
                    println!("  int {k} = {v}");
                }
                let mut floats: Vec<_> = r.floats.iter().collect();
                floats.sort_by(|a, b| a.0.cmp(b.0));
                for (k, v) in floats {
                    println!("  float {k} = {v}");
                }
            }
            None => println!("{}: no such record", w[1]),
        }
    }
    if args.iter().any(|a| a == "--warnings") {
        for w in &c.warnings {
            println!("{w}");
        }
    }
    ExitCode::SUCCESS
}
