//! skeldump <file.skeleton|dir>... [-v] : parse Ogre skeletons; summary per file with -v.

use kenshi_io::ogre_skeleton;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

fn collect(p: PathBuf, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(&p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        v.sort();
        for e in v {
            collect(e, out);
        }
    } else if p
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("skeleton"))
    {
        out.push(p);
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "-v");
    let mut files = Vec::new();
    for a in args.iter().filter(|a| !a.starts_with('-')) {
        collect(PathBuf::from(a), &mut files);
    }
    let (mut failed, mut bones, mut anims, mut keys, mut scaled) = (0, 0, 0, 0, 0);
    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    for p in &files {
        match ogre_skeleton::read(p) {
            Ok(s) => {
                *versions.entry(s.version.clone()).or_default() += 1;
                bones += s.bones.len();
                anims += s.animations.len();
                scaled += s.bones.iter().filter(|b| b.scale.is_some()).count();
                keys += s
                    .animations
                    .iter()
                    .flat_map(|a| &a.tracks)
                    .map(|t| t.keyframes.len())
                    .sum::<usize>();
                if verbose {
                    println!(
                        "ok   {} {} bones={} anims={} links={:?}",
                        p.display(),
                        s.version,
                        s.bones.len(),
                        s.animations.len(),
                        s.links
                    );
                }
            }
            Err(e) => {
                failed += 1;
                println!("FAIL {}  {e}", p.display());
            }
        }
    }
    println!(
        "{} skeletons, {failed} failed, {bones} bones ({scaled} with scale), {anims} animations, {keys} keyframes",
        files.len()
    );
    for (v, n) in versions {
        println!("  {v} {n}");
    }
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
