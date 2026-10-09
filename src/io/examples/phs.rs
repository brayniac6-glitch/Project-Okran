//! Dump a Scythe physics file: `cargo run -p kenshi-io --example phs -- <file.phs>`
fn main() {
    let p = std::env::args().nth(1).expect("phs path");
    let r = kenshi_io::phs::read(std::path::Path::new(&p)).unwrap();
    println!("version {} actors {} joints {}", r.version, r.actors.len(), r.joints.len());
    for (i, a) in r.actors.iter().enumerate() {
        println!("{i:2} {:18} bone {:18} pos {:?} damping {:?} density {}", a.name, a.bone, a.pos, a.damping, a.density);
        println!("     bone frame {:?}", a.bone_frame);
        for o in &a.offsets {
            println!("     offset {:?}", o);
        }
        for s in &a.shapes {
            println!("     {:?} size {:?} at {:?}", s.kind, s.size, s.frame.pos);
        }
    }
    for j in &r.joints {
        println!("joint {:?} {:?} limits {} .. {} swing {} at {:?} rows {:?}", j.actors, j.kind, j.low, j.high, j.swing, j.frame.pos, j.frame.rows);
    }
}
