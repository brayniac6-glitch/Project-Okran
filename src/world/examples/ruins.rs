//! Destroyed buildings placed in zones and what their interior parts and doors draw:
//! `cargo run --release -p kenshi-world --example ruins -- <install> [zones to scan, default all]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut shown = 0;
    'z: for x in 0..64 {
        for y in 0..64 {
            let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
            for p in kenshi_world::placements(&z).iter().filter(|p| p.destroyed) {
                let Some(b) = data.get(&p.building) else { continue };
                let short = |s: &String| s.rsplit(['/', '\\']).next().unwrap_or("").to_string();
                let interior: Vec<String> = b.refs("interior").iter().chain(b.refs("doors")).filter_map(|(s, _)| data.get(s)).map(|r| format!("{} (mesh {:?}, destroyed {:?})", r.name, r.files.get("phs or mesh").map(short), r.files.get("destroyed mesh").map(short))).collect();
                println!("{x}.{y} {} at {:?}: {:?}", b.name, p.position, interior);
                // its shell's parts (and theirs)
                fn walk(data: &kenshi_data::GameDataContainer, r: &kenshi_data::GameData, depth: usize) {
                    for (sid, _) in r.refs("parts") {
                        if let Some(c) = data.get(sid) {
                            let f = |k: &str| c.files.get(k).map(|s| s.rsplit(['/', '\\']).next().unwrap_or("").to_string());
                            println!("    {}{} mesh {:?} destroyed {:?} roof {:?} floor {:?}", "  ".repeat(depth), c.name, f("phs or mesh"), f("destroyed mesh"), c.bools.get("is unwalkable roof"), c.ints.get("building floor"));
                            walk(data, c, depth + 1);
                        }
                    }
                }
                walk(&data, b, 0);
                shown += 1;
                if shown >= 3 {
                    break 'z;
                }
            }
        }
    }
}
