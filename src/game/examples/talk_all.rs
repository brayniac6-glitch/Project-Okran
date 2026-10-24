//! Talk to everyone in a zone (`kenshi_game::dialogue`): each NPC's opening line and the
//! player's replies, uniques marked:
//! `cargo run --release -p kenshi-game --example talk_all -- <install> X Y [seconds] [only uniques]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(5.0, |n| n.parse().unwrap());
    let only_uniques = a.get(4).is_some_and(|v| v == "1");
    let mut sim = Sim::load(install, zx, zy, 0, &["player:1".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    while sim.t < seconds {
        sim.step();
    }
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.id).expect("a player character");
    let (mut talked, mut silent) = (0, 0);
    for npc in 0..sim.folk.len() {
        let p = &sim.folk[npc];
        if p.player || p.dead || p.down {
            continue;
        }
        let unique = p.character.as_deref().and_then(|c| sim.data.get(c)).is_some_and(|c| c.bools.get("unique").copied().unwrap_or(false));
        if only_uniques && !unique {
            continue;
        }
        let name = sim.display_name(npc);
        // beside them, in their building
        let at = sim.folk[npc].mover.position;
        sim.folk[me].mover.position = [at[0] + 3.0, at[1], at[2]];
        sim.folk[me].inside = sim.folk[npc].inside;
        match sim.talk(npc, me) {
            Some(conv) => {
                talked += 1;
                println!("{}{name} [{}]: {}", if unique { "* " } else { "" }, sim.folk[npc].personality, conv.npc_text);
                for (_, c) in &conv.choices {
                    println!("      > {c}");
                }
            }
            None => {
                silent += 1;
                if unique {
                    println!("* {name}: (nothing to say)");
                }
            }
        }
    }
    println!("talked {talked}, nothing to say {silent}");
}
