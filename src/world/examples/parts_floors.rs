//! A building's parts and interior parts with their "building floor" and flags:
//! `cargo run --release -p kenshi-world --example parts_floors -- <install> <building name>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let b = data.get(&a[1]).or_else(|| data.records.values().find(|r| r.name == a[1] && !r.refs("parts").is_empty())).expect("building");
    fn walk(data: &kenshi_data::GameDataContainer, r: &kenshi_data::GameData, kind: &str, depth: usize) {
        for key in ["parts", "interior"] {
            for (sid, _) in r.refs(key) {
                if let Some(c) = data.get(sid) {
                    let mut ints: Vec<String> = c.ints.iter().filter(|(_, v)| **v != 0).map(|(k, v)| format!("{k}={v}")).collect();
                    ints.sort();
                    let mut bools: Vec<String> = c.bools.iter().filter(|(_, v)| **v).map(|(k, _)| k.to_string()).collect();
                    bools.sort();
                    let mesh = c.files.get("phs or mesh").map(|s| s.rsplit(['/', '\\']).next().unwrap_or("").to_string());
                    println!("{}{kind}/{key}: {} {:?} ints {:?} on {:?}", "  ".repeat(depth), c.name, mesh, ints, bools);
                    walk(data, c, kind, depth + 1);
                }
            }
        }
    }
    walk(&data, b, "", 0);
}
