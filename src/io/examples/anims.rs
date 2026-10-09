//! List a skeleton's animations: `cargo run -p kenshi-io --example anims -- <file.skeleton>`
fn main() {
    let p = std::env::args().nth(1).expect("skeleton path");
    let s = kenshi_io::ogre_skeleton::read(std::path::Path::new(&p)).unwrap();
    for a in &s.animations {
        println!("{:7.2}s {:3} tracks  {}", a.length, a.tracks.len(), a.name);
    }
}
