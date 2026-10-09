//! A walk between two world points (units), printing each tick's face when it changes:
//! `cargo run --release -p kenshi-char --example walkto -- <install> x,y,z x,y,z [seconds]`
use kenshi_char::movement::{Mover, Pathing};
use kenshi_world::navmesh::{METRES, NavWorld, PathManager, tile_of};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let p = |s: &str| -> [f32; 3] {
        let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        [v[0], v[1], v[2]]
    };
    let (s, g) = (p(&a[1]), p(&a[2]));
    let secs: f32 = a.get(3).map_or(30.0, |v| v.parse().unwrap());
    let (tx, ty) = tile_of(s.map(|v| v * METRES));
    let nav = NavWorld::load_around(Path::new(&a[0]), tx, ty, 1).unwrap();
    let mut mover = Mover::new(s, 4.0, 20.0);
    mover.set_max_speed(15.0);
    let mut manager: PathManager<u32> = PathManager::default();
    let mut pathing = Pathing::default();
    pathing.go_to(0, &mover, g, false, &mut manager);
    let (_, r) = manager.update(&nav).expect("answer");
    pathing.answer(r);
    let mut t = 0.0;
    let mut last = None;
    while t < secs && !mover.arrived {
        pathing.tick(&mut mover, &nav, 1.0 / 60.0);
        t += 1.0 / 60.0;
        let face = pathing.follow.as_ref().map(|f| f.face);
        if (16.4..16.9).contains(&t)
            && let Some(f) = pathing.follow.as_mut()
        {
            let q = mover.position;
            let tg = f.path.target(q);
            println!(
                "  t {t:.3} pos {:?} next {} of {} target {:?} last {}",
                q.map(|v| v / METRES),
                f.path.next,
                f.path.gates.len(),
                tg.map(|v| v / METRES),
                f.path.on_last_segment()
            );
        }
        if face != last {
            let q = mover.position_units();
            let next = pathing.follow.as_ref().map(|f| f.path.next);
            println!(
                "t {t:.2} at ({:.1},{:.1},{:.1}) face {face:?} next {next:?}",
                q[0], q[1], q[2]
            );
            last = face;
        }
    }
    println!("arrived {} at t {t:.1}", mover.arrived);
}
