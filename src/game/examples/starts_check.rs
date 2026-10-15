//! Each base-game start's places (F046699: its forced position, else its towns, else the open
//! towns) and, for one place each, where its squad is put: the zone loaded with "spawn:x,z":
//! `cargo run --release -p kenshi-game --example starts_check -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_game::sim::Sim;
use kenshi_io::mods::Install;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(&a[0]);
    let order = Install::new(root).game_order().unwrap();
    let mut core_order = order.clone();
    core_order.files.retain(|f| f.core);
    let core = GameDataContainer::load(&core_order).unwrap();
    let data = GameDataContainer::load(&order).unwrap();
    let towns = kenshi_world::town::towns(root, &data);
    for s in data.of_type(ItemType::NewGameStartoff) {
        if core.get(&s.string_id).is_none() {
            println!("{}: a mod's, left out", s.name);
            continue;
        }
        let forced = s.bools.get("force start pos").copied().unwrap_or(false);
        let spots: Vec<(String, [f32; 2])> = if forced {
            let n = |k: &str| s.ints.get(k).map(|&v| v as f32).unwrap_or(0.0);
            vec![("start pos".into(), [n("start pos X"), n("start pos Z")])]
        } else {
            s.refs("town").iter().filter_map(|(sid, _)| towns.iter().find(|t| &t.record == sid)).map(|t| (t.name.clone(), [t.position[0], t.position[2]])).collect()
        };
        let Some((what, at)) = spots.first().cloned() else {
            println!("{}: no town listed (an open town at random)", s.name);
            continue;
        };
        let zone = kenshi_world::zone_of(at[0], at[1]);
        // its squads, as the menu gives them (each template's leaders and squad counts)
        let kits: Vec<String> = s.refs("squad").iter().map(|(t, _)| t.clone()).collect();
        let members: usize = kits.iter().filter_map(|t| data.get(t)).map(|t| t.refs("leader").iter().chain(t.refs("squad")).map(|(_, v)| v[0].max(1) as usize).sum::<usize>().max(1)).sum::<usize>().max(1);
        let mut extras = vec![format!("spawn:{},{}", at[0], at[1]), format!("player:{members}")];
        extras.extend(kits.iter().map(|t| format!("kit:{t}")));
        let sim = Sim::load(root, zone.0, zone.1, 0, &extras);
        let who: Vec<String> = sim.folk.iter().filter(|p| p.player).map(|p| p.character.as_deref().and_then(|c| data.get(c)).map_or("-".to_string(), |r| r.name.clone())).collect();
        println!("   squad: {who:?}");
        let p = sim.folk.iter().find(|p| p.player).map(|p| p.mover.position_units()).unwrap_or([f32::NAN; 3]);
        println!(
            "{}: {} places; {what} at ({:.0}, {:.0}) zone {}.{}: squad at ({:.0}, {:.0}, {:.0}), {:.0} away",
            s.name,
            spots.len(),
            at[0],
            at[1],
            zone.0,
            zone.1,
            p[0],
            p[1],
            p[2],
            (p[0] - at[0]).hypot(p[2] - at[1])
        );
    }
}
