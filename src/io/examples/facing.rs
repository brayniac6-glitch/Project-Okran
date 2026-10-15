//! Which way a mesh's level faces point, by height band: for each submesh, the area of
//! triangles whose winding normal points up or down (|n.y| > 0.7), grouped by the triangle's
//! height (rounded to 5): `cargo run -p kenshi-io --example facing -- <file.mesh>...`
fn main() {
    for p in std::env::args().skip(1) {
        let m = kenshi_io::ogre_mesh::read(std::path::Path::new(&p)).unwrap();
        println!("{p}");
        for (k, s) in m.submeshes.iter().enumerate() {
            let Some(g) = s.geometry.as_ref().or(m.shared.as_ref()) else { continue };
            let Some(pos) = g.positions() else { continue };
            let mut bands: std::collections::BTreeMap<i32, (f32, f32)> = Default::default();
            for t in s.indices.chunks(3) {
                let [a, b, c] = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if len == 0.0 || (n[1] / len).abs() < 0.7 {
                    continue;
                }
                let y = ((a[1] + b[1] + c[1]) / 3.0 / 5.0).round() as i32 * 5;
                let e = bands.entry(y).or_default();
                if n[1] > 0.0 {
                    e.0 += len * 0.5;
                } else {
                    e.1 += len * 0.5;
                }
            }
            println!("  submesh {k} {}: {} tris", s.material, s.indices.len() / 3);
            for (y, (up, down)) in bands {
                println!("    y {y:>4}: up {up:>8.0} down {down:>8.0}");
            }
        }
    }
}
