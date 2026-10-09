//! A Kenshi save imported into a fresh world (`Sim::apply_kenshi`), run a while, written back in
//! Kenshi's layout (`Sim::kenshi_snapshot`) and read again:
//! `cargo run --release -p kenshi-game --example import_trip -- <install> <kenshi save dir> <out dir>`
use kenshi_game::kenshi_save::{self, ImportOptions};
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let k = kenshi_save::read(Path::new(&a[1])).expect("kenshi save");
    let chars: Vec<&kenshi_save::KChar> = k.squads.iter().flat_map(|s| &s.characters).collect();
    let at = chars.first().map_or(k.camera, |c| c.position);
    let zone = |v: f32| ((v + kenshi_world::WORLD_SIZE / 2.0) / kenshi_world::ZONE_SIZE).floor() as i32;
    let (zx, zy) = (zone(at[0]), zone(at[2]));
    println!("{} characters, {} buildings, {} dead, {} factions' relations; zone {zx}.{zy}", chars.len(), k.buildings.len(), k.dead.len(), k.relations.len());
    let mut sim = Sim::load(install, zx, zy, 0, &[format!("player:{}", chars.len()), format!("money:{}", k.money)]);
    let looks = sim.apply_kenshi(&k, &ImportOptions::default());
    println!("applied to {} characters: money {}, research {}, buildings {}, dead uniques {}, day/hour {:.2}", looks.len(), sim.money, sim.research_done.len(), sim.buildings.len(), sim.world.dead_uniques.len(), sim.start_hour() + sim.hours_gone());
    for p in sim.folk.iter().filter(|p| p.player) {
        let f = &sim.arena.f[p.id];
        let strength = f.skills.levels.get(&kenshi_char::StatsEnumerated::StatStrength).copied();
        println!("  {} [{:?}] at {:?}: strength {strength:?}, {} items, blood {:.0}", p.name, p.character, p.mover.position_units().map(|v| v as i32), p.inventory.sections.iter().map(|s| s.items.len()).sum::<usize>(), f.medical.blood);
    }
    while sim.t < 30.0 {
        sim.step();
    }
    let back = sim.kenshi_snapshot(|id| looks.iter().find(|(i, _)| *i == id).map(|(_, b)| b.clone()));
    kenshi_save::write(Path::new(&a[2]), &back).expect("write");
    let again = kenshi_save::read(Path::new(&a[2])).expect("read back");
    let people = |s: &kenshi_save::KenshiSave| s.squads.iter().map(|q| q.characters.len()).sum::<usize>();
    let items = |s: &kenshi_save::KenshiSave| s.squads.iter().flat_map(|q| &q.characters).map(|c| c.items.len()).sum::<usize>();
    println!(
        "written and read back: {} people / {}, {} items / {}, money {} / {}, research {} / {}, relations {}, dead {}, day {:?}",
        people(&again),
        people(&k),
        items(&again),
        items(&k),
        again.money,
        k.money,
        again.research.len(),
        k.research.len(),
        again.relations.len(),
        again.dead.len(),
        again.time
    );
}
