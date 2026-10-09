//! Count the record kinds of FCS files (game data, Kenshi saves):
//! `cargo run -p kenshi-io --example fcs_kinds -- <file>...`
fn main() {
    for a in std::env::args().skip(1) {
        let f = kenshi_io::fcs::read(std::path::Path::new(&a)).expect("fcs file");
        let mut by: std::collections::BTreeMap<i32, (usize, Vec<String>)> = Default::default();
        for r in &f.records {
            let e = by.entry(r.kind).or_default();
            e.0 += 1;
            if e.1.len() < 3 {
                e.1.push(r.name.clone());
            }
        }
        println!("{a}: {} records, header {:?}", f.records.len(), f.header);
        for (k, (n, eg)) in by {
            println!("  kind {k:3}: {n:5}  e.g. {eg:?}");
        }
    }
}
