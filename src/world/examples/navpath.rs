//! A path over the shipped navmesh, drawn top-down:
//! `cargo run -p kenshi-world --example navpath -- <install> X Y <start fx,fz> <goal fx,fz> [out.png] [water cost] [view fx,fz,size]`
//! loads the tiles round tile X.Y; start and goal are fractions (0..1) of that 3 x 3 tile block.
//! Faces are shaded by type (grey ground, blue water, red blocked, yellow costly), the path white.
use kenshi_world::navmesh::{FACE_BLOCKED, FACE_COSTLY, FACE_WATER, METRES, NavWorld, PathResult};
use kenshi_world::{WORLD_SIZE, ZONE_SIZE};
use std::path::Path;

fn frac(s: &str) -> (f32, f32) {
    let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
    (v[0], v[1])
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (tx, ty): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let world = NavWorld::load_around(install, tx, ty, 1).unwrap();
    let size = ZONE_SIZE * METRES;
    let x0 = (tx - 1) as f32 * size - WORLD_SIZE * METRES / 2.0;
    let z0 = (ty - 1) as f32 * size - WORLD_SIZE * METRES / 2.0;
    let span = 3.0 * size;
    let at = |(fx, fz): (f32, f32)| [x0 + fx * span, 2000.0, z0 + fz * span];
    let (mut s, mut g) = (at(frac(&a[3])), at(frac(&a[4])));
    // drop the points onto the mesh from above
    for p in [&mut s, &mut g] {
        if let Some((_, q)) = world.closest_point([p[0], 0.0, p[2]], 2000.0) {
            *p = q;
        }
    }
    let faces: usize = world.sections.values().map(|t| t.faces.len()).sum();
    println!("{} sections, {faces} faces", world.sections.len());
    let t = std::time::Instant::now();
    let water: f32 = a.get(6).map_or(1.0, |w| w.parse().unwrap());
    let r = world.find_path(s, g, 0.4, water);
    println!(
        "{:?} in {:.1} ms",
        std::mem::discriminant(&r),
        t.elapsed().as_secs_f64() * 1e3
    );
    let path = match &r {
        PathResult::Found(found) => {
            let p = &found.points;
            let len: f32 = p
                .windows(2)
                .map(|w| kenshi_world::navmesh::distance(w[0], w[1]))
                .sum();
            println!(
                "{} points, {:.0} m (straight {:.0} m)",
                p.len(),
                len,
                kenshi_world::navmesh::distance(s, g)
            );
            p.clone()
        }
        other => {
            println!("{other:?}");
            vec![]
        }
    };
    let n = 900u32;
    // the drawn square: the whole block, or `view fx,fz,size` (fractions of the block)
    let (vx, vz, vs) = a.get(7).map_or((0.0, 0.0, 1.0), |v| {
        let v: Vec<f32> = v.split(',').map(|x| x.parse().unwrap()).collect();
        (v[0], v[1], v[2])
    });
    let (vx0, vz0, vspan) = (x0 + vx * span, z0 + vz * span, vs * span);
    let px = |p: [f32; 3]| {
        (
            (p[0] - vx0) / vspan * n as f32,
            (p[2] - vz0) / vspan * n as f32,
        )
    };
    let mut img = image::RgbImage::from_pixel(n, n, image::Rgb([20, 20, 24]));
    for tile in world.sections.values() {
        for (fi, f) in tile.faces.iter().enumerate() {
            let c: Vec<(f32, f32)> = tile.corners(fi as u32).map(px).collect();
            let col = match f.kind {
                FACE_WATER => [40, 80, 170],
                FACE_BLOCKED => [170, 40, 40],
                FACE_COSTLY => [190, 170, 40],
                _ => [110, 105, 95],
            };
            let (minx, maxx) = c
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.0), m.1.max(p.0)));
            let (minz, maxz) = c
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.1), m.1.max(p.1)));
            for y in (minz.max(0.0) as u32)..=(maxz.min(n as f32 - 1.0) as u32) {
                for x in (minx.max(0.0) as u32)..=(maxx.min(n as f32 - 1.0) as u32) {
                    let (qx, qy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let s: Vec<f32> = (0..c.len())
                        .map(|i| {
                            let (a, b) = (c[i], c[(i + 1) % c.len()]);
                            (b.0 - a.0) * (qy - a.1) - (b.1 - a.1) * (qx - a.0)
                        })
                        .collect();
                    if s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0) {
                        img.put_pixel(x, y, image::Rgb(col));
                    }
                }
            }
        }
    }
    let mut line = |a: (f32, f32), b: (f32, f32), col: [u8; 3]| {
        let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) as usize).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (x, y) = (x as i32 + dx, y as i32 + dy);
                if x >= 0 && y >= 0 && (x as u32) < n && (y as u32) < n {
                    img.put_pixel(x as u32, y as u32, image::Rgb(col));
                }
            }
        }
    };
    for w in path.windows(2) {
        line(px(w[0]), px(w[1]), [255, 255, 255]);
    }
    for (p, col) in [(s, [80, 230, 80]), (g, [230, 80, 230])] {
        let c = px(p);
        line((c.0 - 6.0, c.1), (c.0 + 6.0, c.1), col);
        line((c.0, c.1 - 6.0), (c.0, c.1 + 6.0), col);
    }
    let out = a.get(5).cloned().unwrap_or("navpath.png".into());
    img.save(&out).unwrap();
    println!("wrote {out}");
}
