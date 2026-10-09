//! EFFECT records with fog volumes or lights, and the weathers using them:
//! `cargo run -p kenshi-data --example effect_fog -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for r in c.of_type(ItemType::Effect) {
        let fog = r.refs("fog volumes");
        let lights = r.refs("lights");
        if fog.is_empty() && lights.is_empty() {
            continue;
        }
        let used: Vec<String> = c.of_type(ItemType::Weather).filter(|w| w.refs("effects").iter().any(|(s, _)| *s == r.string_id)).map(|w| w.name.clone()).collect();
        println!("{} '{}' type {:?} fade in/out {:?}/{:?} particle {:?} weathers {:?}", r.string_id, r.name, r.ints.get("type"), r.floats.get("fog fade in duration"), r.floats.get("fog fade out duration"), r.strings.get("particle system"), used);
        for (s, v) in fog {
            if let Some(f) = c.get(s) {
                println!("   fog {:?} {} ints {:?} floats {:?} bools {:?}", v, f.name, f.ints, f.floats, f.bools);
            }
        }
        for (s, v) in lights {
            if let Some(f) = c.get(s) {
                println!("   light {:?} {} ints {:?} floats {:?}", v, f.name, f.ints, f.floats);
            }
        }
    }
}
