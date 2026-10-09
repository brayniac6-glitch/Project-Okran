//! Disguises (port_map `disguise`): faction uniforms worn as a disguise.
//!
//! What the game's data and text give: a CHARACTER with "wears uniform" has its clothing
//! "branded as a faction uniform" (fcs.def: it "reduces re-sale value and also means wearing it
//! has implications"); "If you have the complete outfit, you can use faction uniforms to
//! disguise yourself. Disguise success is dependent on the Stealth skill stat."; the character's
//! state then reads "You are currently disguised as {1}" ("In Disguise", the slave state 1 that
//! F039148 reads, the status panel F047087); seen through, "You've been discovered, somebody has
//! seen through your disguise." ("Disguise Blown!") and a cooldown ("disguiseblown", the state's
//! +0x110, F040034) before it works again.
//!
//! Here: an NPC of a "wears uniform" character has its armour branded with its faction when
//! kitted (`Item::uniform`); a character whose worn clothing is all one faction's uniform, at
//! least two pieces and one of them on the body, is disguised as that faction while not blown;
//! that faction's people take it for one of their own (the AI sensor, the guards, the slavers'
//! recognition), and each of them that sees it within 500 units sees through it on a roll every
//! 2 s of 0.05 x (1 - its stealth level), blowing it for 60 s.
//! (stand-in: the game's outfit test, its see-through roll and its cooldown length are not
//! decompiled; the roll is F046538's sneaking chance, as its tutorial ties disguise to Stealth.
//! The uniforms' lower resale value is not ported.)

use crate::sim::Sim;
use kenshi_char::StatsEnumerated;
use kenshi_data::ItemType;

/// How long a blown disguise stays blown (seconds). (stand-in)
pub const BLOWN_SECONDS: f32 = 60.0;

impl Sim {
    /// A "wears uniform" character's clothing branded with its faction (at its kitting).
    pub fn brand_uniform(&mut self, id: usize) {
        let p = &self.folk[id];
        let Some(faction) = p.faction.clone() else { return };
        let wears = p.character.as_deref().and_then(|c| self.data.get(c)).is_some_and(|c| c.bools.get("wears uniform").copied().unwrap_or(false));
        if !wears {
            return;
        }
        for s in self.folk[id].inventory.sections.iter_mut().filter(|s| s.is_equipment()) {
            for it in s.items.iter_mut().filter(|it| it.item.kind == Some(ItemType::Armour)) {
                it.item.uniform = Some(faction.clone());
            }
        }
    }

    /// The faction whose complete uniform `id` wears, if any (blown or not).
    pub fn uniform_of(&self, id: usize) -> Option<String> {
        let worn: Vec<&kenshi_char::inventory::Item> = self.folk[id].inventory.equipped().filter(|i| i.kind == Some(ItemType::Armour)).collect();
        if worn.len() < 2 || !worn.iter().any(|i| i.slot == 5 || i.slot == 8) {
            return None;
        }
        let f = worn[0].uniform.clone()?;
        worn.iter().all(|i| i.uniform.as_deref() == Some(f.as_str())).then_some(f)
    }

    /// Every second: each character's disguise (its uniform, unless blown); the blown timers
    /// run down; the disguise's faction's people near who see the disguised roll to see
    /// through it.
    pub fn disguise_tick(&mut self, dt: f32) {
        for p in self.folk.iter_mut() {
            p.disguise_blown = (p.disguise_blown - dt).max(0.0);
        }
        if !self.tick.is_multiple_of(60) {
            return;
        }
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            if p.dead {
                if p.disguised.is_some() {
                    self.folk[id].disguised = None;
                }
                continue;
            }
            let now = if p.disguise_blown > 0.0 { None } else { self.uniform_of(id) };
            if now != self.folk[id].disguised {
                if let Some(f) = &now
                    && self.folk[id].player
                {
                    let name = self.data.get(f).map_or(f.clone(), |r| r.name.clone());
                    self.messages.push(format!("{}: You are currently disguised as {name}", self.display_name(id)));
                }
                self.folk[id].disguised = now.clone();
            }
            let Some(f) = now else { continue };
            // seeing through: its faction's people near who see it
            let at = self.folk[id].mover.position_units();
            let stealth = (self.arena.f[id].level(StatsEnumerated::StatStealth) * 0.01).clamp(0.0, 1.0);
            let window = (self.t / 2.0).max(0.0) as u64;
            let seer = (0..self.folk.len()).find(|&w| {
                let q = &self.folk[w];
                if w == id || q.player || q.down || q.dead || q.away || q.faction.as_deref() != Some(f.as_str()) {
                    return false;
                }
                let r = q.mover.position_units();
                (r[0] - at[0]).hypot(r[2] - at[2]) < 500.0 && self.can_see(w, id) && see_through_roll(w, id, window) < 0.05 * (1.0 - stealth)
            });
            if seer.is_some() {
                self.folk[id].disguise_blown = BLOWN_SECONDS;
                self.folk[id].disguised = None;
                if self.folk[id].player {
                    self.messages.push("You've been discovered, somebody has seen through your disguise.".to_string());
                }
            }
        }
    }

    /// Whether `id` passes as one of `faction`'s own (disguised as it).
    pub fn passes_as(&self, id: usize, faction: &str) -> bool {
        self.folk[id].disguised.as_deref() == Some(faction)
    }
}

/// A roll for observer `w` and the disguised `id` in 2 s window `window` (0 .. 1).
fn see_through_roll(w: usize, id: usize, window: u64) -> f32 {
    let mut z = (w as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93) ^ (id as u64).rotate_left(23) ^ window.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}
