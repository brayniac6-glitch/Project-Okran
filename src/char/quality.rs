//! Item quality (port_map `items`): armour grades and weapon makers.
//!
//! An item's quality is 0 .. 100 (the item's +0x220 x 100; combat reads it over 100).
//! - Armour: its grade's quality (F040001 / F049345: grade 0 - 5 -> 5, 20, 40, 60, 80, 95),
//!   named by F047292 (< 11 Prototype, <= 20 Shoddy, <= 40 Standard, <= 60 High, <= 80
//!   Specialist, else Masterwork).
//! - Weapons: a maker (WEAPON_MANUFACTURER) and its model (MATERIAL_SPECS_WEAPON), the model
//!   picked from the maker's "weapon models" (F038261: by the entry's second value as its
//!   chance), the quality its first value (Mk III 60, Rusted Junk 5) (F038264); with no maker,
//!   "Ancient" (917) (F040017).
//! - Value (F047094): a weapon: its "value" x the maker's "price mod" x lerp(s, 15, 160), s =
//!   1 / (1 + e^-((2q - 1) x 5 x 0.8)); armour with "part coverage" at a quality above 0:
//!   lerp(q^2, class lo price x material lo factor, class hi price x material hi factor)
//!   (armourTables F047256), else its "value"; then x "relative price mult" and by slot (head
//!   0.3, legs 0.5, shirt 1.5, boots 0.2).

use crate::inventory::Item;
use kenshi_data::{GameData, GameDataContainer, ItemType};

/// F040001: an armour grade's quality.
pub fn grade_quality(grade: i32) -> f32 {
    match grade {
        i32::MIN..=0 => 5.0,
        1 => 20.0,
        2 => 40.0,
        3 => 60.0,
        4 => 80.0,
        _ => 95.0,
    }
}

/// F047292: an armour quality's grade name.
pub fn grade_name(quality: f32) -> &'static str {
    let q = quality as i32;
    if q < 11 {
        "Prototype"
    } else if q <= 20 {
        "Shoddy"
    } else if q <= 40 {
        "Standard"
    } else if q <= 60 {
        "High"
    } else if q <= 80 {
        "Specialist"
    } else {
        "Masterwork"
    }
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}

/// Kenshi's default maker ("Ancient").
pub const DEFAULT_MAKER: &str = "917-gamedata.base";

/// F038261: an entry of `refs` picked at random by its second value (its chance; all 0: alike).
pub fn pick_ref<'a>(refs: &'a [(String, [i32; 3])], rand: &mut dyn FnMut() -> f32) -> Option<&'a (String, [i32; 3])> {
    if refs.is_empty() {
        return None;
    }
    let total: f32 = refs.iter().map(|(_, v)| v[1].max(0) as f32).sum();
    if total <= 0.0 {
        return refs.get(((rand() * refs.len() as f32) as usize).min(refs.len() - 1));
    }
    let mut x = rand() * total;
    refs.iter().find(|(_, v)| {
        x -= v[1].max(0) as f32;
        x <= 0.0
    }).or(refs.last())
}

impl Item {
    /// A weapon of WEAPON record `r` made by `maker` (else "Ancient"): its model picked from
    /// the maker's models, its quality the model's (F038264).
    pub fn weapon(data: &GameDataContainer, r: &GameData, maker: Option<&str>, rand: &mut dyn FnMut() -> f32) -> Item {
        let mut item = Item::from_record(r, 1);
        let maker = maker.and_then(|m| data.get(m)).or_else(|| data.get(DEFAULT_MAKER));
        if let Some(m) = maker {
            item.maker = Some(m.string_id.clone());
            if let Some((model, v)) = pick_ref(m.refs("weapon models"), rand) {
                item.model = Some(model.clone());
                item.quality = v[0].max(0) as f32;
            }
        }
        item.value = item_value(data, &item);
        item
    }

    /// A weapon of model `model` (an artifact): a maker that makes it, one of the maker's
    /// weapon types (F038264 with no weapon given), the model's quality.
    pub fn weapon_of_model(data: &GameDataContainer, model: &str, rand: &mut dyn FnMut() -> f32) -> Option<Item> {
        let maker = data
            .of_type(ItemType::WeaponManufacturer)
            .filter(|m| m.refs("weapon models").iter().any(|(s, _)| s == model))
            .min_by_key(|m| m.string_id.clone())?;
        let (_, mv) = maker.refs("weapon models").iter().find(|(s, _)| s == model)?.clone();
        let (wt, _) = pick_ref(maker.refs("weapon types"), rand)?;
        let r = data.get(wt)?;
        let mut item = Item::from_record(r, 1);
        item.maker = Some(maker.string_id.clone());
        item.model = Some(model.to_string());
        item.quality = mv[0].max(0) as f32;
        item.value = item_value(data, &item);
        Some(item)
    }

    /// Armour of ARMOUR record `r` at `grade` (0 - 5).
    pub fn armour(data: &GameDataContainer, r: &GameData, grade: i32) -> Item {
        let mut item = Item::from_record(r, 1);
        item.quality = grade_quality(grade);
        item.value = item_value(data, &item);
        item
    }

    /// The item at quality `q`, its value made again.
    pub fn with_quality(mut self, data: &GameDataContainer, q: f32) -> Item {
        self.quality = q.clamp(0.0, 100.0);
        self.value = item_value(data, &self);
        self
    }

    /// The name shown: a weapon's model before it ("Mk III Katana"), armour's grade after it
    /// ("Leather Shirt (Standard)").
    /// (assumption) The game's own label text is not decompiled: this is how its inventory
    /// shows them.
    pub fn shown_name(&self, data: &GameDataContainer) -> String {
        match self.kind {
            Some(ItemType::Weapon) => match self.model.as_ref().and_then(|m| data.get(m)) {
                Some(m) => format!("{} {}", m.name.trim(), self.name),
                None => self.name.clone(),
            },
            Some(ItemType::Armour) if graded(data, &self.record) => format!("{} ({})", self.name, grade_name(self.quality)),
            _ => self.name.clone(),
        }
    }
}

/// Whether ARMOUR record `sid` has grades (its "part coverage": real armour, not a bag).
pub fn graded(data: &GameDataContainer, sid: &str) -> bool {
    data.get(sid).is_some_and(|r| !r.refs("part coverage").is_empty())
}

/// armourTables (F047256): (class lo price, class hi price) by "class", (material lo, material
/// hi) factors by "material type".
const CLASS_PRICE: [(f32, f32); 4] = [(100.0, 5000.0), (400.0, 30000.0), (800.0, 35000.0), (400.0, 40000.0)];
const MATERIAL_PRICE: [(f32, f32); 4] = [(0.2, 0.8), (0.5, 1.0), (1.0, 1.5), (1.5, 1.5)];

/// @fn F047094
///
/// An item's value (before the trade multipliers), from its record and quality.
pub fn item_value(data: &GameDataContainer, item: &Item) -> i32 {
    let Some(r) = data.get(&item.record) else { return item.value };
    let int = |r: &GameData, k: &str| r.ints.get(k).copied().unwrap_or(0);
    let float = |r: &GameData, k: &str, d: f32| r.floats.get(k).copied().unwrap_or(d);
    let q = item.quality * 0.01;
    match item.kind {
        Some(ItemType::Weapon) => {
            let mut v = int(r, "value") as f32;
            if let Some(m) = item.maker.as_ref().and_then(|m| data.get(m)) {
                v = (v * float(m, "price mod", 1.0)).trunc();
            }
            let x = -((q * 2.0 - 1.0) * 5.0 * 0.8);
            let s = 1.0 / (x.exp() + 1.0);
            (lerp(s, 15.0, 160.0) * v) as i32
        }
        Some(ItemType::Armour) => {
            let class = int(r, "class").clamp(0, 3) as usize;
            let mat = int(r, "material type").clamp(0, 3) as usize;
            let mut v = if q > 0.0 && graded(data, &item.record) {
                let lo = CLASS_PRICE[class].0 * MATERIAL_PRICE[mat].0;
                let hi = CLASS_PRICE[class].1 * MATERIAL_PRICE[mat].1;
                lerp(q * q, lo, hi).trunc()
            } else {
                int(r, "value") as f32
            };
            v = (v * float(r, "relative price mult", 1.0)).trunc();
            v *= match int(r, "slot") {
                3 => 0.3,
                6 => 0.5,
                8 => 1.5,
                9 => 0.2,
                _ => 1.0,
            };
            v as i32
        }
        _ => item.value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades() {
        assert_eq!(grade_name(grade_quality(0)), "Prototype");
        assert_eq!(grade_name(grade_quality(1)), "Shoddy");
        assert_eq!(grade_name(grade_quality(2)), "Standard");
        assert_eq!(grade_name(grade_quality(3)), "High");
        assert_eq!(grade_name(grade_quality(4)), "Specialist");
        assert_eq!(grade_name(grade_quality(5)), "Masterwork");
    }
}
