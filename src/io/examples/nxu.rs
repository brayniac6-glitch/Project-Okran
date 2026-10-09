//! Shapes in PhysX NxuStream2 collision files: `cargo run -p kenshi-io --example nxu -- <file.xml>...`
fn main() {
    let mut kinds = std::collections::BTreeMap::new();
    for p in std::env::args().skip(1) {
        let s = kenshi_io::nxu::read(std::path::Path::new(&p)).unwrap();
        for x in &s {
            let k = match &x.kind {
                kenshi_io::nxu::ShapeKind::Box(_) => "box".to_string(),
                kenshi_io::nxu::ShapeKind::Sphere(_) => "sphere".to_string(),
                kenshi_io::nxu::ShapeKind::Capsule { .. } => "capsule".to_string(),
                kenshi_io::nxu::ShapeKind::Convex(v) => format!("convex ({} verts max)", if v.len() > 32 { ">32" } else { "<=32" }),
                kenshi_io::nxu::ShapeKind::Mesh(_, t) => format!("mesh ({} tris)", if t.len() > 500 { ">500" } else { "<=500" }),
            };
            *kinds.entry(k).or_insert(0) += 1;
        }
        if std::env::var("NXU_LIST").is_ok() {
            for x in &s {
                println!("{p}: {:?} at {:?}", match &x.kind { kenshi_io::nxu::ShapeKind::Convex(v) => format!("convex {}", v.len()), kenshi_io::nxu::ShapeKind::Mesh(v, t) => format!("mesh {} {}", v.len(), t.len()), k => format!("{k:?}") }, x.pose.pos);
            }
        }
    }
    println!("{kinds:?}");
}
