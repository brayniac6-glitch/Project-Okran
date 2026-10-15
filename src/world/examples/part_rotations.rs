//! The BUILDING_PARTs that move ("rotation function" not 0) with their axis, speed fields and the
//! buildings using them: `cargo run --release -p kenshi-world --example part_rotations -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut users: std::collections::HashMap<String, Vec<String>> = Default::default();
    for b in data.of_type(kenshi_data::ItemType::Building) {
        let mut stack: Vec<&kenshi_data::GameData> = vec![b];
        while let Some(r) = stack.pop() {
            for (sid, _) in r.refs("parts") {
                if let Some(p) = data.get(sid) {
                    users.entry(sid.clone()).or_default().push(b.name.clone());
                    stack.push(p);
                }
            }
        }
    }
    let mut by: std::collections::BTreeMap<i32, Vec<String>> = Default::default();
    for p in data.of_type(kenshi_data::ItemType::BuildingPart) {
        let f = p.ints.get("rotation function").copied().unwrap_or(0);
        if f == 0 {
            continue;
        }
        let mut fields: Vec<String> = p.floats.iter().filter(|(k, _)| k.contains("rot") || k.contains("speed") || k.contains("spin")).map(|(k, v)| format!("{k}={v}")).collect();
        fields.extend(p.ints.iter().filter(|(k, _)| k.contains("rot") || k.contains("axis")).map(|(k, v)| format!("{k}={v}")));
        fields.sort();
        let u = users.get(&p.string_id).map(|v| v.iter().take(3).cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default();
        by.entry(f).or_default().push(format!("{} {:?} [{u}]", p.name, fields));
    }
    for (f, v) in by {
        println!("rotation function {f}: {} parts", v.len());
        for l in v.iter().take(8) {
            println!("  {l}");
        }
    }
}
