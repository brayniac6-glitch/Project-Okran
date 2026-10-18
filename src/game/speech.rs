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
pub const EV_GET_UP_UNNECCESSARY_FIGHT: i32 = 25;
pub const EV_SPEECH_INTERRUPTED_ATTACKED_BY_TARGET: i32 = 28;
pub const EV_SPEECH_INTERRUPTED_ATTACKED_BY_STRANGERS: i32 = 29;
pub const EV_BETRAYAL: i32 = 31;
pub const EV_ASSASSINATION_FAILED: i32 = 56;
pub const EV_I_SEE_ENEMY_PLAYER: i32 = 66;
pub const EV_I_SEE_ALLY_PLAYER: i32 = 67;
pub const EV_I_SEE_ANIMAL_SQUAD: i32 = 27;
pub const EV_WITNESS_GENERIC_ASSAULT: i32 = 41;
pub const EV_WITNESS_LOOTING_ALLY: i32 = 42;
pub const EV_WITNESS_THIEF_OR_LOCKPICK: i32 = 43;
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
            let event = self.get_up_event(id);
            let target = self.nearest_player(id).map_or(id, |(p, _)| p);
            self.bark(id, target, event);
        }
        self.combat_events();
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
            // others about (within 1000 units, not of its squad nor its faction: those make no
            // event, `sighted`), by those whose packages answer a sighting of them
            // (EV_I_SEE_NEUTRAL_SQUAD, EV_I_SEE_ANIMAL_SQUAD)
            if self.answers_sightings(id) {
                let at = self.folk[id].mover.position_units();
                let (pl, mine) = (self.folk[id].platoon, self.folk[id].faction.clone());
                let others: Vec<usize> = (0..self.folk.len())
                    .filter(|&k| {
                        let q = &self.folk[k];
                        let w = q.mover.position_units();
                        k != id
                            && !q.player
                            && !q.dead
                            && !q.down
                            && !q.away
                            && (pl.is_none() || q.platoon != pl)
                            && (q.race.is_some() || q.faction != mine)
                            && (w[0] - at[0]).hypot(w[2] - at[2]) < 1000.0
                    })
                    .collect();
                for k in others {
                    if self.can_see(id, k) {
                        seen.insert((id, k));
                    }
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

    /// @fn F047465
    ///
    /// The event of `id` getting up: no foes about (its combat's +0x94 / +0x98), EV_GET_UP_PEACE;
    /// foes but none of its own standing (+0x4c), EV_GET_UP_UNNECCESSARY_FIGHT; else
    /// EV_GET_UP_FIGHT. (assumption) Its foes: fighters within 400 units fighting its side; its
    /// own: its squad's others standing. (The game raises it at no one: here at the player's
    /// nearest.)
    fn get_up_event(&self, id: usize) -> i32 {
        let at = self.folk[id].mover.position_units();
        let team = self.arena.f[id].team;
        let foes = self.arena.f.iter().enumerate().any(|(k, f)| {
            let q = self.folk[k].mover.position_units();
            k != id && f.active && !f.down && f.foes.as_ref().is_some_and(|x| x.contains(&team)) && (q[0] - at[0]).hypot(q[2] - at[2]) < 400.0
        });
        if !foes {
            return EV_GET_UP_PEACE;
        }
        let mates = self.squad_of(id).into_iter().any(|k| k != id && !self.folk[k].down);
        if mates { EV_GET_UP_FIGHT } else { EV_GET_UP_UNNECCESSARY_FIGHT }
    }

    /// Every second, those newly in a fight (CombatTask::engage, F050266):
    /// - one speaking (F040924: a line still shown, or its dialogue begun within
    ///   `ONE_AT_A_TIME_HOURS`), not in a fighting event (EV_I_SEE_ENEMY_PLAYER,
    ///   EV_I_DEFEATED_SQUAD, EV_LAUNCH_ATTACK, EV_INTRUDER_FOUND, EV_GET_UP_FIGHT): its talk ends
    ///   and it says EV_SPEECH_INTERRUPTED_ATTACKED_BY_TARGET at the one it talked to, or
    ///   _BY_STRANGERS at another (the nobles' guards called on the player);
    /// - one on a contract attacked by its hirer's side (Contract_HiredAlly, F029019): betrayed,
    ///   it says EV_BETRAYAL. (assumption) The contract ends there, the squad back on its own
    ///   package.
    ///   (assumption) Its attacker: the nearest fighter with it as target, else its own target.
    fn combat_events(&mut self) {
        let now = self.hours_gone();
        let fighting: std::collections::HashSet<usize> = (0..self.folk.len()).filter(|&k| self.arena.f[k].active && !self.folk[k].dead && !self.folk[k].down).collect();
        let new: Vec<usize> = fighting.iter().copied().filter(|k| !self.dialog_memory.fighting.contains(k)).collect();
        self.dialog_memory.fighting = fighting;
        for id in new {
            let at = self.folk[id].mover.position_units();
            let attacker = (0..self.folk.len())
                .filter(|&k| k != id && self.arena.f[k].focus == Some(id) && !self.folk[k].dead)
                .min_by(|&a, &b| {
                    let d = |k: usize| {
                        let q = self.folk[k].mover.position_units();
                        (q[0] - at[0]).hypot(q[2] - at[2])
                    };
                    d(a).total_cmp(&d(b))
                })
                .or(self.arena.f[id].focus);
            let Some(attacker) = attacker else { continue };
            // betrayed by its hirer's side
            if let Some(c) = self.contracts.iter().position(|c| c.members.contains(&id)) {
                let hirer = self.contracts[c].hirer;
                if self.faction_key(attacker).is_some() && self.faction_key(attacker) == self.faction_key(hirer) {
                    let contract = self.contracts.remove(c);
                    for (m, was) in contract.members.iter().zip(&contract.was) {
                        if let Some(p) = was {
                            self.set_package(*m, p);
                        }
                    }
                    self.bark_next.remove(&id);
                    self.bark(id, attacker, EV_BETRAYAL);
                    continue;
                }
            }
            // interrupted speaking
            let running = self.dialog_memory.running.get(&id).and_then(|d| self.dialog_memory.spoken.get(&(id, d.clone()))).is_some_and(|&t| now < t + crate::dialogue::ONE_AT_A_TIME_HOURS);
            let speaking = self.bark_next.get(&id).is_some_and(|&t| now < t) || running;
            let event = self.dialog_memory.running_event.get(&id).copied().unwrap_or(0);
            if speaking && ![EV_I_SEE_ENEMY_PLAYER, crate::dialogue::EV_I_DEFEATED_SQUAD, 16, 17, EV_GET_UP_FIGHT].contains(&event) {
                let partner = self.talking_to(id);
                self.dialog_memory.partner.remove(&id);
                self.bark_next.remove(&id);
                self.bark(id, attacker, if partner == Some(attacker) { EV_SPEECH_INTERRUPTED_ATTACKED_BY_TARGET } else { EV_SPEECH_INTERRUPTED_ATTACKED_BY_STRANGERS });
            }
        }
    }

    /// Task_StealthKO (F031279): a failed knockout makes its `target` say
    /// EV_ASSASSINATION_FAILED at `assassin` (the nobles' "Assassin noble": their guards' hunt).
    /// For crate::law's stealth knockout to call as its roll fails.
    pub fn assassination_failed(&mut self, target: usize, assassin: usize) {
        self.bark(target, assassin, EV_ASSASSINATION_FAILED);
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

    /// Whether `id`'s packages answer a sighting of others (EV_I_SEE_NEUTRAL_SQUAD or
    /// EV_I_SEE_ANIMAL_SQUAD; F040904 flags such a package so its owner looks), kept per
    /// (package, character, faction).
    fn answers_sightings(&mut self, id: usize) -> bool {
        let p = &self.folk[id];
        let key = (p.dialogue.clone(), p.character.clone(), p.faction.clone());
        if let Some(&v) = self.dialog_memory.sighting_packages.get(&key) {
            return v;
        }
        let v = self.has_event(id, crate::dialogue::EV_I_SEE_NEUTRAL_SQUAD) || self.has_event(id, EV_I_SEE_ANIMAL_SQUAD);
        self.dialog_memory.sighting_packages.insert(key, v);
        v
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
    /// `id`'s sensor newly sees `other`: an animal is EV_I_SEE_ANIMAL_SQUAD
    /// (an animal sighting no one unless its dialogue answers EV_I_SEE_NEUTRAL_SQUAD); a slave
    /// says nothing; an ally of its faction (its squad not on a campaign, the other no slave)
    /// is EV_I_SEE_ALLY_PLAYER; else, neither fighting, one training on its faction's
    /// equipment is EV_USING_MY_TRAINING_EQUIPMENT, and (its squad not on a campaign, the
    /// other no slave) an enemy of its faction EV_I_SEE_ENEMY_PLAYER, anyone else
    /// EV_I_SEE_NEUTRAL_SQUAD (the ally and enemy events are for the player's people only: an
    /// NPC's ally is nothing, any other NPC neutral). (not ported here) The escaped prisoner,
    /// escaping slave, intruder, shoo and uniform imposter sightings the function also makes
    /// (crate::law, crate::slavery, crate::intruders, crate::disguise raise theirs).
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
        let theirs_player = self.folk[other].player;
        if self.counts_ally(id, other) {
            let faction_ally = self.folk[id].faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_ally(self.faction_key(other).as_deref()));
            if theirs_player && !self.on_campaign(id) && self.slave_state(other) != 1 && faction_ally {
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
        // (the enemy event is the player's; anyone else is EV_I_SEE_NEUTRAL_SQUAD)
        let enemy = theirs_player && self.folk[id].faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(self.faction_key(other).as_deref()));
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
