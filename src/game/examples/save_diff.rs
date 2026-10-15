//! Compare a Kenshi save folder with the rewrite's write of it (kenshi_game::kenshi_save): the
//! save is read, written to `out`, and the two compared file by file: for each record type, the
//! fields (bools, floats, ints, strings, references) one has and the other lacks, and the
//! header and record counts:
//! `cargo run --release -p kenshi-game --example save_diff -- <kenshi save dir> <out dir>`
//! (Read only: nothing is written into the save folder.)
use kenshi_io::fcs;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn shape(f: &fcs::File) -> BTreeMap<i32, (usize, BTreeSet<String>)> {
    let mut m: BTreeMap<i32, (usize, BTreeSet<String>)> = BTreeMap::new();
    for r in &f.records {
        let e = m.entry(r.kind).or_default();
        e.0 += 1;
        for (k, _) in &r.bools {
            e.1.insert(format!("b:{k}"));
        }
        for (k, _) in &r.floats {
            e.1.insert(format!("f:{k}"));
        }
        for (k, _) in &r.ints {
            e.1.insert(format!("i:{k}"));
        }
        for (k, _) in &r.strings {
            e.1.insert(format!("s:{k}"));
        }
        for (k, _) in &r.references {
            e.1.insert(format!("r:{k}"));
        }
        if !r.instances.is_empty() {
            e.1.insert("instances".into());
        }
    }
    m
}

fn compare(name: &str, a: &fcs::File, b: &fcs::File) {
    println!("== {name}: header type {} v{} / {} v{}, {} records / {}", a.header.filetype, a.header.version, b.header.filetype, b.header.version, a.records.len(), b.records.len());
    let (sa, sb) = (shape(a), shape(b));
    for (kind, (n, keys)) in &sa {
        match sb.get(kind) {
            None => println!("  type {kind}: {n} records in Kenshi's, none written"),
            Some((m, ours)) => {
                let missing: Vec<&String> = keys.difference(ours).collect();
                let extra: Vec<&String> = ours.difference(keys).collect();
                if !missing.is_empty() || !extra.is_empty() || n != m {
                    println!("  type {kind}: {n} / {m} records; missing {missing:?}; extra {extra:?}");
                }
            }
        }
    }
    for (kind, (m, _)) in &sb {
        if !sa.contains_key(kind) {
            println!("  type {kind}: {m} written, none in Kenshi's");
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (src, out) = (Path::new(&a[0]), Path::new(&a[1]));
    let mut s = kenshi_game::kenshi_save::read(src).expect("save");
    // OVER=1: written over the save itself (kenshi_save_over), as the rewrite saves; CHANGE=1
    // also changes it first (money, a character renamed, one more squad member) to see the
    // changes carried
    if std::env::var("CHANGE").is_ok() {
        s.money += 1234;
        if let Some(sq) = s.squads.first_mut()
            && let Some(c) = sq.characters.first().cloned()
        {
            let mut n = c.clone();
            n.name = "Okran Test".into();
            n.position[0] += 30.0;
            sq.characters.push(n);
        }
    }
    if std::env::var("OVER").is_ok() {
        kenshi_game::kenshi_save_over::write_over(src, out, &s).expect("write over");
        let back = kenshi_game::kenshi_save::read(out).expect("read back");
        let names: Vec<String> = back.squads.iter().flat_map(|q| q.characters.iter().map(|c| c.name.clone())).collect();
        println!("read back: money {} (was {}), {} squads, characters {:?}", back.money, s.money, back.squads.len(), names);
        let items: usize = back.squads.iter().flat_map(|q| &q.characters).map(|c| c.items.len()).sum();
        let items0: usize = s.squads.iter().flat_map(|q| &q.characters).map(|c| c.items.len()).sum();
        println!("items {items} (written {items0}); research {} (written {})", back.research.len(), s.research.len());
    } else {
        kenshi_game::kenshi_save::write(out, &s).expect("write");
    }
    let q = |d: &Path| fcs::read(&d.join("quick.save")).expect("quick.save");
    compare("quick.save", &q(src), &q(out));
    // the player's platoon files, by name, as both have them
    let theirs: BTreeSet<String> = std::fs::read_dir(src.join("platoon")).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    let ours: BTreeSet<String> = std::fs::read_dir(out.join("platoon")).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    println!("platoon files: {} in Kenshi's, {} written, {} in both", theirs.len(), ours.len(), theirs.intersection(&ours).count());
    // (FILE=<name>: that platoon file only)
    let only = std::env::var("FILE").ok();
    for f in theirs.intersection(&ours).filter(|f| only.as_ref().is_none_or(|o| *f == o)).take(3) {
        let (x, y) = (fcs::read(&src.join("platoon").join(f)), fcs::read(&out.join("platoon").join(f)));
        if let (Ok(x), Ok(y)) = (x, y) {
            compare(&format!("platoon/{f}"), &x, &y);
        }
    }
}
