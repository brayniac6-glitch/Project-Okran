//! The map's fog features (fogfeatures.dat) and their plane volumes' corners:
//! `cargo run -p kenshi-world --example fog_features -- <install>`
fn main() {
    let install = std::path::PathBuf::from(std::env::args().nth(1).expect("install dir"));
    for f in kenshi_world::fog_features::load(&install) {
        let shape = match &f.shape {
            kenshi_world::fog_features::FogShape::Planes { edge, planes } => {
                let (c, faces) = kenshi_world::fog_features::plane_hull(planes);
                let lo = c.iter().fold([f32::MAX; 3], |m, p| [m[0].min(p[0]), m[1].min(p[1]), m[2].min(p[2])]);
                let hi = c.iter().fold([f32::MIN; 3], |m, p| [m[0].max(p[0]), m[1].max(p[1]), m[2].max(p[2])]);
                format!("planes edge {edge}: {} corners, faces {:?}, {lo:.0?}..{hi:.0?}", c.len(), faces.iter().map(|f| f.len()).collect::<Vec<_>>())
            }
            s => format!("{s:?}"),
        };
        println!("{:40} colour {:.2?} distance {}: {shape}", f.name, f.colour, f.distance);
    }
}
