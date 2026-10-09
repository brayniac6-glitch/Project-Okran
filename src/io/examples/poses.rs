//! List a mesh's poses (morph targets): `cargo run -p kenshi-io --example poses -- <file.mesh>`
fn main() {
    let p = std::env::args().nth(1).expect("mesh path");
    let m = kenshi_io::ogre_mesh::read(std::path::Path::new(&p)).unwrap();
    for pose in &m.poses {
        let max = pose
            .offsets
            .iter()
            .map(|(_, d)| (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
            .fold(0.0f32, f32::max);
        println!(
            "{:24} target {} {:5} vertices, max offset {max:.2}",
            pose.name,
            pose.target,
            pose.offsets.len()
        );
    }
}
