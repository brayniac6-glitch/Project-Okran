//! Building types placed intact in zones whose parts reach "building floor" 2 or more (towers),
//! with how many are placed and how many have a squad living in them:
//! `cargo run --release -p kenshi-world --example tall -- <install>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    fn top(data: &kenshi_data::GameDataContainer, r: &kenshi_data::GameData) -> i32 {
        let mut m = r.ints.get("building floor").copied().unwrap_or(0) as i32;
        for (sid, _) in r.refs("parts").iter().chain(r.refs("interior")) {
            if let Some(c) = data.get(sid) {
                m = m.max(top(data, c));
            }
        }
        m
    }
    let mut by: std::collections::BTreeMap<String, (i32, usize, usize, String)> = Default::default();
    for x in 0..64 {
        for y in 0..64 {
            let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
            for p in kenshi_world::placements(&z).iter().filter(|p| !p.destroyed) {
                let Some(b) = data.get(&p.building) else { continue };
                let t = top(&data, b);
                if t < 2 {
                    continue;
                }
                let e = by.entry(b.name.clone()).or_insert((t, 0, 0, String::new()));
                e.1 += 1;
                if p.squad.is_some() {
                    e.2 += 1;
                    if e.3.is_empty() {
                        e.3 = format!("{x}.{y} {:.0},{:.0}", p.position[0], p.position[2]);
                    }
                }
            }
        }
    }
    for (n, (t, c, s, at)) in by {
        println!("floors {t} placed {c:4} lived in {s:4} {at:24} {n}");
    }
}
