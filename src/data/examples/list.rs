//! List records of one type with their reference categories:
//! `cargo run -p kenshi-data --example list -- <install> <type id> [n]`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    let n: usize = a.get(2).map_or(10, |x| x.parse().unwrap());
    let mut recs: Vec<_> = c.records.values().filter(|r| r.kind == kind).collect();
    recs.sort_by(|x, y| x.name.cmp(&y.name));
    for r in recs.into_iter().take(n) {
        let refs: Vec<String> = r
            .references
            .iter()
            .map(|(cat, v)| {
                let names: Vec<&str> = v
                    .iter()
                    .filter_map(|(t, _)| c.get(t).map(|x| x.name.as_str()))
                    .take(3)
                    .collect();
                format!("{cat}={names:?}{}", if v.len() > 3 { "..." } else { "" })
            })
            .collect();
        println!("{} ({}): {}", r.name, r.string_id, refs.join("; "));
    }
}
