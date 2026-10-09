//! The movement animations as the library sorts them, with what picks them:
//! `cargo run -p kenshi-anim --example moves -- <install> [name filter]`
use kenshi_anim::select::Library;
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let lib = Library::sort(c.of_type(kenshi_data::ItemType::Animation));
    let filter = a.get(1).cloned().unwrap_or_default();
    for (list, name) in [(&lib.upper_moves, "upper"), (&lib.lower_moves, "lower"), (&lib.strafes, "strafe")] {
        for x in list.iter().filter(|x| x.name.contains(&filter)) {
            println!(
                "{name:6} {:28} anim {:24} move {:7.1} play {:6.2} speed {:7.1}..{:7.1} combat {} stealth {} cat {} weapons {:#x} L {:?} R {:?} layer {}",
                x.name, x.anim_name, x.move_speed, x.play_speed, x.min_speed, x.max_speed, x.combat_mode, x.stealth_mode, x.category, x.weapons, x.weapon_l, x.weapon_r, x.layer
            );
        }
    }
}
