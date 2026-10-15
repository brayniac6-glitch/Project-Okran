//! The player's squads (Kenshi's SquadManagementScreen, the overview window's SQUADS tab, and
//! the squad tabs over the portraits): each a platoon of the player's faction with a name, in
//! the order they are listed; the faction's own name.
//!
//! Spec:
//! - F035111 (NEW SQUAD): another squad while there are fewer than GLOBAL CONSTANTS "max squads",
//!   else the message "Maximum number of squads reached."; (F045528) its platoon made where the
//!   camera is;
//! - F035112 (a squad's DELETE): only an empty squad goes ("The squad needs to be empty before it
//!   can be removed."), never the last one ("You must have at least one squad.");
//! - F035150 / F035068 (a portrait dropped on a squad): it joins that squad while the squad has
//!   fewer than "max squad size" (its own squad: its place in it changes); dropped on DRAG HERE TO
//!   DISMISS: "Cannot dismiss the only remaining character in the faction.", else the question
//!   "Dismiss character '{1}'?";
//! - F035103 (the answer yes): the character leaves for the Tech Hunters ("1233-gamedata.base"),
//!   deselected, in a platoon of its own on the "Wandering Squad" template ("5006-squad.mod"),
//!   its AI begun afresh;
//! - F035090: a squad's name is what its box holds; F035125: the faction's name likewise.
//!
//! (assumption) A new squad is named as the first is ("<faction>_<n>", the platoon id Kenshi's
//! save gives the first squad, upper case on its tab).
use crate::sim::{PLAYER_FACTION, Sim};
use serde::{Deserialize, Serialize};

/// One of the player's squads: its platoon number and its name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSquad {
    pub platoon: usize,
    pub name: String,
}

/// F035103: where a dismissed character goes.
pub const DISMISSED_FACTION: &str = "1233-gamedata.base";
pub const DISMISSED_TEMPLATE: &str = "5006-squad.mod";

impl Sim {
    fn global_int(&self, key: &str, default: i32) -> i32 {
        self.data.get("110-gamedata.quack").and_then(|g| g.ints.get(key).copied()).unwrap_or(default)
    }

    /// GLOBAL CONSTANTS "max squads" (10 with the game's mods).
    pub fn max_squads(&self) -> usize {
        self.global_int("max squads", 10).max(1) as usize
    }

    /// GLOBAL CONSTANTS "max squad size" (20 with the game's mods).
    pub fn max_squad_size(&self) -> usize {
        self.global_int("max squad size", 20).max(1) as usize
    }

    /// The player's faction's name: as typed in the FACTION tab, else its record's.
    pub fn player_faction_name(&self) -> String {
        if !self.faction_name.trim().is_empty() {
            return self.faction_name.clone();
        }
        self.data.get(PLAYER_FACTION).map_or("Nameless".to_string(), |r| r.name.clone())
    }

    fn squad_name(&self, n: usize) -> String {
        let f = self.data.get(PLAYER_FACTION).map_or("Nameless".to_string(), |r| r.name.clone());
        format!("{f}_{n}")
    }

    /// The squads made to match the player's people: each platoon of theirs a squad (a new one
    /// named), squads kept in order; one with no platoon put in the first squad.
    pub fn squads_ready(&mut self) {
        let players: Vec<usize> = self.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
        if self.player_squads.is_empty() && players.is_empty() {
            return;
        }
        for &id in &players {
            let pl = self.folk[id].platoon;
            match pl {
                Some(pid) if self.player_squads.iter().any(|s| s.platoon == pid) => {}
                Some(pid) => {
                    let name = self.squad_name(self.player_squads.len());
                    self.player_squads.push(PlayerSquad { platoon: pid, name });
                }
                None => {
                    if self.player_squads.is_empty() {
                        let pid = self.bar_states.len();
                        self.bar_states.push(None);
                        let name = self.squad_name(0);
                        self.player_squads.push(PlayerSquad { platoon: pid, name });
                    }
                    self.folk[id].platoon = Some(self.player_squads[0].platoon);
                }
            }
        }
        // the platoons' numbers must have their bar state slots (a loaded empty squad's too)
        let top = self.player_squads.iter().map(|s| s.platoon + 1).max().unwrap_or(0);
        while self.bar_states.len() < top {
            self.bar_states.push(None);
        }
    }

    /// Squad `k`'s people and animals (the living and the dead the player still has), in their
    /// order.
    pub fn squad_members(&self, k: usize) -> Vec<usize> {
        let Some(s) = self.player_squads.get(k) else { return Vec::new() };
        self.folk.iter().filter(|p| p.player && p.platoon == Some(s.platoon)).map(|p| p.id).collect()
    }

    /// The squad person `id` is in.
    pub fn player_squad_index(&self, id: usize) -> Option<usize> {
        let pl = self.folk.get(id)?.platoon?;
        self.player_squads.iter().position(|s| s.platoon == pl)
    }

    /// NEW SQUAD (F035111): its number, else the message why not.
    pub fn new_squad(&mut self) -> Result<usize, &'static str> {
        self.squads_ready();
        if self.player_squads.len() >= self.max_squads() {
            return Err("Maximum number of squads reached.");
        }
        let pid = self.bar_states.len();
        self.bar_states.push(None);
        let mut n = self.player_squads.len();
        while self.player_squads.iter().any(|s| s.name == self.squad_name(n)) {
            n += 1;
        }
        let name = self.squad_name(n);
        self.player_squads.push(PlayerSquad { platoon: pid, name });
        Ok(self.player_squads.len() - 1)
    }

    /// A squad's DELETE (F035112): gone, else the message why not.
    pub fn remove_squad(&mut self, k: usize) -> Result<(), &'static str> {
        if k >= self.player_squads.len() {
            return Ok(());
        }
        if !self.squad_members(k).is_empty() {
            return Err("The squad needs to be empty before it can be removed.");
        }
        if self.player_squads.len() == 1 {
            return Err("You must have at least one squad.");
        }
        self.player_squads.remove(k);
        Ok(())
    }

    /// Person `id` dropped on squad `k` (F035150): whether it went (the squad not full; its own
    /// squad: moved to the end of it).
    pub fn move_to_squad(&mut self, id: usize, k: usize) -> bool {
        let Some(target) = self.player_squads.get(k).map(|s| s.platoon) else { return false };
        if !self.folk.get(id).is_some_and(|p| p.player) {
            return false;
        }
        if self.folk[id].platoon != Some(target) && self.squad_members(k).len() >= self.max_squad_size() {
            return false;
        }
        self.folk[id].platoon = Some(target);
        true
    }

    /// Whether person `id` may be dismissed (F035150): the message why not.
    pub fn may_dismiss(&self, id: usize) -> Result<(), &'static str> {
        let players = self.folk.iter().filter(|p| p.player && p.race.is_none() && !p.dead).count();
        if players <= 1 && self.folk.get(id).is_some_and(|p| p.player) {
            return Err("Cannot dismiss the only remaining character in the faction.");
        }
        Ok(())
    }

    /// F035103: person `id` dismissed: a Tech Hunter in a "Wandering Squad" of its own where it
    /// stands, its orders and jobs dropped, its AI begun on the template's goals.
    pub fn dismiss(&mut self, id: usize) {
        if self.may_dismiss(id).is_err() || !self.folk.get(id).is_some_and(|p| p.player) {
            return;
        }
        let data = self.data.clone();
        let faction = if data.get(DISMISSED_FACTION).is_some() { DISMISSED_FACTION.to_string() } else { "Nobody".to_string() };
        let pid = self.bar_states.len();
        self.bar_states.push(None);
        let template = data.get(DISMISSED_TEMPLATE);
        if let Some(t) = template {
            self.squad_template.insert(pid, t.string_id.clone());
        }
        let package = template.and_then(|t| crate::sim::running_package(&data, t));
        // its talk as one of the world's again: its CHARACTER's "dialogue package"
        let talk = self.folk[id].character.as_deref().and_then(|c| data.get(c)).and_then(|c| c.refs("dialogue package").first().map(|(s, _)| s.clone()));
        let p = &mut self.folk[id];
        p.player = false;
        p.platoon = Some(pid);
        p.faction = Some(faction.clone());
        p.home = None;
        p.ai.queue.clear();
        if talk.is_some() {
            p.dialogue = talk;
        }
        if let Some(pk) = package {
            p.package = Some(pk.string_id.clone());
            let goals = kenshi_ai::package::package_goals(&data, pk, kenshi_ai::package::Role::Leader);
            p.ai.jobs = crate::sim::jobs_of(goals, None, None);
        }
        let side = self.side(&Some(faction));
        self.arena.f[id].team = side;
    }
}
