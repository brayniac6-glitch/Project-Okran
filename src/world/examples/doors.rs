//! A building record's door references and instances:
//! `cargo run -p kenshi-world --example doors -- <install> <building name>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let b = data
        .records
        .values()
        .find(|r| r.name == a[1])
        .expect("building");
    println!("doors {:?}", b.refs("doors"));
    for i in &b.instances {
        let t = data
            .get(&i.target)
            .map_or("?".to_string(), |t| t.name.clone());
        println!(
            "instance {t} ({}) at {:?} rot {:?}",
            i.target, i.position, i.rotation
        );
    }
    let mut parts = Vec::new();
    meshes(&data, b, 0, &mut parts);
    for (sid, _) in b.refs("interior") {
        if let Some(r) = data.get(sid) {
            meshes(&data, r, 1, &mut parts);
        }
    }
    println!("building meshes {parts:?}");
    for (sid, _) in b.refs("doors") {
        if let Some(d) = data.get(sid) {
            let mut dm = Vec::new();
            meshes(&data, d, 0, &mut dm);
            println!("door {} meshes {dm:?} (also a building part: {:?})", d.name, dm.iter().filter(|m| parts.contains(m)).collect::<Vec<_>>());
            println!(
                "door {} ints {:?} floats {:?} bools {:?}",
                d.name, d.ints, d.floats, d.bools
            );
            for i in &d.instances {
                println!("  door instance {} at {:?}", i.target, i.position);
            }
        }
    }
}

/// The meshes a record's "parts" reach (its own "phs or mesh" and its parts', recursively).
#[allow(dead_code)]
fn meshes(data: &GameDataContainer, r: &kenshi_data::GameData, depth: u32, out: &mut Vec<String>) {
    if depth > 6 {
        return;
    }
    if let Some(m) = r.files.get("phs or mesh").filter(|m| !m.is_empty()) {
        out.push(m.clone());
    }
    for (sid, _) in r.refs("parts") {
        if let Some(c) = data.get(sid) {
            meshes(data, c, depth + 1, out);
        }
    }
}
