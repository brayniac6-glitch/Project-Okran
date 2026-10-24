//! Doors and gates (port_map `build`, DoorStuff): each door slides open and shut by its record's
//! "door move speed" (F029337), opened by people coming through it (Task_OpenDoor, F030903)
//! and shut and locked by its residents at night (Task_LockDoor, F030911); a door that "always
//! locks" (a town gate) locks itself once shut. Town gates are doors of the town's wall
//! buildings: once someone has walked through, they stand open until shut again, as in Kenshi.
//!
//! A gate (the door of a wall gate, a gateway or a linking wall, or one that always locks) shut
//! is a barrier across the navmesh (kenshi_world::navmesh::Barrier): its owner's people (the
//! town's faction, the player's for the player's) pass it, opening it as they come; the
//! player's pass it while it is not locked; anyone else's path stops there. Someone hostile to
//! the gate's owner whose way is shut bashes the gate (Task_BashDoor, F031240) until it breaks
//! (F029386: each blow takes min(max(2 blunt, 20), 60) x 0.015 off the gate's "health";
//! broken, it stands open and takes no lock, "Break").
//!
//! (assumption) Kenshi opens a door for a path that crosses it; here someone walking toward a
//! closed door within `DOOR_OPEN_REACH` of it opens it (one walking away, as from a door just
//! locked, does not). A locked door is opened only by those who live
//! behind it (taken to hold its key), and a gate also by its owner's people. A blow lands every
//! `BASH_EVERY` seconds (Kenshi lands one per attack animation); unarmed, a quarter of it.
use crate::sim::{DOOR_CLOSED, DOOR_OPEN_REACH, PLAYER_FACTION, Sim};
use kenshi_world::navmesh::{Barrier, Pass};

/// Right-click targets from here on are doors (`Sim::door_states`).
pub const DOOR_TARGET: usize = 1 << 24;

/// Seconds between blows at a gate.
pub const BASH_EVERY: f32 = 1.5;
/// How near the gate a basher stands (units).
const BASH_REACH: f32 = 70.0;

impl Sim {
    /// DA_OPEN_NEAREST_GATE (F040912 case 0x36, F029271): the shut gate of `speaker`'s town
    /// nearest it starts opening; (assumption) unlocked to do so.
    pub fn open_nearest_gate(&mut self, speaker: usize) {
        let at = self.folk[speaker].mover.position_units();
        let town = self.folk[speaker].town;
        let gate = (0..self.door_states.len())
            .filter(|&d| self.is_gate(d) && !self.door_states[d].gone && self.door_states[d].phase == crate::sim::DOOR_CLOSED)
            .filter(|&d| town.is_none_or(|t| self.towns.get(t).is_some_and(|tw| tw.contains(self.door_states[d].door.position, 1.0))))
            .min_by(|&a, &b| {
                let l = |d: usize| (self.door_states[d].door.position[0] - at[0]).hypot(self.door_states[d].door.position[2] - at[2]);
                l(a).total_cmp(&l(b))
            });
        if let Some(d) = gate {
            self.door_states[d].locked = false;
            self.door_states[d].start_open();
            if std::env::var("KENSHI_DOOR_DEBUG").is_ok() {
                eprintln!("door: {speaker} has gate {d} opened");
            }
        }
    }

    /// The door within `reach` (units) of `at`, nearest first.
    pub fn door_near(&self, at: [f32; 3], reach: f32) -> Option<usize> {
        self.door_states
            .iter()
            .enumerate()
            .map(|(k, d)| (k, (d.door.position[0] - at[0]).hypot(d.door.position[2] - at[2]), (d.door.position[1] - at[1]).abs()))
            .filter(|&(_, l, dy)| l < reach && dy < 150.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, ..)| k)
    }

    /// The "Open" order (task 72, orderName F044568): the first of `ids` free walks to door `d`
    /// and toggles it there (`door_order_tick`).
    pub fn order_door(&mut self, ids: &[usize], d: usize) {
        let Some(&id) = ids.iter().find(|&&i| self.folk.get(i).is_some_and(|p| !p.down && p.carried_by.is_none())) else { return };
        let Some(s) = self.door_states.get(d) else { return };
        let at = s.door.position;
        self.order_move(&[id], at);
        self.folk[id].door_order = Some(d);
    }

    /// The door orders each tick: one within reach of its door (3 x DOOR_REACH of the door or its
    /// inside point) toggles it; one that stopped short walks on.
    pub fn door_order_tick(&mut self) {
        for id in 0..self.folk.len() {
            let Some(d) = self.folk[id].door_order else { continue };
            if self.folk[id].down || d >= self.door_states.len() {
                self.folk[id].door_order = None;
                continue;
            }
            let me = self.folk[id].mover.position_units();
            let s = &self.door_states[d];
            let near = |q: [f32; 3]| (q[0] - me[0]).hypot(q[2] - me[2]) < crate::sim::DOOR_REACH * 3.0;
            if near(s.door.position) || near(s.inside) {
                self.folk[id].door_order = None;
                if let Some(say) = self.toggle_door(d) {
                    self.messages.push(say.to_string());
                }
            } else if self.folk[id].ai.orders.is_empty() && self.folk[id].mover.velocity[0].hypot(self.folk[id].mover.velocity[2]) < 0.5 {
                let at = s.door.position;
                self.order_move(&[id], at);
            }
        }
    }

    /// F037658: a door toggled: open, it starts closing; shut, it starts opening (a locked one
    /// only if it is the player's own). Returns what to tell the player, if anything.
    pub fn toggle_door(&mut self, d: usize) -> Option<&'static str> {
        let own = self.door_owner(d).as_deref() == Some(PLAYER_FACTION);
        let s = self.door_states.get_mut(d)?;
        if s.broken {
            return Some("It's broken.");
        }
        if s.phase == crate::sim::DOOR_OPEN || s.phase == crate::sim::DOOR_OPENING {
            s.phase = crate::sim::DOOR_OPEN;
            s.start_close();
            return None;
        }
        if s.locked && s.level >= 1 {
            if !own {
                return Some("It's locked.");
            }
            s.locked = false;
        }
        s.start_open();
        None
    }

    /// BASH_DOOR_PLAYER_ORDER: the selection goes and bashes door `d`.
    pub fn order_bash(&mut self, ids: &[usize], d: usize) {
        for &id in ids {
            if let Some(p) = self.folk.get_mut(id) {
                p.bash_order = Some(d);
                p.ai.queue.clear();
                p.pathing = Default::default();
            }
        }
    }

    /// The number a faction has among the barriers' owners.
    pub fn faction_number(&mut self, f: &str) -> u32 {
        let n = self.faction_numbers.len() as u32;
        *self.faction_numbers.entry(f.to_string()).or_insert(n)
    }

    /// Whether door `d` is a gate (shut, it stops paths): the door of a wall gate, a gateway or
    /// a wall that links, or one that always locks.
    pub fn is_gate(&self, d: usize) -> bool {
        let s = &self.door_states[d];
        let b = self.data.get(&s.door.building);
        let flag = |k: &str| b.is_some_and(|r| r.bools.get(k).copied().unwrap_or(false));
        s.door.always_locks || flag("is wall gate") || flag("is gateway") || flag("links together")
    }

    /// The faction a door's building belongs to: its town's (the building's, F038257: a fresh
    /// zone's building takes its town's faction), else its squad's.
    pub fn door_owner(&self, d: usize) -> Option<String> {
        let s = &self.door_states[d];
        if s.player_building.is_some() {
            return Some(PLAYER_FACTION.to_string());
        }
        let town = match self.placed_town.get(s.door.placement) {
            Some(t) => *t,
            None => kenshi_world::town::current_town(&self.towns, s.door.position, None),
        };
        if let Some(t) = town
            && let Some(f) = self.towns[t].faction.clone()
        {
            return Some(f);
        }
        let p = self.placed.get(s.door.placement)?;
        [&p.squad, &p.residents].into_iter().flatten().find_map(|t| self.data.get(t)?.refs("faction").first().map(|(f, _)| f.clone()))
    }

    /// New doors' barriers (gates), and every barrier shut or not as its door is.
    fn sync_barriers(&mut self) {
        while self.barrier_of.len() < self.door_states.len() {
            let d = self.barrier_of.len();
            if !self.is_gate(d) {
                self.barrier_of.push(None);
                continue;
            }
            let owner = self.door_owner(d).map_or(u32::MAX - 1, |f| self.faction_number(&f));
            let m = kenshi_world::navmesh::METRES;
            let door = &self.door_states[d].door;
            let b = Barrier {
                a: door.ends[0].map(|v| v * m),
                b: door.ends[1].map(|v| v * m),
                lo: door.heights[0] * m,
                hi: door.heights[1] * m,
                owner,
                shut: false,
                locked: false,
            };
            self.barrier_of.push(Some(self.nav.barriers.len()));
            self.barrier_door.push(d);
            self.nav.barriers.push(b);
        }
        for (d, k) in self.barrier_of.iter().enumerate() {
            if let Some(k) = *k {
                let s = &self.door_states[d];
                let b = &mut self.nav.barriers[k];
                b.shut = !s.passable();
                b.locked = s.locked && s.level >= 1;
            }
        }
    }

    /// The doors moving, the gates barring the way, those walking up to a closed door opening
    /// it, and those hostile to a shut gate's owner bashing it.
    pub fn doors_tick(&mut self, dt: f32) {
        self.player_doors_sync();
        for d in &mut self.door_states {
            d.step(dt);
        }
        self.sync_barriers();
        // everyone's pass through the gates
        let player = self.faction_number(PLAYER_FACTION);
        // (each tick: the numbers looked up without copying the names)
        let mut numbers: Vec<u32> = Vec::with_capacity(self.folk.len());
        for i in 0..self.folk.len() {
            let n = if self.folk[i].player {
                player
            } else {
                match self.folk[i].faction.as_deref().and_then(|f| self.faction_numbers.get(f).copied()) {
                    Some(n) => n,
                    None => match self.folk[i].faction.clone() {
                        Some(f) => self.faction_number(&f),
                        None => u32::MAX,
                    },
                }
            };
            numbers.push(n);
        }
        for (p, &n) in self.folk.iter_mut().zip(&numbers) {
            p.pathing.pass = Pass { faction: if p.player { player } else { n }, player: p.player, ignore: false };
        }
        // who is walking where (people standing still open nothing), and which way
        let walkers: Vec<([f32; 3], Option<usize>, u32, bool, [f32; 3])> = self
            .folk
            .iter()
            .zip(&numbers)
            .filter(|(p, _)| !p.dead && !p.down && !p.away && p.speed > 0.1)
            .map(|(p, &n)| (p.mover.position_units(), p.home, if p.player { player } else { n }, p.player, p.mover.velocity))
            .collect();
        let reach2 = DOOR_OPEN_REACH * DOOR_OPEN_REACH;
        for d in 0..self.door_states.len() {
            let gate = self.barrier_of.get(d).copied().flatten().map(|k| self.nav.barriers[k].owner);
            let s = &mut self.door_states[d];
            if s.phase != DOOR_CLOSED || s.broken {
                continue;
            }
            let at = s.door.position;
            let locked = s.locked && s.level >= 1;
            let opener = walkers.iter().find(|(p, home, f, is_player, v)| {
                let (dx, dz) = (p[0] - at[0], p[2] - at[2]);
                if dx * dx + dz * dz >= reach2 || (p[1] - at[1]).abs() > 120.0 {
                    return false;
                }
                // heading for the door
                if v[0] * -dx + v[2] * -dz <= 0.0 {
                    return false;
                }
                let own = *home == Some(s.door.placement) || gate == Some(*f);
                match gate {
                    // a gate: its owner's people, the player's while it is not locked
                    Some(_) => own || (*is_player && !locked),
                    None => !locked || own,
                }
            });
            if opener.is_some() {
                s.locked = false;
                s.start_open();
            }
        }
        self.bash_tick(dt, &numbers);
    }

    /// The player's characters ordered to bash a door (BASH_DOOR_PLAYER_ORDER), and those whose
    /// way a gate shut who are hostile to its owner (F031240, F029386).
    fn bash_tick(&mut self, dt: f32, numbers: &[u32]) {
        let owners: std::collections::HashMap<u32, String> = self.faction_numbers.iter().map(|(f, &n)| (n, f.clone())).collect();
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            if p.dead || p.down {
                continue;
            }
            // the door: as ordered, else the gate in the way
            let (d, gate) = match (p.bash_order, p.gate_in_way.and_then(|k| self.barrier_door.get(k).map(|&d| (d, k)))) {
                (Some(d), _) => (d, self.barrier_of.get(d).copied().flatten()),
                (None, Some((d, k))) => (d, Some(k)),
                _ => continue,
            };
            if self.door_states[d].broken {
                let p = &mut self.folk[id];
                (p.bash_order, p.gate_in_way) = (None, None);
                continue;
            }
            if p.bash_order.is_none() {
                // only those hostile to the gate's owner bash a gate in their way, while it is shut
                let k = gate.unwrap_or(usize::MAX);
                let Some(b) = self.nav.barriers.get(k) else { continue };
                let mine = if p.player { Some(PLAYER_FACTION) } else { p.faction.as_deref() };
                let hostile = match (mine, owners.get(&b.owner)) {
                    (Some(m), Some(o)) => self.relations.get(m).is_some_and(|r| r.is_enemy(Some(o))) || self.relations.get(o).is_some_and(|r| r.is_enemy(Some(m))),
                    _ => false,
                };
                if self.door_states[d].passable() || !hostile || p.player || numbers.get(id) == Some(&b.owner) {
                    self.folk[id].gate_in_way = None;
                    continue;
                }
            }
            // to the door's near side
            let door = self.door_states[d].door.clone();
            let me = p.mover.position_units();
            let (dx, dz) = (me[0] - door.position[0], me[2] - door.position[2]);
            let l = dx.hypot(dz).max(1.0);
            if l > BASH_REACH {
                let to = [door.position[0] + dx / l * BASH_REACH * 0.6, door.position[1], door.position[2] + dz / l * BASH_REACH * 0.6];
                let p = &mut self.folk[id];
                if p.pathing.follow.is_none() && !p.pathing.pending {
                    p.pathing.go_to(id, &p.mover, to, true, &mut self.manager);
                }
                continue;
            }
            // a blow now and then
            let p = &mut self.folk[id];
            p.facing = [-dx / l, 0.0, -dz / l];
            p.bash_timer -= dt;
            if p.bash_timer > 0.0 {
                continue;
            }
            p.bash_timer = BASH_EVERY;
            let f = &self.arena.f[id];
            let (str_, dex) = (f.level(kenshi_char::StatsEnumerated::StatStrength), f.level(kenshi_char::StatsEnumerated::StatDexterity));
            let att = f.wielded.attacker(f.weapon_stat().map_or(0.0, |w| f.level(w)), (dex, 1.0), (str_, 1.0), 1.0);
            let mut blunt = kenshi_combat::blunt_damage(&att, &self.arena.sh.cc);
            if f.wielded.category == kenshi_combat::weapon_category::WeaponCategory::SkillUnarmed {
                blunt *= 0.25;
            }
            let blow = (blunt * 2.0).clamp(20.0, 60.0) * 0.015;
            let s = &mut self.door_states[d];
            s.health -= blow;
            s.paid = s.paid.min(s.health.max(0.0).ceil());
            if s.health <= 0.0 {
                s.broken = true;
                s.locked = false;
                s.phase = crate::sim::DOOR_OPENING;
                for q in self.folk.iter_mut().filter(|q| q.bash_order == Some(d) || (gate.is_some() && q.gate_in_way == gate)) {
                    (q.bash_order, q.gate_in_way) = (None, None);
                }
            }
        }
    }
}
