//! Combat (port_map `combat`): damage from skills and attributes, block chance. Written from the
//! sheet's specs: F047014 cutDamage, F047016 bluntDamage, F047017 pierceDamage, F047056 blockChance;
//! hit resolution in [`hit`], armour in [`armour`], weapons in [`weapon`].

pub mod arena;
pub mod armour;
pub mod armour_class;
pub mod armour_type;
pub mod attack;
pub mod engage;
pub mod fighter;
pub mod hit;
pub mod melee;
pub mod ranged;
pub mod states;
pub mod stumble;
pub mod sword_state;
pub mod technique;
pub mod weapon;
pub mod weapon_category;

pub use armour_class::ArmourClass;
pub use armour_type::ArmourType;
pub use sword_state::SwordState;
pub use weapon_category::WeaponCategory;

/// The GLOBAL CONSTANTS values combat reads (T00025, by record key; shipped values as defaults).
#[derive(Debug, Clone, PartialEq)]
pub struct CombatConstants {
    pub cut_damage_99: f32,
    pub blunt_damage_99: f32,
    pub pierce_damage_multiplier: f32,
    pub base_block_chance: f32,
    pub block_chance_reduction: f32,
    pub block_chance_increase: f32,
}

impl Default for CombatConstants {
    fn default() -> Self {
        CombatConstants {
            cut_damage_99: 80.0,
            blunt_damage_99: 80.0,
            pierce_damage_multiplier: 1.2,
            base_block_chance: 70.0,
            block_chance_reduction: 1.5,
            block_chance_increase: 1.2,
        }
    }
}

impl CombatConstants {
    /// @type GlobalConstants
    pub fn from_floats(get: impl Fn(&str) -> Option<f32>) -> Self {
        let d = CombatConstants::default();
        CombatConstants {
            cut_damage_99: get("cut damage 99").unwrap_or(d.cut_damage_99),
            blunt_damage_99: get("blunt damage 99").unwrap_or(d.blunt_damage_99),
            pierce_damage_multiplier: get("pierce damage multiplier")
                .unwrap_or(d.pierce_damage_multiplier),
            base_block_chance: get("base block chance").unwrap_or(d.base_block_chance),
            // stored * 0.1: the record's "per 10 levels" becomes per level
            block_chance_reduction: get("block chance reduction per 10levels")
                .map_or(d.block_chance_reduction, |v| v * 0.1),
            block_chance_increase: get("block chance increase per 10levels")
                .map_or(d.block_chance_increase, |v| v * 0.1),
        }
    }
}

/// The attacker's inputs to a damage calculation (CharStats fields named in the specs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attacker {
    pub weapon_skill: f32,
    pub dexterity: f32,
    /// Dexterity with buffs/penalties applied (the "buffed" getter in the specs).
    pub dexterity_buffed: f32,
    pub strength: f32,
    pub strength_buffed: f32,
    /// CharStats +0x34 (not yet identified; 1.0 = none).
    pub scale: f32,
    /// Per-weapon factors: stats +0x19c (cut), +0x1a0 (blunt), +0x1a8 (pierce).
    pub cut_factor: f32,
    pub blunt_factor: f32,
    pub pierce_factor: f32,
    /// Damage at the bottom of the curve: stats +0x1ac (cut), +0x1b0 (blunt).
    pub cut_base: f32,
    pub blunt_base: f32,
}

/// @fn F047014
pub fn cut_damage(a: &Attacker, c: &CombatConstants) -> f32 {
    let v = (a.weapon_skill + a.dexterity_buffed * a.dexterity * a.scale) * a.cut_factor / 200.0;
    (c.cut_damage_99 - a.cut_base) * v + a.cut_base
}

/// @fn F047016
pub fn blunt_damage(a: &Attacker, c: &CombatConstants) -> f32 {
    let v = (a.weapon_skill * 0.5 + a.strength_buffed * a.strength * 1.5) * a.blunt_factor / 200.0;
    (c.blunt_damage_99 - a.blunt_base) * v + a.blunt_base
}

/// @fn F047017
pub fn pierce_damage(a: &Attacker, c: &CombatConstants) -> f32 {
    c.pierce_damage_multiplier
        * (a.weapon_skill + a.dexterity_buffed * a.dexterity * a.scale)
        * a.pierce_factor
        + a.pierce_factor * 10.0
}

/// @fn F047056
///
/// Chance (0..95) to block, by the gap `d` = defender's defence level - attacker's skill level.
pub fn block_chance(d: f32, c: &CombatConstants) -> f32 {
    let mut chance = if d <= 0.0 {
        c.base_block_chance + c.block_chance_reduction * d
    } else {
        c.base_block_chance + c.block_chance_increase * d
    };
    if chance > 90.0 {
        chance = (chance - 90.0) * 0.2 + 90.0;
    }
    // not `clamp`: the original tests `<= 95` first, so a NaN chance comes out as 95
    #[allow(clippy::manual_clamp, clippy::neg_cmp_op_on_partial_ord)]
    if !(chance <= 95.0) {
        95.0
    } else if chance < 0.0 {
        0.0
    } else {
        chance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attacker(skill: f32, attr: f32) -> Attacker {
        Attacker {
            weapon_skill: skill,
            dexterity: attr,
            dexterity_buffed: 1.0,
            strength: attr,
            strength_buffed: 1.0,
            scale: 1.0,
            cut_factor: 1.0,
            blunt_factor: 1.0,
            pierce_factor: 1.0,
            cut_base: 20.0,
            blunt_base: 20.0,
        }
    }

    #[test]
    fn damage_rises_from_base_to_99_value_with_skill() {
        let c = CombatConstants::default();
        assert_eq!(cut_damage(&attacker(0.0, 0.0), &c), 20.0);
        assert_eq!(cut_damage(&attacker(100.0, 100.0), &c), 80.0); // (100 + 100) / 200 = 1
        assert_eq!(cut_damage(&attacker(50.0, 50.0), &c), 50.0);
        // blunt leans on strength: 0.5 * skill + 1.5 * strength
        assert_eq!(blunt_damage(&attacker(100.0, 0.0), &c), 20.0 + 60.0 * 0.25);
        assert_eq!(blunt_damage(&attacker(0.0, 100.0), &c), 20.0 + 60.0 * 0.75);
        assert!((pierce_damage(&attacker(10.0, 10.0), &c) - (1.2 * 20.0 + 10.0)).abs() < 1e-5);
    }

    #[test]
    fn block_chance_soft_and_hard_caps() {
        let c = CombatConstants::default();
        assert_eq!(block_chance(0.0, &c), 70.0);
        assert!((block_chance(10.0, &c) - 82.0).abs() < 1e-4); // 10 levels better
        assert!((block_chance(-20.0, &c) - 40.0).abs() < 1e-4);
        assert!((block_chance(20.0, &c) - (90.0 + (94.0 - 90.0) * 0.2)).abs() < 1e-4);
        assert_eq!(block_chance(100.0, &c), 95.0);
        assert_eq!(block_chance(-100.0, &c), 0.0);
        assert_eq!(block_chance(f32::NAN, &c), 95.0); // as the original
    }
}
