//! A mesh's submeshes as the renderer converts them: vertices, indices and bounds:
//! `cargo run --release -p kenshi-render --example mesh_info -- <mesh file>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = kenshi_io::ogre_mesh::read(std::path::Path::new(&a[0])).expect("mesh");
    println!("submeshes {}, shared vertices {:?}", m.submeshes.len(), m.shared.as_ref().map(|g| g.positions().map(|p| p.len())));
    for (name, mesh) in kenshi_render::to_bevy(&m) {
        let n = mesh.count_vertices();
        let aabb = bevy::camera::primitives::MeshAabb::compute_aabb(&mesh);
        println!("  {name:?}: {n} vertices, indices {:?}, aabb {:?}", mesh.indices().map(|i| i.len()), aabb.map(|b| (b.center, b.half_extents)));
    }
}
