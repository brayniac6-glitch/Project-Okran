//! Speech (port_map `dialogue`): the lines people say aloud, over their heads
//! (Kenshi_SpeechPanel), on the dialogue events (EventTrigger, T00044) the game raises as things
//! happen: seeing the player's characters (EV_I_SEE_ENEMY_PLAYER, EV_I_SEE_ALLY_PLAYER),
//! getting up after a knockout (EV_GET_UP_PEACE, EV_GET_UP_FIGHT), losing a limb (EV_LOST_ARM,
//! EV_LOST_LEG), townspeople seeing them (EV_I_SEE_NEUTRAL_SQUAD), witnessing a crime (EV_WITNESS_GENERIC_ASSAULT, EV_WITNESS_LOOTING_ALLY,
//! EV_WITNESS_THIEF_OR_LOCKPICK), and the squads meeting the player's (EV_I_SEE_NEUTRAL_SQUAD,
//! `dialogue::meet_tick`). The line said is the one `talk_on` picks (the dialogue's chance
//! rolled, its conditions passing).
//! (assumption) The game's triggers for these events are not decompiled: the ranges, the
//! repetition limits and the enemy / ally thresholds here are stand-ins.
use crate::sim::Sim;

pub const EV_LOST_LEG: i32 = 70;
pub const EV_LOST_ARM: i32 = 71;
pub const EV_GET_UP_PEACE: i32 = 23;
pub const EV_GET_UP_FIGHT: i32 = 24;
pub const EV_I_SEE_ENEMY_PLAYER: i32 = 66;
pub const EV_I_SEE_ALLY_PLAYER: i32 = 67;
pub const EV_WITNESS_GENERIC_ASSAULT: i32 = 41;
pub const EV_WITNESS_LOOTING_ALLY: i32 = 42;
pub const EV_WITNESS_THIEF_OR_LOCKPICK: i32 = 43;

/// (assumption) How near a player character is seen for EV_I_SEE_*_PLAYER (world units).
const SEE_RANGE: f32 = 250.0;
/// (assumption) Game hours before someone says another line, and before they greet the same
/// way again.
const BARK_GAP: f64 = 0.05;
const SEE_GAP: f64 = 3.0;
/// (assumption) The relation at or below which the player is an enemy, at or above an ally.
const ENEMY_AT: f32 = -30.0;
const ALLY_AT: f32 = 30.0;

impl Sim {
    /// Person `who` says `text` (shown over their head).
    pub fn say(&mut self, who: usize, text: String) {
        if !text.is_empty() {
            self.said.push((who, text));
        }
    }

    /// Person `who` says their line for `event` about `target`, if they have one (its chance
    /// rolled, its conditions passing) and have not just spoken; a line with replies to one
    /// of the player's characters is a conversation instead (`npc_talks`). Whether they spoke.
    pub fn bark(&mut self, who: usize, target: usize, event: i32) -> bool {
        if who >= self.folk.len() || self.folk[who].dead || self.folk[who].down || self.folk[who].away {
            return false;
        }
        let now = self.hours_gone();
        if self.bark_next.get(&who).is_some_and(|&t| now < t) {
            return false;
        }
        let Some((mut conv, effects)) = self.talk_on(who, target, event, true) else { return false };
        self.bark_next.insert(who, now + BARK_GAP);
        if !conv.choices.is_empty() && self.folk[target].player && !self.folk[who].player {
            self.npc_talks.push((conv, effects));
            return true;
        }
        conv.choices.clear();
        let text = std::mem::take(&mut conv.npc_text);
        self.say(who, text);
        if !effects.is_empty() {
            self.npc_talks.push((conv, effects));
        }
        true
    }

    /// The nearest of the player's characters to `id` (and how far).
    fn nearest_player(&self, id: usize) -> Option<(usize, f32)> {
        let at = self.folk[id].mover.position_units();
        self.folk
            .iter()
            .filter(|p| p.player && !p.dead && !p.away && p.id != id)
            .map(|p| {
                let q = p.mover.position_units();
                (p.id, (q[0] - at[0]).hypot(q[2] - at[2]))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Every second: people getting up say so; people near the player's characters greet or
    /// threaten them.
    pub fn speech_tick(&mut self) {
        // getting up after a knockout: a fight still on near them, or not
        let down: std::collections::HashSet<usize> = self.folk.iter().filter(|p| p.down && !p.dead).map(|p| p.id).collect();
        let got_up: Vec<usize> = self.was_down.difference(&down).copied().filter(|&i| i < self.folk.len() && !self.folk[i].dead).collect();
        self.was_down = down;
        for id in got_up {
            let fight = self.arena.f.iter().enumerate().any(|(k, f)| k != id && f.active && !f.down && {
                let (a, b) = (self.folk[k].mover.position_units(), self.folk[id].mover.position_units());
                (a[0] - b[0]).hypot(a[2] - b[2]) < 400.0
            });
            let target = self.nearest_player(id).map_or(id, |(p, _)| p);
            self.bark(id, target, if fight { EV_GET_UP_FIGHT } else { EV_GET_UP_PEACE });
        }
        // seeing the player's characters
        let now = self.hours_gone();
        let ids: Vec<usize> = self
            .folk
            .iter()
            .filter(|p| !p.player && !p.down && !p.dead && !p.away && (p.dialogue.is_some() || p.package.is_some()))
            .filter(|p| self.seen_player.get(&p.id).is_none_or(|&t| now >= t))
            .map(|p| p.id)
            .collect();
        for id in ids {
            if self.arena.f[id].active {
                continue;
            }
            let Some((player, d)) = self.nearest_player(id) else { return };
            if d > SEE_RANGE || !self.can_see(id, player) {
                continue;
            }
            let rel = self.relation_toward_player(id);
            // (assumption) the neutral (townspeople; the roaming squads' own greeting is
            // `meet_tick`'s) greet with their EV_I_SEE_NEUTRAL_SQUAD line
            let roaming = self.folk[id].home.is_none() && self.folk[id].race.is_none() && self.folk[id].role == 5;
            let event = if rel <= ENEMY_AT {
                EV_I_SEE_ENEMY_PLAYER
            } else if rel >= ALLY_AT {
                EV_I_SEE_ALLY_PLAYER
            } else if !roaming {
                crate::dialogue::EV_I_SEE_NEUTRAL_SQUAD
            } else {
                continue;
            };
            // (assumption) a missed roll is tried again in a tenth of an hour
            let spoke = self.bark(id, player, event);
            if std::env::var("KENSHI_SPEECH_DEBUG").is_ok() {
                let pk = self.dialogue_package(id).map(|r| (r.name.clone(), r.refs("dialogs").iter().map(|(s, v)| format!("{}:{}:{}", v[0], v[1], self.data.get(s).map_or("?", |d| d.name.as_str()))).collect::<Vec<_>>()));
                eprintln!("speech: {id} sees {player} at {d:.0} (relation {rel:.0}, event {event}): {} {pk:?}", if spoke { "spoke" } else { "nothing" });
            }
            self.seen_player.insert(id, now + if spoke { SEE_GAP } else { 0.1 });
        }
    }

    /// Person `id`'s faction's relation to the player's.
    fn relation_toward_player(&self, id: usize) -> f32 {
        let f = self.folk[id].faction.as_deref();
        f.and_then(|f| self.relations.get(f)).map_or(0.0, |r| r.relation(crate::sim::PLAYER_FACTION))
    }
}
