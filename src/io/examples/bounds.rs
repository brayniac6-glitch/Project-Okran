//! Bounds and skeleton link of meshes: `cargo run -p kenshi-io --example bounds -- <file.mesh>...`
fn main() {
    for p in std::env::args().skip(1) {
        let m = kenshi_io::ogre_mesh::read(std::path::Path::new(&p)).unwrap();
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        let geos = m
            .shared
            .iter()
            .chain(m.submeshes.iter().filter_map(|s| s.geometry.as_ref()));
        for v in geos.filter_map(|g| g.positions()).flatten() {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
        }
        let assigned = m.bone_assignments.len()
            + m.submeshes
                .iter()
                .map(|s| s.bone_assignments.len())
                .sum::<usize>();
        println!(
            "{p}: {lo:?}..{hi:?} skel={:?} bone_assignments={assigned}",
            m.skeleton
        );
    }
}
