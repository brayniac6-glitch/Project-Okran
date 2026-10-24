//! (testing) An old save still loads (headless): a save of ours written by an older build (the
//! fixture `src/game/tests/fixtures/v0.3.0-alpha.ron`, the v0.3.0-alpha release's save_trip at
//! the Hub), loaded as the menu loads a slot, then saved and loaded again; the counts of people,
//! squads, towns, items and buildings each time, and whether the round trip kept them:
//! `cargo run --release -p kenshi-game --example old_save -- <install> <save.ron>`
//! The save is written to the temp folder only, never to the game's save folder.
use kenshi_char::inventory::Inventory;
use kenshi_game::save::SaveGame;
use kenshi_game::sim::Sim;
use std::path::Path;

/// Items in an inventory, and what they hold (a backpack's contents): (stacks, quantity).
fn items(inv: &Inventory) -> (usize, u64) {
    let mut n = (0, 0);
    for p in inv.sections.iter().flat_map(|s| &s.items) {
        n.0 += 1;
        n.1 += u64::from(p.item.quantity);
        if let Some(c) = &p.item.contents {
            let (a, b) = items(c);
            n.0 += a;
            n.1 += b;
        }
    }
    n
}

#[derive(Debug, PartialEq)]
struct Counts {
    people: usize,
    players: usize,
    dead: usize,
    squads: usize,
    world_squads: usize,
    towns: usize,
    item_stacks: usize,
    item_quantity: u64,
    ground_items: usize,
    buildings: usize,
    /// The towns' placed buildings and furniture, and those destroyed.
    placed: usize,
    destroyed: usize,
}

fn counts(sim: &Sim) -> Counts {
    let mut squads: Vec<usize> = sim.folk.iter().filter_map(|p| p.platoon).collect();
    squads.sort_unstable();
    squads.dedup();
    let mut it = (0, 0);
    let invs = sim.folk.iter().map(|p| &p.inventory).chain(sim.boxes.iter().map(|b| &b.inventory)).chain(sim.buildings.iter().filter_map(|b| b.storage.as_ref()));
    for inv in invs {
        let (a, b) = items(inv);
        it.0 += a;
        it.1 += b;
    }
    Counts {
        people: sim.folk.len(),
        players: sim.folk.iter().filter(|p| p.player).count(),
        dead: sim.folk.iter().filter(|p| p.dead).count(),
        squads: squads.len(),
        world_squads: sim.world.squads.len(),
        towns: sim.towns.len(),
        item_stacks: it.0,
        item_quantity: it.1,
        ground_items: sim.ground.len(),
        buildings: sim.buildings.len(),
        placed: sim.placed.len(),
        destroyed: sim.placed.iter().filter(|p| p.destroyed).count(),
    }
}

/// A world built from a save as the menu's load does (`begin_load`, `build_world`).
fn load(install: &Path, save: &SaveGame) -> Sim {
    let extras = vec![format!("player:{}", save.players.len().max(1)), format!("money:{}", save.money)];
    let mut sim = Sim::load(install, save.zone.0, save.zone.1, 0, &extras);
    sim.restore(save);
    sim
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = Path::new(&a[0]);
    let old = kenshi_game::save::read(Path::new(&a[1])).expect("the old save reads");
    println!(
        "old save: version {}, zone {:?}, t {:.1}, {} players, {} people, {} buildings, {} ground items, {} boxes, {} zones",
        old.version,
        old.zone,
        old.t,
        old.players.len(),
        old.people.len(),
        old.buildings.len(),
        old.ground.len(),
        old.boxes.len(),
        old.zone_order.len()
    );
    let sim = load(install, &old);
    let first = counts(&sim);
    println!("loaded: {first:?}");
    // saved again and loaded again
    let path = std::env::temp_dir().join("kenshi-old-save-trip.ron");
    kenshi_game::save::write(&sim.snapshot(old.zone, |_| None), &path).expect("the save writes");
    let back = kenshi_game::save::read(&path).expect("the save reads back");
    let _ = std::fs::remove_file(&path);
    let again = load(install, &back);
    let second = counts(&again);
    println!("round trip: {second:?}");
    let checks = [
        ("people as saved", first.people == old.people.len()),
        ("players as saved", first.players == old.players.len()),
        ("buildings as saved", first.buildings == old.buildings.len()),
        ("ground items as saved", first.ground_items == old.ground.len()),
        ("round trip keeps the counts", first == second),
    ];
    for (what, ok) in checks {
        println!("check: {what}: {ok}");
    }
    if checks.iter().any(|c| !c.1) {
        std::process::exit(1);
    }
}
