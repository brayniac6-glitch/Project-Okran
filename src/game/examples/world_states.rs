//! A unique's world states (`kenshi_game::world`) before and after it dies, with or without the
//! player's hand in it, and the campaigns that would then become possible:
//! `cargo run --release -p kenshi-game --example world_states -- <install> X Y "<unique name>" [involved|kidnap]`
use kenshi_data::ItemType;
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let name = a[3].clone();
    let involved = a.get(4).is_some_and(|v| v == "involved");
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    while sim.t < 3.0 {
        sim.step();
    }
    let data = sim.data.clone();
    let u = sim.folk.iter().find(|p| p.character.as_deref().and_then(|c| data.get(c)).is_some_and(|c| c.name == name)).map(|p| p.id).expect("the unique in this zone");
    let c = sim.folk[u].character.clone().unwrap();
    let states: Vec<String> = data.of_type(ItemType::WorldEventState).filter(|s| s.refs("NPC is").iter().chain(s.refs("NPC is NOT")).any(|(x, _)| *x == c)).map(|s| s.string_id.clone()).collect();
    let show = |sim: &Sim, when: &str| {
        println!("== {when}: {:?}", sim.unique_state(&c));
        for s in &states {
            let r = sim.data.get(s).unwrap();
            println!("  {:5} {}{}", sim.world_state(s), r.name, if r.bools.get("player involvement").copied().unwrap_or(false) { " [involvement]" } else { "" });
        }
    };
    show(&sim, "alive");
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).unwrap();
    // "kidnap": knocked out and carried off by the player's, then handed to its own faction's
    // cage (as a bounty turned in)
    if a.get(4).is_some_and(|v| v == "kidnap") {
        sim.knock_out(u);
        sim.folk[me].carrying = Some(u);
        sim.folk[u].carried_by = Some(me);
        let until = sim.t + 2.0;
        while sim.t < until {
            sim.step();
        }
        show(&sim, "carried by the player");
        sim.folk[me].carrying = None;
        sim.folk[u].carried_by = None;
        let cage = (0..sim.furn.len()).find(|&k| sim.furn[k].function == kenshi_world::furniture::BF_CAGE);
        if let Some(k) = cage {
            let f = sim.folk[u].faction.clone().unwrap_or_default();
            sim.imprison(u, k, &f, false);
        }
        let until = sim.t + 2.0;
        while sim.t < until {
            sim.step();
        }
        show(&sim, "caged by others after");
        return;
    }
    if involved {
        sim.arena.f[me].focus = Some(u);
    }
    // dies: its blood gone
    sim.arena.f[u].medical.blood = -1000.0;
    let until = sim.t + 3.0;
    while sim.t < until {
        sim.step();
    }
    show(&sim, if involved { "dead, the player involved" } else { "dead" });
    for m in &sim.messages {
        println!("message: {m}");
    }
}
