//! Every COMBAT_TECHNIQUE record as the technique loader reads it:
//! `cargo run -p kenshi-combat --example techniques -- <install>`
use kenshi_combat::technique::CombatTechnique;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let mut recs: Vec<_> = c.of_type(ItemType::CombatTechnique).collect();
    recs.sort_by(|x, y| x.name.cmp(&y.name));
    for r in recs {
        let t = CombatTechnique::from_record(r);
        let kind = if t.is_dodge {
            "dodge"
        } else if t.is_block {
            "block"
        } else {
            "attack"
        };
        let frames: Vec<String> = t
            .frames
            .iter()
            .map(|f| {
                format!(
                    "dir {} pow {:.2} hit@{:.2} stop@{:.2} limb {}",
                    f.direction, f.power, f.impact, f.stop, f.limb
                )
            })
            .collect();
        println!(
            "{:32} {kind:6} chance {} reach {} skill {}..{} {:?} [{}]",
            r.name,
            t.chance,
            t.attack_distance,
            t.min_skill,
            t.max_skill,
            t.allowed,
            frames.join("; ")
        );
    }
}
