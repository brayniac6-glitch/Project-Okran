//! Resistances of every ARMOUR record at a quality, as the armour constructor computes them:
//! `cargo run -p kenshi-combat --example armour -- <install> [quality 0..1] [name filter]`
use kenshi_combat::hit::Armour;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let q: f32 = a.get(1).map_or(0.5, |x| x.parse().unwrap());
    let filter = a.get(2).map_or("", String::as_str);
    let mut recs: Vec<_> = c
        .of_type(ItemType::Armour)
        .filter(|r| r.name.contains(filter))
        .collect();
    recs.sort_by(|x, y| x.name.cmp(&y.name));
    for r in recs {
        let part = |sid: &str| c.get(sid).map(|p| p.name.len()); // stand-in index: shows coverage
        match Armour::from_record(r, q, part) {
            Some(x) => {
                let mut cov: Vec<String> = r
                    .refs("part coverage")
                    .iter()
                    .map(|(sid, v)| {
                        format!("{} {}%", c.get(sid).map_or("?", |p| p.name.as_str()), v[0])
                    })
                    .collect();
                cov.sort();
                println!(
                    "{:40} cut {:.2} blunt {:.2} pierce {:5.1} stun {:.2}  [{}]",
                    r.name,
                    x.cut_resistance,
                    x.blunt_resistance,
                    x.pierce_reduction,
                    x.cut_into_stun,
                    cov.join(", ")
                );
            }
            None => println!("{:40} (no table row)", r.name),
        }
    }
}
