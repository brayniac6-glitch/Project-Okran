//! Combat curves computed by the ported rules from shipped data, as CSV:
//! `cargo run -p kenshi-combat --example curves -- <install>` prints
//! `block,<gap>,<chance>` rows and `damage,<weapon>,<level>,<cut>,<blunt>` rows.
use kenshi_combat::weapon::{Weapon, set_weapon};
use kenshi_combat::{CombatConstants, block_chance, blunt_damage, cut_damage};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let gc = c.get("110-gamedata.quack"); // GLOBAL CONSTANTS
    let g = |k: &str| gc.and_then(|r| r.floats.get(k).copied());
    let cc = CombatConstants::from_floats(g);
    for gap in -50..=30 {
        println!("block,{gap},{}", block_chance(gap as f32, &cc));
    }
    for name in ["Katana", "Iron Club", "Plank"] {
        let r = c
            .of_type(ItemType::Weapon)
            .find(|r| r.name == name)
            .expect(name);
        let w = Weapon::from_record(
            r,
            None,
            None,
            0.5,
            g("weapon inventory weight mult").unwrap_or(0.5),
        );
        let s = set_weapon(
            Some((&w, r)),
            g("blunt damage 1").unwrap_or(20.0),
            g("cut damage 1").unwrap_or(20.0),
        );
        for lvl in (0..=100).step_by(5) {
            let l = lvl as f32;
            let at = s.attacker(l, (l, 1.0), (l, 1.0), 1.0);
            println!(
                "damage,{name},{lvl},{},{}",
                cut_damage(&at, &cc),
                blunt_damage(&at, &cc)
            );
        }
    }
}
