//! Dump every record of a mod file (what a workshop mod adds or changes):
//! `cargo run -p kenshi-io --example mod_dump -- <file.mod> [max]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let f = kenshi_io::fcs::read(std::path::Path::new(&a[0])).expect("fcs file");
    let max: usize = a.get(1).map_or(usize::MAX, |v| v.parse().unwrap());
    println!("{} records", f.records.len());
    for r in f.records.iter().take(max) {
        println!("record kind {} {:?} {:?} id {}", r.kind, r.name, r.string_id, r.id);
        let show = |n: &str, s: String| {
            if s != "{}" {
                println!("  {n} {s}");
            }
        };
        show("bools", format!("{:?}", r.bools));
        show("ints", format!("{:?}", r.ints));
        show("floats", format!("{:?}", r.floats));
        show("strings", format!("{:?}", r.strings));
        let refs: Vec<_> = r.references.iter().map(|(c, v)| (c, v.iter().map(|x| (x.target.clone(), x.values)).collect::<Vec<_>>())).collect();
        if !refs.is_empty() {
            println!("  refs {refs:?}");
        }
        if !r.instances.is_empty() {
            println!("  instances {}", r.instances.len());
        }
    }
}
