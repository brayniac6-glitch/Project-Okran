//! Speech (port_map `dialogue`): the lines people say aloud, over their heads
//! (Kenshi_SpeechPanel), on the dialogue events (EventTrigger, T00044) the game raises as things
//! happen: seeing someone (the sensor's new sightings, F046547: EV_I_SEE_NEUTRAL_SQUAD,
//! EV_I_SEE_ENEMY_PLAYER, EV_I_SEE_ALLY_PLAYER, EV_I_SEE_ANIMAL_SQUAD,
//! EV_USING_MY_TRAINING_EQUIPMENT), getting up after a knockout (EV_GET_UP_PEACE,
//! EV_GET_UP_FIGHT), losing a limb (EV_LOST_ARM, EV_LOST_LEG), witnessing a crime
//! (EV_WITNESS_GENERIC_ASSAULT, EV_WITNESS_LOOTING_ALLY, EV_WITNESS_THIEF_OR_LOCKPICK), being freed
//! (EV_UNLOCK_MY_CAGE_OR_SHACKLES), first aid (EV_HEALING_OTHER_START .. _FINISHED,
//! EV_FIRSTAID_KIT_EMPTY), the weather's hurts (EV_ACID_*, EV_BURNING, EV_POISON_GAS) and a
//! squad beaten (EV_I_DEFEATED_SQUAD, crate::dialogue). The line said is the one `talk_on` picks (the dialogue's conditions
//! passing, its chances rolled).
use crate::sim::Sim;

pub const EV_LOST_LEG: i32 = 70;
pub const EV_LOST_ARM: i32 = 71;
pub const EV_GET_UP_PEACE: i32 = 23;
pub const EV_GET_UP_FIGHT: i32 = 24;
pub const EV_I_SEE_ENEMY_PLAYER: i32 = 66;
pub const EV_I_SEE_ALLY_PLAYER: i32 = 67;
pub const EV_I_SEE_ANIMAL_SQUAD: i32 = 27;
pub const EV_WITNESS_GENERIC_ASSAULT: i32 = 41;
pub const EV_WITNESS_LOOTING_ALLY: i32 = 42;
pub const EV_WITNESS_THIEF_OR_LOCKPICK: i32 = 43;
/// "I recognised someone has a bounty and is wanted. Won't trigger for allies" (fcs.def;
/// raised by crate::disguise's notice, F046537).
pub const EV_BOUNTY_SPOTTED: i32 = 44;
pub const EV_USING_MY_TRAINING_EQUIPMENT: i32 = 59;
pub const EV_UNLOCK_MY_CAGE_OR_SHACKLES: i32 = 13;
pub const EV_HEALING_OTHER_START: i32 = 18;
pub const EV_BEING_HEALED_START: i32 = 19;
pub const EV_HEALING_OTHER_FINISHED: i32 = 20;
pub const EV_BEING_HEALED_FINISHED: i32 = 21;
pub const EV_FIRSTAID_KIT_EMPTY: i32 = 22;
pub const EV_ACID_FEET: i32 = 61;
pub const EV_ACID_RAIN: i32 = 62;
pub const EV_ACID_WATER: i32 = 63;
pub const EV_POISON_GAS: i32 = 65;
pub const EV_BURNING: i32 = 69;

/// The events a knocked-out character still raises (the mask at 0x141eeaecc, F040925 /
/// F040920): EV_SCREAMING_TORTURE and the three EV_GET_UP_*.
pub const DOWN_EVENTS: u32 = 0x0380_0800;

/// (stand-in) Game hours a line is shown before its speaker says another (the dialogue's line
/// timer, +0x230, not decompiled).
const BARK_GAP: f64 = 0.05;

impl Sim {
    /// Person `who` says `text` (shown over their head).
    pub fn say(&mut self, who: usize, text: String) {
        if !text.is_empty() {
            self.said.push((who, text));
        }
    }

    /// @fn F040925
    ///
    /// Person `who`'s dialogue raises `event` about `target`: none from the dead, none from the
    /// knocked out but `DOWN_EVENTS`, none while it is still saying a line (`BARK_GAP`); else
    /// the line `talk_on` picks, said aloud, or with replies to one of the player's
    /// characters a conversation (`npc_talks`). Whether it spoke.
    pub fn bark(&mut self, who: usize, target: usize, event: i32) -> bool {
        if who >= self.folk.len() || target >= self.folk.len() || self.folk[who].dead || self.folk[who].away {
            return false;
        }
        if self.folk[who].down && (DOWN_EVENTS >> (event & 31)) & 1 == 0 {
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

    /// Every second: people getting up say so; the player's characters newly in sight of
    /// someone are sighted (`sighted`); the weather's hurts said.
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
        // the sensors: each of the player's characters newly seen
        let players: Vec<usize> = self.folk.iter().filter(|p| p.player && !p.dead && !p.away).map(|p| p.id).collect();
        let watchers: Vec<usize> = self.folk.iter().filter(|p| !p.player && !p.dead && !p.away && (p.dialogue.is_some() || p.package.is_some())).map(|p| p.id).collect();
        let mut seen = std::collections::HashSet::new();
        for &id in &watchers {
            if self.folk[id].down {
                continue;
            }
            for &p in &players {
                if self.can_see(id, p) {
                    seen.insert((id, p));
                }
            }
        }
        let new: Vec<(usize, usize)> = seen.iter().copied().filter(|k| !self.dialog_memory.in_sight.contains(k)).collect();
        self.dialog_memory.in_sight = seen;
        for (id, p) in new {
            self.sighted(id, p);
        }
        self.hazard_events();
    }

    /// @fn F039251
    ///
    /// The weather update's events (crate::hazards): one hurt by it says so, at itself, as the
    /// hurt lands: acidic water EV_ACID_WATER, acidic ground EV_ACID_FEET, a fire EV_BURNING,
    /// poison gas EV_POISON_GAS, acid rain EV_ACID_RAIN (a dust storm none). (assumption) Here
    /// each second it lasts (its dialogues' repetition limits hold them back).
    fn hazard_events(&mut self) {
        use crate::hazards::{WA_ACID, WA_BURNING, WA_GAS};
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            if p.dead || p.away {
                continue;
            }
            let Some(h) = p.hazard.as_ref() else { continue };
            let event = match (h.affect, h.ground) {
                (WA_ACID, true) if p.water == 3 => EV_ACID_WATER,
                (WA_ACID, true) => EV_ACID_FEET,
                (WA_ACID, false) => EV_ACID_RAIN,
                (WA_BURNING, _) => EV_BURNING,
                (WA_GAS, _) => EV_POISON_GAS,
                _ => continue,
            };
            self.bark(id, id, event);
        }
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

    /// Whether `id`'s dialogue packages list a dialogue under `event` (F00bc9e: the package's
    /// event list, F040904).
    pub fn has_event(&self, id: usize, event: i32) -> bool {
        self.packages_of(id).iter().any(|p| p.refs("dialogs").iter().any(|(_, v)| v.contains(&event)))
    }

    /// (assumption) Whether `id` counts `other` an ally (Character vtable 0x3f0): its faction's
    /// ally (F041495), or its squad's temporary ally.
    fn counts_ally(&self, id: usize, other: usize) -> bool {
        let mine = self.folk[id].faction.as_deref().and_then(|f| self.relations.get(f));
        mine.is_some_and(|r| r.is_ally(self.faction_key(other).as_deref())) || self.has_short(id, other, kenshi_ai::PerceptionShortTerm::StTemporaryAlly)
    }

    /// (assumption) Whether `id`'s squad is out on a campaign (its squad's +0x78 mission,
    /// F02d709): it greets no one.
    fn on_campaign(&self, id: usize) -> bool {
        self.folk[id].world_squad.and_then(|k| self.world.squads.get(k)).is_some_and(|s| s.alive && matches!(s.goal, crate::world::Goal::Campaign { .. }))
    }

    /// @fn F046547
    ///
    /// `id`'s sensor newly sees `other` (one of the player's): an animal is EV_I_SEE_ANIMAL_SQUAD
    /// (an animal sighting no one unless its dialogue answers EV_I_SEE_NEUTRAL_SQUAD); a slave
    /// says nothing; an ally of its faction (its squad not on a campaign, the other no slave)
    /// is EV_I_SEE_ALLY_PLAYER; else, neither fighting, one training on its faction's
    /// equipment is EV_USING_MY_TRAINING_EQUIPMENT, and (its squad not on a campaign, the
    /// other no slave) an enemy of its faction EV_I_SEE_ENEMY_PLAYER, anyone else
    /// EV_I_SEE_NEUTRAL_SQUAD. (not ported here) The escaped prisoner, escaping slave,
    /// intruder, shoo and uniform imposter sightings the function also makes (crate::law,
    /// crate::slavery, crate::intruders, crate::disguise raise theirs); NPCs' sightings of one
    /// another.
    pub fn sighted(&mut self, id: usize, other: usize) {
        if id == other || self.folk[id].dead || self.folk[other].dead {
            return;
        }
        if self.folk[id].race.is_some() && !self.has_event(id, crate::dialogue::EV_I_SEE_NEUTRAL_SQUAD) {
            return;
        }
        // one caged sees nothing to speak of (+0x2f8 2); one down is not spoken to
        if self.folk[id].prison.is_some() || self.folk[other].down {
            return;
        }
        if self.folk[other].race.is_some() {
            self.bark(id, other, EV_I_SEE_ANIMAL_SQUAD);
            return;
        }
        if self.slave_state(id) == 1 {
            return;
        }
        if self.counts_ally(id, other) {
            let faction_ally = self.folk[id].faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_ally(self.faction_key(other).as_deref()));
            if !self.on_campaign(id) && self.slave_state(other) != 1 && faction_ally {
                self.bark(id, other, EV_I_SEE_ALLY_PLAYER);
            }
            return;
        }
        if self.slave_state(id) != 0 {
            return;
        }
        if self.arena.f[other].active || self.arena.f[id].active {
            return;
        }
        let theirs = self.training_owner(other).is_some_and(|f| Some(&f) == self.folk[id].faction.as_ref());
        if theirs {
            self.bark(id, other, EV_USING_MY_TRAINING_EQUIPMENT);
            return;
        }
        if self.on_campaign(id) || self.slave_state(other) == 1 {
            return;
        }
        let enemy = self.folk[id].faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(self.faction_key(other).as_deref()));
        let spoke = self.bark(id, other, if enemy { EV_I_SEE_ENEMY_PLAYER } else { crate::dialogue::EV_I_SEE_NEUTRAL_SQUAD });
        if std::env::var("KENSHI_SPEECH_DEBUG").is_ok() {
            eprintln!("speech: {id} sights {other} ({}): {}", if enemy { "enemy" } else { "neutral" }, if spoke { "spoke" } else { "nothing" });
        }
    }

    /// The faction whose training equipment person `id` is training on (Character +0x158):
    /// (assumption) its building's residents', else the faction of the town it stands in.
    pub fn training_owner(&self, id: usize) -> Option<String> {
        let f = self.folk[id].operating?;
        self.furniture_training(f)?;
        let home = self.furn[f].placement;
        if let Some(r) = self.folk.iter().find(|p| p.home == Some(home) && !p.player && !p.dead).and_then(|p| p.faction.clone()) {
            return Some(r);
        }
        let at = self.furn[f].position;
        self.towns.iter().find(|t| t.contains(at, 1.0)).and_then(|t| t.faction.clone())
    }
}
