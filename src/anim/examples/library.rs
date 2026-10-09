//! The sorted ANIMATION library for humans, one line per clip with the fields animation choice
//! reads: `cargo run -p kenshi-anim --example library -- <install> [list]`
//! (list: idles, upper, lower, strafes, other, weather; default all).
use kenshi_anim::select::Library;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let lib = Library::sort(data.of_type(ItemType::Animation));
    let want = a.get(1).map(String::as_str);
    for (name, list) in [
        ("idles", &lib.idles),
        ("upper", &lib.upper_moves),
        ("lower", &lib.lower_moves),
        ("strafes", &lib.strafes),
        ("other", &lib.other),
        ("weather", &lib.weather),
    ] {
        if want.is_some_and(|w| w != name) {
            continue;
        }
        println!("== {name} ({})", list.len());
        for x in list {
            println!(
                "  {:32} anim {:28} cat {} layer {} combat {} stealth {} w {}/{} carry l{} r{} carried {} speed {}..{} move {} legs r{:?} l{:?}",
                x.name, x.anim_name, x.category, x.layer, x.combat_mode, x.stealth_mode, x.weapon_l, x.weapon_r,
                x.carrying_left, x.carrying_right, x.being_carried, x.min_speed, x.max_speed, x.move_speed, x.r_leg, x.l_leg
            );
        }
    }
}
