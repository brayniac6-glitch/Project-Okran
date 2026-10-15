//! A mesh's vertex colours per submesh (alpha zero ones cut an item's icon):
//! `cargo run -p kenshi-render --example mesh_colours -- <file.mesh>`
use kenshi_io::ogre_mesh;

fn main() {
    let path = std::env::args().nth(1).expect("mesh");
    let m = ogre_mesh::read(std::path::Path::new(&path)).expect("read");
    for (name, mesh) in kenshi_render::to_bevy(&m) {
        let n = mesh.count_vertices();
        println!("{name}: attributes {:?}", mesh.attributes().map(|(a, _)| a.name).collect::<Vec<_>>());
        println!("{name}: aabb {:?}", bevy::camera::primitives::MeshAabb::compute_aabb(&mesh));
        match mesh.attribute(bevy::prelude::Mesh::ATTRIBUTE_COLOR) {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(c)) => {
                let lo = c.iter().fold([f32::MAX; 4], |a, v| std::array::from_fn(|i| a[i].min(v[i])));
                let hi = c.iter().fold([f32::MIN; 4], |a, v| std::array::from_fn(|i| a[i].max(v[i])));
                println!("{name}: {n} vertices, colours {lo:?} .. {hi:?}");
            }
            _ => println!("{name}: {n} vertices, no colours"),
        }
    }
}
