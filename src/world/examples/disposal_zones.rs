//! The zones whose placed buildings hold furniture of a BuildingFunction (default 11,
//! BF_CORPSE_DISPOSAL: the Cooking Pit, Skeleton Shredder, Corpse Furnace), with the town each
//! stands in:
//! `cargo run --release -p kenshi-world --example disposal_zones -- <install> [function]`
use kenshi_world::furniture::furniture;
use kenshi_world::nodes::Layouts;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let function: i32 = a.get(1).map_or(11, |s| s.parse().unwrap());
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let layouts = Layouts::load(install);
    let towns = kenshi_world::town::towns(install, &data);
    for x in 0..64 {
        for y in 0..64 {
            let Some(zone) = kenshi_world::read_zone(install, x, y) else { continue };
            let placed = kenshi_world::placements(&zone);
            for f in furniture(&data, &layouts, &placed).iter().filter(|f| f.function == function) {
                let town = kenshi_world::town::current_town(&towns, f.position, None).map_or("-".to_string(), |t| towns[t].name.clone());
                println!("zone {x} {y}: {} at {:.0?} in {town}", f.name, f.position);
            }
        }
    }
}
