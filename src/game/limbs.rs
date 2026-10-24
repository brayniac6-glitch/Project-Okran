//! Limbs (port_map `medical`): limbs lost to hits and robotic limbs put in their place.
//!
//! Specs: F040472 (a hit taking a limb below minus its most health loses it, `kenshi_char::
//! medical`), F040471 (a limb lost: its slot state 1; the character's toughness trained; a leg
//! lost knocks them down with chance 1 - toughness / 100; blood lost, 5 to 30), the options'
//! limb loss "Rare" (the default: only by a hit), LIMB_REPLACEMENT records (a robotic limb: its
//! skill multipliers "<skill> mult" at the worst and "<skill> mult 1" at the best, its mesh),
//! Kenshi_LimbReplacementWindow.layout (arms and legs, a slot each).
//! (assumption) A robotic limb goes only where a limb is lost; it fills the slot (state 2) at
//! its full health; items have no quality yet, so a limb's multipliers are halfway between its
//! worst and best; each skill's multiplier is the average of the four limbs' (a flesh limb 1);
//! the toughness training is 3 units of experience.

use crate::sim::Sim;
use kenshi_char::StatsEnumerated;
use kenshi_char::inventory::Item;
use kenshi_data::GameData;

/// The limb slots: left arm, right arm, left leg, right leg (Kenshi_LimbReplacementWindow's
/// left_arm_replace, right_arm_replace, left_leg_replace, right_leg_replace).
pub const SLOT_NAMES: [&str; 4] = ["left arm", "right arm", "left leg", "right leg"];

/// A LIMB_REPLACEMENT's slot by its name ("Arm (left)", "Leg (right)").
pub fn item_slot(name: &str) -> Option<usize> {
    let n = name.to_lowercase();
    let left = n.contains("left");
    let right = n.contains("right");
    let arm = n.contains("arm");
    let leg = n.contains("leg");
    match (arm, leg, left, right) {
        (true, false, true, false) => Some(0),
        (true, false, false, true) => Some(1),
        (false, true, true, false) => Some(2),
        (false, true, false, true) => Some(3),
        _ => None,
    }
}

/// The skill multipliers a LIMB_REPLACEMENT gives (halfway between its worst and best).
fn limb_mults(r: &GameData) -> Vec<(StatsEnumerated, f32)> {
    use StatsEnumerated::*;
    let f = |k: &str| r.floats.get(k).copied().unwrap_or(1.0);
    let mid = |k: &str| (f(&format!("{k} mult")) + f(&format!("{k} mult 1"))) * 0.5;
    vec![
        (StatDexterity, mid("dexterity")),
        (StatStrength, mid("strength")),
        (StatAthletics, mid("athletics")),
        (StatSwimming, mid("swimming")),
        (StatStealth, mid("stealth")),
        (StatThieving, mid("thievery")),
        (StatCrossbows, mid("ranged")),
    ]
}

impl Sim {
    /// The body part (index) of person `id` in limb slot `slot`, by its LOCATIONAL_DAMAGE name.
    pub fn limb_part(&self, id: usize, slot: usize) -> Option<usize> {
        let (arm, left) = (slot < 2, slot % 2 == 0);
        self.arena.f[id].medical.parts.iter().position(|p| {
            let name = self.data.get(&p.id).map_or(String::new(), |r| r.name.to_lowercase());
            (if arm { p.kind == 2 } else { p.kind == 1 }) && name.contains(if left { "left" } else { "right" })
        })
    }

    /// Whether person `id` has lost the limb in `slot` (and has nothing in its place).
    pub fn limb_lost(&self, id: usize, slot: usize) -> bool {
        self.limb_part(id, slot).is_some_and(|i| self.arena.f[id].medical.parts[i].slot == 1)
    }

    /// Put robotic limb `item` in its slot of person `id` (where the limb is lost).
    pub fn attach_limb(&mut self, id: usize, item: Item) -> Result<(), Item> {
        let Some(slot) = item_slot(&item.name) else { return Err(item) };
        if !self.limb_lost(id, slot) || self.folk[id].limbs[slot].is_some() {
            return Err(item);
        }
        let Some(i) = self.limb_part(id, slot) else { return Err(item) };
        let p = &mut self.arena.f[id].medical.parts[i];
        p.slot = 2;
        p.robotic = true;
        p.flesh = p.max_health();
        p.stun = 0.0;
        p.treatment = 0.0;
        self.folk[id].limbs[slot] = Some(item);
        self.limb_mults(id);
        Ok(())
    }

    /// Take the robotic limb out of `slot` of person `id`: the limb is lost again.
    pub fn detach_limb(&mut self, id: usize, slot: usize) -> Option<Item> {
        let item = self.folk[id].limbs[slot].take()?;
        if let Some(i) = self.limb_part(id, slot) {
            let p = &mut self.arena.f[id].medical.parts[i];
            p.slot = 1;
            p.robotic = false;
            p.flesh = -p.max_health() - 1.0;
        }
        self.limb_mults(id);
        Some(item)
    }

    /// Person `id`'s skill multipliers from their robotic limbs.
    pub(crate) fn limb_mults(&mut self, id: usize) {
        let mut sum: std::collections::HashMap<StatsEnumerated, f32> = std::collections::HashMap::new();
        for slot in 0..4 {
            let ms = self.folk[id].limbs[slot].as_ref().and_then(|it| self.data.get(&it.record)).map(limb_mults);
            for s in [
                StatsEnumerated::StatDexterity,
                StatsEnumerated::StatStrength,
                StatsEnumerated::StatAthletics,
                StatsEnumerated::StatSwimming,
                StatsEnumerated::StatStealth,
                StatsEnumerated::StatThieving,
                StatsEnumerated::StatCrossbows,
            ] {
                let m = ms.as_ref().and_then(|v| v.iter().find(|(k, _)| *k == s).map(|(_, m)| *m)).unwrap_or(1.0);
                *sum.entry(s).or_insert(0.0) += m * 0.25;
            }
        }
        self.arena.f[id].skills.mults = sum.into_iter().filter(|(_, m)| (*m - 1.0).abs() > 1e-4).collect();
    }

    /// @fn F040471
    ///
    /// The limbs hits severed this tick: blood lost, toughness trained, a leg's loss knocking
    /// them down by chance, a message, and the limb dropped: the race's "severed limbs" item for
    /// the slot (its first value the slot: left arm 0, right arm 1, left leg 2, right leg 3)
    /// dropped with physics on (Item vtable 0x228, F043877 setting +0x1da) at the part's "bone
    /// name 2" bone (`limb_drops`, the game's to place). Not for one being carried (Character
    /// vtable 0x248).
    pub fn limb_tick(&mut self) {
        for id in 0..self.folk.len() {
            let severed = std::mem::take(&mut self.arena.f[id].medical.severed);
            if self.folk[id].carried_by.is_some() {
                continue;
            }
            for i in severed {
                // the limb as an item, lying where they stand until the game puts it at the bone
                let slot = (0..4).find(|&s| self.limb_part(id, s) == Some(i));
                // (one with no character record, as the harness's squad: Greenlander, `race_of`'s
                // own fallback race)
                let race = self.race_of(id).or_else(|| self.data.of_type(kenshi_data::ItemType::Race).find(|r| r.name == "Greenlander").map(|r| r.string_id.clone()));
                let record = slot.and_then(|s| {
                    let r = self.data.get(race.as_ref()?)?;
                    r.refs("severed limbs").iter().find(|(_, v)| v.first() == Some(&(s as i32))).map(|(sid, _)| sid.clone())
                });
                if std::env::var("KENSHI_RAGDOLL_DEBUG").is_ok() {
                    eprintln!("limb lost by {id}: part {i} slot {slot:?} race {race:?} item {record:?}");
                }
                if let Some(rec) = record.and_then(|sid| self.data.get(&sid)) {
                    let item = kenshi_char::inventory::Item::from_record(rec, 1);
                    self.next_ground += 1;
                    let at = self.folk[id].mover.position_units();
                    self.ground.push(crate::loot::GroundItem { id: self.next_ground, item, at, yaw: 0.0 });
                    self.limb_drops.push((self.next_ground, id, i));
                    // (with no game to take them, the queue keeps only the newest)
                    if self.limb_drops.len() > 32 {
                        self.limb_drops.remove(0);
                    }
                }
                let kind = self.arena.f[id].medical.parts[i].kind;
                let loss = 5.0 + self.rng.uniform() * 25.0;
                self.arena.f[id].medical.blood -= loss;
                self.arena.f[id].skills.train(StatsEnumerated::StatToughness, 3.0, 1.0);
                if kind == 1 {
                    let tough = self.arena.f[id].level(StatsEnumerated::StatToughness);
                    if self.rng.uniform() < (1.0 - tough * 0.01).clamp(0.0, 1.0) {
                        self.knock_out(id);
                    }
                }
                let part = self.data.get(&self.arena.f[id].medical.parts[i].id).map_or("limb".to_string(), |r| r.name.to_lowercase());
                self.bark(id, id, if kind == 1 { crate::speech::EV_LOST_LEG } else { crate::speech::EV_LOST_ARM });
                if self.folk[id].player {
                    self.messages.push(format!("{} has lost their {part}!", self.display_name(id)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_by_name() {
        assert_eq!(item_slot("Skeleton Arm (left)"), Some(0));
        assert_eq!(item_slot("Economy Leg (right)"), Some(3));
        assert_eq!(item_slot("Industrial Lifter Arm (right)"), Some(1));
        assert_eq!(item_slot("Nothing"), None);
    }
}
