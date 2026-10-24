//! Read a Kenshi save folder (kenshi_game::kenshi_save), print it, write it back elsewhere and
//! read that again: `cargo run --release -p kenshi-game --example kenshi_save -- <save dir> [out dir]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let s = kenshi_game::kenshi_save::read(std::path::Path::new(&a[0])).expect("save");
    println!("day {} {:02}:{:02}, money {}, faction {:?}, version {:?}, zones {:?}, research {}", s.time.0, s.time.1, s.time.2, s.money, s.faction_name, s.version, s.zones, s.research.len());
    if let Some((owner, known)) = &s.known {
        println!("{owner} knows {} factions: {known:?}", known.len());
    }
    for sq in &s.squads {
        println!("squad {} ({}, {} people) at {:?}", sq.id, sq.template, sq.characters.len(), sq.position.map(|v| v as i32));
        for c in &sq.characters {
            let strength = c.stats.iter().find(|(k, _)| k == "strength").map(|x| x.1);
            println!("  {} [{}] at {:?}: strength {strength:?}, {} parts, blood {:.0}, hunger {:.2}, {} items, race {:?} female {}, slave state {} escaped {} blown {:.0}", c.name, c.template, c.position.map(|v| v as i32), c.parts.len(), c.blood, c.hunger, c.items.len(), c.appearance.race, c.appearance.female, c.slave_state, c.escaped, c.disguise_blown);
        }
    }
    if let Some(out) = a.get(1) {
        kenshi_game::kenshi_save::write(std::path::Path::new(out), &s).expect("write");
        let back = kenshi_game::kenshi_save::read(std::path::Path::new(out)).expect("read back");
        let people = |s: &kenshi_game::kenshi_save::KenshiSave| s.squads.iter().map(|q| q.characters.len()).sum::<usize>();
        let items = |s: &kenshi_game::kenshi_save::KenshiSave| s.squads.iter().flat_map(|q| &q.characters).map(|c| c.items.len()).sum::<usize>();
        println!("written and read back: {} squads / {}, {} people / {}, {} items / {}, money {} / {}", back.squads.len(), s.squads.len(), people(&back), people(&s), items(&back), items(&s), back.money, s.money);
        println!("known read back: {:?} / {:?}", back.known.as_ref().map(|k| k.1.len()), s.known.as_ref().map(|k| k.1.len()));
    }
}
