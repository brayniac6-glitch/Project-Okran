//! A person's race and sex as their look draws them (shared by the game's looks and the
//! dialogue's DC_IS_FEMALE): the CHARACTER's "race" (else one of its faction's "races" by their
//! shares), female by its "female chance" (a race with no female mesh: male).
//! (assumption) The game's own rolls are not decompiled: each person's are drawn from a
//! generator seeded by their number, so they stay the same.

use crate::sim::Person;
use kenshi_data::{GameData, GameDataContainer, ItemType};

/// A small generator (splitmix64) for a person's rolls.
pub struct Rolls(pub u64);

impl Rolls {
    /// Person `id`'s rolls.
    pub fn of(id: usize) -> Rolls {
        Rolls(0x4b65_6e73_6869 ^ (id as u64).wrapping_mul(0x2545_f491_4f6c_dd1d))
    }

    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// One of `items` by its weight (none when they weigh nothing).
    pub fn pick<'a, T>(&mut self, items: &'a [(T, f32)]) -> Option<&'a T> {
        let total: f32 = items.iter().map(|(_, w)| w.max(0.0)).sum();
        if total <= 0.0 {
            return None;
        }
        let mut r = self.next() * total;
        for (t, w) in items {
            r -= w.max(0.0);
            if r < 0.0 {
                return Some(t);
            }
        }
        items.last().map(|(t, _)| t)
    }
}

/// A record's refs of `cat` with their first value as a weight (100 by default).
pub fn refs<'a>(data: &'a GameDataContainer, r: &GameData, cat: &str) -> Vec<(&'a GameData, f32)> {
    r.refs(cat)
        .iter()
        .filter_map(|(sid, v)| Some((data.get(sid)?, v.first().copied().unwrap_or(100) as f32)))
        .collect()
}

/// Person `p`'s race and sex, and its rolls after them (None for animals and those without a
/// CHARACTER record).
pub fn race_and_sex<'a>(data: &'a GameDataContainer, install: &std::path::Path, p: &Person, fallback_race: &str) -> Option<(&'a GameData, bool, Rolls)> {
    if p.race.is_some() {
        return None;
    }
    let ch = p.character.as_deref().and_then(|c| data.get(c))?;
    let mut roll = Rolls::of(p.id);
    // a named character's own (its body file)
    if let Some(b) = crate::body_file::body_of(install, data, &ch.string_id)
        && let Some(race) = b.race.as_deref().and_then(|r| data.get(r))
    {
        return Some((race, p.female.unwrap_or(b.female), roll));
    }
    let faction = p.faction.as_deref().and_then(|f| data.get(f));
    let has_mesh = |r: &GameData| r.files.get("male mesh").is_some_and(|m| !m.is_empty()) && r.item_type() == Some(ItemType::Race);
    let race = refs(data, ch, "race").into_iter().filter(|(r, _)| has_mesh(r)).collect::<Vec<_>>();
    let race = roll
        .pick(&race)
        .copied()
        .or_else(|| {
            let from_faction: Vec<_> = faction.map(|f| refs(data, f, "races")).unwrap_or_default().into_iter().filter(|(r, _)| has_mesh(r)).collect();
            roll.pick(&from_faction).copied()
        })
        .or_else(|| data.of_type(ItemType::Race).find(|r| r.name == fallback_race))?;
    let female_ok = race.files.get("female mesh").is_some_and(|m| !m.is_empty()) && !race.refs("heads female").is_empty();
    let female = female_ok && roll.next() * 100.0 < ch.ints.get("female chance").copied().unwrap_or(0) as f32;
    Some((race, p.female.unwrap_or(female), roll))
}
