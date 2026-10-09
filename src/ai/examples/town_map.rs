//! Top-down time-lapse of a `town` recording: the navmesh round the town, the town nodes
//! (squares: yellow shopkeeper, white general, red guard post; filled while someone stands
//! there) and each resident with a fading trail coloured by role (blue wanderer, yellow
//! shopkeeper, red guard, green town patrol, grey other, white squad leader, orange follower, purple bar patron; dark blue ring asleep; red ring fighting; dark grey knocked out).
//! `cargo run --release -p kenshi-ai --example town_map -- <install> <town.csv> X Y out.gif [step s]`
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, Rgba, RgbaImage};
use kenshi_data::GameDataContainer;
use kenshi_io::{fcs, mods::Install};
use kenshi_world::navmesh::{METRES, NavWorld, tile_of};
use kenshi_world::nodes::{Layouts, NODE_GENERAL, nodes};
use kenshi_world::{placements, zone_file};
use std::collections::BTreeMap;
use std::path::Path;

/// By role: wanderer, shopkeeper, guard, other, town patrol, squad leader, follower, bar patron.
const COLOURS: [[u8; 3]; 8] = [
    [80, 170, 240],
    [250, 200, 60],
    [230, 80, 70],
    [150, 150, 150],
    [110, 220, 110],
    [255, 255, 255],
    [255, 140, 40],
    [200, 90, 230],
];
/// Node squares by kind: shopkeeper, general, guard post.
const NODE_COLOURS: [[u8; 3]; 3] = [[250, 200, 60], [240, 240, 240], [230, 80, 70]];

/// (id, x, z, task, role, state: 0, 1 fighting, 2 knocked out)
type Row = (usize, f32, f32, u16, usize, u8);

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let step: f32 = a.get(5).map_or(0.5, |s| s.parse().unwrap());
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let zone = fcs::read(&zone_file(install, zx, zy).unwrap()).unwrap();
    let general: Vec<_> = nodes(&data, &Layouts::load(install), &placements(&zone))
        .into_iter()
        .filter(|n| n.kind <= NODE_GENERAL + 1)
        .collect();
    // rows: t,id,x,y,z,speed,task
    let mut frames: BTreeMap<i64, Vec<Row>> = BTreeMap::new();
    for l in std::fs::read_to_string(&a[1]).unwrap().lines() {
        let c: Vec<&str> = l.split(',').collect();
        let t: f32 = c[0].parse().unwrap();
        let k = (t / step).round() as i64;
        if ((t / step) - k as f32).abs() > 0.03 / step {
            continue;
        }
        frames.entry(k).or_default().push((
            c[1].parse().unwrap(),
            c[2].parse().unwrap(),
            c[4].parse().unwrap(),
            c[6].parse().unwrap(),
            c.get(7).map_or(0, |r| r.parse().unwrap()),
            c.get(8).map_or(0, |r| r.parse().unwrap()),
        ));
    }
    // view: the nodes' extent plus a margin
    let xs = general.iter().map(|n| n.position[0]);
    let zs = general.iter().map(|n| n.position[2]);
    let (x0, x1) = xs
        .clone()
        .fold((f32::MAX, f32::MIN), |m, v| (m.0.min(v), m.1.max(v)));
    let (z0, z1) = zs
        .clone()
        .fold((f32::MAX, f32::MIN), |m, v| (m.0.min(v), m.1.max(v)));
    let span = (x1 - x0).max(z1 - z0) + 500.0;
    let (cx, cz) = ((x0 + x1) / 2.0, (z0 + z1) / 2.0);
    let n = 640u32;
    let px = |x: f32, z: f32| {
        (
            ((x - cx) / span + 0.5) * n as f32,
            ((z - cz) / span + 0.5) * n as f32,
        )
    };
    // background: navmesh faces
    let (tx, ty) = tile_of([cx * METRES, 0.0, cz * METRES]);
    let nav = NavWorld::load_around(install, tx, ty, 1).unwrap();
    let mut bg = RgbaImage::from_pixel(n, n, Rgba([22, 22, 26, 255]));
    for s in nav.sections.values() {
        for fi in 0..s.faces.len() as u32 {
            let c: Vec<(f32, f32)> = s
                .corners(fi)
                .map(|v| px(v[0] / METRES, v[2] / METRES))
                .collect();
            let (mx, xx) = c
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.0), m.1.max(p.0)));
            let (mz, xz) = c
                .iter()
                .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.1), m.1.max(p.1)));
            if xx < 0.0 || mx >= n as f32 || xz < 0.0 || mz >= n as f32 {
                continue;
            }
            let shade = if s.info.is_some() {
                [140, 120, 95]
            } else {
                [92, 88, 80]
            };
            for y in (mz.max(0.0) as u32)..=(xz.min(n as f32 - 1.0) as u32) {
                for x in (mx.max(0.0) as u32)..=(xx.min(n as f32 - 1.0) as u32) {
                    let (qx, qy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let side: Vec<f32> = (0..c.len())
                        .map(|i| {
                            let (a, b) = (c[i], c[(i + 1) % c.len()]);
                            (b.0 - a.0) * (qy - a.1) - (b.1 - a.1) * (qx - a.0)
                        })
                        .collect();
                    if side.iter().all(|&v| v >= 0.0) || side.iter().all(|&v| v <= 0.0) {
                        bg.put_pixel(x, y, Rgba([shade[0], shade[1], shade[2], 255]));
                    }
                }
            }
        }
    }
    let dot = |img: &mut RgbaImage, (x, y): (f32, f32), r: i32, col: [u8; 3], fill: bool| {
        for dy in -r..=r {
            for dx in -r..=r {
                let edge = dx.abs() == r || dy.abs() == r;
                if !fill && !edge {
                    continue;
                }
                let (qx, qy) = (x as i32 + dx, y as i32 + dy);
                if qx >= 0 && qy >= 0 && (qx as u32) < n && (qy as u32) < n {
                    img.put_pixel(qx as u32, qy as u32, Rgba([col[0], col[1], col[2], 255]));
                }
            }
        }
    };
    let file = std::fs::File::create(&a[4]).unwrap();
    let mut enc = GifEncoder::new_with_speed(file, 20);
    enc.set_repeat(Repeat::Infinite).unwrap();
    let mut trails: BTreeMap<usize, (usize, Vec<(f32, f32)>)> = BTreeMap::new();
    let count = frames.len();
    for (i, (_, people)) in frames.into_iter().enumerate() {
        let mut img = bg.clone();
        for p in &people {
            let (role, tr) = trails.entry(p.0).or_default();
            *role = p.4;
            tr.push(px(p.1, p.2));
            if tr.len() > 40 {
                tr.remove(0);
            }
        }
        for (role, tr) in trails.values() {
            let col = COLOURS[role % COLOURS.len()];
            for (k, q) in tr.iter().enumerate() {
                let f = 0.25 + 0.5 * k as f32 / tr.len() as f32;
                let c = [
                    (col[0] as f32 * f) as u8,
                    (col[1] as f32 * f) as u8,
                    (col[2] as f32 * f) as u8,
                ];
                dot(&mut img, *q, 0, c, true);
            }
        }
        let standing: Vec<(f32, f32)> = people
            .iter()
            .filter(|p| p.3 == 51)
            .map(|p| px(p.1, p.2))
            .collect();
        for g in &general {
            let q = px(g.position[0], g.position[2]);
            let taken = standing
                .iter()
                .any(|s| (s.0 - q.0).abs() < 4.0 && (s.1 - q.1).abs() < 4.0);
            dot(&mut img, q, 3, NODE_COLOURS[g.kind as usize], taken);
        }
        for p in &people {
            if p.5 == 2 {
                dot(&mut img, px(p.1, p.2), 2, [70, 64, 58], true);
                continue;
            }
            if p.5 == 1 {
                dot(&mut img, px(p.1, p.2), 4, [220, 30, 30], false);
            }
            // asleep (task 98): a dark blue ring
            let asleep = p.3 == 98;
            dot(
                &mut img,
                px(p.1, p.2),
                2,
                if asleep {
                    [40, 50, 160]
                } else {
                    COLOURS[p.4 % COLOURS.len()]
                },
                !asleep,
            );
        }
        let delay = if i + 1 == count { 1500 } else { 80 };
        enc.encode_frame(Frame::from_parts(
            img,
            0,
            0,
            Delay::from_numer_denom_ms(delay, 1),
        ))
        .unwrap();
    }
    eprintln!("{count} frames -> {}", a[4]);
}
