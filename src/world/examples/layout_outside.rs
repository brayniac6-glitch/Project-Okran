//! The furnishing layouts' furniture "outside" or not (its state record's "flags" bit 0, else its
//! "is outside", as F037789 reads them; F037736 keeps only those shown while the building's
//! interior is closed), counted by layout kind:
//! `cargo run -p kenshi-world --example layout_outside -- <install> [building name]`
use kenshi_io::fcs;
use std::collections::{BTreeMap, HashMap};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let only = a.get(1);
    let dir = install.join("data/newland/leveldata");
    let files = std::iter::once(dir.join("interiors.level"))
        .chain(kenshi_world::ZONE_FOLDERS.iter().map(|f| dir.join(f).join("interiors.level")));
    let mut counts: BTreeMap<(bool, String), (usize, usize)> = BTreeMap::new();
    for f in files.filter(|f| f.is_file()) {
        let Ok(file) = fcs::read(&f) else { continue };
        // each state record's "outside"
        let outside: HashMap<&str, bool> = file
            .records
            .iter()
            .filter(|r| r.kind == 35)
            .map(|r| {
                let flags = r.ints.iter().find(|(k, _)| k == "flags").map(|(_, v)| *v);
                let bool_ = r.bools.iter().find(|(k, _)| k == "is outside").map(|(_, v)| *v);
                (r.string_id.as_str(), flags.map_or(bool_.unwrap_or(false), |f| f & 1 != 0))
            })
            .collect();
        for r in file.records.iter().filter(|r| r.kind == 30) {
            let b = r.strings.iter().find(|(k, _)| k == "associated building").map(|(_, v)| v.clone()).unwrap_or_default();
            if only.is_some_and(|o| !b.contains(o.as_str()) && !r.name.contains(o.as_str())) {
                continue;
            }
            let kind = if r.name.to_lowercase().contains("ext") { "exterior-ish" } else { "other" };
            for i in &r.instances {
                let out = i.states.iter().any(|s| outside.get(s.as_str()).copied().unwrap_or(false));
                let e = counts.entry((out, kind.to_string())).or_default();
                e.0 += 1;
                if i.states.is_empty() {
                    e.1 += 1;
                }
                if only.is_some() {
                    println!("{} / {}: {} outside {out} states {:?} at {:?}", b, r.name, i.target, i.states, i.position);
                }
            }
        }
    }
    for ((out, kind), (n, stateless)) in counts {
        println!("outside {out:5} {kind:13} {n:6} instances ({stateless} without a state)");
    }
}
