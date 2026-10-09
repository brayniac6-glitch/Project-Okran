//! Dump the records of one kind from an FCS file (Kenshi saves):
//! `cargo run -p kenshi-io --example fcs_dump -- <file> <kind> [max]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let f = kenshi_io::fcs::read(std::path::Path::new(&a[0])).expect("fcs file");
    let kind: i32 = a[1].parse().unwrap();
    let max: usize = a.get(2).map_or(1, |v| v.parse().unwrap());
    for r in f.records.iter().filter(|r| r.kind == kind).take(max) {
        println!("record {:?} {:?} id {}", r.name, r.string_id, r.id);
        println!("  bools {:?}", r.bools);
        println!("  ints {:?}", r.ints);
        println!("  floats {:?}", r.floats);
        println!("  strings {:?}", r.strings);
        println!("  vec3 {:?}", r.vec3s);
        println!("  vec4 {:?}", r.vec4s);
        println!("  refs {:?}", r.references.iter().map(|(c, v)| (c, v.iter().map(|x| (x.target.clone(), x.values)).collect::<Vec<_>>())).collect::<Vec<_>>());
        println!("  instances {}", r.instances.len());
        for i in r.instances.iter().take(12) {
            println!("    {:?}", i);
        }
    }
}
