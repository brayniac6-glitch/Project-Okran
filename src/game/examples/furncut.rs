//! The navmesh cut round a zone's furniture (kenshi_game::furniture_cut), then a path asked
//! between two points (units): `cargo run --release -p kenshi-game --example furncut -- <install>
//! X Y x,y,z x,y,z`
use kenshi_game::sim::Sim;
use kenshi_world::navmesh::{METRES, PathResult};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let p = |s: &str| -> [f32; 3] {
        let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        [v[0] * METRES, v[1] * METRES, v[2] * METRES]
    };
    let (s, g) = (p(&a[3]), p(&a[4]));
    let mut sim = Sim::load(root, zx, zy, 0, &[]);
    let res = kenshi_io::resources::Resources::index(root);
    let probe = |sim: &Sim, label: &str| {
        let fs = sim.nav.closest_point(s, 5.0);
        let fg = sim.nav.closest_point(g, 5.0);
        let reach = match (fs, fg) {
            (Some((f, _)), Some((t, _))) => sim.nav.reachable(f, t),
            _ => false,
        };
        let r = sim.nav.find_path(s, g, 0.4, 3.0);
        let what = match &r {
            PathResult::Found(f) => format!("found {} points", f.points.len()),
            other => format!("{other:?}"),
        };
        let thin = match sim.nav.find_path(s, g, 0.05, 3.0) {
            PathResult::Found(f) => format!("found {} points", f.points.len()),
            other => format!("{other:?}"),
        };
        println!("{label}: start {fs:?} goal {fg:?} reachable {reach}: {what}; radius 0.05: {thin}");
        if let (Some((f, _)), Some((t, _))) = (fs, fg) {
            println!("  flood from start {:?}, from goal {:?}", sim.nav.flood(f, 100000), sim.nav.flood(t, 100000));
        }
        println!("  cutters at start {:?} goal {:?}", sim.nav.cutters_at(s[0], s[2]), sim.nav.cutters_at(g[0], g[2]));
    };
    probe(&sim, "before");
    let mut next = 1_000_000;
    let only = std::env::var("ONLY_CAGES").is_ok();
    let (n, freed) = kenshi_game::furniture_cut::cut_furniture(&mut sim, &res, root, &mut next, only);
    println!("{n} furniture cut, {freed} left uncut");
    probe(&sim, "after");
    // the overlay's edges with no link that lie along another of its edges (they should link)
    if let Some((f, _)) = sim.nav.closest_point(s, 5.0) {
        let sec = &sim.nav.sections[&f.section];
        let (mut unlinked, mut along) = (0, 0);
        for (k, e) in sec.edges.iter().enumerate() {
            if sec.links.contains_key(&(k as u32)) {
                continue;
            }
            unlinked += 1;
            let (a, b) = (sec.vertices[e.a as usize], sec.vertices[e.b as usize]);
            let hit = sec.edges.iter().enumerate().any(|(o, x)| {
                if o == k {
                    return false;
                }
                let (c, d) = (sec.vertices[x.a as usize], sec.vertices[x.b as usize]);
                let same = |p: [f32; 3], q: [f32; 3]| (p[0] - q[0]).abs() < 0.02 && (p[2] - q[2]).abs() < 0.02;
                (same(a, d) && same(b, c)) || (same(a, c) && same(b, d))
            });
            if hit {
                along += 1;
                if along < 4 {
                    println!("  unlinked twin edge {k}: {a:?} -> {b:?}");
                }
            }
        }
        println!("overlay {}: {} edges, {unlinked} unlinked, {along} of them with a twin", f.section, sec.edges.len());
    }
    // each cut face of the start's building: its pieces (the overlay's clusters are its cut
    // faces in order)
    if let Some((f, _)) = sim.nav.closest_point(s, 5.0) {
        let ov = &sim.nav.sections[&f.section];
        let base = f.section - (1i64 << 40);
        if let Some(b) = sim.nav.sections.get(&base) {
            let cut: Vec<u32> = (0..b.faces.len() as u32).filter(|&i| b.faces[i as usize].kind == kenshi_world::navmesh::FACE_BLOCKED).collect();
            let area = |c: &[[f32; 3]]| (0..c.len()).map(|i| { let (p, q) = (c[i], c[(i + 1) % c.len()]); p[0] * q[2] - q[0] * p[2] }).sum::<f32>().abs() * 0.5;
            for (k, &fi) in cut.iter().enumerate() {
                let c: Vec<[f32; 3]> = b.corners(fi).collect();
                let near = c.iter().any(|v| (v[0] - s[0]).hypot(v[2] - s[2]) < 4.0);
                if !near {
                    continue;
                }
                let pieces: Vec<u32> = (0..ov.faces.len() as u32).filter(|&j| ov.faces[j as usize].cluster == k as i32).collect();
                let pa: f32 = pieces.iter().map(|&j| area(&ov.corners(j).collect::<Vec<_>>())).sum();
                let ys: Vec<f32> = pieces.iter().flat_map(|&j| ov.corners(j).map(|v| v[1]).collect::<Vec<_>>()).collect();
                println!("cut face {fi}: area {:.2} corners {} -> {} pieces area {pa:.2} y {:?}..{:?} (face y {:.2})", area(&c), c.len(), pieces.len(), ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max), c[0][1]);
            }
        }
    }
    // FURNCUT_DUMP=<file>: the open faces and cutters within 12 m of the start, for plotting
    if let Ok(out) = std::env::var("FURNCUT_DUMP") {
        use std::io::Write;
        let mut w = std::fs::File::create(out).unwrap();
        let reach = sim.nav.closest_point(s, 5.0).map(|(f, _)| {
            let mut seen = std::collections::HashSet::from([f]);
            let mut st = vec![f];
            while let Some(g) = st.pop() {
                let face = sim.nav.sections[&g.section].faces[g.face as usize];
                for e in face.start_edge..face.start_edge + face.num_edges {
                    for (n, _) in sim.nav.across(g, e) {
                        if seen.len() < 100000 && seen.insert(n) {
                            st.push(n);
                        }
                    }
                }
            }
            seen
        }).unwrap_or_default();
        for (f, c) in sim.nav.open_faces() {
            if c.iter().any(|v| (v[0] - s[0]).hypot(v[2] - s[2]) < 12.0 && (v[1] - s[1]).abs() < 3.0) {
                let pts: Vec<String> = c.iter().map(|v| format!("{:.3},{:.3}", v[0], v[2])).collect();
                writeln!(w, "F {} {} {}", f.section, u8::from(reach.contains(&f)), pts.join(" ")).unwrap();
            }
        }
        // the blocked (cut) faces
        for sec in sim.nav.sections.values() {
            for fi in 0..sec.faces.len() as u32 {
                if sec.faces[fi as usize].kind != kenshi_world::navmesh::FACE_BLOCKED {
                    continue;
                }
                let c: Vec<[f32; 3]> = sec.corners(fi).collect();
                if c.iter().any(|v| (v[0] - s[0]).hypot(v[2] - s[2]) < 12.0 && (v[1] - s[1]).abs() < 3.0) {
                    let pts: Vec<String> = c.iter().map(|v| format!("{:.3},{:.3}", v[0], v[2])).collect();
                    writeln!(w, "B {} {}", sec.uid, pts.join(" ")).unwrap();
                }
            }
        }
        for (o, lo, hi, furn) in sim.nav.cutters_around(s[0], s[2], 12.0) {
            let pts: Vec<String> = o.iter().map(|v| format!("{:.3},{:.3}", v[0], v[1])).collect();
            writeln!(w, "C {lo:.2} {hi:.2} {} {}", u8::from(furn), pts.join(" ")).unwrap();
        }
        writeln!(w, "S {:.3},{:.3} {:.3},{:.3}", s[0], s[2], g[0], g[2]).unwrap();
    }
    if let Some((f, _)) = sim.nav.closest_point(s, 5.0) {
        let sec = &sim.nav.sections[&f.section];
        let face = sec.faces[f.face as usize];
        println!("start piece {f:?} kind {} cluster {}", face.kind, face.cluster);
        for e in face.start_edge..face.start_edge + face.num_edges {
            let x = sec.edges[e as usize];
            let ac = sim.nav.across(f, e);
            println!("  edge {e}: {:?} -> {:?} links {:?} across {:?}", sec.vertices[x.a as usize], sec.vertices[x.b as usize], sec.links.get(&e), ac.iter().map(|(g, _)| *g).collect::<Vec<_>>());
        }
    }
}
