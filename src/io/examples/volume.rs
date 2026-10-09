//! A mesh's bounds and signed volume (positive: outward-facing triangles, CCW front faces):
//! `cargo run -p kenshi-io --example volume -- <file.mesh>`
use kenshi_io::ogre_mesh;

fn main() {
    let path = std::env::args().nth(1).expect("mesh");
    let m = ogre_mesh::read(std::path::Path::new(&path)).expect("read");
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut vol = 0.0f64;
    let mut tris = 0;
    for sub in &m.submeshes {
        let Some(geo) = sub.geometry.as_ref().or(m.shared.as_ref()) else { continue };
        let Some(p) = geo.positions() else { continue };
        for v in &p {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        for t in sub.indices.chunks(3) {
            let [a, b, c] = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            vol += (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) as f64 / 6.0;
            // the face normal's y for triangles high up (above 90% of the bounds)
            let n_y = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
            let cy = (a[1] + b[1] + c[1]) / 3.0;
            if std::env::var("TOP").is_ok() && cy > 60.0 {
                println!("top tri at y {cy:.1} normal y {}", if n_y > 0.0 { "up" } else { "down" });
            }
            tris += 1;
        }
    }
    println!("{tris} triangles, bounds {lo:?} .. {hi:?}, signed volume {vol:.0}");
}
