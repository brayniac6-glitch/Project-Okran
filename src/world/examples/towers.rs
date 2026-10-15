//! Towers placed in zones (intact ones, by type, with a zone and position of each):
//! `cargo run --release -p kenshi-world --example towers -- <install> [X.Y]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let zones: Vec<(i32, i32)> = match a.get(1) {
        Some(z) => {
            let v: Vec<i32> = z.split('.').map(|x| x.parse().unwrap()).collect();
            vec![(v[0], v[1])]
        }
        None => (0..64).flat_map(|x| (0..64).map(move |y| (x, y))).collect(),
    };
    let mut seen: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    for (x, y) in zones {
        let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
        for p in kenshi_world::placements(&z) {
            let Some(b) = data.get(&p.building) else { continue };
            if !b.name.to_lowercase().contains("tower") || p.destroyed {
                continue;
            }
            let e = seen.entry(format!("{} ({})", b.name, p.building)).or_insert((0, format!("{x}.{y} {:?} world_y {:?} rot {:?}", p.position, p.world_y, p.rotation)));
            e.0 += 1;
        }
    }
    for (k, (n, at)) in seen {
        println!("{n:4} {k}: e.g. {at}");
    }
}
