//! Fighters from the shipped data, run against each other on the ported rules:
//! `cargo run -p kenshi-combat --example duel -- <install> [weapon] [skill A] [skill B] [seed] [NvM]`
//! (`NvM`: team sizes, default `1v1`; team A has skill A, team B skill B).
//!
//! Each fighter's combat AI is the ported CombatClass (`kenshi_combat::fighter`): its update
//! runs the original's state machine. This example is the game side around it ([`Fighting`]):
//! the two characters' animation actions (timed by the ported animation clock, moved by the
//! clips' root bone), movement (the ported mover and formation steering), hits (resolution,
//! armour, stumbles, knockouts) and target finding (F040734: the current target is kept for a
//! second, then the best standing enemy). Not ported here: the threat lists and the squad's
//! scoring of them (F038638; harness: nearest standing enemy), perception (everyone is known),
//! neighbour avoidance, knockback and knockdowns.
//!
//! Prints one line per event and CSV rows `pos,<t>,<a x>,<b x>` and
//! `hp,<t>,<fighter>,<part>,<health>` for charts, and for the `duel` renderer one row per fighter
//! every third tick: `f,<t>,<fighter>,<x>,<z>,<root side>,<root forward>,<face x>,<face z>,<clip>:<time>:<weight>|...`
//! that is, the clips the ported animation layers blend (a combat stance or the run on the base
//! layer, the move on the action layer) and the root bone's in-clip travel to take back out.

use kenshi_combat::arena::{Arena, Fighter, Kit, Shared, skeleton_clips};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{mods::Install, resources::Resources};

/// What both fighters wear (the `duel` renderer dresses them the same), at quality 0.5.
const OUTFIT: [&str; 3] = ["Leather Shirt", "Cargopants", "Drifter's Boots"];

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let root = std::path::PathBuf::from(&a[0]);
    let weapon_name = a.get(1).map_or("Katana", |s| s.as_str());
    let skill_a: f32 = a.get(2).map_or(40.0, |s| s.parse().unwrap());
    let skill_b: f32 = a.get(3).map_or(20.0, |s| s.parse().unwrap());
    let seed = a.get(4).map_or(7, |s| s.parse().unwrap());
    let (na, nb): (usize, usize) = a.get(5).map_or((1, 1), |s| {
        let (x, y) = s.split_once('v').expect("NvM");
        (x.parse().unwrap(), y.parse().unwrap())
    });

    let mut order = Install::new(&root).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let res = Resources::index(&root);
    let gc = data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
    let g = |k: &str| {
        gc.floats
            .get(k)
            .copied()
            .or_else(|| gc.ints.get(k).map(|i| *i as f32))
    };
    let race = data
        .of_type(ItemType::Race)
        .find(|r| r.name == "Greenlander")
        .expect("race");
    let (lengths, roots) = skeleton_clips(&res, race).expect("skeleton");
    let sh = Shared::load(&data, race, lengths, seed);
    let kit = Kit::new(&data, weapon_name, &OUTFIT)
        .unwrap_or_else(|| panic!("no WEAPON '{weapon_name}'"));
    let make = |name: &str, team: u16, skill: f32, x: f32, z: f32| {
        Fighter::new(&data, race, &kit, name, team, skill, [x, 0.0, z])
    };
    // (harness) two blocks facing each other 300 units apart: ranks of up to 10, 25 between
    // neighbours and between ranks
    let mut f: Vec<Fighter> = Vec::new();
    for (team, n, skill, x) in [(0u16, na, skill_a, 0.0), (1, nb, skill_b, 300.0)] {
        for k in 0..n {
            let letter = if team == 0 { "A" } else { "B" };
            let name = if n == 1 {
                letter.to_string()
            } else {
                format!("{letter}{}", k + 1)
            };
            let cols = n.min(10);
            let (rank, col) = (k / cols, k % cols);
            let z = (col as f32 - (cols as f32 - 1.0) * 0.5) * 25.0;
            let back = rank as f32 * 25.0;
            let x = if team == 0 { x - back } else { x + back };
            f.push(make(&name, team, skill, x, z));
        }
    }
    // (testing) DUEL_LEG=<flesh>: the last fighter's legs start at that flesh (below the KO
    // point they collapse: it fights prone)
    if let Some(v) = std::env::var("DUEL_LEG").ok().and_then(|v| v.parse::<f32>().ok())
        && let Some(x) = f.last_mut()
    {
        for p in x.medical.parts.iter_mut().filter(|p| p.kind == 1) {
            p.flesh = v;
            p.update_frac();
        }
        eprintln!("{} prone: {}", x.name, x.prone());
    }
    for x in &f {
        eprintln!(
            "{}: skill {}, attack speed {:.2}, defence speed {:.2}, run {:.0}",
            x.name, x.attack_level(), x.attack_speed, x.defence_speed, x.run
        );
    }
    eprintln!(
        "{} attacks, {} defences, {} stumbles usable; {weapon_name}: length {}, weight {:.1}",
        sh.attacks.len(),
        sh.defences.len(),
        sh.stumbles.len(),
        f[0].wielded.length,
        kit.weapon.combat_weight
    );
    // the animation layers the renderer replays (F038910: base and action layers)
    let blend_rate = g("animation blend rate").unwrap_or(4.0);
    let dt = 1.0 / 60.0;
    let limit = if f.len() > 2 { 600.0 } else { 120.0 };
    let mut arena = Arena::new(f, sh, roots, blend_rate);
    arena.rows = true;
    while arena.t < limit {
        arena.step(dt);
        let t = arena.t;
        let f = &arena.f;
        if (t * 10.0).fract() < dt * 10.0 {
            let xs: Vec<String> = f.iter().map(|x| x.pos()[0].to_string()).collect();
            println!("pos,{t},{}", xs.join(","));
        }
        let standing = |team: u16| f.iter().any(|x| x.team == team && !x.down);
        if !standing(0) || !standing(1) {
            break;
        }
    }
    let w: Vec<&str> = arena
        .f
        .iter()
        .filter(|x| !x.down)
        .map(|x| x.name.as_str())
        .collect();
    eprintln!("after {:.1} s: standing: {}", arena.t, w.join(" "));
}
