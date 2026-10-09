//! Parses every FCS file of a real install. Needs the player's copy, so it is opt-in:
//!   KENSHI_DIR="C:/Program Files (x86)/Steam/steamapps/common/Kenshi" cargo test -p kenshi-io -- --ignored

use kenshi_io::{fcs, ogre_mesh, ogre_skeleton};
use std::path::PathBuf;

#[test]
#[ignore = "needs KENSHI_DIR pointing at a Kenshi install"]
fn parses_every_installed_fcs_file() {
    let dir = PathBuf::from(std::env::var("KENSHI_DIR").expect("set KENSHI_DIR")).join("data");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if matches!(p.extension().and_then(|x| x.to_str()), Some("mod" | "base")) {
            let f = fcs::read(&p).unwrap_or_else(|err| panic!("{}: {err}", p.display()));
            assert_eq!(f.records.len() as i32, f.header.record_count);
            n += 1;
        }
    }
    assert!(
        n >= 4,
        "expected gamedata.base and the shipped .mod files in {}",
        dir.display()
    );
}

fn files_with(ext: &str, dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_with(ext, &p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
}

#[test]
#[ignore = "needs KENSHI_DIR pointing at a Kenshi install"]
fn parses_every_installed_mesh() {
    let dir = PathBuf::from(std::env::var("KENSHI_DIR").expect("set KENSHI_DIR")).join("data");
    let mut all = Vec::new();
    files_with("mesh", &dir, &mut all);
    assert!(all.len() > 3000, "{} meshes", all.len());
    let failed: Vec<String> = all
        .iter()
        .filter_map(|p| {
            ogre_mesh::read(p)
                .err()
                .map(|e| format!("{}: {e}", p.display()))
        })
        .collect();
    assert!(
        failed.is_empty(),
        "{} failed, first: {}",
        failed.len(),
        failed[0]
    );
}

#[test]
#[ignore = "needs KENSHI_DIR pointing at a Kenshi install"]
fn parses_every_installed_skeleton() {
    let dir = PathBuf::from(std::env::var("KENSHI_DIR").expect("set KENSHI_DIR")).join("data");
    let mut all = Vec::new();
    files_with("skeleton", &dir, &mut all);
    // the game ships at least one empty .skeleton (items/armour/meshes/trousers_m.mesh.skeleton)
    all.retain(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0));
    assert!(all.len() > 300, "{} skeletons", all.len());
    let failed: Vec<String> = all
        .iter()
        .filter_map(|p| {
            ogre_skeleton::read(p)
                .err()
                .map(|e| format!("{}: {e}", p.display()))
        })
        .collect();
    assert!(
        failed.is_empty(),
        "{} failed, first: {}",
        failed.len(),
        failed[0]
    );
}
