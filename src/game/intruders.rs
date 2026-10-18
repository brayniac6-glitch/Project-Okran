//! Intruders, trespassing and what NPCs make of the player's people they see (port_map `law`):
//! the look an NPC gives a character it senses (F046547, `look`), before its greetings
//! (crate::speech): escaped prisoners spotted by a town's guards, slaves getting away and found
//! out (with crate::slavery), trespassers and intruders of a town, those shooed from a home, and
//! uniform imposters.
//! - F046487, trespassing: inside the NPC's own home while it is private (not a shop and not
//!   "public day" by its residents' squad template, or between 22 and 7 o'clock) and locked up;
//!   or, for a town's guards, inside any locked-up building whose owners are not the intruder's
//!   allies, or a slave inside a private one. The intruder commits the crime of trespassing
//!   against the NPC's faction (F046495), the NPC raises EV_INTRUDER_FOUND and its squad tags
//!   them ST_INTRUDER (F040854). F046486: inside a building of a town that is not public, the
//!   NPC's squad's home town, also an intruder (no crime).
//! - F046488, shooing: inside the NPC's home while it is not public (Building::isPublic,
//!   F037699: the residents' "public day", never while locked up): EV_SHOO_FROM_MY_BUILDING.
//!
//! What follows is the dialogue's: the shoo lines follow and talk (DA_FOLLOW_WHILE_TALKING), the
//! intruder lines attack (DA_ATTACK_CHASE_FOREVER), mark a temporary enemy or raise the crime
//! alarm; a guard witnessing the trespassing adds its bounty (crate::law).
//!
//! "Locked up" is a squad's flag (F044498) set when one of its people closes a lockable door of
//! its home or locks one (Task_CloseDoor F030907, Task_LockDoor F030911), so long as no door of
//! the home stands open or opening, and cleared when one is opened or a lock picked
//! (F030901..F030903, Task_PickLock F031344). (assumption) A home met with every door shut,
//! locked and whole starts locked up (the squad's flag as it was left).
//!
//! Then "Remove Home Intruder" (task 223) and "Intruder out" (224, Task_TakeIntruderOutside,
//! done once INTRUDER_IS_OUTSIDE, F038580) run on the planner (crate::law): a resident lifts
//! anyone knocked out inside its home (F050282) and carries them out of the door.
use crate::sim::{DOOR_CLOSED, Sim};
use kenshi_ai::PerceptionShortTerm;

/// EventTriggerEnum values (fcs.def).
pub const EV_SHOO_FROM_MY_BUILDING: i32 = 9;
pub const EV_INTRUDER_FOUND: i32 = 17;
/// BuildingDesignation BD_SHOP: open to all by day.
const BD_SHOP: i32 = 1;
/// CharacterTypeEnum OT_LAW_ENFORCEMENT / OT_MILITARY (F038593: who guards a town).
const OT_LAW_ENFORCEMENT: i32 = 1;
const OT_MILITARY: i32 = 2;
/// F046547: how visible a slave must be to be seen getting away (F046500).
const SLAVE_SEEN: f32 = 0.05;

/// "Closing door" (73, Task_CloseDoor) and "Locking door from inside" (162, Task_LockDoor):
/// the door tasks run here (`door_tasks_tick`; crate::sim leaves them to this).
pub const CLOSING_DOOR: u16 = 73;
pub const DOOR_RUNNABLE: [u16; 2] = [CLOSING_DOOR, crate::sim::LOCKING_DOOR];
/// Task_CloseDoor's wait before the door closes (s, F030907); Task_LockDoor's lock turns at this
/// much a second, done at 1 (F030911).
const CLOSE_SECONDS: f32 = 2.0;
const LOCK_RATE: f32 = 0.3;

/// @fn F044498
///
/// The squad living in `home` locks it up (its AI +0xe8 set): unless set already, and only while
/// no door of it stands open or opening.
pub fn lock_up(locked_up: &mut std::collections::HashMap<usize, bool>, doors: &[crate::sim::DoorState], home: usize) {
    if locked_up.get(&home).copied().unwrap_or(false) {
        return;
    }
    if doors.iter().any(|d| d.door.placement == home && !d.gone && matches!(d.phase, crate::sim::DOOR_OPEN | crate::sim::DOOR_OPENING)) {
        return;
    }
    locked_up.insert(home, true);
}

impl Sim {
    /// The squad template of whoever lives in building (placement) `b`.
    fn residents_template(&self, b: usize) -> Option<&kenshi_data::GameData> {
        let pl = self.folk.iter().find(|p| p.home == Some(b) && !p.player).and_then(|p| p.platoon)?;
        self.squad_template.get(&pl).and_then(|t| self.data.get(t))
    }

    /// Building::isPublic (F037699): its residents' "public day", and never while locked up.
    /// A building nobody lives in is public.
    pub fn building_public(&self, b: usize) -> bool {
        !self.locked_up(b) && self.residents_template(b).is_none_or(|t| t.bools.get("public day").copied().unwrap_or(true))
    }

    /// Whether building `b`'s residents have locked it up (the squad's flag, F044498, kept by
    /// `locked_up_tick`; a building not yet seen by it: every door shut, locked and whole).
    /// DC_BUILDING_IS_CLOSED_AND_SECURED reads this of the speaker's home.
    pub fn locked_up(&self, b: usize) -> bool {
        self.law_state.locked_up.get(&b).copied().unwrap_or_else(|| self.doors_secured(b))
    }

    /// Building `b` has doors, each shut, locked and whole.
    fn doors_secured(&self, b: usize) -> bool {
        let mut doors = self.door_states.iter().filter(|d| d.door.placement == b && !d.gone).peekable();
        doors.peek().is_some() && doors.all(|d| d.phase == DOOR_CLOSED && d.locked && !d.broken)
    }

    /// @fn F030902
    ///
    /// Each tick, the homes' "locked up" flags (set by the residents' door tasks,
    /// `door_tasks_tick`): cleared when someone who is not the player's opens a door of it
    /// (Task_OpenDoor F030902 / F030901). The player's people opening it leave the flag set:
    /// inside they are intruders still.
    /// (assumption) Who opened a door is taken as the nearest one walking within twice the
    /// door's reach; a home first met with every door shut, locked and whole is locked up.
    pub fn locked_up_tick(&mut self) {
        let n = self.door_states.len();
        if self.law_state.door_phase.len() != n {
            self.law_state.door_phase = self.door_states.iter().map(|d| d.phase).collect();
            let homes: Vec<usize> = self.door_states.iter().filter(|d| !d.gone).map(|d| d.door.placement).collect();
            for b in homes {
                if !self.law_state.locked_up.contains_key(&b) {
                    let secured = self.doors_secured(b);
                    self.law_state.locked_up.insert(b, secured);
                }
            }
        }
        let mut cleared: Vec<usize> = Vec::new();
        for d in 0..n {
            let (was, now) = (self.law_state.door_phase[d], self.door_states[d].phase);
            self.law_state.door_phase[d] = now;
            // opened: from shut or shutting to opening or open
            let opening = |ph: i32| matches!(ph, crate::sim::DOOR_OPEN | crate::sim::DOOR_OPENING);
            if opening(was) || !opening(now) {
                continue;
            }
            let at = self.door_states[d].door.position;
            let reach = 2.0 * crate::sim::DOOR_OPEN_REACH;
            let opener = self
                .folk
                .iter()
                .filter(|p| !p.dead && !p.down && !p.away)
                .map(|p| {
                    let q = p.mover.position_units();
                    (p.player, (q[0] - at[0]).hypot(q[2] - at[2]))
                })
                .filter(|x| x.1 < reach)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if opener.is_none_or(|(player, _)| !player) {
                cleared.push(self.door_states[d].door.placement);
            }
        }
        for b in cleared {
            self.law_state.locked_up.insert(b, false);
        }
    }

    /// @fn F030907
    /// @fn F030911
    ///
    /// The door tasks of whoever has one in hand (its target a door), facing the door:
    /// - Closing door (73, Task_CloseDoor): after 2 s the door starts closing (F029334); each
    ///   update, a door with a lock (level 1 or more, not broken) of the closer's home locks the
    ///   home up (F044498); done once the door is shut.
    /// - Locking door from inside (162, Task_LockDoor): the lock turns 0.3 a second; at 1 the
    ///   door starts closing, its lock is locked ("Character_Lock") and a door of the locker's
    ///   home locks the home up (F044498); done.
    /// (not ported) The lock sound.
    pub fn door_tasks_tick(&mut self, dt: f32) {
        use crate::sim::Target;
        let ids: Vec<usize> = self.folk.iter().filter(|p| !p.dead && !p.down && p.ai.queue.first().is_some_and(|s| DOOR_RUNNABLE.contains(&s.task))).map(|p| p.id).collect();
        self.law_state.door_time.retain(|id, _| ids.contains(id));
        for me in ids {
            let Some(step) = self.folk[me].ai.queue.first().cloned() else { continue };
            let done = |sim: &mut Sim| {
                sim.folk[me].ai.queue.remove(0);
                sim.folk[me].started = false;
                sim.law_state.door_time.remove(&me);
            };
            let Target::Door(d) = step.target else {
                done(self);
                continue;
            };
            if d >= self.door_states.len() || self.door_states[d].gone {
                done(self);
                continue;
            }
            // facing the door
            let (at, q) = (self.door_states[d].door.position, self.folk[me].mover.position_units());
            let (dx, dz) = (at[0] - q[0], at[2] - q[2]);
            let len = dx.hypot(dz);
            if len > 1e-3 {
                self.folk[me].facing = [dx / len, 0.0, dz / len];
            }
            let home = self.folk[me].home;
            let b = self.door_states[d].door.placement;
            let before = self.law_state.door_time.get(&me).copied().unwrap_or(0.0);
            if step.task == CLOSING_DOOR {
                let mut t = before + dt;
                if t > CLOSE_SECONDS {
                    self.door_states[d].start_close();
                    t = 0.0;
                }
                self.law_state.door_time.insert(me, t);
                let s = &self.door_states[d];
                if s.level >= 1 && !s.broken && home == Some(b) {
                    lock_up(&mut self.law_state.locked_up, &self.door_states, b);
                }
                if self.door_states[d].phase == DOOR_CLOSED {
                    done(self);
                }
            } else {
                let t = before + dt * LOCK_RATE;
                self.law_state.door_time.insert(me, t);
                if before < 1.0 && 1.0 <= t {
                    let s = &mut self.door_states[d];
                    s.start_close();
                    if !s.broken {
                        s.locked = true;
                    }
                    if home == Some(b) {
                        lock_up(&mut self.law_state.locked_up, &self.door_states, b);
                    }
                    done(self);
                }
            }
        }
    }

    /// F046487's hours: a home is private between 22 and 7 o'clock whatever its residents'
    /// flags, and by day unless a shop or "public day".
    fn private_now(&self, b: usize) -> bool {
        let hour = self.hour() as i32;
        let open = self.residents_template(b).is_none_or(|t| t.ints.get("building designation").copied().unwrap_or(0) == BD_SHOP || t.bools.get("public day").copied().unwrap_or(true));
        !open || !(7..=22).contains(&hour)
    }

    /// @fn F038593
    ///
    /// Whether `id` guards a town: not of a faction of "fundamental type" 8; law enforcement,
    /// or military in the town that is its squad's home (F044526 +0x30).
    pub fn guards_town(&self, id: usize) -> bool {
        let p = &self.folk[id];
        if self.faction_id(id).is_some_and(|f| self.fundamental_type(&f) == 8) {
            return false;
        }
        let class = self.npc_class(id);
        class == OT_LAW_ENFORCEMENT || (class == OT_MILITARY && p.town.is_some() && self.home_town(id) == p.town)
    }

    /// The town `id`'s squad calls home (F044526 +0x30): (assumption) the town of the building
    /// it lives in, else the town its squad was made for (its first destination).
    pub(crate) fn home_town(&self, id: usize) -> Option<usize> {
        let p = &self.folk[id];
        p.home.and_then(|h| self.placed_town.get(h).copied().flatten()).or(p.destination)
    }

    /// @fn F046487
    ///
    /// Whether `me` takes `other` (inside a building) for a trespasser: never a friend of it
    /// (F044470, by its disguise), an animal or one held. Inside `me`'s own home, while it is
    /// private (not a shop and not "public day", or between 22 and 7 o'clock) and locked up.
    /// For a town's guard (F038593), inside a building whose owners are no allies of its faction:
    /// a slave of any state while it is not public, or anyone while its owners have locked it up;
    /// never one the owners' leader takes for a friend.
    /// A prisoner released within the hour (`lately_released`) is no trespasser.
    /// (not ported) NPCs of the same home town of class law, military or diplomat are none in
    /// their homes (only the player's people are looked at here).
    pub fn trespassing(&self, me: usize, other: usize) -> bool {
        let o = &self.folk[other];
        let Some(b) = o.inside else { return false };
        if self.takes_for_friend(me, other, true) || o.race.is_some() || o.prison.as_ref().is_some_and(|pr| !pr.out) || self.lately_released(other) {
            return false;
        }
        if self.folk[me].home == Some(b) {
            return self.private_now(b) && self.locked_up(b);
        }
        if !self.guards_town(me) {
            return false;
        }
        // another's building: its owners (its residents' faction) no allies of the intruder's
        let owner = self.folk.iter().find(|p| p.home == Some(b) && !p.player && !p.dead).map(|p| p.id);
        let owners = owner.and_then(|w| self.folk[w].faction.clone());
        let theirs = self.faction_id(other);
        if owners.as_deref().is_some_and(|f| theirs.as_deref().is_some_and(|t| self.relations.get(f).is_some_and(|r| r.is_ally(Some(t))))) {
            return false;
        }
        if owner.is_some_and(|w| self.takes_for_friend(w, other, true)) {
            return false;
        }
        (!self.building_public(b) && self.slave_state(other) != 0) || self.locked_up(b)
    }

    /// @fn F046488
    ///
    /// Whether `me` shoos `other` from its home: one of the player's people inside it, not of
    /// its household, not a friend (F044470), not an animal, not held or carried, not released
    /// from prison within the hour, while the home is not public.
    pub fn shooing(&self, me: usize, other: usize) -> bool {
        let o = &self.folk[other];
        let Some(home) = self.folk[me].home else { return false };
        o.inside == Some(home) && o.player && o.home != Some(home) && !self.takes_for_friend(me, other, true) && o.race.is_none() && o.prison.is_none() && o.carried_by.is_none() && !self.lately_released(other) && !self.building_public(home)
    }

    /// @fn F046486
    ///
    /// Whether `me` takes `other` for an intruder of its town: inside a building in the town
    /// its squad calls home (F044526 +0x30) while the town is not public, not a slave (state 1),
    /// not held.
    /// Nor a prisoner released within the hour.
    /// (not ported) Nor whoever `me`'s AI +0x650 object passes (F03a355).
    pub fn town_intruder(&self, me: usize, other: usize) -> bool {
        let o = &self.folk[other];
        let Some(home) = self.home_town(me) else { return false };
        o.town == Some(home) && o.inside.is_some() && self.towns.get(home).is_some_and(|t| !t.public) && self.slave_state(other) != crate::slavery::IS_SLAVE && o.prison.is_none() && !self.lately_released(other)
    }

    /// The disguise a uniform imposter needs to pass `me`'s notice (F046547): 0.25 carrying
    /// anything branded as `me`'s faction (Character vtable +0x408), 0.5 disguised as an ally of
    /// it, else never (99).
    fn imposter_threshold(&self, me: usize, other: usize) -> f32 {
        let Some(mine) = self.folk[me].faction.as_deref() else { return 99.0 };
        let branded = self.folk[other].inventory.sections.iter().flat_map(|s| s.items.iter()).any(|pl| pl.item.uniform.as_deref() == Some(mine));
        if branded {
            return 0.25;
        }
        let as_who = self.outfit(other).disguise_as;
        if self.relations.get(mine).is_some_and(|r| r.is_ally(as_who.as_deref())) { 0.5 } else { 99.0 }
    }

    /// Once a second: the NPCs look at the player's people they sense (`look`).
    /// (assumption) The sensor's look comes once a second for each pair; the sensed are those
    /// it sees (`visibility` past 0.01, F046539) or hears.
    pub fn intruders_tick(&mut self) {
        let targets: Vec<usize> = self.folk.iter().filter(|p| p.player && !p.dead && !p.away).map(|p| p.id).collect();
        if targets.is_empty() {
            return;
        }
        let lookers: Vec<usize> = self.folk.iter().filter(|p| !p.player && !p.dead && !p.away && p.faction.is_some()).map(|p| p.id).collect();
        for w in lookers {
            let at = self.folk[w].mover.position_units();
            for &t in &targets {
                let q = self.folk[t].mover.position_units();
                if (q[0] - at[0]).hypot(q[2] - at[2]) > 3000.0 || !self.can_see(w, t) {
                    continue;
                }
                if self.look(w, t) {
                    break;
                }
            }
        }
    }

    /// @fn F046547
    ///
    /// `w` looks at `t`, one of the player's people it senses (the parts before its greetings,
    /// which are crate::speech's). Nothing from an animal, one held in a cage, one not alert
    /// (F046492), a slave, an escaped prisoner or a criminal itself, at one down. In order:
    /// - an escaped prisoner (the state object +0xe8) not disguised (or found out) and not held,
    ///   seen in a town by a guard of the town its squad calls home (F038593; of a type-8 town's
    ///   faction, its own people): EV_ESCAPED_PRISONER_SPOTTED;
    /// - `w` not taking `t` for an enemy (F044572, by its disguise), of a faction not the
    ///   player's and not "anti slavery", seeing `t` (F046500 past 0.05): a slave of its faction
    ///   (or of none) behaving like one getting away (F046534): EV_ESCAPING_SLAVE_SPOTTED and it
    ///   is escaping; an escaping slave found out (its disguise blown): EV_ESCAPING_SLAVE_SPOTTED,
    ///   its owners' people (or anyone, when it has none) keeping it escaping; an ex-slave found
    ///   out: EV_ESCAPED_EX_SLAVE_SPOTTED;
    /// - then a trespasser (F046487) commits trespassing against `w`'s faction (F046495) and an
    ///   intruder (that, or F046486) is called out (EV_INTRUDER_FOUND) and tagged ST_INTRUDER
    ///   (F040854); else one in `w`'s home is shooed (EV_SHOO_FROM_MY_BUILDING);
    /// - neither fighting, `t` committing no crime, not owed and no friend without its disguise
    ///   (F044470): disguised past the imposter threshold (`imposter_threshold`) before one with
    ///   words for EV_I_SEE_UNIFORM_IMPOSTER that is not busy: a crime of uniform theft for 20 s
    ///   (F046496) and EV_I_SEE_UNIFORM_IMPOSTER.
    ///
    /// Whether `w` spoke up (it looks at no one else this time).
    /// (not ported) The look also needs the character's dialogue +0x28 set and the sensor's
    /// entry for `t` (+0x38 / +0x39); the imposter's watcher must not be busy (AI +0x94,
    /// unknown; here: not in a conversation). Out of town, a bandit fighting is tagged a
    /// temporary enemy (F040863).
    pub fn look(&mut self, w: usize, t: usize) -> bool {
        use crate::slavery::{ESCAPING_SLAVE, EX_SLAVE, IS_SLAVE};
        let debug = std::env::var("KENSHI_LAW_DEBUG").is_ok() || std::env::var("KENSHI_SLAVE_DEBUG").is_ok();
        let (pw, pt) = (&self.folk[w], &self.folk[t]);
        if w == t || pw.race.is_some() || pw.prison.as_ref().is_some_and(|pr| !pr.out) || pt.down || pt.dead || self.alertness(w) <= 0.0 {
            return false;
        }
        if self.slave_state(w) != 0 || pw.escaped.is_some() || pw.crime.is_some() {
            return false;
        }
        let Some(wf) = pw.faction.clone() else { return false };
        let held = pt.prison.as_ref().is_some_and(|pr| !pr.out) || pt.carried_by.is_some();
        let outfit = self.outfit(t);
        let blown = pt.disguise_blown > 0.0;
        // an escaped prisoner
        if pt.escaped.is_some() && (blown || outfit.disguise <= 0.0) && !held && pt.town.is_some() && self.folk[w].town.is_some() && self.home_town(w) == self.folk[w].town {
            let town_faction = self.folk[t].town.and_then(|k| self.towns.get(k)).and_then(|k| k.faction.clone());
            let spots = match town_faction {
                Some(f) if self.fundamental_type(&f) == 8 => f == wf,
                _ => self.guards_town(w),
            };
            if spots {
                if debug {
                    eprintln!("look: {w} spots escaped prisoner {t}");
                }
                self.bark(w, t, crate::slavery::EV_ESCAPED_PRISONER_SPOTTED);
            }
        }
        if !self.takes_for_enemy(w, t, true) {
            if wf != crate::sim::PLAYER_FACTION && !self.anti_slavery(&wf) && !held && self.slave_state(t) != 0 {
                let seen = self.sight(w, t) > SLAVE_SEEN;
                let owners = self.folk[t].slave.as_ref().map(|s| s.faction.clone());
                let mine = owners.as_deref().is_none_or(|o| o == wf);
                match self.slave_state(t) {
                    IS_SLAVE if mine && seen && self.slave_out_of_place(w, t) => {
                        if debug {
                            eprintln!("look: {w} sees slave {t} getting away");
                        }
                        self.bark(w, t, crate::disguise::EV_ESCAPING_SLAVE_SPOTTED);
                        self.set_slave_state(t, ESCAPING_SLAVE);
                        return true;
                    }
                    ESCAPING_SLAVE if blown && seen => {
                        self.bark(w, t, crate::disguise::EV_ESCAPING_SLAVE_SPOTTED);
                        if mine {
                            self.set_slave_state(t, ESCAPING_SLAVE);
                        }
                        return true;
                    }
                    EX_SLAVE if blown && seen => {
                        self.bark(w, t, crate::disguise::EV_ESCAPED_EX_SLAVE_SPOTTED);
                    }
                    _ => {}
                }
            }
            let trespass = self.trespassing(w, t);
            if trespass || self.town_intruder(w, t) {
                if trespass {
                    self.commit_crime_on(t, kenshi_ai::CrimeEnum::CrimeTresspassing, Some(&wf), Some(w));
                }
                self.bark(w, t, EV_INTRUDER_FOUND);
                self.tag_short(w, t, PerceptionShortTerm::StIntruder);
                if debug {
                    eprintln!("look: {w} finds intruder {t} in {:?} (trespassing {trespass})", self.folk[t].inside);
                }
                return true;
            }
            if self.shooing(w, t) {
                let spoke = self.bark(w, t, EV_SHOO_FROM_MY_BUILDING);
                if debug && spoke {
                    eprintln!("look: {w} shoos {t} from {:?}", self.folk[w].home);
                }
                return true;
            }
        }
        // a uniform imposter
        if !self.arena.f[t].active && self.folk[t].crime.is_none() && !self.arena.f[w].active {
            let owes = self.dialog_memory.tags.get(&(w, t)).is_some_and(|k| k & 0xc != 0);
            if !owes && !self.takes_for_friend(w, t, false) {
                let threshold = self.imposter_threshold(w, t);
                if outfit.disguise > threshold && self.talking_to(w).is_none() && self.has_event_dialogue(w, crate::disguise::EV_I_SEE_UNIFORM_IMPOSTER) {
                    if debug {
                        eprintln!("look: {w} sees uniform imposter {t} (disguise {:.2} past {threshold})", outfit.disguise);
                    }
                    self.commit_crime_for(t, kenshi_ai::CrimeEnum::CrimeUniformTheft, Some(&wf), Some(w), 20.0);
                    self.bark(w, t, crate::disguise::EV_I_SEE_UNIFORM_IMPOSTER);
                    return true;
                }
            }
        }
        false
    }
}
