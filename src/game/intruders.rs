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
//! "Locked up" is a squad's flag (F044498) set when one of its people locks a door of its home
//! with every door of it shut (Task_LockDoor, F030907 / F030911) and cleared when one is opened
//! or a lock picked (F030901..F030903, Task_PickLock F031344). (stand-in) Here it is read off the
//! doors: a home is locked up while it has doors and every one is shut, locked and whole.
//!
//! Then "Remove Home Intruder" (task 223) and "Intruder out" (224, Task_TakeIntruderOutside,
//! done once INTRUDER_IS_OUTSIDE, F038580): a resident picks up anyone knocked out inside its
//! home (F050282: down, not carried, not held, in the building that is the squad's home) and
//! carries them out of the door. (assumption) The sensor's list F050282 reads (filter 0x201) is
//! taken as those not of the household, squad or faction. (stand-in) They are put down `OUT_BY`
//! units out from the nearest door of the home; their scores (Method 59) are not ported: an idle
//! resident takes the task.
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
/// (assumption) How far out from the door an intruder is put down (units).
const OUT_BY: f32 = 80.0;

/// A knocked-out intruder being carried out of `home` (placement) by `by` to `out` (units).
#[derive(Debug, Clone)]
pub struct Eject {
    pub by: usize,
    pub whom: usize,
    pub home: usize,
    pub out: [f32; 3],
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

    /// @fn F044498
    /// @fn F030902
    ///
    /// Each tick, the homes' "locked up" flags: set (F044498: only while no door of the home is
    /// open or opening) as its residents lock it up; cleared when someone who is not the
    /// player's opens a door of it (Task_OpenDoor F030902 / F030901). The player's people
    /// opening it leave the flag set: inside they are intruders still.
    /// (stand-in) The residents' closing and locking (Task_CloseDoor F030907, Task_LockDoor
    /// F030911) is not ported: the flag is set whenever every door of the home is shut, locked and
    /// whole. Who opened a door is taken as the nearest one walking within twice the door's
    /// reach.
    pub fn locked_up_tick(&mut self) {
        let n = self.door_states.len();
        if self.law_state.door_phase.len() != n {
            self.law_state.door_phase = self.door_states.iter().map(|d| d.phase).collect();
        }
        let mut cleared: Vec<usize> = Vec::new();
        for d in 0..n {
            let (was, now) = (self.law_state.door_phase[d], self.door_states[d].phase);
            self.law_state.door_phase[d] = now;
            if was != DOOR_CLOSED || now == DOOR_CLOSED {
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
        let homes: Vec<usize> = self.door_states.iter().filter(|d| !d.gone).map(|d| d.door.placement).collect();
        for b in homes {
            if self.doors_secured(b) {
                self.law_state.locked_up.insert(b, true);
            } else {
                self.law_state.locked_up.entry(b).or_insert(false);
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

    /// The point `OUT_BY` out from the door of building `b` nearest `at` (units).
    fn outside_of(&self, b: usize, at: [f32; 3]) -> Option<[f32; 3]> {
        let inner = self.inside_point.get(b).copied().flatten()?;
        let d = self.door_states.iter().filter(|d| d.door.placement == b && !d.gone).min_by(|x, y| {
            let l = |p: [f32; 3]| (p[0] - at[0]).hypot(p[2] - at[2]);
            l(x.door.position).total_cmp(&l(y.door.position))
        })?;
        let p = d.door.position;
        let (dx, dz) = (p[0] - inner[0], p[2] - inner[2]);
        let l = dx.hypot(dz).max(1e-3);
        let out = [p[0] + dx / l * OUT_BY, p[1], p[2] + dz / l * OUT_BY];
        // onto the navmesh (its coordinates are tenths of units)
        self.nav.closest_point(out.map(|v| v / 10.0), 10.0).map(|(_, q)| q.map(|v| v * 10.0))
    }

    /// @fn F050282
    ///
    /// Tasks 223 / 224: residents carry the knocked out out of their homes (see the module notes).
    fn eject_tick(&mut self) {
        // new ones: an idle resident and someone down in its home not of its own
        let busy: Vec<usize> = self.ejecting.iter().flat_map(|e| [e.by, e.whom]).collect();
        let residents: Vec<usize> = self
            .folk
            .iter()
            .filter(|p| !p.player && !p.down && !p.dead && !p.away && p.home.is_some() && p.inside == p.home && p.carrying.is_none() && p.pickup.is_none() && p.prison.is_none())
            .map(|p| p.id)
            .filter(|&r| !self.arena.f[r].active && !busy.contains(&r))
            .collect();
        for r in residents {
            let home = self.folk[r].home.unwrap();
            let busy: Vec<usize> = self.ejecting.iter().map(|e| e.whom).collect();
            let downed = self.folk.iter().find(|q| {
                q.down && !q.dead && q.inside == Some(home) && q.carried_by.is_none() && q.prison.is_none() && q.bed.is_none() && q.home != Some(home) && !busy.contains(&q.id) && (q.platoon.is_none() || q.platoon != self.folk[r].platoon) && (q.player || q.faction != self.folk[r].faction)
            });
            let Some(w) = downed.map(|q| q.id) else { continue };
            let Some(out) = self.outside_of(home, self.folk[w].mover.position_units()) else { continue };
            self.order_pickup(&[r], w);
            self.ejecting.push(Eject { by: r, whom: w, home, out });
            if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                eprintln!("law: {r} takes intruder {w} out of {home}");
            }
        }
        // under way
        let tick = self.tick;
        let mut k = 0;
        while k < self.ejecting.len() {
            let e = self.ejecting[k].clone();
            let (b, w) = (&self.folk[e.by], &self.folk[e.whom]);
            let lost = b.down || b.dead || self.arena.f[e.by].active || w.dead || !w.down || w.carried_by.is_some_and(|c| c != e.by) || (b.carrying != Some(e.whom) && b.pickup != Some(e.whom));
            if lost {
                if self.folk[e.by].carrying == Some(e.whom) {
                    self.order_drop(&[e.by]);
                }
                self.ejecting.remove(k);
                continue;
            }
            if self.folk[e.by].carrying == Some(e.whom) {
                let at = self.folk[e.by].mover.position_units();
                if self.folk[e.by].inside != Some(e.home) && (at[0] - e.out[0]).hypot(at[2] - e.out[2]) < 40.0 {
                    // INTRUDER_IS_OUTSIDE: put down
                    self.order_drop(&[e.by]);
                    self.ejecting.remove(k);
                    if std::env::var("KENSHI_LAW_DEBUG").is_ok() {
                        eprintln!("law: {} put intruder {} outside", e.by, e.whom);
                    }
                    continue;
                }
                // on its way unless it already is (a stalled walk is ordered again now and then)
                let going = self.folk[e.by].ai.orders.first().is_some_and(|o| o.pos == e.out);
                if !going || (self.folk[e.by].speed < 1.0 && tick % 300 == 0) {
                    self.order_move(&[e.by], e.out);
                }
            }
            k += 1;
        }
    }

    /// Once a second: residents carry out the knocked out (`eject_tick`) and the NPCs look at
    /// the player's people they sense (`look`).
    /// (assumption) The sensor's look comes once a second for each pair; the sensed are those
    /// it sees (`visibility` past 0.01, F046539) or hears.
    pub fn intruders_tick(&mut self) {
        self.eject_tick();
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
                let seen = self.visibility(w, t) > SLAVE_SEEN;
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
