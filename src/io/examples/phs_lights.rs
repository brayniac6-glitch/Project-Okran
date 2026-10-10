//! The lights in an install's `.phs` files: `cargo run --release -p kenshi-io --example phs_lights -- <install>`
fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("phs")) {
            out.push(p);
        }
    }
}

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let mut files = Vec::new();
    walk(&root.join("data"), &mut files);
    let (mut with, mut total, mut bad) = (0, 0, 0);
    for f in &files {
        match kenshi_io::phs::read(f) {
            Ok(p) if !p.lights.is_empty() => {
                with += 1;
                total += p.lights.len();
                if with <= 12 {
                    for l in &p.lights {
                        println!("{}: {:?} pos {:?} value {} params {:?} extra {:?}", f.strip_prefix(&root).unwrap().display(), l.name, l.frame.pos, l.value, l.params, l.extra);
                    }
                }
            }
            Ok(_) => {}
            Err(_) => bad += 1,
        }
    }
    println!("{} phs files, {with} with lights ({total} lights), {bad} unread", files.len());
}
