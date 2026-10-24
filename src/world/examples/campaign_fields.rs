//! Each FACTION_CAMPAIGN's own fields (not its bases'): its ints, floats, bools, strings and refs,
//! for those whose name holds the filter:
//! `cargo run --release -p kenshi-world --example campaign_fields -- <install> [name filter]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut all: Vec<&kenshi_data::GameData> = data.of_type(kenshi_data::ItemType::FactionCampaign).collect();
    all.sort_by(|x, y| x.name.cmp(&y.name));
    for c in all {
        if a.get(1).is_some_and(|f| !c.name.to_lowercase().contains(&f.to_lowercase())) {
            continue;
        }
        println!("{} [{}]", c.name, c.string_id);
        let mut ints: Vec<_> = c.ints.iter().collect();
        ints.sort();
        let mut floats: Vec<_> = c.floats.iter().map(|(k, v)| (k.clone(), *v)).collect();
        floats.sort_by(|x, y| x.0.cmp(&y.0));
        let mut bools: Vec<_> = c.bools.iter().collect();
        bools.sort();
        let mut strings: Vec<_> = c.strings.iter().collect();
        strings.sort();
        println!("    ints {ints:?}");
        println!("    floats {floats:?}");
        println!("    bools {bools:?}");
        println!("    strings {strings:?}");
        for (cat, refs) in &c.references {
            for (r, v) in refs {
                println!("    {cat}: {} {v:?}", data.get(r).map_or(r.as_str(), |x| x.name.as_str()));
            }
        }
    }
}
