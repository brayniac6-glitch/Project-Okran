//! Intact placements of buildings whose name holds a word, with their zone, position, squad:
//! `cargo run --release -p kenshi-world --example intact -- <install> <word>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let w = a[1].to_lowercase();
    for x in 0..64 {
        for y in 0..64 {
            let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
            for p in kenshi_world::placements(&z).iter().filter(|p| !p.destroyed) {
                let Some(b) = data.get(&p.building) else { continue };
                if b.name.to_lowercase().contains(&w) {
                    let sq = p.squad.as_ref().and_then(|s| data.get(s)).map(|r| r.name.clone());
                    println!("{x}.{y} {} at {:.0},{:.0} squad {:?} town {:?}", b.name, p.position[0], p.position[2], sq, p.town);
                }
            }
        }
    }
}
