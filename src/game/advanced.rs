//! The new game's advanced options (Kenshi_AdvancedOptionsWindow, F048535; the game keeps them at
//! 0x142133578 .. 0x142133598 and a save brings them back, F048498), and where each acts:
//! - "Hunger time" (0.25 .. 8): how long characters take to get hungry, the starvation period's
//!   multiplier;
//! - "Chance of death" (0.5 .. 4): wound degeneration and blood loss (the medical tick's global
//!   factor, F040438 / F040473);
//! - "Global damage multiplier" (0.5 .. 4): all combat damage (hit resolution's factor, F040476 /
//!   F040731; crossbows' damage, F033879);
//! - "Production speed" (0.5 .. 2): machines', mines' and crafting's output (F029326);
//! - "Research speed" (0.5 .. 2): research's progress (F046230);
//! - "Building speed" (0.5 .. 2): construction (F037833);
//! - "Number of nests multiplier" (0.5 .. 4): the nests the wilds keep (F048131);
//! - "Bandits loot the player" ("Makes life unfair"): looters strip the player's knocked-out
//!   characters too (F050277);
//! - "Easy prospecting".
//!
//! (stand-in) The defaults (all 1, bandits looting, no easy prospecting) are not read from the
//! game; easy prospecting is taken as prospecting at once, the game's use of it not traced.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Advanced {
    pub hunger_time: f32,
    pub death: f32,
    pub damage: f32,
    pub production: f32,
    pub research: f32,
    pub building: f32,
    pub nests: f32,
    pub bandits_loot: bool,
    pub easy_prospecting: bool,
}

impl Default for Advanced {
    fn default() -> Advanced {
        Advanced { hunger_time: 1.0, death: 1.0, damage: 1.0, production: 1.0, research: 1.0, building: 1.0, nests: 1.0, bandits_loot: true, easy_prospecting: false }
    }
}

/// The sliders in the window's order: name, tooltip, range.
pub const SLIDERS: [(&str, &str, f32, f32); 7] = [
    ("Hunger time", "This sets how long it takes characters to get hungry.  A higher number means it takes longer, therefore they need less food to get by.", 0.25, 8.0),
    ("Chance of death", "Determines the overall likelyhood of any character dying (players or NPCs).  Directly affects rates of wound degeneration and blood loss (Higher=more likely to die)", 0.5, 4.0),
    ("Global damage multiplier", "Overall combat damage for everything.  Doesn't necessarily make the game harder or easier, but affects the speed of battles", 0.5, 4.0),
    ("Production speed", "Affects speed of item production such as mines and crafting (more than 1.0 == easier)", 0.5, 2.0),
    ("Research speed", "Affects speed of research (more than 1.0 == easier)", 0.5, 2.0),
    ("Building speed", "Affects speed of building (more than 1.0 == easier)", 0.5, 2.0),
    ("Number of nests multiplier", "Affects number of animal nests, bandit camps etc that appear in the wilds.  Higher number makes life more dangerous.", 0.5, 4.0),
];

/// The ticks: name, tooltip.
pub const TICKS: [(&str, &str); 2] = [("Bandits loot the player", "Makes life unfair"), ("Easy prospecting", "")];

impl Advanced {
    /// Slider `k`'s value.
    pub fn slider(&mut self, k: usize) -> Option<&mut f32> {
        Some(match k {
            0 => &mut self.hunger_time,
            1 => &mut self.death,
            2 => &mut self.damage,
            3 => &mut self.production,
            4 => &mut self.research,
            5 => &mut self.building,
            6 => &mut self.nests,
            _ => return None,
        })
    }

    /// Tick `k`'s value.
    pub fn tick(&mut self, k: usize) -> Option<&mut bool> {
        Some(match k {
            0 => &mut self.bandits_loot,
            1 => &mut self.easy_prospecting,
            _ => return None,
        })
    }

    /// As a world's start extra ("advanced:" then the values, comma separated).
    pub fn to_extra(&self) -> String {
        format!(
            "advanced:{},{},{},{},{},{},{},{},{}",
            self.hunger_time, self.death, self.damage, self.production, self.research, self.building, self.nests, self.bandits_loot, self.easy_prospecting
        )
    }

    /// From a start extra's values (`to_extra` after its prefix).
    pub fn from_extra(s: &str) -> Advanced {
        let v: Vec<&str> = s.split(',').collect();
        let mut a = Advanced::default();
        for k in 0..7 {
            if let (Some(x), Some(f)) = (v.get(k).and_then(|x| x.parse::<f32>().ok()), a.slider(k)) {
                *f = x;
            }
        }
        for k in 0..2 {
            if let (Some(x), Some(b)) = (v.get(7 + k).and_then(|x| x.parse::<bool>().ok()), a.tick(k)) {
                *b = x;
            }
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_round_trip() {
        let a = Advanced { hunger_time: 2.5, death: 0.5, bandits_loot: false, easy_prospecting: true, ..Advanced::default() };
        let e = a.to_extra();
        assert_eq!(Advanced::from_extra(e.strip_prefix("advanced:").unwrap()), a);
    }
}
