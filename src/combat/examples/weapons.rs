//! Damage of every WEAPON record at a quality for a wielder with all skills/attributes at a level:
//! `cargo run -p kenshi-combat --example weapons -- <install> [quality 0..1] [level]`
use kenshi_combat::weapon::{Weapon, set_weapon};
use kenshi_combat::{CombatConstants, blunt_damage, cut_damage, pierce_damage};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let q: f32 = a.get(1).map_or(0.5, |x| x.parse().unwrap());
    let lvl: f32 = a.get(2).map_or(50.0, |x| x.parse().unwrap());
    let gc = c.get("110-gamedata.quack"); // GLOBAL CONSTANTS
    let g = |k: &str| gc.and_then(|r| r.floats.get(k).copied());
    let cc = CombatConstants::from_floats(g);
    let mut recs: Vec<_> = c.of_type(ItemType::Weapon).collect();
    recs.sort_by(|x, y| x.name.cmp(&y.name));
    for r in recs {
        let w = Weapon::from_record(
            r,
            None,
            None,
            q,
            g("weapon inventory weight mult").unwrap_or(0.5),
        );
        let s = set_weapon(
            Some((&w, r)),
            g("blunt damage 1").unwrap_or(20.0),
            g("cut damage 1").unwrap_or(20.0),
        );
        let at = s.attacker(lvl, (lvl, 1.0), (lvl, 1.0), 1.0);
        println!(
            "{:28} {:?} cut {:5.1} blunt {:5.1} pierce {:5.1} weight {:4.1}",
            r.name,
            s.category,
            cut_damage(&at, &cc),
            blunt_damage(&at, &cc),
            pierce_damage(&at, &cc),
            w.combat_weight
        );
    }
}
