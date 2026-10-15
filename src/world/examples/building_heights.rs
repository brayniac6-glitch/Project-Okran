//! Buildings' saved "world Y pos" against the terrain under them: per zone (X.Y) each
//! building, or (no zone) the types most often far below or above the ground:
//! `cargo run --release -p kenshi-world --example building_heights -- <install> [X.Y]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut t = kenshi_world::Terrain::open(install).unwrap();
    let one = a.get(1).map(|z| z.split('.').map(|x| x.parse::<i32>().unwrap()).collect::<Vec<_>>());
    let zones: Vec<(i32, i32)> = match &one {
        Some(v) => vec![(v[0], v[1])],
        None => (0..64).flat_map(|x| (0..64).map(move |y| (x, y))).collect(),
    };
    // by type: (count, below by 5+, the worst, where)
    let mut by: std::collections::HashMap<String, (usize, usize, f32, String)> = Default::default();
    for (x, y) in zones {
        let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
        for p in kenshi_world::placements(&z) {
            let name = data.get(&p.building).map_or("?".into(), |b| b.name.clone());
            let g = t.height_at(p.position[0], p.position[2]).unwrap_or(f32::NAN);
            let Some(wy) = p.world_y else { continue };
            if one.is_some() {
                println!("{:>9.1},{:<9.1} ground {g:8.1} world_y {wy:8.1} pos_y {:7.1} diff {:7.1}  {name}{}", p.position[0], p.position[2], p.position[1], wy - g, if p.destroyed { " (destroyed)" } else { "" });
            }
            let e = by.entry(name).or_insert((0, 0, 0.0, String::new()));
            e.0 += 1;
            if wy - g < -5.0 {
                e.1 += 1;
            }
            if wy - g < e.2 {
                e.2 = wy - g;
                e.3 = format!("{x}.{y} {:.0},{:.0}", p.position[0], p.position[2]);
            }
        }
    }
    if one.is_none() {
        let mut v: Vec<_> = by.into_iter().filter(|e| e.1.1 > 0).collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.1.1));
        for (n, (c, b, w, at)) in v.iter().take(40) {
            println!("{b:4}/{c:<4} below, worst {w:7.1} at {at}  {n}");
        }
    }
}
