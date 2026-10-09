//! Every weather region (BIOME_GROUP) with the weathers its seasons can bring, marking the harmful
//! ones by "affect type" (1 sand, 2 acid, 3+ other) and their EFFECT refs:
//! `cargo run -p kenshi-world --example weather_map -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::weather::weather_regions;
use std::collections::BTreeSet;

fn main() {
    let install = std::path::PathBuf::from(std::env::args().nth(1).expect("install dir"));
    let mut order = Install::new(&install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let mut rows: Vec<String> = Vec::new();
    for r in weather_regions(&data, 365) {
        let name = data.get(&r.biome_group).map_or(r.biome_group.clone(), |g| g.name.clone());
        let mut bad = BTreeSet::new();
        for w in r.seasons.iter().flat_map(|s| &s.weathers) {
            let wname = data.get(&w.record).map_or("?", |x| x.name.as_str());
            let effects: Vec<&str> =
                w.effects.iter().filter_map(|(e, _)| data.get(e).map(|x| x.name.as_str())).collect();
            if w.affect_type != 0 || std::env::args().nth(2).is_some_and(|a| a == name) {
                bad.insert(format!("{wname} [type {} x{}] {effects:?}", w.affect_type, w.affect_strength));
            }
        }
        rows.push(format!("{name}: {}", bad.into_iter().collect::<Vec<_>>().join("; ")));
    }
    rows.sort();
    for r in rows {
        println!("{r}");
    }
}
