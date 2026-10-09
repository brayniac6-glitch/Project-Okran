//! Parse and write back FCS files, comparing the bytes:
//! `cargo run --release -p kenshi-io --example fcs_roundtrip -- <file or dir>...`
fn walk(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if p.is_dir() {
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            walk(&e.path(), out);
        }
    } else if p.extension().is_some_and(|e| ["mod", "base", "save", "platoon", "zone", "bod2", "level"].contains(&e.to_string_lossy().as_ref())) {
        out.push(p.to_path_buf());
    }
}

fn main() {
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        walk(std::path::Path::new(&a), &mut files);
    }
    let (mut same, mut differ, mut unread) = (0, 0, 0);
    for f in &files {
        let Ok(bytes) = std::fs::read(f) else { continue };
        match kenshi_io::fcs::parse(&bytes) {
            Ok(file) => {
                let back = kenshi_io::fcs::write(&file);
                if back == bytes {
                    same += 1;
                } else {
                    differ += 1;
                    let at = back.iter().zip(&bytes).position(|(a, b)| a != b).unwrap_or(back.len().min(bytes.len()));
                    if differ <= 5 {
                        println!("differs: {} at byte {at} (sizes {} / {})", f.display(), back.len(), bytes.len());
                    }
                }
            }
            Err(e) => {
                unread += 1;
                if unread <= 15 {
                    println!("unread: {} ({e})", f.display());
                }
            }
        }
    }
    println!("{} files: {same} identical, {differ} differ, {unread} not parsed", files.len());
}
