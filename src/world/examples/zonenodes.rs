//! Node buildings placed by zone files, across every zone folder:
//! `cargo run -p kenshi-world --example zonenodes -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use kenshi_world::ZONE_FOLDERS;
use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let install = std::env::args().nth(1).unwrap();
    let mut order = Install::new(&install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let mut by: BTreeMap<(String, i32), usize> = BTreeMap::new();
    let mut files = 0;
    let mut per_zone: BTreeMap<String, usize> = BTreeMap::new();
    for f in ZONE_FOLDERS {
        let dir = Path::new(&install).join("data/newland/leveldata").join(f);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(only) = std::env::args().nth(2)
                && name != format!("zone.{only}.zone")
            {
                continue;
            }
            let Ok(z) = fcs::read(&e.path()) else {
                continue;
            };
            if std::env::args().nth(2).is_some() {
                let insts: usize = z.records.iter().map(|r| r.instances.len()).sum();
                println!("{f}/{name}: {} records, {insts} instances", z.records.len());
            }
            files += 1;
            for r in &z.records {
                for i in &r.instances {
                    if let Some(t) = data.get(&i.target)
                        && t.bools.get("is node").copied().unwrap_or(false)
                    {
                        let k = *t.ints.get("node type").unwrap_or(&1);
                        *by.entry((f.to_string(), k)).or_default() += 1;
                        if k == 1 {
                            *per_zone.entry(name.clone()).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    println!("{files} zone files");
    let mut pz: Vec<_> = per_zone.into_iter().collect();
    pz.sort_by_key(|e| std::cmp::Reverse(e.1));
    println!("most general nodes: {:?}", &pz[..pz.len().min(12)]);
    for ((f, k), n) in by {
        println!("{f} type {k}: {n}");
    }
}
