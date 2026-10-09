//! The named characters of a zone with their body files (`kenshi_game::body_file`): race, sex,
//! head and idle stance: `cargo run --release -p kenshi-game --example bodies -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let sim = Sim::load(install, zx, zy, 0, &[]);
    for p in &sim.folk {
        let Some(c) = p.character.as_deref() else { continue };
        let Some(b) = kenshi_game::body_file::body_of(install, &sim.data, c) else { continue };
        let race = b.race.as_deref().and_then(|r| sim.data.get(r)).map(|r| r.name.clone());
        println!("{}: race {:?} female {} head {:?} hair {:?} stance {:?} sliders {} | sim race {:?} female {}", sim.display_name(p.id), race, b.female, b.head, b.hair, b.idle_stance, b.values.len(), sim.race_of(p.id).and_then(|r| sim.data.get(&r).map(|x| x.name.clone())), sim.is_female(p.id));
    }
}
