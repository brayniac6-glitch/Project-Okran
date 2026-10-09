//! People's looks (port_map `char`): each of the world's people drawn as themselves, from the
//! game data as fcs.def describes it: the CHARACTER's "race" (else one of its faction's
//! "races" by their shares), female by its "female chance" (a race with no female mesh: male),
//! a head of the race's "heads male" / "heads female", a hairstyle of the race's "hairs" by
//! their "% of how commonly used" (only the faction's "hairstyles" when it lists any; none when
//! the character is "shaved"), a beard for a man the same way, a hair colour of the race's "hair
//! colors" by their shares ("color 1", 0xRRGGBB), and what they wear: the armour equipped in
//! their inventory, a backpack among it.
//!
//! (assumption) The game's own rolls (and its saved morph faces, "morph num") are not decompiled:
//! each person's are drawn from a generator seeded by their number, so a look stays the same.
//! (stand-in) The race is how they look only: the simulation's people share one race's
//! movement and anatomy.

use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_game::sim::Person;
use kenshi_render::character::CharacterSpec;

use kenshi_game::looks_roll::refs;

/// The look of person `p` (None for animals and those without a CHARACTER record).
pub fn look_of(data: &GameDataContainer, install: &std::path::Path, p: &Person, fallback_race: &str) -> Option<CharacterSpec> {
    let (race, female, mut roll) = kenshi_game::looks_roll::race_and_sex(data, install, p, fallback_race)?;
    let ch = p.character.as_deref().and_then(|c| data.get(c))?;
    let faction = p.faction.as_deref().and_then(|f| data.get(f));
    let mut spec = CharacterSpec::new(&race.name);
    spec.female = female;
    // a named character: its body file's head, hair, beard and hair colour
    if let Some(body) = kenshi_game::body_file::body_of(install, data, &ch.string_id) {
        let shaved = p.shaved || ch.bools.get("shaved").copied().unwrap_or(false);
        let mut spec = spec_of_body(data, race, female, &body, shaved);
        spec.wear = worn(p);
        return Some(spec);
    }
    let heads = refs(data, race, if female { "heads female" } else { "heads male" });
    spec.head = roll.pick(&heads).map(|h| h.name.clone());
    // hair and beard: the race's, kept to the faction's styles when it names any
    let styles: Vec<String> = faction.map(|f| f.refs("hairstyles").iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default();
    let hairs: Vec<(&GameData, f32)> = refs(data, race, "hairs")
        .into_iter()
        .filter(|(h, _)| styles.is_empty() || styles.contains(&h.string_id))
        .collect();
    let beard = |h: &GameData| h.ints.get("attach slot") == Some(&13);
    let shaved = p.shaved || ch.bools.get("shaved").copied().unwrap_or(false);
    if !shaved {
        let scalp: Vec<_> = hairs.iter().filter(|(h, _)| !beard(h)).copied().collect();
        if let Some(h) = roll.pick(&scalp) {
            spec.hairs.push(h.name.clone());
        }
    }
    if !female {
        // (assumption) a man is clean-shaven as often as all the race's beards together are
        // worn: a beard by its share against an equal share of none
        let beards: Vec<_> = hairs.iter().filter(|(h, _)| beard(h)).copied().collect();
        let total: f32 = beards.iter().map(|(_, w)| *w).sum();
        if roll.next() < 0.5
            && total > 0.0
            && let Some(b) = roll.pick(&beards)
        {
            spec.hairs.push(b.name.clone());
        }
    }
    let colours = refs(data, race, "hair colors");
    if let Some(c) = roll.pick(&colours).and_then(|c| c.ints.get("color 1").copied()) {
        let c = c as u32;
        spec.hair_colour = [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0];
    }
    spec.wear = worn(p);
    Some(spec)
}

/// The look an appearance gives (a body file, or a save's CHARACTER_APPEARANCE): its head, hair,
/// beard and hair colour (the "Hair Colour" slider picks among the race's "hair colors";
/// (assumption) by its rounded value) on `race`.
pub fn spec_of_body(data: &GameDataContainer, race: &GameData, female: bool, body: &kenshi_game::body_file::BodyFile, shaved: bool) -> CharacterSpec {
    let mut spec = CharacterSpec::new(&race.name);
    spec.female = female;
    let name = |id: &Option<String>| id.as_deref().map(|s| data.get(s).map_or(s.to_string(), |r| r.name.clone()));
    spec.head = name(&body.head);
    if !shaved && let Some(h) = name(&body.hair) {
        spec.hairs.push(h);
    }
    if !female && let Some(b) = name(&body.beard) {
        spec.hairs.push(b);
    }
    let colours = refs(data, race, "hair colors");
    let k = body.values.iter().find(|(n, _)| n == "Hair Colour").map_or(0, |(_, v)| v.round().max(0.0) as usize);
    if let Some(c) = colours.get(k.min(colours.len().saturating_sub(1))).and_then(|(c, _)| c.ints.get("color 1").copied()) {
        let c = c as u32;
        spec.hair_colour = [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0];
    }
    spec
}

/// A look as an appearance to save: its race, sex, head, hair and beard (by record), the hair
/// colour as the race's "hair colors" index nearest it, and the body's sliders `values`.
pub fn body_of_spec(data: &GameDataContainer, spec: &CharacterSpec, values: Option<&std::collections::HashMap<String, f32>>) -> kenshi_game::body_file::BodyFile {
    let race = data.of_type(ItemType::Race).find(|r| r.name == spec.race);
    let among = |cat: &str, name: &str| race.and_then(|r| refs(data, r, cat).into_iter().find(|(h, _)| h.name == name).map(|(h, _)| h.string_id.clone()));
    let head = spec.head.as_deref().and_then(|h| among(if spec.female { "heads female" } else { "heads male" }, h));
    let beard = |n: &str| race.is_some_and(|r| refs(data, r, "hairs").iter().any(|(h, _)| h.name == n && h.ints.get("attach slot") == Some(&13)));
    let hair = spec.hairs.iter().find(|h| !beard(h)).and_then(|h| among("hairs", h));
    let bear = spec.hairs.iter().find(|h| beard(h)).and_then(|h| among("hairs", h));
    let mut values: Vec<(String, f32)> = values.map(|v| v.iter().map(|(k, x)| (k.clone(), *x)).collect()).unwrap_or_default();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(r) = race {
        let colours = refs(data, r, "hair colors");
        let dist = |c: &GameData| {
            let c = c.ints.get("color 1").copied().unwrap_or(0) as u32;
            let rgb = [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0];
            (0..3).map(|i| (rgb[i] - spec.hair_colour[i]).powi(2)).sum::<f32>()
        };
        if let Some(k) = (0..colours.len()).min_by(|&a, &b| dist(colours[a].0).total_cmp(&dist(colours[b].0))) {
            values.retain(|(n, _)| n != "Hair Colour");
            values.push(("Hair Colour".into(), k as f32));
        }
    }
    kenshi_game::body_file::BodyFile {
        race: race.map(|r| r.string_id.clone()),
        female: spec.female,
        head,
        hair,
        beard: bear,
        values,
        skin_tone: [1.0; 3],
        idle_stance: None,
    }
}

/// The records of the clothing (armour, a backpack) `p` has on.
pub fn worn(p: &Person) -> Vec<String> {
    p.inventory
        .equipped()
        .filter(|i| matches!(i.kind, Some(ItemType::Armour | ItemType::Container)))
        .map(|i| i.record.clone())
        .collect()
}

/// A look's key: people who look the same share one body.
pub fn key(spec: &CharacterSpec) -> String {
    format!("{}|{}|{:?}|{:?}|{:?}|{:?}", spec.race, spec.female, spec.head, spec.hairs, spec.wear, spec.hair_colour.map(|c| (c * 255.0) as u8))
}
