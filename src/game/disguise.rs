//! Disguises and being recognised (port_map `disguise`): what a character's clothes give away,
//! and the people about seeing through them.
//!
//! The figures live in the character's state object (Character +0x1a0, vtable +0x70, the one
//! that holds the slave state: crate::slavery). F038707 (AI slot 9, each think; the second half,
//! after the current town) works them out for the player's people (Faction +0x250 set), not
//! animals (`outfit`):
//! - the clothing, +0xf0, from the equipment sections (F043521 by slot type, the item in a cell
//!   of it, F043545): no hat 0.4 when shaved (vtable +0x3e0, the appearance +0x168); no body
//!   armour 0.1; no trousers 0.1; bare feet 0.1 (the boots section enabled: not a race with "no
//!   shoes", F040014, nor both legs gone); 1.0 for each "is locked" piece (LockedArmour, vtable
//!   +0x2f0: the shackles) on the body, legs or feet; 0.5 for trousers of "stigma" 6 (OT_SLAVE:
//!   the rags) that are not locked;
//! - "Looks like slave", +0xf4: the clothing, doubled while escaping; at least 0.1 while
//!   escaping and 0.01 as an ex-slave (+0xec: 10 / 1, the tooltip's "Minimum (Escaping)" and
//!   "Minimum (Ex-Slave)"); at most 1;
//! - the clothing disguise, +0x100, and its faction, +0x108 (an item's +0x1e8, branded by a
//!   "wears uniform" CHARACTER): a uniform hat 0.4, uniform body armour 0.4, uniform trousers of
//!   the same faction 0.2; a tenth of that when the body armour or trousers are missing or the
//!   pieces' factions differ;
//! - the outfit's stigma, +0x104 (the hat's, then the body armour's; 9 when they differ);
//! - +0xf8, how wanted the look is: the bounty with the faction of the town it is in (F046513,
//!   F046482) over 10000, halved under a hat.
//!
//! A disguise holds (F044631) while not blown (+0x110) and better than 0.6: the character then
//! passes as the disguise's faction (F046494, the faction others take it for; a slave, state 1,
//! passes as its owners'; F044470 / F044572 judge friend and foe by it).
//!
//! Seeing through (F046537, each sighting of a character within 400 units by an awake NPC that
//! is not fighting, F046540): a disguise that holds, an escaped slave or ex-slave who looks like
//! one (F046471) to someone with words for it (a dialogue for EV_ESCAPING_SLAVE_SPOTTED, or for
//! EV_ESCAPED_EX_SLAVE_SPOTTED in a town that is home to it), or one of the player's people with
//! a bounty with the observer's faction of at least its threshold (F046483: law enforcement 1,
//! military 4000 (2000 a squad leader), adventurers 1000, others 10000, slaves never) builds the
//! observer's notice meter (F046536): seconds to notice T = (5, 1 for a slave) + 40 x (1 +
//! stealth) x the disguise (or 1 - looks like slave, or 1 for a bounty) x 0.5 when it remembers
//! the character as an enemy (LT tags 0x2b0) x 0.6 dressed as its own faction x 0.5 a slave of
//! its own faction x 0.75 indoors in a private town x 2 when the observer is away from home;
//! the meter rises 1 / (lerp(visibility, 5T, T) x lerp(alertness, 4, 1)) a second while seen
//! and falls 1/260 a second otherwise (F046507); past 1 the character is noticed: with no bounty
//! with the observer's faction the disguise is blown for 250 s ("Disguise Blown!" or, for an
//! escaped slave, "Discovered!", F047087), else the observer raises EV_BOUNTY_SPOTTED. A blown
//! disguise counts as noticed at once.
//! Branded clothing is worth 0.3 of its value (crate::trade::UNIFORM_VALUE).

use crate::sim::Sim;
use kenshi_char::StatsEnumerated;
use kenshi_char::inventory::Item;
use kenshi_data::ItemType;
use std::collections::HashMap;

/// How long a blown disguise stays blown (seconds; F046537, F038704: 250).
pub const BLOWN_SECONDS: f32 = 250.0;
/// A disguise better than this passes (F044631, F046494).
pub const DISGUISE_PASSES: f32 = 0.6;
/// How near an observer looks for a disguise (units; F046537: 400 squared).
const NOTICE_RANGE: f32 = 400.0;
/// EventTriggerEnum values (fcs.def).
pub const EV_I_SEE_UNIFORM_IMPOSTER: i32 = 34;
pub const EV_ESCAPING_SLAVE_SPOTTED: i32 = 36;
pub const EV_ESCAPED_EX_SLAVE_SPOTTED: i32 = 40;
pub const EV_BOUNTY_SPOTTED: i32 = 44;
/// CharacterTypeEnum OT_SLAVE: the slaves' "stigma".
const OT_SLAVE: i32 = 6;
/// Long-term tags (as bits) that make an observer know the character (F038592: 0x2b0,
/// LT_STOLE_FROM_ME, LT_MY_CAPTOR, LT_DEFEATED_MY_SQUAD_ONCE, LT_KILLED_MY_FRIEND) and that make
/// it look away (F038591: 0xc, LT_MY_LIFESAVER, LT_FREED_ME).
const KNOWS_ME: u32 = 0x2b0;
const OWES_ME: u32 = 0xc;

/// What a character's clothes give away (the state object's figures, F038707).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outfit {
    /// +0xf0: the clothing's own look of a slave.
    pub clothing: f32,
    /// +0xf4: "Looks like slave", 0 .. 1.
    pub looks: f32,
    /// +0xec: the floor applied (10 escaping, 1 ex-slave), else 0.
    pub minimum: i32,
    /// +0x100: the clothing disguise, 0 .. 1, and +0x108 its faction.
    pub disguise: f32,
    pub disguise_as: Option<String>,
    /// +0x104: the outfit's stigma (CharacterTypeEnum; 9 when the pieces differ).
    pub stigma: i32,
    /// +0xf8: how wanted it looks.
    pub wanted: f32,
}

/// One observer's notice of one character (the sensor's +0x150 map, F046527 / F046528).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notice {
    /// +0: how far noticed (1 and past: noticed).
    pub meter: f32,
    /// +4: the seconds the meter takes to fill while seen.
    pub rate: f32,
    /// +8: seen at the last look.
    pub seen: bool,
}

/// The observers' notice meters (observer, character) (crate::sim::Sim::recognition), and the
/// seconds each look's modifier stays shown on the character (Character +0x1b0, F046635: 14 s;
/// 0 "Same faction", 1 "Slave owner recognition", 2 "Someone knows me personally").
#[derive(Debug, Clone, Default)]
pub struct Recognition {
    pub notice: HashMap<(usize, usize), Notice>,
    pub recent: HashMap<usize, [f32; 3]>,
}

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

    /// @fn F038707
    ///
    /// The figures of `id`'s outfit (see the module notes); all 0 for an NPC or an animal.
    pub fn outfit(&self, id: usize) -> Outfit {
        let p = &self.folk[id];
        let mut o = Outfit::default();
        if !p.player || p.race.is_some() {
            return o;
        }
        let section = |slot: i32| p.inventory.sections.iter().find(|s| s.slot == slot && s.is_equipment());
        let worn = |slot: i32| section(slot).and_then(|s| s.items.first()).map(|pl| &pl.item);
        let rec = |i: &Item| self.data.get(&i.record);
        let locked = |i: &Item| rec(i).is_some_and(|r| r.bools.get("is locked").copied().unwrap_or(false));
        let stigma = |i: &Item| rec(i).and_then(|r| r.ints.get("stigma").copied()).unwrap_or(0);
        let mut uniform: Option<&String> = None;
        let mut incomplete = false;
        // hat (ATTACH_HAT 3)
        let hat = worn(3);
        match hat {
            None if p.shaved => o.looks += 0.4,
            None => {}
            Some(h) => {
                if let Some(f) = &h.uniform {
                    o.disguise += 0.4;
                    uniform = Some(f);
                }
                o.stigma = stigma(h);
            }
        }
        // body armour (ATTACH_BODY 5); (Kenshi reads the hat again when there is no body section:
        // a character always has one here)
        match worn(5) {
            None => {
                incomplete = true;
                o.looks += 0.1;
            }
            Some(b) => {
                if locked(b) {
                    o.looks += 1.0;
                }
                if uniform.is_some() && uniform != b.uniform.as_ref() {
                    incomplete = true;
                }
                if let Some(f) = &b.uniform {
                    o.disguise += 0.4;
                    uniform = Some(f);
                }
                let s = stigma(b);
                if o.stigma < 1 {
                    o.stigma = s;
                } else if o.stigma != s {
                    o.stigma = 9;
                }
            }
        }
        // trousers (ATTACH_LEGS 6)
        match worn(6) {
            None => {
                incomplete = true;
                o.looks += 0.1;
            }
            Some(l) => {
                if locked(l) {
                    o.looks += 1.0;
                } else if stigma(l) == OT_SLAVE {
                    o.looks += 0.5;
                }
                if uniform.is_some() && l.uniform.is_some() && uniform != l.uniform.as_ref() {
                    incomplete = true;
                }
                if l.uniform.is_some() && uniform == l.uniform.as_ref() {
                    o.disguise += 0.2;
                }
            }
        }
        // boots (ATTACH_BOOTS 9): bare feet count while the section is enabled (F040014: not a
        // race with "no shoes", nor with both legs other than their own)
        match worn(9) {
            None => {
                if section(9).is_some() && self.boots_enabled(id) {
                    o.looks += 0.1;
                }
            }
            Some(b) => {
                if locked(b) {
                    o.looks += 1.0;
                }
            }
        }
        o.clothing = o.looks;
        let state = self.slave_state(id);
        if state == crate::slavery::ESCAPING_SLAVE {
            o.looks *= 2.0;
        }
        if state == crate::slavery::EX_SLAVE {
            if o.looks < 0.01 {
                o.looks = 0.01;
                o.minimum = 1;
            }
        } else if state == crate::slavery::ESCAPING_SLAVE && o.looks < 0.1 {
            o.looks = 0.1;
            o.minimum = 10;
        }
        o.looks = o.looks.min(1.0);
        // F046513 with no faction: the town's (F046482)
        let town_faction = p.town.and_then(|t| self.towns.get(t)).and_then(|t| t.faction.clone());
        let bounty = town_faction.map_or(0, |f| self.bounty_with(id, &f));
        o.wanted = bounty as f32 / 10000.0;
        if hat.is_some() {
            o.wanted *= 0.5;
        }
        o.disguise_as = uniform.cloned();
        if incomplete {
            o.disguise *= 0.1;
        }
        o
    }

    /// F040014: whether `id`'s boots section is enabled: not a race with "no shoes", and not with
    /// both legs other than their own (LimbState not LIMB_ORIGINAL).
    pub(crate) fn boots_enabled(&self, id: usize) -> bool {
        let no_shoes = self.race_of(id).and_then(|r| self.data.get(&r)).is_some_and(|r| r.bools.get("no shoes").copied().unwrap_or(false));
        let leg_off = |slot: usize| self.limb_part(id, slot).is_some_and(|i| self.arena.f[id].medical.parts[i].slot != 0);
        !no_shoes && !(leg_off(2) && leg_off(3))
    }

    /// @fn F044631
    ///
    /// The faction `id`'s disguise passes as: while not blown and better than 0.6.
    pub fn disguise_faction(&self, id: usize) -> Option<String> {
        if self.folk[id].disguise_blown > 0.0 {
            return None;
        }
        let o = self.outfit(id);
        (o.disguise > DISGUISE_PASSES).then_some(o.disguise_as).flatten()
    }

    /// @fn F046494
    ///
    /// The faction others take `id` for: its disguise's while it holds; a slave's owners'; else
    /// its own (the player's for the player's people).
    pub fn apparent_faction(&self, id: usize) -> Option<String> {
        let p = &self.folk[id];
        if let Some(f) = &p.disguised {
            return Some(f.clone());
        }
        if let Some(s) = p.slave.as_ref().filter(|s| s.state == crate::slavery::IS_SLAVE) {
            return Some(s.faction.clone());
        }
        if p.player { Some(crate::sim::PLAYER_FACTION.to_string()) } else { p.faction.clone() }
    }

    /// Whether `id` passes as one of `faction`'s own (disguised as it).
    pub fn passes_as(&self, id: usize, faction: &str) -> bool {
        self.folk[id].disguised.as_deref() == Some(faction)
    }

    /// F040846: whether `npc` has words for `event` (its dialogue packages hold a dialogue of
    /// that event, whatever its conditions; the packages `talk_on` reads).
    pub fn has_event_dialogue(&self, npc: usize, event: i32) -> bool {
        let p = &self.folk[npc];
        let mut own: Vec<&kenshi_data::GameData> = self.dialogue_package(npc).into_iter().collect();
        if let Some(c) = p.character.as_deref().and_then(|c| self.data.get(c)) {
            own.extend(c.refs("dialogue package").iter().filter_map(|(s, _)| self.data.get(s)));
        }
        if let Some(f) = p.faction.as_deref().and_then(|f| self.data.get(f)) {
            own.extend(f.refs("dialog default").iter().filter_map(|(s, _)| self.data.get(s)));
        }
        let inherited: Vec<&kenshi_data::GameData> = own.iter().flat_map(|k| k.refs("inheritsFrom").iter().filter_map(|(s, _)| self.data.get(s))).collect();
        own.extend(inherited);
        own.extend(p.package.as_deref().and_then(|k| self.data.get(k)));
        own.iter().any(|k| k.refs("dialogs").iter().any(|(_, v)| v[0] == event))
    }

    /// F046483: the bounty with its faction that `observer` acts on (by its NPC class).
    fn bounty_threshold(&self, observer: usize) -> i32 {
        let class = self.folk[observer].character.as_deref().and_then(|c| self.data.get(c)).and_then(|c| c.ints.get("NPC class").copied()).unwrap_or(0);
        match class {
            1 => 1,
            // F044494: the squad's leader
            2 if self.leader_of(observer) == Some(observer) => 2000,
            2 => 4000,
            6 => i32::MAX,
            9 => 1000,
            _ => 10000,
        }
    }

    /// The first of `id`'s squad (its leader).
    fn leader_of(&self, id: usize) -> Option<usize> {
        let pl = self.folk[id].platoon?;
        self.folk.iter().find(|q| q.platoon == Some(pl) && !q.dead).map(|q| q.id)
    }

    /// The observer's alertness (F046536): its current task's definition +0x3c, 0.5 with none;
    /// none asleep, held (Character +0x2f8), down or a slave.
    pub(crate) fn alertness(&self, observer: usize) -> f32 {
        let p = &self.folk[observer];
        if p.bed.is_some() || p.prison.as_ref().is_some_and(|pr| !pr.out) || p.down || self.slave_state(observer) == crate::slavery::IS_SLAVE {
            return 0.0;
        }
        p.ai.current.as_ref().and_then(|t| kenshi_ai::task_def::def(t.task)).map_or(0.5, |d| d.f3c)
    }

    /// Whether `observer` is at home where it stands (F046537's second test): in a town (not a
    /// nest) of a real faction other than the player's that its own faction is no enemy of.
    fn observer_at_home(&self, observer: usize) -> bool {
        let p = &self.folk[observer];
        let Some(t) = p.town.and_then(|t| self.towns.get(t)) else { return false };
        let Some(tf) = t.faction.as_deref() else { return false };
        if tf == crate::sim::PLAYER_FACTION || t.kind == 0 || self.data.get(tf).is_some_and(|f| f.bools.get("not real").copied().unwrap_or(false)) {
            return false;
        }
        !p.faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(Some(tf)))
    }

    /// @fn F044470
    ///
    /// Whether `me` takes `other` for a friend: one of its own faction; not one its squad has
    /// tagged a temporary enemy (nor, for the player's people, one whose squad has so tagged
    /// them, while one whose squad has tagged them a temporary ally is); one its squad has
    /// tagged a temporary ally or that it owes (F038591); else an ally of the faction it belongs
    /// to, or of the faction its disguise passes as (F044631) when `disguise`.
    pub fn takes_for_friend(&self, me: usize, other: usize, disguise: bool) -> bool {
        use kenshi_ai::PerceptionShortTerm::{StTemporaryAlly, StTemporaryEnemy};
        let (Some(mine), theirs) = (self.faction_id(me), self.faction_id(other)) else { return false };
        if theirs.as_deref() == Some(mine.as_str()) {
            return true;
        }
        if self.has_short(me, other, StTemporaryEnemy) {
            return false;
        }
        if self.folk[me].player {
            if self.has_short(other, me, StTemporaryEnemy) {
                return false;
            }
            if self.has_short(other, me, StTemporaryAlly) {
                return true;
            }
        }
        if self.has_short(me, other, StTemporaryAlly) || self.dialog_memory.tags.get(&(me, other)).is_some_and(|t| t & OWES_ME != 0) {
            return true;
        }
        let as_who = if disguise { self.folk[other].disguised.clone() } else { None }.or(theirs);
        self.relations.get(&mine).is_some_and(|r| r.is_ally(as_who.as_deref()))
    }

    /// @fn F044572
    ///
    /// Whether `me` takes `other` for an enemy: never one of its own faction; of another squad,
    /// one its squad has tagged a temporary enemy (or, for the player's people, whose squad has
    /// so tagged them) is, one tagged a temporary ally is not. A slave (state 1) with no disguise
    /// that holds is no enemy when it is `me`'s faction's prisoner (F030709) or its owners are
    /// no enemy of `me`'s faction; a slave is no enemy of its own owners. Else by the faction it
    /// belongs to, or the one its disguise passes as when `disguise`: at war with `me`'s.
    /// (not ported) The squad home's faction (F044526 -> F0432ca) taken for an enemy, and the
    /// state objects' +0xea (released prisoners) and the relations' +0x60 that gate the war test.
    pub fn takes_for_enemy(&self, me: usize, other: usize, disguise: bool) -> bool {
        use kenshi_ai::PerceptionShortTerm::{StTemporaryAlly, StTemporaryEnemy};
        let (Some(mine), theirs) = (self.faction_id(me), self.faction_id(other)) else { return false };
        if theirs.as_deref() == Some(mine.as_str()) {
            return false;
        }
        let r = self.relations.get(&mine);
        let (a, b) = (self.folk[me].platoon, self.folk[other].platoon);
        if a.is_none() || a != b {
            if self.has_short(me, other, StTemporaryEnemy) || (self.folk[me].player && self.has_short(other, me, StTemporaryEnemy)) {
                return true;
            }
            if self.has_short(me, other, StTemporaryAlly) {
                return false;
            }
        }
        let disguised = self.folk[other].disguised.clone();
        if (disguised.is_none() || !disguise)
            && let Some(s) = self.folk[other].slave.as_ref().filter(|s| s.state == crate::slavery::IS_SLAVE)
            && (s.faction == mine || !r.is_some_and(|r| r.is_enemy(Some(&s.faction))))
        {
            return false;
        }
        if self.folk[me].slave.as_ref().is_some_and(|s| s.state == crate::slavery::IS_SLAVE && theirs.as_deref() == Some(s.faction.as_str())) {
            return false;
        }
        let as_who = if disguise { disguised } else { None }.or(theirs);
        r.is_some_and(|r| r.is_enemy(as_who.as_deref()))
    }

    /// Every tick: blown disguises run down (F038705); once a second (each think) the disguise
    /// each of the player's people passes with (F044631) and the observers' looks (F046537).
    pub fn disguise_tick(&mut self, dt: f32) {
        for p in self.folk.iter_mut() {
            if p.disguise_blown > 0.0 {
                p.disguise_blown -= dt;
            }
        }
        if !self.tick.is_multiple_of(60) {
            return;
        }
        self.recognition.recent.retain(|_, r| {
            r.iter_mut().for_each(|s| *s -= 1.0);
            r.iter().any(|&s| s > 0.0)
        });
        for id in 0..self.folk.len() {
            let now = if self.folk[id].dead { None } else { self.disguise_faction(id) };
            if now != self.folk[id].disguised {
                self.folk[id].disguised = now;
            }
        }
        self.notice_tick(1.0);
    }

    /// @fn F046537
    ///
    /// The observers' looks at the player's people (see the module notes), `dt` seconds of
    /// sighting.
    fn notice_tick(&mut self, dt: f32) {
        // F046601: every meter unseen until looked at again
        for n in self.recognition.notice.values_mut() {
            n.seen = false;
        }
        let debug = std::env::var("KENSHI_SLAVE_DEBUG").is_ok() || std::env::var("KENSHI_LAW_DEBUG").is_ok();
        let targets: Vec<usize> = self.folk.iter().filter(|p| p.player && !p.dead && !p.away).map(|p| p.id).collect();
        for t in targets {
            let q = &self.folk[t];
            // held in a cage (Character +0x2f8 2), carried (+0x3d4): not looked at
            if q.carried_by.is_some() || q.prison.as_ref().is_some_and(|pr| !pr.out) {
                continue;
            }
            let at = q.mover.position_units();
            let outfit = self.outfit(t);
            let state = self.slave_state(t);
            let owners = q.slave.as_ref().map(|s| s.faction.clone());
            let disguised = q.disguised.clone();
            // F046471: looks like an escaped slave
            let looks_escaped = owners.is_some() && state > crate::slavery::IS_SLAVE && outfit.looks > 0.0;
            let stealth = (self.arena.f[t].level(StatsEnumerated::StatStealth) * 0.01).clamp(0.0, 1.0);
            let secure = q.inside.is_some() && q.town.and_then(|k| self.towns.get(k)).is_some_and(|k| !k.public);
            let observers: Vec<usize> = (0..self.folk.len())
                .filter(|&w| {
                    let p = &self.folk[w];
                    if w == t || p.player || p.race.is_some() || p.dead || p.away || p.down || self.arena.f[w].active {
                        return false;
                    }
                    let r = p.mover.position_units();
                    (r[0] - at[0]).powi(2) + (r[1] - at[1]).powi(2) + (r[2] - at[2]).powi(2) <= NOTICE_RANGE * NOTICE_RANGE
                })
                .collect();
            for w in observers {
                let Some(theirs) = self.folk[w].faction.clone() else { continue };
                let home = self.observer_at_home(w);
                // what there is to see through: a disguise, an escaped slave, a bounty
                let mut look = disguised.is_some();
                let mut quality = if look { outfit.disguise } else { 1.0 };
                let mut base = 5.0;
                // (the observer's faction: not the player's, not "anti slavery", Faction +0x8)
                if looks_escaped
                    && !self.anti_slavery(&theirs)
                    && ((state == crate::slavery::EX_SLAVE && home && self.has_event_dialogue(w, EV_ESCAPED_EX_SLAVE_SPOTTED))
                        || (state == crate::slavery::ESCAPING_SLAVE && self.has_event_dialogue(w, EV_ESCAPING_SLAVE_SPOTTED)))
                {
                    look = true;
                    quality = 1.0 - outfit.looks;
                    base = 1.0;
                }
                let bounty = self.bounty_with(t, &theirs);
                if outfit.disguise <= 0.0 && !self.takes_for_friend(w, t, true) && self.bounty_threshold(w) <= bounty {
                    look = true;
                    quality = 1.0;
                    base = 5.0;
                }
                if !look {
                    continue;
                }
                let tags = self.dialog_memory.tags.get(&(w, t)).copied().unwrap_or(0);
                let mut time = (1.0 + stealth) * quality;
                let shown = |k: usize, sim: &mut Sim| sim.recognition.recent.entry(t).or_default()[k] = 14.0;
                if tags & KNOWS_ME != 0 {
                    time *= 0.5;
                    shown(2, self);
                }
                if outfit.disguise_as.as_deref() == Some(theirs.as_str()) {
                    time *= 0.6;
                    shown(0, self);
                }
                if owners.as_deref() == Some(theirs.as_str()) {
                    time *= 0.5;
                    shown(1, self);
                }
                if secure {
                    time *= 0.75;
                }
                if !home {
                    time *= 2.0;
                }
                let time = time * 40.0 + base;
                let noticed = self.folk[t].disguise_blown > 0.0 || self.notice_meter(w, t, time, dt);
                if debug {
                    let m = self.recognition.notice.get(&(w, t)).map_or(0.0, |n| n.meter);
                    eprintln!("notice: {w} looks at {t}: T {time:.1}, meter {m:.2}, noticed {noticed}");
                }
                if !noticed || tags & OWES_ME != 0 || self.takes_for_friend(w, t, false) {
                    continue;
                }
                if bounty < 1 {
                    if self.folk[t].disguise_blown <= 0.0 && debug {
                        eprintln!("notice: {w} sees through {t}");
                    }
                    self.folk[t].disguise_blown = BLOWN_SECONDS;
                    self.folk[t].disguised = None;
                } else {
                    self.bark(w, t, EV_BOUNTY_SPOTTED);
                }
            }
        }
        // F046507: the meters fill while seen and empty otherwise
        self.recognition.notice.retain(|_, n| {
            if n.seen {
                n.meter += dt / n.rate;
            } else {
                n.meter -= dt / 260.0;
            }
            n.meter > 0.0
        });
    }

    /// @fn F046536
    ///
    /// `observer`'s notice meter of `t` for a look taking `time` seconds: whether it is past 1
    /// (noticed). Seen (the visibility F046498 above 0), awake and alert, the meter is set to
    /// fill in lerp(visibility, 5 x time, time) x lerp(alertness, 4, 1) seconds; not seen, it
    /// stops filling.
    /// With the line of sight blocked (F046499) the entry is dropped: the meter starts over.
    /// (not ported) The meter also fills for an observer with no alertness while its state
    /// object's +0xb4 is set (unknown).
    fn notice_meter(&mut self, observer: usize, t: usize, time: f32, _dt: f32) -> bool {
        let alert = self.alertness(observer);
        let vis = self.visibility(observer, t);
        let entry = self.recognition.notice.get(&(observer, t)).copied();
        if alert <= 0.0 || vis <= 0.0 {
            return false;
        }
        if !self.line_of_sight(observer, t) {
            self.recognition.notice.remove(&(observer, t));
            return false;
        }
        let rate = (5.0 * time + (time - 5.0 * time) * vis) * (4.0 - 3.0 * alert);
        let before = entry.map_or(0.0, |n| n.meter);
        self.recognition.notice.insert((observer, t), Notice { meter: before, rate: rate.max(0.01), seen: true });
        before > 1.0
    }
}
