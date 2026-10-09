//! Print the first `n` records of each type in an FCS file, every field and instance:
//! `cargo run -p kenshi-io --example fcsrec -- <file> [n]`
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let f = kenshi_io::fcs::read(std::path::Path::new(&a[0])).unwrap();
    let n: usize = a.get(1).map_or(2, |x| x.parse().unwrap());
    let mut seen: HashMap<i32, usize> = HashMap::new();
    for r in &f.records {
        let c = seen.entry(r.kind).or_default();
        *c += 1;
        if *c > n {
            continue;
        }
        println!(
            "type {} '{}' {} lead={} datatype={:#x}",
            r.kind, r.name, r.string_id, r.lead, r.datatype
        );
        for (k, v) in &r.bools {
            println!("  bool {k} = {v}");
        }
        for (k, v) in &r.floats {
            println!("  float {k} = {v}");
        }
        for (k, v) in &r.ints {
            println!("  int {k} = {v}");
        }
        for (k, v) in &r.vec3s {
            println!("  vec3 {k} = {v:?}");
        }
        for (k, v) in &r.vec4s {
            println!("  vec4 {k} = {v:?}");
        }
        for (k, v) in &r.strings {
            println!("  string {k} = {v:?}");
        }
        for (cat, refs) in &r.references {
            for rf in refs {
                println!("  ref {cat} -> {} {:?}", rf.target, rf.values);
            }
        }
        for i in r.instances.iter().take(4) {
            println!(
                "  instance {} -> {} pos {:?} rot {:?} states {:?}",
                i.id, i.target, i.position, i.rotation, i.states
            );
        }
        if r.instances.len() > 4 {
            println!("  ... {} instances", r.instances.len());
        }
    }
}
