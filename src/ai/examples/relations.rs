//! A faction's allies and enemies as the ported relation rules judge them:
//! `cargo run -p kenshi-ai --example relations -- <install> <faction name>`
use kenshi_ai::faction::FactionRelations;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let rec = c
        .of_type(ItemType::Faction)
        .find(|r| r.name == a[1])
        .expect("no such faction");
    let f = FactionRelations::from_record(rec);
    println!("{} (default relation {})", rec.name, f.default_relation);
    let mut rows: Vec<_> = c
        .of_type(ItemType::Faction)
        .filter(|o| f.relations.contains_key(&o.string_id))
        .map(|o| {
            (
                f.relations[&o.string_id].relation,
                o.name.clone(),
                o.string_id.clone(),
            )
        })
        .collect();
    rows.sort_by(|x, y| x.0.total_cmp(&y.0));
    for (rel, name, sid) in rows {
        let verdict = if f.is_enemy(Some(&sid)) {
            "enemy"
        } else if f.is_ally(Some(&sid)) {
            "ally"
        } else {
            "neutral"
        };
        println!("{rel:6} {verdict:8} {name}");
    }
}
