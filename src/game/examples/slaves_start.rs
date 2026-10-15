//! The Slaves start (`kenshi_game::slavery`): the player's slaves at Rebirth for a while, their
//! slave states, work and what the NPCs make of them:
//! `KENSHI_SLAVE_DEBUG=1 cargo run --release -p kenshi-game --example slaves_start -- <install> [seconds]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let seconds: f32 = a.get(1).map_or(120.0, |n| n.parse().unwrap());
    let extras = ["player:3".to_string(), "kit:1532935-rebirth.mod".to_string(), "spawn:-47334,-55623".to_string()];
    let mut sim = Sim::load(install, 21, 19, 0, &extras);
    let players: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let mut last = vec![255u8; players.len()];
    let mut ordered = false;
    while sim.t < seconds {
        // (the test) at 40 s the first one gets out of its cage and walks off, 1500 units away
        if !ordered && sim.t > 40.0 {
            ordered = true;
            let id = players[0];
            sim.escape_cage(id);
            let at = sim.folk[id].mover.position_units();
            sim.order_move(&[id], [at[0] + 1500.0, at[1], at[2]]);
            println!("t {:.0}: {id} ordered away", sim.t);
        }
        sim.step();
        sim.arena.sh.hits.clear();
        for m in sim.messages.drain(..) {
            println!("t {:.0}: {m}", sim.t);
        }
        for (who, text) in sim.said.drain(..) {
            println!("t {:.0}: {who} says \"{text}\"", sim.t);
        }
        for (k, &id) in players.iter().enumerate() {
            let s = sim.slave_state(id);
            if s != last[k] || sim.tick % 1800 == 0 {
                let p = &sim.folk[id];
                println!(
                    "t {:.0}: {id} state {s}, obedient {:?}, doing {:?}, prison {:?}, shackled {}, looks {:.2}, blown {:.0}, bounties {:?}",
                    sim.t,
                    p.slave.as_ref().map(|s| s.obedient),
                    p.slave.as_ref().map(|s| s.doing),
                    p.prison.as_ref().map(|q| (q.cage, q.out)),
                    p.shackled,
                    sim.looks_like_slave(id),
                    p.disguise_blown,
                    p.bounties.iter().map(|b| b.amount).collect::<Vec<_>>()
                );
                last[k] = s;
            }
        }
    }
}
