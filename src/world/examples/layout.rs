//! One building's furnishing layout and what it places (record, position, its meshes):
//! `cargo run -p kenshi-world --example layout -- <install> <building name> <layout name>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_world::nodes::Layouts;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let layouts = Layouts::load(install);
    let Some(b) = data.records.values().find(|r| r.name == a[1]) else {
        panic!("no building {}", a[1]);
    };
    for i in layouts.for_building(&data, &b.string_id, &a[2]) {
        let t = data.get(&i.target);
        let name = t.map_or("?", |t| t.name.as_str());
        let mut meshes = vec![];
        if let Some(t) = t {
            for (sid, _) in t.refs("parts").iter().chain(t.refs("interior")) {
                if let Some(p) = data.get(sid)
                    && let Some(m) = p.files.get("phs or mesh")
                {
                    meshes.push(m.clone());
                }
            }
        }
        println!("{name:40} at {:.0?} {:?}", i.position, meshes);
    }
}
