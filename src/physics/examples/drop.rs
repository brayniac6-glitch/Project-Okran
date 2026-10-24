//! Drop a ragdoll from its bind pose onto flat ground and report how it lands:
//! `cargo run -p kenshi-physics --example drop -- <file.phs> [push x,z]`
//! (env DROP_FROM: start poses dumped from the game, ground height first; DROP_DT: step;
//! DROP_FRAMES: steps; DROP_DEBUG: the bodies still turning)
use glam::{Quat, Vec3};
use kenshi_physics::ragdoll::{Ragdoll, Template};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let p = kenshi_io::phs::read(std::path::Path::new(&a[0])).unwrap();
    let t = Template::new(&p);
    let push = a.get(1).map_or(Vec3::ZERO, |s| {
        let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        Vec3::new(v[0], 0.0, v[1])
    });
    let mut ground = 0.0f32;
    let poses: Vec<_> = match std::env::var("DROP_FROM") {
        Ok(f) => {
            let text = std::fs::read_to_string(f).unwrap();
            let mut lines = text.lines();
            ground = lines.next().unwrap().parse().unwrap();
            lines
                .map(|l| {
                    let v: Vec<f32> = l.split(' ').map(|x| x.parse().unwrap()).collect();
                    (Vec3::new(v[0], v[1], v[2]), Quat::from_xyzw(v[3], v[4], v[5], v[6]))
                })
                .collect()
        }
        Err(_) => t.bodies.iter().map(|b| (b.bind, Quat::IDENTITY)).collect(),
    };
    let vel: Vec<_> = t.bodies.iter().map(|b| push * (b.bind.y / 18.0)).collect();
    // DROP_ONTO=<collision.xml>: that building's collision under the ragdoll, its top beneath
    // the feet (DROP_SIDE: beside it instead, pushed into it)
    let onto = std::env::var("DROP_ONTO").ok().map(|f| {
        let shapes = kenshi_io::nxu::read(std::path::Path::new(&f)).unwrap();
        kenshi_physics::world::Solids::new(&shapes)
    });
    let statics = onto
        .as_ref()
        .map(|s| kenshi_physics::world::StaticWorld::new([(glam::Affine3A::IDENTITY, s)]))
        .unwrap_or_default();
    let (mut poses, mut vel) = (poses, vel);
    if !statics.is_empty() {
        let (lo, hi) = statics.bounds().fold((Vec3::MAX, Vec3::MIN), |(a, b), (l, h)| (a.min(l), b.max(h)));
        let c = (lo + hi) * 0.5;
        let feet = poses.iter().map(|p| p.0.y).fold(f32::MAX, f32::min) - 1.0;
        let to = if std::env::var("DROP_SIDE").is_ok() {
            Vec3::new(lo.x - 6.0, 1.0, c.z)
        } else {
            Vec3::new(c.x, hi.y + 2.0, c.z)
        };
        let shift = to - Vec3::new(poses[0].0.x, feet, poses[0].0.z);
        for p in &mut poses {
            p.0 += shift;
        }
        if std::env::var("DROP_SIDE").is_ok() {
            vel = vec![Vec3::new(120.0, 0.0, 0.0); poses.len()];
        }
        println!("collision {} solids, {lo:.1}..{hi:.1}; dropped at {to:.1}", statics.len());
        let mut y = hi.y + 1.0;
        let mut hits = vec![];
        while y > lo.y {
            let mut out = vec![];
            statics.touches(Vec3::new(to.x, y, to.z), 0.5, &mut out);
            if !out.is_empty() {
                hits.push(format!("{y:.0}"));
            }
            y -= 1.0;
        }
        println!("solid under the drop at heights: {}", hits.join(" "));
        ground = if std::env::var("DROP_SIDE").is_ok() { 0.0 } else { lo.y - 0.5 };
    }
    let mut r = Ragdoll::new(&poses, &vel);
    let mut mark: Vec<(Vec3, Quat)> = vec![];
    let frames: usize = std::env::var("DROP_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(600);
    for f in 0..frames {
        if f == frames - 120 { mark = r.poses().collect(); }
        if f == frames - 1 {
            for ((a, b), d) in mark.iter().zip(r.poses()).zip(&t.bodies) { println!("  drift {} {:.3} turn {:.3}", d.bone, a.0.distance(b.0), a.1.angle_between(b.1)); }
        }
        r.step(&t, std::env::var("DROP_DT").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0 / 60.0), &kenshi_physics::world::Scene { ground: |_, _| ground, statics: &statics, ignored: 0 });
        if f % 60 == 59 || r.asleep {
            let ys: Vec<String> = r.poses().zip(&t.bodies).map(|((x, _), b)| format!("{}:{:.1}", &b.bone[6.min(b.bone.len())..], x.y)).collect();
            println!("t {:.1} asleep {} speeds {:?} {}", (f + 1) as f32 / 60.0, r.asleep, r.speeds(), ys.join(" "));
            if std::env::var("DROP_DEBUG").is_ok() {
                for ((v, w), b) in r.body_speeds().iter().zip(&t.bodies) { if *w > 0.5 { println!("   {} v {v:.2} w {w:.2}", b.bone); } }
            }
            if r.asleep {
                println!("rests with its pelvis at {:.1}", r.poses().next().unwrap().0);
                break;
            }
        }
    }
}
