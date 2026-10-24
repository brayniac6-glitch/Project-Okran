//! Slavery (`kenshi_game::slavery`): buying a slave from a slave trader, selling one of the
//! player's people to it, the slave states running on (escaping, ex-slave, free) and a slave
//! far from its owners outside town; prints what happens:
//! `KENSHI_SLAVE_DEBUG=1 cargo run --release -p kenshi-game --example slave_test -- <install> X Y`
use kenshi_game::sim::Sim;
use std::path::Path;

fn run(sim: &mut Sim, seconds: f32, what: &str) {
    let end = sim.t + seconds;
    while sim.t < end {
        sim.step();
        sim.arena.sh.hits.clear();
        for m in sim.messages.drain(..) {
            println!("[{what}] t {:.0}: {m}", sim.t);
        }
        for (who, text) in sim.said.drain(..) {
            println!("[{what}] t {:.0}: {who} says \"{text}\"", sim.t);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let mut sim = Sim::load(install, zx, zy, 0, &["player:2".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    let players: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let (me, me2) = (players[0], players[1]);
    run(&mut sim, 3.0, "load");
    let Some(trader) = (0..sim.folk.len()).find(|&t| !sim.folk[t].player && !sim.slaves_for_sale(t).is_empty()) else {
        println!("no slave trader in zone {zx}.{zy}");
        return;
    };
    let faction = sim.folk[trader].faction.clone().unwrap();
    let rel = |sim: &Sim, f: &str| sim.relations.get(f).map_or(0.0, |r| r.relation(kenshi_game::sim::PLAYER_FACTION));
    println!("trader {trader} ({}) of {faction}: {} slaves for sale, relation {:.1}", sim.display_name(trader), sim.slaves_for_sale(trader).len(), rel(&sim, &faction));
    // buying
    sim.money = 100_000;
    let s = sim.slaves_for_sale(trader)[0];
    println!("[buy] slave {s}: state {}, price {}, looks {:.2}", sim.slave_state(s), sim.slave_buy_price(s), sim.looks_like_slave(s));
    let paid = sim.buy_slave(s);
    run(&mut sim, 1.0, "buy");
    println!("[buy] paid {paid:?}: state {}, player {}, shackled {}, prison {:?}, relation {:.1}", sim.slave_state(s), sim.folk[s].player, sim.folk[s].shackled, sim.folk[s].prison.is_some(), rel(&sim, &faction));
    // selling
    let at = sim.folk[trader].mover.position;
    sim.folk[me].mover.position = [at[0] + 30.0, at[1], at[2]];
    sim.arena.f[me].mover.position = sim.folk[me].mover.position;
    println!("[sell] before: {:?}", sim.folk[me].inventory.sections.iter().flat_map(|s| s.items.iter().map(|p| p.item.name.clone())).collect::<Vec<_>>());
    let got = sim.sell_slave(me, trader);
    run(&mut sim, 4.0, "sell");
    println!("[sell] after: {:?}", sim.folk[me].inventory.sections.iter().flat_map(|s| s.items.iter().map(|p| p.item.name.clone())).collect::<Vec<_>>());
    println!("[sell] got {got}: state {}, player {}, prison {:?}, shackled {}, outfit {:?}, relation {:.1}", sim.slave_state(me), sim.folk[me].player, sim.folk[me].prison.as_ref().map(|p| p.cage), sim.folk[me].shackled, sim.outfit(me), rel(&sim, &faction));
    // the states: out of the cage escaping, 12 hours on an ex-slave, 4 days on free
    sim.folk[me].player = true;
    sim.escape_cage(me);
    println!("[states] out of the cage: state {}, escaped {:?}, looks {:.2} (min {})", sim.slave_state(me), sim.folk[me].escaped.is_some(), sim.looks_like_slave(me), sim.outfit(me).minimum);
    let now = sim.hours_gone();
    if let Some(sl) = sim.folk[me].slave.as_mut() {
        sl.since = now - 12.5;
    }
    run(&mut sim, 2.0, "states");
    println!("[states] 12.5 h on: state {}, looks {:.2} (min {})", sim.slave_state(me), sim.looks_like_slave(me), sim.outfit(me).minimum);
    let now = sim.hours_gone();
    if let Some(sl) = sim.folk[me].slave.as_mut() {
        sl.since = now - 97.0;
    }
    run(&mut sim, 2.0, "states");
    println!("[states] 97 h on: state {}, slave {:?}", sim.slave_state(me), sim.folk[me].slave.is_some());
    // a slave far from its owners, outside town
    let owner = sim.folk[trader].platoon;
    sim.make_slave(me2, owner, &faction);
    sim.folk[me2].mover.position = [at[0] + 6000.0 * kenshi_world::navmesh::METRES, at[1], at[2]];
    sim.arena.f[me2].mover.position = sim.folk[me2].mover.position;
    println!("[far] state {}", sim.slave_state(me2));
    run(&mut sim, 3.0, "far");
    println!("[far] town {:?}: state {}, blown {:.0}", sim.folk[me2].town, sim.slave_state(me2), sim.folk[me2].disguise_blown);
}
