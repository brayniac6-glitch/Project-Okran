//! A Greenlander running a found path over the shipped navmesh through the ported mover:
//! `cargo run --release -p kenshi-char --example walk -- <install> X Y <start fx,fz> <goal fx,fz>`
//! loads the tiles round tile X.Y (start and goal are fractions of that 3 x 3 block), asks for a
//! path through goTo (F040653) and the manager's queue (F032210/F032235) with the race's radius and
//! water cost, then follows it at 60 Hz (F024361 heading, F024365 step). Prints `t,x,y,z,speed` rows in units, then a summary on stderr.
use kenshi_char::encumbrance::run_speed;
use kenshi_char::movement::{MoveSpeed, Mover, Pathing, RaceMovement, speed_limit};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use kenshi_world::navmesh::{METRES, NavWorld, PathManager, distance};
use kenshi_world::{WORLD_SIZE, ZONE_SIZE};
use std::path::Path;

fn frac(s: &str) -> (f32, f32) {
    let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
    (v[0], v[1])
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let mut order = Install::new(install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let r = data
        .of_type(ItemType::Race)
        .find(|r| r.name == "Greenlander")
        .unwrap();
    let race = RaceMovement::from_record(r);

    let (tx, ty): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let nav = NavWorld::load_around(install, tx, ty, 1).unwrap();
    let size = ZONE_SIZE * METRES;
    let x0 = (tx - 1) as f32 * size - WORLD_SIZE * METRES / 2.0;
    let z0 = (ty - 1) as f32 * size - WORLD_SIZE * METRES / 2.0;
    let at = |(fx, fz): (f32, f32)| [x0 + fx * 3.0 * size, 0.0, z0 + fz * 3.0 * size];
    let start = nav.closest_point(at(frac(&a[3])), 2000.0).unwrap().1;
    let goal = nav.closest_point(at(frac(&a[4])), 2000.0).unwrap().1;

    let mut mover = Mover::for_character(&race, start.map(|v| v / METRES), 1.0, 1.0, false);
    mover.position = start;
    mover.set_max_speed(speed_limit(
        run_speed(50.0, 70.0, 120.0, 1.0),
        MoveSpeed::Run.cap(15.0).unwrap(),
        None,
    ));
    // goTo -> setGoal -> the manager answers on its next update
    let mut manager: PathManager<u32> = PathManager::default();
    let mut pathing = Pathing::default();
    assert!(pathing.go_to(0, &mover, goal.map(|v| v / METRES), false, &mut manager));
    let (_, result) = manager.update(&nav).expect("an answer");
    let points = match &result {
        kenshi_world::navmesh::PathResult::Found(f) => f.points.clone(),
        other => panic!("no path: {other:?}"),
    };
    pathing.answer(result);
    let gates = pathing.follow.as_ref().map_or(0, |f| f.path.gates.len());
    let dt = 1.0 / 60.0;
    let mut t = 0.0f32;
    let mut walked = 0.0;
    while !mover.arrived && t < 900.0 {
        let before = mover.position;
        pathing.tick(&mut mover, &nav, dt);
        walked += distance(before, mover.position);
        t += dt;
        let p = mover.position_units();
        let v = (mover.velocity[0].powi(2) + mover.velocity[2].powi(2)).sqrt() / METRES;
        println!("{t:.4},{:.1},{:.1},{:.1},{v:.1}", p[0], p[1], p[2]);
    }
    let planned: f32 = points.windows(2).map(|w| distance(w[0], w[1])).sum();
    eprintln!(
        "path {} points, {planned:.0} m planned; walked {walked:.0} m in {t:.1} s ({}), {} gates",
        points.len(),
        if mover.arrived {
            "arrived"
        } else {
            "timed out"
        },
        gates
    );
}
