//! What the ported animation choice picks for a human from the shipped ANIMATION records:
//! `cargo run -p kenshi-anim --example choose -- <install> [combat 0/1] [weapon category]`
//! prints the idle standing still and the movement blend at a few speeds.
use kenshi_anim::select::{Library, Situation, idle, movement};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let lib = Library::sort(data.of_type(ItemType::Animation));
    println!(
        "{} idles, {} lower moves, {} upper moves, {} strafes, {} other",
        lib.idles.len(),
        lib.lower_moves.len(),
        lib.upper_moves.len(),
        lib.strafes.len(),
        lib.other.len()
    );
    if a.get(3).is_some() {
        for m in lib.lower_moves.iter().chain(&lib.upper_moves) {
            println!(
                "{:28} layer {} combat {} move {} min {} max {} play {} weapons {:#x} L{} R{}",
                m.name,
                m.layer,
                m.combat_mode,
                m.move_speed,
                m.min_speed,
                m.max_speed,
                m.play_speed,
                m.weapons,
                m.weapon_l,
                m.weapon_r
            );
        }
    }
    let combat: i32 = a.get(1).map_or(1, |s| s.parse().unwrap());
    let cat: u32 = a.get(2).map_or(0, |s| s.parse().unwrap());
    let mut s = Situation {
        category: 0,
        speed: 0.0,
        weapon_l: false,
        weapon_r: true,
        combat_mode: combat,
        stealth_mode: 0,
        r_leg: 100.0,
        l_leg: 100.0,
        weapon_category: cat,
        ..Default::default()
    };
    println!(
        "standing: {:?}",
        idle(&lib, &s, 1.0).map(|a| (&a.name, &a.anim_name))
    );
    for speed in [5.0, 15.0, 40.0, 60.0, 90.0] {
        s.speed = speed;
        let m = movement(&lib, &s, 1.0, 1.0);
        let list: Vec<String> = m
            .iter()
            .map(|(a, rate, w)| format!("{} x{rate:.2} w{w:.2}", a.anim_name))
            .collect();
        println!("speed {speed}: {}", list.join(", "));
    }
}
