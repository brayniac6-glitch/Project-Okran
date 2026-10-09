//! Print vertex declarations of a mesh: `cargo run -p kenshi-io --example elements -- <file.mesh>`
fn main() {
    let p = std::env::args().nth(1).expect("mesh path");
    let m = kenshi_io::ogre_mesh::read(std::path::Path::new(&p)).unwrap();
    let geos = m
        .shared
        .iter()
        .chain(m.submeshes.iter().filter_map(|s| s.geometry.as_ref()));
    for g in geos {
        println!("vertices {}", g.vertex_count);
        for e in &g.elements {
            println!("  {e:?}");
        }
        for b in &g.buffers {
            println!("  buffer bind {} stride {}", b.bind, b.stride);
        }
    }
}
