//! Weapons: damage multipliers from the WEAPON record, and what equipping one sets on the wielder.
//!
//! Specs: sheet rows F047085 Weapon::setupStats, F047095 CharStats::setWeapon; types T00035
//! Weapon, T00034 WeaponCategory, T00026 CharStats.

use crate::Attacker;
use crate::WeaponCategory;
use kenshi_char::medical::lerp;
use kenshi_data::GameData;
use std::collections::HashMap;

/// @type Weapon
#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    pub quality: f32,
    pub cut_mult: f32,
    pub blunt_mult: f32,
    pub min_cut: f32,
    pub bleed: f32,
    pub combat_weight: f32,
    pub inventory_weight: f32,
    pub attack_mod: i32,
    pub defence_mod: i32,
    pub indoors_mod: i32,
    /// Record int "skill category" (+0x234).
    pub category: Option<WeaponCategory>,
}

fn float(r: &GameData, k: &str) -> f32 {
    r.floats.get(k).copied().unwrap_or(0.0)
}

fn int(r: &GameData, k: &str) -> i32 {
    r.ints.get(k).copied().unwrap_or(0)
}

impl Weapon {
    /// @fn F047085
    ///
    /// A weapon of `quality` (0..1) from its WEAPON record, the optional material spec and maker
    /// records; `inventory_weight_mult` is GLOBAL CONSTANTS "weapon inventory weight mult".
    pub fn from_record(
        r: &GameData,
        material: Option<&GameData>,
        maker: Option<&GameData>,
        quality: f32,
        inventory_weight_mult: f32,
    ) -> Weapon {
        let mut cut_mult = lerp(quality, 0.3, 2.0) * float(r, "cut damage multiplier");
        let mut blunt_mult = lerp(quality, 0.0, 2.0) * float(r, "blunt damage multiplier");
        let mut min_cut = float(r, "min cut damage mult");
        let mut w = float(r, "weight kg");
        if let Some(m) = material {
            cut_mult *= float(m, "cut damage mod");
            blunt_mult *= float(m, "blunt damage mod");
            min_cut *= float(m, "min cut damage");
            w *= float(m, "weight mod");
        }
        let heft = blunt_mult * 40.0;
        let mut combat_weight = heft;
        let mut inventory_weight = heft * inventory_weight_mult * float(r, "weight mult");
        if combat_weight < w {
            combat_weight = w;
        }
        if inventory_weight < w {
            inventory_weight = w;
        }
        let mut attack_mod = int(r, "attack mod");
        let mut defence_mod = int(r, "defence mod");
        if let Some(m) = maker {
            defence_mod += int(m, "defence mod");
            attack_mod += int(m, "attack mod");
        }
        Weapon {
            quality,
            cut_mult,
            blunt_mult,
            min_cut,
            bleed: float(r, "bleed mult"),
            combat_weight,
            inventory_weight,
            attack_mod,
            defence_mod,
            indoors_mod: int(r, "indoors mod"),
            category: WeaponCategory::from_i32(int(r, "skill category")),
        }
    }
}

/// What the equipped weapon sets on the wielder's stats (CharStats +0x19c..+0x228).
#[derive(Debug, Clone, PartialEq)]
pub struct Wielded {
    pub cut_factor: f32,
    pub blunt_factor: f32,
    pub pierce_factor: f32,
    pub bleed: f32,
    pub weight: f32,
    pub cut_base: f32,
    pub blunt_base: f32,
    pub robot_damage_mult: f32,
    pub human_damage_mult: f32,
    pub animal_damage_mult: f32,
    pub armour_penetration: f32,
    /// Race string id -> damage factor.
    pub race_damage: HashMap<String, f32>,
    pub length: f32,
    pub category: WeaponCategory,
}

/// @fn F047095
///
/// Equip `weapon` (its item and WEAPON record), or nothing (unarmed). `blunt_damage_1` and
/// `cut_damage_1` are the GLOBAL CONSTANTS curve bottoms.
pub fn set_weapon(
    weapon: Option<(&Weapon, &GameData)>,
    blunt_damage_1: f32,
    cut_damage_1: f32,
) -> Wielded {
    let mut w = Wielded {
        cut_factor: 0.0,
        blunt_factor: 0.0,
        pierce_factor: 0.0,
        bleed: 0.0,
        weight: 0.0,
        cut_base: 0.0,
        blunt_base: 0.0,
        robot_damage_mult: 0.0,
        human_damage_mult: 0.0,
        animal_damage_mult: 0.0,
        armour_penetration: 0.0,
        race_damage: HashMap::new(),
        length: 0.0,
        category: WeaponCategory::SkillUnarmed,
    };
    let Some((item, r)) = weapon else {
        return w;
    };
    w.cut_factor = item.cut_mult;
    w.blunt_factor = item.blunt_mult;
    w.pierce_factor = float(r, "pierce damage multiplier");
    w.bleed = item.bleed;
    w.weight = item.combat_weight;
    w.robot_damage_mult = float(r, "robot damage mult");
    w.human_damage_mult = float(r, "human damage mult");
    w.animal_damage_mult = float(r, "animal damage mult");
    w.armour_penetration = float(r, "armour penetration");
    w.race_damage = r
        .refs("race damage")
        .iter()
        .map(|(sid, v)| (sid.clone(), v[0] as f32 * 0.01))
        .collect();
    if w.cut_factor < w.blunt_factor {
        w.blunt_base = blunt_damage_1;
        w.cut_base = (blunt_damage_1 as f64 * 0.4) as f32;
    } else {
        w.cut_base = cut_damage_1;
    }
    w.length = int(r, "length") as f32;
    if let Some(c) = item.category {
        w.category = c;
    }
    w
}

impl Wielded {
    /// The damage inputs for this weapon, given the wielder's skill and attributes.
    pub fn attacker(
        &self,
        weapon_skill: f32,
        dex: (f32, f32),
        str: (f32, f32),
        scale: f32,
    ) -> Attacker {
        Attacker {
            weapon_skill,
            dexterity: dex.0,
            dexterity_buffed: dex.1,
            strength: str.0,
            strength_buffed: str.1,
            scale,
            cut_factor: self.cut_factor,
            blunt_factor: self.blunt_factor,
            pierce_factor: self.pierce_factor,
            cut_base: self.cut_base,
            blunt_base: self.blunt_base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CombatConstants, blunt_damage, cut_damage};

    fn katana() -> GameData {
        let mut r = GameData::default();
        r.floats.insert("cut damage multiplier".into(), 1.0);
        r.floats.insert("blunt damage multiplier".into(), 0.0);
        r.floats.insert("bleed mult".into(), 1.5);
        r.floats.insert("weight kg".into(), 6.0);
        r.floats.insert("weight mult".into(), 1.0);
        r.ints.insert("skill category".into(), 0);
        r.ints.insert("length".into(), 21);
        r
    }

    #[test]
    fn quality_scales_damage() {
        let r = katana();
        let lo = Weapon::from_record(&r, None, None, 0.0, 0.5);
        let hi = Weapon::from_record(&r, None, None, 1.0, 0.5);
        assert!((lo.cut_mult - 0.3).abs() < 1e-6);
        assert!((hi.cut_mult - 2.0).abs() < 1e-6);
        assert_eq!(hi.combat_weight, 6.0); // no blunt heft: the record weight
        assert_eq!(hi.category, Some(WeaponCategory::SkillKatanas));
        let mut club = r.clone();
        club.floats.insert("blunt damage multiplier".into(), 1.0);
        let c = Weapon::from_record(&club, None, None, 1.0, 0.5);
        assert_eq!(c.combat_weight, 80.0); // 2.0 * 40
        assert_eq!(c.inventory_weight, 40.0);
    }

    #[test]
    fn equipping_sets_curve_bottoms() {
        let r = katana();
        let k = Weapon::from_record(&r, None, None, 0.5, 0.5);
        let w = set_weapon(Some((&k, &r)), 20.0, 20.0);
        assert_eq!((w.cut_base, w.blunt_base), (20.0, 0.0));
        assert_eq!(w.length, 21.0);
        let mut club = r.clone();
        club.floats.insert("cut damage multiplier".into(), 0.1);
        club.floats.insert("blunt damage multiplier".into(), 1.0);
        let cw = Weapon::from_record(&club, None, None, 0.5, 0.5);
        let w = set_weapon(Some((&cw, &club)), 20.0, 20.0);
        assert_eq!(w.blunt_base, 20.0);
        assert!((w.cut_base - 8.0).abs() < 1e-6);
        let c = CombatConstants::default();
        let a = w.attacker(100.0, (100.0, 1.0), (100.0, 1.0), 1.0);
        assert!(blunt_damage(&a, &c) > cut_damage(&a, &c));
        assert_eq!(
            set_weapon(None, 20.0, 20.0).category,
            WeaponCategory::SkillUnarmed
        );
    }
}
