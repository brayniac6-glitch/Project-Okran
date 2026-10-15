//! Records of a type that have a reference category set, with what they list:
//! `cargo run --release -p kenshi-data --example refs_using -- <install> <type id> <category>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    let cat = &a[2];
    for r in c.records.values() {
        if r.kind != kind {
            continue;
        }
        let refs = r.refs(cat);
        if refs.is_empty() {
            continue;
        }
        let text = r.strings.get("text0").map(|t| t.chars().take(60).collect::<String>()).unwrap_or_default();
        let list: Vec<String> = refs.iter().map(|(s, v)| format!("{} {v:?}", c.get(s).map_or(s.clone(), |x| x.name.clone()))).collect();
        println!("{} [{}] {:?} -> {}", r.name, r.string_id, text, list.join("; "));
    }
}
