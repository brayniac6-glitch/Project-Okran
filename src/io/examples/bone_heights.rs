//! A skeleton's bones in the bind pose, in model space (metres as stored):
//! `cargo run -p kenshi-io --example bone_heights -- <file.skeleton> [name filter]`
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let u = [x, y, z];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v).map(|c| c * 2.0);
    let c2 = cross(u, t);
    [v[0] + w * t[0] + c2[0], v[1] + w * t[1] + c2[1], v[2] + w * t[2] + c2[2]]
}

fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let s = kenshi_io::ogre_skeleton::read(std::path::Path::new(&a[0])).unwrap();
    let filter = a.get(1).cloned().unwrap_or_default();
    let world = |h: u16| {
        let mut chain = vec![];
        let mut cur = s.bone(h);
        while let Some(b) = cur {
            chain.push(b);
            cur = b.parent.and_then(|p| s.bone(p));
        }
        let (mut pos, mut rot) = ([0.0f32; 3], [0.0, 0.0, 0.0, 1.0]);
        for b in chain.iter().rev() {
            let p = rotate(rot, b.position);
            pos = [pos[0] + p[0], pos[1] + p[1], pos[2] + p[2]];
            rot = mul(rot, b.orientation);
        }
        pos
    };
    for b in &s.bones {
        if b.name.contains(&filter) {
            let p = world(b.handle);
            println!("{:<28} {:8.3} {:8.3} {:8.3}", b.name, p[0], p[1], p[2]);
        }
    }
}
