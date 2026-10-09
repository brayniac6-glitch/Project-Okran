//! Zones ranked by placed buildings that have a mesh:
//! `cargo run -p kenshi-world --example towns -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use std::path::Path;

fn main() {
    let root = std::env::args().nth(1).expect("install dir");
    let mut order = Install::new(&root).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let mut rows = Vec::new();
    for folder in ["Newwworld", "Dialogue", "rebirth"] {
        let dir = Path::new(&root).join("data/newland/leveldata").join(folder);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("zone.") {
                continue;
            }
            let Ok(f) = fcs::read(&e.path()) else {
                continue;
            };
            let ps = kenshi_world::placements(&f);
            let named: Vec<&str> = ps
                .iter()
                .filter_map(|p| data.get(&p.building))
                .filter(|b| b.files.get("mesh").is_some_and(|m| !m.is_empty()))
                .map(|b| b.name.as_str())
                .collect();
            let towns: Vec<_> = ps
                .iter()
                .filter_map(|p| data.get(&p.building))
                .filter(|b| b.item_type() == Some(kenshi_data::ItemType::Town))
                .map(|b| b.name.clone())
                .collect();
            rows.push((
                named.len(),
                ps.len(),
                folder,
                name,
                named.first().map(|s| s.to_string()),
                towns,
            ));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    if let Some(top) = rows.iter().max_by_key(|r| r.1) {
        let dir = Path::new(&root).join("data/newland/leveldata").join(top.2);
        let f = fcs::read(&dir.join(&top.3)).unwrap();
        let mut count = std::collections::BTreeMap::new();
        for p in kenshi_world::placements(&f) {
            *count.entry(p.building).or_insert(0) += 1;
        }
        let mut v: Vec<_> = count.into_iter().collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.1));
        println!("{} {}: most placed buildings", top.2, top.3);
        for (sid, n) in v.iter().take(8) {
            if let Some(b) = data.get(sid) {
                let files: Vec<_> = b
                    .files
                    .iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                let refs: Vec<_> = b
                    .references
                    .iter()
                    .map(|(c, r)| format!("{c}({})", r.len()))
                    .collect();
                println!(
                    "  {n:>4} x {} [{:?}] files {:?} refs {:?}",
                    b.name,
                    b.item_type(),
                    files,
                    refs
                );
            }
        }
    }
    for r in rows.iter().take(15) {
        println!(
            "{:>4} meshed / {:>4} placed  {:<10} {:<18} e.g. {:?} towns {:?}",
            r.0, r.1, r.2, r.3, r.4, r.5
        );
    }
}
