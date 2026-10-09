//! List a skeleton's bones: `cargo run -p kenshi-io --example bones -- <file.skeleton>`
fn main() {
    let p = std::env::args().nth(1).expect("skeleton path");
    let s = kenshi_io::ogre_skeleton::read(std::path::Path::new(&p)).unwrap();
    for b in &s.bones {
        let parent = b
            .parent
            .and_then(|h| s.bone(h))
            .map_or("-", |p| p.name.as_str());
        println!("{:3} {:<28} parent {}", b.handle, b.name, parent);
    }
}
