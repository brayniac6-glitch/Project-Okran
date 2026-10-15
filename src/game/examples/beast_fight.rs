//! An animal in the world's fight (`Sim::make_animal`, the arena's animal body): the resident
//! nearest the player's people becomes the ANIMAL_CHARACTER given (at `growth`) and the player's
//! people are ordered to attack it; prints the fight's events and the end:
//! `cargo run --release -p kenshi-game --example beast_fight -- <install> X Y "<animal>" [growth] [seconds] [players]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let growth: f32 = a.get(4).map_or(1.0, |s| s.parse().unwrap());
    let seconds: f32 = a.get(5).map_or(60.0, |s| s.parse().unwrap());
    let n: usize = a.get(6).map_or(2, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &[format!("player:{n}")]);
    let animal = sim
        .data
        .records
        .values()
        .find(|r| r.name == a[3] && r.item_type() == Some(kenshi_data::ItemType::AnimalCharacter))
        .map(|r| r.string_id.clone())
        .expect("animal");
    let players: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let at = sim.folk[players[0]].mover.position_units();
    let id = (0..sim.folk.len())
        .filter(|&i| !sim.folk[i].player && !sim.folk[i].down && sim.folk[i].race.is_none())
        .min_by(|&x, &y| {
            let d = |i: usize| {
                let q = sim.folk[i].mover.position_units();
                (q[0] - at[0]).hypot(q[2] - at[2])
            };
            d(x).total_cmp(&d(y))
        })
        .expect("a resident");
    sim.folk[id].age = growth;
    sim.make_animal(id, &animal);
    let f = &sim.arena.f[id];
    println!(
        "{id} is a {} (size {:.2}, maturity {:.2}, attack {:.1}, weapon skill {:.1}, {:?}); players {players:?}",
        a[3],
        f.body.size,
        f.body.maturity,
        f.attack_level(),
        f.weapon_skill(),
        f.wielded.category
    );
    sim.order_attack(&players, id);
    while sim.t < seconds {
        sim.step();
        if sim.folk[id].down || players.iter().all(|&p| sim.folk[p].down) {
            break;
        }
    }
    let state = |i: usize| if sim.folk[i].dead { "dead" } else if sim.folk[i].down { "down" } else { "up" };
    let ps: Vec<String> = players.iter().map(|&p| format!("{p} {}", state(p))).collect();
    println!("after {:.1} s: the animal {}; players {}", sim.t, state(id), ps.join(", "));
}
