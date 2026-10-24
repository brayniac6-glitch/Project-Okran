//! The world's artifacts (port_map `items`): F022241, run once for a new world, hands the
//! ARTIFACTS record's valuables out to the towns whose value ranges take them; each town puts
//! its share into its buildings' storage when it first loads (`loot::artifacts_tick`, F048891).
//!
//! Spec F022241:
//! - the ARTIFACTS record's lists, each entry (record, count = its first ref value, value =
//!   the item's value, F047094): "items" (the item artifacts); "armours" (quality 80), "armours
//!   hq" (quality 95), "crossbows", "robotics", "weapons" (the gear);
//! - the towns (TownBase, type not 0, 8 or 10) with an item range ("item artifacts min / max
//!   value", max not 0) and with a gear range ("gear artifacts min / max value");
//! - each artifact's candidate towns: those whose range takes its value (F022237);
//! - a Mersenne twister seeded 0xb2068d (the same handout every new world);
//! - gear: each gear town takes one of the first artifact its range takes (F022240 moves the
//!   count); then every gear artifact's remaining count, one at a time, to a random candidate;
//! - items: each item artifact's count spread over its candidates in proportion to their max
//!   value (a running total stepping total / count from -1), the remainder (rounding) one at a
//!   time to a candidate picked at random weighted by its max value.
//!
//! (assumption) The two sorts before the handout (towns, artifacts) are by max value and by
//! value, ascending; a weapon model's value is that of a weapon of its maker's first type; a
//! random index is the twister's next number modulo the count.

use kenshi_data::{GameDataContainer, ItemType};
use kenshi_world::town::Town;
use std::collections::HashMap;

/// Town (its level id, `Town::id`) -> (item record, gear?, count, quality) given to it.
pub type Handout = HashMap<String, Vec<(String, bool, i32, f32)>>;

/// An artifact entry as an item: armour at its list's quality, a weapon model as a weapon of
/// it (`Item::weapon_of_model`), anything else as its record.
pub fn artifact_item(data: &GameDataContainer, sid: &str, quality: f32, rand: &mut dyn FnMut() -> f32) -> Option<kenshi_char::inventory::Item> {
    let r = data.get(sid)?;
    match r.item_type() {
        Some(ItemType::MaterialSpecsWeapon) => kenshi_char::inventory::Item::weapon_of_model(data, sid, rand),
        Some(ItemType::Armour) => Some(kenshi_char::inventory::Item::from_record(r, 1).with_quality(data, quality)),
        _ => Some(kenshi_char::inventory::Item::from_record(r, 1)),
    }
}

/// MT19937 (std::mt19937).
struct Mt {
    s: [u32; 624],
    i: usize,
}

impl Mt {
    fn new(seed: u32) -> Mt {
        let mut s = [0u32; 624];
        s[0] = seed;
        for k in 1..624 {
            s[k] = 1812433253u32.wrapping_mul(s[k - 1] ^ (s[k - 1] >> 30)).wrapping_add(k as u32);
        }
        Mt { s, i: 624 }
    }

    fn next(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.s[k] & 0x8000_0000) | (self.s[(k + 1) % 624] & 0x7fff_ffff);
                let mut v = self.s[(k + 397) % 624] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= 0x9908_b0df;
                }
                self.s[k] = v;
            }
            self.i = 0;
        }
        let mut y = self.s[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// In [0, 1).
    fn uniform(&mut self) -> f64 {
        self.next() as f64 / 4294967296.0
    }
}

struct Art {
    record: String,
    gear: bool,
    quality: f32,
    count: i32,
    value: i32,
    candidates: Vec<usize>,
}

struct ArtTown {
    /// The town's level id (`Town::id`).
    id: String,
    min: i32,
    max: i32,
    given: Vec<(usize, i32)>,
}

impl ArtTown {
    /// F022240: `n` of artifact `a` moved to the town.
    fn take(&mut self, arts: &mut [Art], a: usize, n: i32) {
        match self.given.iter_mut().find(|g| g.0 == a) {
            Some(g) => g.1 += n,
            None => self.given.push((a, n)),
        }
        arts[a].count -= n;
    }
}

/// @fn F022241
///
/// The handout for the world's towns.
pub fn distribute(data: &GameDataContainer, towns: &[Town]) -> Handout {
    let mut out = Handout::new();
    let Some(list) = data.of_type(ItemType::Artifacts).next() else { return out };
    // the artifacts: items and gear
    let mut items: Vec<Art> = Vec::new();
    let mut gear: Vec<Art> = Vec::new();
    // the quality each list's items are made at (armours 80, armours hq 95; F022241)
    for (cat, is_gear, quality) in [("items", false, 0.0), ("armours", true, 80.0), ("armours hq", true, 95.0), ("crossbows", true, 0.0), ("robotics", true, 0.0), ("weapons", true, 0.0)] {
        for (sid, v) in list.refs(cat) {
            let Some(r) = data.get(sid) else { continue };
            let count = v[0].max(0);
            if count == 0 {
                continue;
            }
            // its value (F047094): the item as it is made (a weapon model: its first type)
            let _ = r;
            let mut first = || 0.0;
            let value = artifact_item(data, sid, quality, &mut first).map_or(0, |i| i.value);
            let a = Art { record: sid.clone(), gear: is_gear, quality, count, value, candidates: Vec::new() };
            if is_gear { gear.push(a) } else { items.push(a) }
        }
    }
    // the towns with ranges
    let range = |t: &Town, k: &str| -> Option<ArtTown> {
        if matches!(t.kind, 0 | 8 | 10) {
            return None;
        }
        let r = data.get(&t.record)?;
        let i = |f: String| r.ints.get(&f).copied().unwrap_or(0);
        let max = i(format!("{k} artifacts max value"));
        (max != 0).then(|| ArtTown { id: t.id.clone(), min: i(format!("{k} artifacts min value")), max, given: Vec::new() })
    };
    let mut item_towns: Vec<ArtTown> = towns.iter().filter_map(|t| range(t, "item")).collect();
    let mut gear_towns: Vec<ArtTown> = towns.iter().filter_map(|t| range(t, "gear")).collect();
    item_towns.sort_by_key(|t| t.max);
    gear_towns.sort_by_key(|t| t.max);
    items.sort_by_key(|a| a.value);
    gear.sort_by_key(|a| a.value);
    // F022237: the candidates
    for (arts, ts) in [(&mut items, &item_towns), (&mut gear, &gear_towns)] {
        for a in arts.iter_mut() {
            a.candidates = (0..ts.len()).filter(|&k| ts[k].min <= a.value && a.value <= ts[k].max).collect();
        }
    }
    let mut mt = Mt::new(0x00b2_068d);
    // gear: one each, then the rest at random
    for k in 0..gear_towns.len() {
        let (lo, hi) = (gear_towns[k].min, gear_towns[k].max);
        if let Some(a) = gear.iter().position(|a| a.count > 0 && lo <= a.value && a.value <= hi) {
            gear_towns[k].take(&mut gear, a, 1);
        }
    }
    for a in 0..gear.len() {
        if gear[a].candidates.is_empty() {
            continue;
        }
        while gear[a].count > 0 {
            let c = gear[a].candidates.len();
            let k = gear[a].candidates[(mt.next() as usize) % c];
            gear_towns[k].take(&mut gear, a, 1);
        }
    }
    // items: in proportion to the candidates' max value, then the rest weighted at random
    for a in 0..items.len() {
        let cands = items[a].candidates.clone();
        if cands.is_empty() {
            eprintln!("Can't find any towns for artifact: {}", data.get(&items[a].record).map_or("?", |r| r.name.as_str()));
            continue;
        }
        let n = items[a].count;
        let total: i32 = cands.iter().map(|&k| item_towns[k].max).sum();
        let step = total as f32 / n as f32;
        let mut f = -1.0f32;
        let mut idx: isize = -1;
        let mut run = 0;
        for _ in 0..n {
            if f < 0.0 {
                if run != 0 && idx >= 0 {
                    item_towns[cands[idx as usize]].take(&mut items, a, run);
                }
                if (idx + 1) as usize >= cands.len() {
                    break;
                }
                idx += 1;
                f += item_towns[cands[idx as usize]].max as f32;
                run = 0;
            }
            run += 1;
            f -= step;
        }
        if run != 0 && idx >= 0 {
            let n = run.min(items[a].count);
            item_towns[cands[idx as usize]].take(&mut items, a, n);
        }
        while items[a].count > 0 {
            let mut x = mt.uniform() as f32 * total as f32;
            let k = cands.iter().copied().find(|&k| {
                x -= item_towns[k].max as f32;
                x <= 0.0
            });
            let k = k.unwrap_or(*cands.last().unwrap());
            item_towns[k].take(&mut items, a, 1);
        }
    }
    for (ts, arts) in [(&item_towns, &items), (&gear_towns, &gear)] {
        for t in ts.iter() {
            for &(a, n) in &t.given {
                if n > 0 {
                    out.entry(t.id.clone()).or_default().push((arts[a].record.clone(), arts[a].gear, n, arts[a].quality));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt19937_reference() {
        // std::mt19937 with the default seed 5489: its 10000th number is 4123659995
        let mut m = Mt::new(5489);
        let mut v = 0;
        for _ in 0..10000 {
            v = m.next();
        }
        assert_eq!(v, 4123659995);
    }
}
