//! meshdump <file.mesh|dir>... [-v] [-m] : parse Ogre meshes; -v summary per file, -m submesh materials.

use kenshi_io::ogre_mesh;
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
        .is_some_and(|e| e.eq_ignore_ascii_case("mesh"))
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
    let (mut failed, mut verts, mut tris) = (0, 0u64, 0u64);
    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    for p in &files {
        match ogre_mesh::read(p) {
            Ok(m) => {
                *versions.entry(m.version.clone()).or_default() += 1;
                let v: u32 = m
                    .shared
                    .iter()
                    .chain(m.submeshes.iter().filter_map(|s| s.geometry.as_ref()))
                    .map(|g| g.vertex_count)
                    .sum();
                let t: usize = m.submeshes.iter().map(|s| s.indices.len() / 3).sum();
                verts += v as u64;
                tris += t as u64;
                if args.iter().any(|a| a == "-b") {
                    println!("  bounds {:?}", m.bounds);
                }
                if args.iter().any(|a| a == "-e") {
                    for g in m.shared.iter().chain(m.submeshes.iter().filter_map(|s| s.geometry.as_ref())) {
                        for e in &g.elements {
                            println!("  element source {} semantic {} kind {} offset {} index {}", e.source, e.semantic, e.kind, e.offset, e.index);
                        }
                        if let Some(uv) = g.uvs(0) {
                            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                            for t in &uv {
                                for k in 0..2 {
                                    lo[k] = lo[k].min(t[k]);
                                    hi[k] = hi[k].max(t[k]);
                                }
                            }
                            println!("  uv {lo:?}..{hi:?} first {:?}", &uv[..4.min(uv.len())]);
                        }
                    }
                }
                if args.iter().any(|a| a == "-m") {
                    for s in &m.submeshes {
                        println!(
                            "  material {:?} tex_aliases={:?}",
                            s.material, s.texture_aliases
                        );
                    }
                }
                if verbose {
                    println!(
                        "ok   {} {} sub={} v={v} t={t} skel={:?}",
                        p.display(),
                        m.version,
                        m.submeshes.len(),
                        m.skeleton
                    );
                }
            }
            Err(e) => {
                failed += 1;
                let key: String = e
                    .to_string()
                    .split(" (")
                    .next()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_ascii_digit())
                    .collect();
                *errors.entry(key).or_default() += 1;
                println!("FAIL {}  {e}", p.display());
            }
        }
    }
    println!(
        "{} meshes, {failed} failed, {verts} vertices, {tris} triangles",
        files.len()
    );
    for (v, n) in versions {
        println!("  {v} {n}");
    }
    for (e, n) in errors {
        println!("  error kind x{n}: {e}");
    }
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
