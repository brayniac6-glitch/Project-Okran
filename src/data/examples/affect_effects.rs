//! The EFFECT records that hurt ("affect type" WeatherAffecting: 1 dust, 2 acid, 3 burning,
//! 4 gas), their kind of spawning ("type" EffectType) and radius, and the WEATHERs and BIOMES
//! that use them: `cargo run -p kenshi-data --example affect_effects -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for e in c.of_type(ItemType::Effect) {
        let t = e.ints.get("affect type").copied().unwrap_or(0);
        if t == 0 {
            continue;
        }
        let users: Vec<String> = c
            .records
            .values()
            .filter(|r| r.refs("effects").iter().chain(r.refs("ground effects")).any(|(s, _)| s == &e.string_id))
            .map(|r| format!("{} [{:?}] {:?}", r.name, r.item_type().map(|t| t.name()), r.refs("effects").iter().find(|(s, _)| s == &e.string_id).map(|x| x.1)))
            .collect();
        println!(
            "{} {:?}: affect {t}, type {:?}, radius {:?}, strength {:?}..{:?}, ttl {:?}..{:?}\n    used by {users:?}",
            e.string_id,
            e.name,
            e.ints.get("type"),
            e.floats.get("effect radius"),
            e.floats.get("effect strength min"),
            e.floats.get("effect strength max"),
            e.floats.get("min time to live"),
            e.floats.get("max time to live"),
        );
    }
}
