//! Animals from the shipped data against a person, on the ported rules:
//! `cargo run -p kenshi-combat --example beast -- <install> <ANIMAL_CHARACTER> [weapon] [skill] [growth] [seed] [animals]`
//! (defaults: Katana, skill 40, a grown animal, seed 7, one animal).
//!
//! The animals are made as the game makes them (`Sim::make_animal`): a fighter of the animal's
//! race with its natural weapon ("weapon", quality 50) and its CHARACTER stats, its own
//! skeleton's clips for its "attack type" techniques, its body grown to `growth`
//! (`Fighter::set_animal`). Prints one line per event and a summary.

use kenshi_combat::arena::{Arena, Fighter, Kit, Rng, Shared, skeleton_clips};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{mods::Install, resources::Resources};

const OUTFIT: [&str; 3] = ["Leather Shirt", "Cargopants", "Drifter's Boots"];

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let root = std::path::PathBuf::from(&a[0]);
    let animal_name = a.get(1).map_or("Bonedog", |s| s.as_str());
    let weapon_name = a.get(2).map_or("Katana", |s| s.as_str());
    let skill: f32 = a.get(3).map_or(40.0, |s| s.parse().unwrap());
    let growth: f32 = a.get(4).map_or(1.0, |s| s.parse().unwrap());
    let seed: u64 = a.get(5).map_or(7, |s| s.parse().unwrap());
    let n: usize = a.get(6).map_or(1, |s| s.parse().unwrap());

    let mut order = Install::new(&root).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let res = Resources::index(&root);
    let gc = data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
    let g = |k: &str| gc.floats.get(k).copied().or_else(|| gc.ints.get(k).map(|i| *i as f32));
    let people = data.of_type(ItemType::Race).find(|r| r.name == "Greenlander").expect("race");
    let (lengths, roots) = skeleton_clips(&res, people).expect("skeleton");
    let sh = Shared::load(&data, people, lengths, seed);
    let kit = Kit::new(&data, weapon_name, &OUTFIT).unwrap_or_else(|| panic!("no WEAPON '{weapon_name}'"));
    let animal = data
        .of_type(ItemType::AnimalCharacter)
        .find(|r| r.name == animal_name)
        .unwrap_or_else(|| panic!("no ANIMAL_CHARACTER '{animal_name}'"));
    let race = animal.refs("race").first().and_then(|(r, _)| data.get(r)).expect("animal race");
    let (lengths_a, roots_a) = skeleton_clips(&res, race).unwrap_or_default();
    let (clips, animal_roots) = (std::sync::Arc::new(lengths_a), std::sync::Arc::new(roots_a));
    let mut rng = Rng(seed ^ 0x5eed);

    let mut f: Vec<Fighter> = vec![Fighter::new(&data, people, &kit, "Person", 0, skill, [0.0, 0.0, 0.0])];
    for k in 0..n {
        let z = (k as f32 - (n as f32 - 1.0) * 0.5) * 25.0;
        let mut b = Fighter::new(&data, race, &kit, &format!("{}{}", animal.name, k + 1), 1, 0.0, [300.0, 0.0, z]);
        let weapon = animal
            .refs("weapon")
            .first()
            .and_then(|(w, _)| data.get(w))
            .map(|r| kenshi_char::inventory::Item::from_record(r, 1).with_quality(&data, 50.0));
        b.equip(&data, race, weapon.as_ref(), &[]);
        b.clips = Some(clips.clone());
        b.roots = Some(animal_roots.clone());
        let mult = kenshi_char::stats::race_multipliers(race);
        let mut skills = b.skills.clone();
        skills.levels = kenshi_char::stats::character_levels(&data, animal, &mult, &mut || rng.next());
        skills.race_xp = mult;
        b.set_skills(&data, race, skills);
        b.set_animal(&data, race, animal, growth);
        b.body.aim = kenshi_combat::arena::bone_height(&res, race, "Bip01 Spine1").unwrap_or(b.body.aim);
        f.push(b);
    }
    eprintln!("person's Spine2 at {:?}", kenshi_combat::arena::bone_height(&res, people, "Bip01 Spine2"));
    for x in &f {
        let hp: f32 = x.medical.parts.iter().map(|p| p.max_health()).sum();
        eprintln!(
            "{}: attack {:.1}, weapon skill {:.1}, defence {:.1}, dodge {:.1}, attack speed {:.2}, size {:.2}, aim {:.1}, body {:.0} hp, blood {:.0}, weapon {:?} length {}",
            x.name,
            x.attack_level(),
            x.weapon_skill(),
            x.defence_level(),
            x.dodge_level(true),
            x.attack_speed,
            x.body.size,
            x.body.aim * x.body.size,
            hp,
            x.medical.max_blood,
            x.wielded.category,
            x.wielded.length
        );
    }
    let blend_rate = g("animation blend rate").unwrap_or(4.0);
    let dt = 1.0 / 60.0;
    let mut arena = Arena::new(f, sh, roots, blend_rate);
    while arena.t < 180.0 {
        arena.step(dt);
        let f = &arena.f;
        let standing = |team: u16| f.iter().any(|x| x.team == team && !x.down);
        if !standing(0) || !standing(1) {
            break;
        }
    }
    let w: Vec<&str> = arena.f.iter().filter(|x| !x.down).map(|x| x.name.as_str()).collect();
    eprintln!("after {:.1} s: standing: {}", arena.t, w.join(" "));
}
