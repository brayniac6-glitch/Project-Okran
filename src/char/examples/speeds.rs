//! Run speed by athletics for some races, and load by carried weight, from the ported rules:
//! `cargo run -p kenshi-char --example speeds -- <install>` prints CSV rows
//! `run,<race>,<athletics>,<speed>` and `load,<strength>,<weight>,<load>`.
use kenshi_char::encumbrance::{EncumbranceConstants, load, run_speed, strength_level};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for name in ["Greenlander", "Shek", "Hive Worker Drone", "Skeleton"] {
        let Some(r) = c.of_type(ItemType::Race).find(|r| r.name == name) else {
            continue;
        };
        let min = *r.ints.get("speed min skill").unwrap_or(&70) as f32;
        let max = *r.ints.get("speed max skill").unwrap_or(&120) as f32;
        for at in (0..=100).step_by(5) {
            println!("run,{name},{at},{}", run_speed(at as f32, min, max, 1.0));
        }
    }
    let k = EncumbranceConstants::default();
    let gc = c.get("110-gamedata.quack");
    let g = |key: &str, d: f32| gc.and_then(|r| r.floats.get(key).copied()).unwrap_or(d);
    let k = EncumbranceConstants {
        carry_weight_mult: g("carry weight mult", k.carry_weight_mult),
        encumbrance_base: g("encumbrance base", k.encumbrance_base),
        ..k
    };
    for strength in [20, 50, 80] {
        for w in (0..=400).step_by(10) {
            let l = load(w as f32, 1.0, strength_level(strength as f32, 1.0), &k);
            println!("load,{strength},{w},{l}");
        }
    }
}
