//! A building record's door references and instances:
//! `cargo run -p kenshi-world --example doors -- <install> <building name>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let b = data
        .records
        .values()
        .find(|r| r.name == a[1])
        .expect("building");
    println!("doors {:?}", b.refs("doors"));
    for i in &b.instances {
        let t = data
            .get(&i.target)
            .map_or("?".to_string(), |t| t.name.clone());
        println!(
            "instance {t} ({}) at {:?} rot {:?}",
            i.target, i.position, i.rotation
        );
    }
    for (sid, _) in b.refs("doors") {
        if let Some(d) = data.get(sid) {
            println!(
                "door {} ints {:?} floats {:?} bools {:?}",
                d.name, d.ints, d.floats, d.bools
            );
            for i in &d.instances {
                println!("  door instance {} at {:?}", i.target, i.position);
            }
        }
    }
}
