//! fcsdump <file.mod|file.base|dir>...  : parse FCS files and print a summary.
//!   --types     per record type: count and the union of field keys (for naming types from fcs.def)
//!   --records   one line per record: type, string_id, name

use kenshi_io::fcs;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::ExitCode;

fn collect(p: PathBuf, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for e in entries {
            collect(e, out);
        }
    } else if matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("mod" | "base" | "zone" | "level")
    ) {
        out.push(p);
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let types = args.iter().any(|a| a == "--types");
    let records = args.iter().any(|a| a == "--records");
    let mut files = Vec::new();
    for a in args.iter().filter(|a| !a.starts_with("--")) {
        collect(PathBuf::from(a), &mut files);
    }
    if files.is_empty() {
        eprintln!("usage: fcsdump [--types] [--records] <file.mod|file.base|dir>...");
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    let mut by_type: BTreeMap<i32, (usize, BTreeSet<String>)> = BTreeMap::new();
    for path in &files {
        match fcs::read(path) {
            Ok(f) => {
                println!(
                    "ok   {}  v{} records={} deps={:?}",
                    path.display(),
                    f.header.version,
                    f.records.len(),
                    f.header.dependencies
                );
                for r in &f.records {
                    if records {
                        println!("  {:>3} {:<28} {}", r.kind, r.string_id, r.name);
                    }
                    let e = by_type.entry(r.kind).or_default();
                    e.0 += 1;
                    let keys = r
                        .bools
                        .iter()
                        .map(|x| &x.0)
                        .chain(r.floats.iter().map(|x| &x.0))
                        .chain(r.ints.iter().map(|x| &x.0))
                        .chain(r.vec3s.iter().map(|x| &x.0))
                        .chain(r.vec4s.iter().map(|x| &x.0))
                        .chain(r.strings.iter().map(|x| &x.0))
                        .chain(r.files.iter().map(|x| &x.0))
                        .chain(r.references.iter().map(|x| &x.0));
                    e.1.extend(keys.cloned());
                }
            }
            Err(e) => {
                failed += 1;
                println!("FAIL {}  {e}", path.display());
            }
        }
    }
    if types {
        for (k, (n, keys)) in &by_type {
            println!(
                "type {k}\t{n}\t{}",
                keys.iter().cloned().collect::<Vec<_>>().join("|")
            );
        }
    }
    println!("{} files, {} failed", files.len(), failed);
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
