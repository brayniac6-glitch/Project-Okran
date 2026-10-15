//! Armour items: resistances from the ARMOUR record, the material/class tables and item quality.
//!
//! Specs: sheet rows F047099 Armour::Armour, F047256 armourTables; types T00030 Armour,
//! T00032 ArmourType, T00033 ArmourClass.

use crate::hit::Armour;
use crate::{ArmourClass, ArmourType};
use kenshi_char::medical::lerp;
use kenshi_data::GameData;
use std::collections::HashMap;

/// One row of the material tables (by [`ArmourType`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct MaterialRow {
    cut: f32,
    blunt: f32,
    pierce: f32,
    stealth: f32,
}

/// One row of the class tables (by [`ArmourClass`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ClassRow {
    cut: f32,
    blunt: f32,
    stealth: f32,
}

const fn m(cut: f32, blunt: f32, pierce: f32, stealth: f32) -> MaterialRow {
    MaterialRow {
        cut,
        blunt,
        pierce,
        stealth,
    }
}

const fn c(cut: f32, blunt: f32, stealth: f32) -> ClassRow {
    ClassRow {
        cut,
        blunt,
        stealth,
    }
}

/// @fn F047256
///
/// Material tables at quality 0 and 1: cloth, leather, chain, plate.
const MATERIAL_LO: [MaterialRow; 4] = [
    m(0.0, 0.0, 0.0, 1.0),
    m(0.0, 0.0, 0.0, 1.0),
    m(0.05, 0.0, 0.0, 0.3),
    m(0.1, 0.0, 0.0, 0.1),
];
const MATERIAL_HI: [MaterialRow; 4] = [
    m(0.1, 0.3, 0.0, 1.0),
    m(0.55, 0.4, 35.0, 1.0),
    m(0.65, 0.3, 60.0, 0.7),
    m(0.85, 0.6, 90.0, 0.8),
];
/// Class tables at quality 0 and 1: cloth, light, medium, heavy.
const CLASS_LO: [ClassRow; 4] = [
    c(0.4, 0.1, 1.0),
    c(0.8, 0.7, 1.0),
    c(0.9, 0.9, 0.9),
    c(1.0, 1.0, 0.0),
];
const CLASS_HI: [ClassRow; 4] = [
    c(0.4, 0.1, 1.0),
    c(0.8, 0.7, 1.0),
    c(0.9, 0.9, 0.9),
    c(1.0, 1.0, 0.4),
];

/// ARMOUR "slot" values with their own rules.
pub const SLOT_HEAD: i32 = 3;
pub const SLOT_SHIRT: i32 = 8;

impl Armour {
    /// @fn F047099
    ///
    /// An armour piece of `quality` (0..1) from its ARMOUR record. `part_index` maps a body-part
    /// (LOCATIONAL_DAMAGE) string id to the wearer's part index. Returns `None` for a material
    /// or class outside the tables.
    pub fn from_record(
        r: &GameData,
        quality: f32,
        part_index: impl Fn(&str) -> Option<usize>,
    ) -> Option<Armour> {
        let int = |k: &str| r.ints.get(k).copied().unwrap_or(0);
        let float = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
        let material = ArmourType::from_i32(int("material type"))? as usize;
        let class = ArmourClass::from_i32(int("class"))? as usize;
        if class >= CLASS_LO.len() {
            return None;
        }
        let covered = r.refs("part coverage");
        let quality = if covered.is_empty() { 1.0 } else { quality };
        let t = quality + int("level bonus") as f32 * 0.01;
        let (lo, hi) = (MATERIAL_LO[material], MATERIAL_HI[material]);
        let class_lo = CLASS_LO[class];

        let mut cut = lerp(t, lo.cut, hi.cut) * class_lo.cut + lerp(t, 0.0, float("cut def bonus"));
        cut = cut.clamp(0.0, 0.9);
        let mut pierce = lerp(t, lo.pierce, hi.pierce) * float("pierce def mult");
        let mut blunt =
            lerp(t, lo.blunt, hi.blunt) * class_lo.blunt + lerp(t, 0.0, float("blunt def bonus"));
        blunt = blunt.clamp(0.0, 0.8);
        if !covered.is_empty() && int("slot") == SLOT_SHIRT {
            cut *= 0.8;
            blunt *= 0.8;
            pierce *= 0.5;
        }
        let coverage: HashMap<usize, f32> = covered
            .iter()
            .filter_map(|(sid, v)| Some((part_index(sid)?, v[0] as f32 * 0.01)))
            .collect();
        Some(Armour {
            cut_resistance: cut,
            blunt_resistance: blunt,
            pierce_reduction: pierce,
            cut_into_stun: float("cut into stun").clamp(0.0, 1.0),
            material: material as i32,
            coverage,
        })
    }
}

/// The stealth multiplier an armour piece gives (F047099, +0x25c).
pub fn stealth_mult(r: &GameData, quality: f32) -> Option<f32> {
    let int = |k: &str| r.ints.get(k).copied().unwrap_or(0);
    let material = ArmourType::from_i32(int("material type"))? as usize;
    let class = ArmourClass::from_i32(int("class"))? as usize;
    if class >= CLASS_LO.len() {
        return None;
    }
    let covered = !r.refs("part coverage").is_empty();
    let t = if covered { quality } else { 1.0 } + int("level bonus") as f32 * 0.01;
    let mut s = lerp(
        t,
        MATERIAL_LO[material].stealth,
        MATERIAL_HI[material].stealth,
    ) * lerp(t, CLASS_LO[class].stealth, CLASS_HI[class].stealth);
    let rec = r.floats.get("stealth mult").copied().unwrap_or(0.0);
    s = if rec < 0.0 { 1.0 } else { s * rec };
    if covered && int("slot") == SLOT_HEAD && s < 1.0 {
        s = 1.0;
    }
    Some(s)
}

/// Class tables' athletics and combat speed factors (F047256, row +0x10 and +0x14) at quality 0
/// and 1: cloth, light, medium, heavy.
const CLASS_MOVE_LO: [(f32, f32); 4] = [(1.0, 1.0), (1.0, 1.0), (0.85, 1.0), (0.8, 0.9)];
const CLASS_MOVE_HI: [(f32, f32); 4] = [(1.0, 1.0), (1.0, 1.0), (1.0, 1.0), (0.95, 1.0)];
pub const SLOT_BODY: i32 = 5;

/// What one armour piece does to its wearer besides covering them (F047099, the fields after the
/// resistances): integer bonuses to attack, defence, unarmed and perception, and multipliers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PieceMods {
    /// "combat attk bonus" (+0x24c), "combat def bonus" (+0x250); leg armour (slot 6) has
    /// none.
    pub attack: i32,
    pub defence: i32,
    /// "unarmed bonus" (+0x270), "perception bonus" (+0x254).
    pub unarmed: i32,
    pub perception: i32,
    /// "dodge mult" (+0x26c), "ranged skill mult" (+0x27c), "dexterity mult" (+0x264),
    /// "damage output mult" (+0x268), "asassination mult" (+0x260), "fist injury mult" (+0x274).
    pub dodge: f32,
    pub ranged: f32,
    pub dexterity: f32,
    pub damage_output: f32,
    pub assassination: f32,
    pub fist_injury: f32,
    /// The stealth multiplier (+0x25c, [`stealth_mult`]).
    pub stealth: f32,
    /// Athletics (+0x248): lerp(quality, the class's athletics) x "athletics mult", 1 for a
    /// helmet, body armour or shirt; combat speed (+0x258): lerp(quality, the class's combat
    /// speed) x "combat speed mult", 1 for a helmet.
    pub athletics: f32,
    pub combat_speed: f32,
}

/// @fn F047099
///
/// The wearer modifiers of an ARMOUR record at `quality` (0..1), as Armour::Armour reads them.
/// (assumption) "dexterity mult" and "damage output mult" are read when the record has them
/// (the original reads them behind a key-present test), else 1.
pub fn piece_mods(r: &GameData, quality: f32) -> Option<PieceMods> {
    let int = |k: &str| r.ints.get(k).copied().unwrap_or(0);
    let float = |k: &str, d: f32| r.floats.get(k).copied().unwrap_or(d);
    let class = ArmourClass::from_i32(int("class"))? as usize;
    if class >= CLASS_MOVE_LO.len() {
        return None;
    }
    let covered = !r.refs("part coverage").is_empty();
    let t = if covered { quality } else { 1.0 } + int("level bonus") as f32 * 0.01;
    let slot = int("slot");
    let mut athletics = lerp(t, CLASS_MOVE_LO[class].0, CLASS_MOVE_HI[class].0) * float("athletics mult", 1.0);
    if matches!(slot, SLOT_HEAD | SLOT_BODY | SLOT_SHIRT) {
        athletics = 1.0;
    }
    let mut combat_speed = lerp(t, CLASS_MOVE_LO[class].1, CLASS_MOVE_HI[class].1) * float("combat speed mult", 1.0);
    if slot == SLOT_HEAD {
        combat_speed = 1.0;
    }
    let (mut attack, mut defence) = (int("combat attk bonus"), int("combat def bonus"));
    if slot == 6 {
        attack = 0;
        defence = 0;
    }
    Some(PieceMods {
        attack,
        defence,
        unarmed: int("unarmed bonus"),
        perception: int("perception bonus"),
        dodge: float("dodge mult", 1.0),
        ranged: float("ranged skill mult", 1.0),
        dexterity: float("dexterity mult", 1.0),
        damage_output: float("damage output mult", 1.0),
        assassination: float("asassination mult", 1.0),
        fist_injury: float("fist injury mult", 1.0),
        stealth: stealth_mult(r, quality).unwrap_or(1.0),
        athletics,
        combat_speed,
    })
}

/// The worn armour's modifiers together (CharStats +0x18..+0x50, F047071 -> F047028).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GearMods {
    /// The armour's attack and defence bonuses summed (+0x20 and +0x24 before the weapon's
    /// mods), unarmed (+0x2c) and perception (+0x30) bonuses.
    pub attack: i32,
    pub defence: i32,
    pub unarmed: i32,
    pub perception: i32,
    /// Multipliers: each piece's product. Athletics (+0x18) is the highest piece's times the
    /// lowest's.
    pub athletics: f32,
    pub combat_speed: f32,
    pub stealth: f32,
    pub dodge: f32,
    pub fist_injury: f32,
    pub ranged: f32,
    pub dexterity: f32,
    pub assassination: f32,
    pub damage_output: f32,
}

impl Default for GearMods {
    fn default() -> Self {
        gear_mods(&[], None)
    }
}

/// A worn backpack's modifiers (CONTAINER, F043920: "athletics mult" +0x238, "combat speed mult"
/// +0x240, "combat skill bonus" +0x244, "stealth mult" +0x248).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BagMods {
    pub athletics: f32,
    pub combat_speed: f32,
    pub skill_bonus: i32,
    pub stealth: f32,
}

impl BagMods {
    pub fn from_record(r: &GameData) -> BagMods {
        let float = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|&v| v as f32)).unwrap_or(1.0);
        BagMods {
            athletics: float("athletics mult"),
            combat_speed: float("combat speed mult"),
            skill_bonus: r.ints.get("combat skill bonus").copied().or_else(|| r.floats.get("combat skill bonus").map(|&v| v as i32)).unwrap_or(0),
            stealth: float("stealth mult"),
        }
    }
}

/// @fn F047071
///
/// The pieces' modifiers summed and multiplied, starting from the worn backpack's (`bag`): its
/// combat skill bonus starts the attack and defence sums, its combat speed the combat speed
/// product, its stealth the stealth and (as the original has it) the dodge products, its
/// athletics the lowest athletics. Wearing nothing at all (and no backpack) one is nimbler:
/// dodge and dexterity x1.1, stealth x1.2.
pub fn gear_mods(pieces: &[PieceMods], bag: Option<BagMods>) -> GearMods {
    let mut g = GearMods {
        attack: 0,
        defence: 0,
        unarmed: 0,
        perception: 0,
        athletics: 1.0,
        combat_speed: 1.0,
        stealth: 1.0,
        dodge: 1.0,
        fist_injury: 1.0,
        ranged: 1.0,
        dexterity: 1.0,
        assassination: 1.0,
        damage_output: 1.0,
    };
    if let Some(b) = bag {
        g.attack = b.skill_bonus;
        g.defence = b.skill_bonus;
        g.combat_speed = b.combat_speed;
        g.stealth = b.stealth;
        g.dodge = b.stealth;
    }
    if pieces.is_empty() {
        match bag {
            None => {
                g.dodge = 1.1;
                g.stealth = 1.2;
                g.dexterity = 1.1;
            }
            Some(b) => g.athletics = b.athletics,
        }
        return g;
    }
    let (mut lo, mut hi) = (bag.map_or(1.0f32, |b| b.athletics), 1.0f32);
    for p in pieces {
        g.attack += p.attack;
        g.defence += p.defence;
        g.unarmed += p.unarmed;
        g.perception += p.perception;
        g.fist_injury *= p.fist_injury;
        g.stealth *= p.stealth;
        g.dodge *= p.dodge;
        g.ranged *= p.ranged;
        g.assassination *= p.assassination;
        g.dexterity *= p.dexterity;
        g.damage_output *= p.damage_output;
        g.combat_speed *= p.combat_speed;
        lo = lo.min(p.athletics);
        hi = hi.max(p.athletics);
    }
    g.athletics = hi * lo;
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(material: i32, class: i32, slot: i32) -> GameData {
        let mut r = GameData::default();
        r.ints.insert("material type".into(), material);
        r.ints.insert("class".into(), class);
        r.ints.insert("slot".into(), slot);
        r.floats.insert("pierce def mult".into(), 1.0);
        r.floats.insert("cut into stun".into(), 1.5);
        r.floats.insert("stealth mult".into(), 1.0);
        r.references.push((
            "part coverage".into(),
            vec![("32-gamedata.quack".into(), [95, 0, 0])],
        ));
        r
    }

    #[test]
    fn plate_by_quality() {
        let head = |sid: &str| (sid == "32-gamedata.quack").then_some(0);
        let r = record(3, 3, 5);
        let low = Armour::from_record(&r, 0.0, head).unwrap();
        let high = Armour::from_record(&r, 1.0, head).unwrap();
        assert!((low.cut_resistance - 0.1).abs() < 1e-6);
        assert!((high.cut_resistance - 0.85).abs() < 1e-6);
        assert!((high.blunt_resistance - 0.6).abs() < 1e-6);
        assert_eq!(high.pierce_reduction, 90.0);
        assert_eq!(high.cut_into_stun, 1.0); // clamped
        assert_eq!(high.coverage[&0], 0.95);
        // a light-class shirt
        let shirt = Armour::from_record(&record(2, 1, SLOT_SHIRT), 1.0, head).unwrap();
        assert!((shirt.cut_resistance - 0.65 * 0.8 * 0.8).abs() < 1e-6);
        assert!((shirt.pierce_reduction - 30.0).abs() < 1e-6);
        // heavy plate at quality 0 hides nothing; a helmet never lowers stealth
        assert_eq!(stealth_mult(&r, 0.0), Some(0.0));
        assert_eq!(stealth_mult(&record(3, 3, SLOT_HEAD), 0.0), Some(1.0));
    }

    #[test]
    fn heavy_armour_slows_and_naked_is_nimble() {
        let mut r = record(3, 3, 7);
        r.floats.insert("combat speed mult".into(), 0.94);
        r.ints.insert("combat attk bonus".into(), -8);
        let p = piece_mods(&r, 0.0).unwrap();
        assert!((p.combat_speed - 0.9 * 0.94).abs() < 1e-6);
        assert!((p.athletics - 0.8).abs() < 1e-6);
        let g = gear_mods(&[p], None);
        assert_eq!(g.attack, -8);
        assert!((g.athletics - 0.8).abs() < 1e-6);
        let none = gear_mods(&[], None);
        assert_eq!((none.dodge, none.stealth, none.dexterity), (1.1, 1.2, 1.1));
    }
}
