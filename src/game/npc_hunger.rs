//! NPC hunger (port_map `char`): everyone's hunger falls (F039255 runs MedicalSystem's
//! updateHunger, F040466, for every character), and whenever the stomach is empty and the
//! hunger is under the character's cap, F040466 puts the difference (at most 1.0 a time,
//! `STOMACH_CHUNK`) back into the stomach: a character never goes hungrier than its cap.
//!
//! F039188 (Character vtable +0x348), the cap: the player's people -10 (no refill: they must
//! eat); a character held (its state +0x2f8 at 2, its holder the handle at +0x300): -10 when
//! the player holds it (the player must feed its prisoners), else 2.0 (1.15 for a character of
//! the other kind, +0x1a0, not traced: none here); anyone else its squad's hunger, F045433: 3.0
//! (full), or for a "malnourished" squad template 1.0 + 1.2 x random (the hungry bandits).
//! (assumption) A new NPC starts at its cap; held is caged (`Person::prison`) or the player's
//! slave. (not ported: the 1.0 of a character of kind 1, and the 3.0 of a subclass whose
//! vtable +0x248 answers, both untraced kinds)
use crate::sim::Sim;
use kenshi_data::GameData;

/// F045433: a squad's hunger when its template does not make it malnourished.
pub const FULL: f32 = 3.0;
/// F039188: the cap of one held by another faction than the player's.
pub const HELD: f32 = 2.0;
/// F040880: DC_STARVING's hunger level.
pub const STARVING_UNDER: f32 = 1.7;

impl Sim {
    /// F045433: squad `pid`'s hunger, from its template's "malnourished".
    pub(crate) fn note_squad_hunger(&mut self, pid: usize, t: &GameData) {
        let v = if t.bools.get("malnourished").copied().unwrap_or(false) { 1.0 + 1.2 * self.rng.uniform() } else { FULL };
        self.squad_hunger.insert(pid, v);
    }

    /// F039188: each NPC's hunger cap (the player's people: none); a newcomer starts at it.
    pub(crate) fn hunger_caps(&mut self) -> std::collections::HashMap<usize, f32> {
        let mut caps = std::collections::HashMap::new();
        let mine = self.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone()).unwrap_or_else(|| crate::sim::PLAYER_FACTION.to_string());
        for p in &self.folk {
            if p.player || p.dead {
                continue;
            }
            // held: the player's prisoners and slaves are fed by the player; others' kept at 2
            let held_by = p.prison.as_ref().map(|c| c.faction.clone()).or_else(|| p.slave.as_ref().map(|s| s.faction.clone()));
            let cap = match held_by {
                Some(f) if f == mine => continue,
                Some(_) => HELD,
                None => p.platoon.and_then(|pl| self.squad_hunger.get(&pl)).copied().unwrap_or(FULL),
            };
            caps.insert(p.id, cap);
            if self.hunger_started.insert(p.id) {
                let h = &mut self.arena.f[p.id].medical.hunger;
                h.level = h.level.min(cap);
            }
        }
        caps
    }

    /// DC_STARVING (F040880 case 71): the hunger level (MedicalSystem +0x60) under 1.7, 170
    /// as shown.
    pub fn starving(&self, id: usize) -> bool {
        self.arena.f[id].medical.hunger.level < STARVING_UNDER
    }
}
