//! The towns buildings belong to (Kenshi's rule, F038257: `town::building_town`) against the
//! town each stands in (`town::current_town`): for every zone file's placements, those whose
//! town differs, by town, with their zone state's town handle (townC / townCS, which Kenshi
//! does not read for an NPC town):
//! `cargo run --release -p kenshi-world --example town_handles -- <install> [town name]`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::town::{building_town, current_town, towns};
use kenshi_world::{ZONE_FOLDERS, placements, zone_of};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let all = towns(install, &data);
    let mut zones: BTreeSet<(i32, i32)> = BTreeSet::new();
    for f in ZONE_FOLDERS {
        let dir = install.join("data/newland/leveldata").join(f);
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let mut it = n.strip_prefix("zone.").and_then(|r| r.strip_suffix(".zone")).into_iter().flat_map(|r| r.split('.'));
            if let (Some(x), Some(y)) = (it.next().and_then(|v| v.parse().ok()), it.next().and_then(|v| v.parse().ok())) {
                zones.insert((x, y));
            }
        }
    }
    let name = |t: Option<usize>| t.map_or("-".to_string(), |t| all[t].name.clone());
    let mut moved: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let (mut total, mut changed) = (0, 0);
    for &(x, y) in &zones {
        let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
        for pl in placements(&z) {
            let at = [pl.position[0], pl.world_y.unwrap_or(pl.position[1]), pl.position[2]];
            let interior = data.get(&pl.building).is_some_and(|b| !b.refs("interior").is_empty());
            let now = building_town(&all, at, interior, pl.inside, |_| false);
            let was = current_town(&all, at, None);
            total += 1;
            if now != was {
                changed += 1;
                let b = data.get(&pl.building).map_or(pl.building.clone(), |b| b.name.clone());
                let key = name(now);
                if a.get(1).is_none_or(|w| *w == key || *w == name(was)) {
                    moved.entry(key).or_default().push(format!("{b} in {:?} (was {}, handle {:?})", zone_of(at[0], at[2]), name(was), pl.town));
                }
            }
        }
    }
    for (t, v) in &moved {
        println!("{t}: {} placements", v.len());
        for s in v.iter().take(if a.get(1).is_some() { usize::MAX } else { 4 }) {
            println!("    {s}");
        }
    }
    println!("{changed} of {total} placements in another town than the one they stand in");
}
