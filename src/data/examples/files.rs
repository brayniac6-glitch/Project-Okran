//! Print the file fields of records of one type: `cargo run -p kenshi-data --example files -- <install> <type id> [n]`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let kind: i32 = a[1].parse().unwrap();
    let n: usize = a.get(2).map_or(5, |x| x.parse().unwrap());
    let mut recs: Vec<_> = c
        .records
        .values()
        .filter(|r| r.kind == kind && !r.files.is_empty())
        .collect();
    recs.sort_by(|x, y| x.name.cmp(&y.name));
    for r in recs.into_iter().take(n) {
        let mut f: Vec<_> = r.files.iter().filter(|(_, v)| !v.is_empty()).collect();
        f.sort();
        println!("{} ({}): {:?}", r.name, r.string_id, f);
    }
}
