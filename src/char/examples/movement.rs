//! Races running 300 units (30 m) to a goal through the ported mover, from the shipped race data:
//! `cargo run -p kenshi-char --example movement -- <install>` prints CSV rows
//! `<race>,<maturity>,<time s>,<distance units>,<speed units/s>` every tick (60 Hz).
use kenshi_char::encumbrance::run_speed;
use kenshi_char::movement::{Mover, RaceMovement};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for (name, maturity) in [
        ("Greenlander", 1.0),
        ("Bull", 1.0),
        ("Garru", 0.0),
        ("Leviathan", 1.0),
    ] {
        let Some(r) = c.of_type(ItemType::Race).find(|r| r.name == name) else {
            continue;
        };
        let race = RaceMovement::from_record(r);
        let min = *r.ints.get("speed min skill").unwrap_or(&70) as f32;
        let max = *r.ints.get("speed max skill").unwrap_or(&120) as f32;
        let speed = run_speed(50.0, min, max, maturity);
        let mut m = Mover::for_character(&race, [0.0; 3], 1.0, maturity, false);
        m.set_max_speed(speed);
        let goal = [30.0, 0.0, 0.0];
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        println!("{name},{maturity},0,0,0");
        while !m.arrived && t < 30.0 {
            m.step(goal, dt);
            t += dt;
            let v = m.velocity.iter().map(|x| x * x).sum::<f32>().sqrt() * 10.0;
            println!("{name},{maturity},{t},{},{v}", m.position_units()[0]);
        }
        eprintln!(
            "{name} (maturity {maturity}): run {speed:.1} u/s, accel {:.0} m/s², radius {:.1} m, arrived after {t:.2} s",
            m.acceleration, m.radius
        );
    }
}
