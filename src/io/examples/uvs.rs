//! UV ranges of a mesh split by height: `cargo run -p kenshi-io --example uvs -- <file.mesh> <y split>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = kenshi_io::ogre_mesh::read(std::path::Path::new(&a[0])).unwrap();
    let split: f32 = a[1].parse().unwrap();
    for s in &m.submeshes {
        let g = if s.use_shared {
            m.shared.as_ref()
        } else {
            s.geometry.as_ref()
        }
        .unwrap();
        let (p, uv) = (g.positions().unwrap(), g.uvs(0).unwrap());
        for (label, above) in [("above", true), ("below", false)] {
            let sel: Vec<_> = p
                .iter()
                .zip(&uv)
                .filter(|(p, _)| (p[1] > split) == above)
                .map(|(_, u)| *u)
                .collect();
            let r = |i: usize| {
                sel.iter()
                    .map(|u| u[i])
                    .fold((f32::MAX, f32::MIN), |a, x| (a.0.min(x), a.1.max(x)))
            };
            println!(
                "{label} y={split}: {} verts, u {:?} v {:?}",
                sel.len(),
                r(0),
                r(1)
            );
        }
        let elems: Vec<_> = g.elements.iter().filter(|e| e.semantic == 7).collect();
        println!("uv sets: {}", elems.len());
    }
}
