//! BiomeManager's cells (F048152: 64 x 64 of 0xb8 bytes at +0x88, one a zone): the squads each
//! holds, as the population tick (F048140), the homeless spawn (F048137) and the area order's
//! zone weights (F048108) count them.
//!
//! Spec:
//! - F045409 (Platoon vtable +0xe0, its update): the cell its position (+0x48) is in;
//!   F048125: unchanged, nothing; else out of the old cell's three sets and into the new one's
//!   (F048118);
//! - F048118: into the "town" set (+0x38) when F045424 keeps the platoon or its home
//!   (+0x148 +0x30) is a nest (TownBase vtable +0x270, non-zero for a Nest only); else into the
//!   "roaming" set (+0x60, counted at +0x70);
//! - F045424: kept when +0x115 is set; else when its state (+0x118) is not 3, it is not loaded
//!   or has people (+0x1d8 +0x58), and its faction's missions (+0x88, F028862) hold an entry for
//!   it not done (+0xc0 != 2: F029166 sets 2 once the squad carries its spoils home);
//! - F048126 (each population tick, for each zone loaded): a roaming squad no longer able
//!   (F044612) moves to the "beaten" set (+0x88, counted at +0x98, half in the tick's sums);
//!   of the town set the first not able leaves; of the beaten set the first unloaded
//!   (+0x1d8 null) leaves (each set ordered by the platoon's address: the port's platoon
//!   number);
//! - F044612: a platoon deleted (+0x1f0) is not able; a loaded one works it out again
//!   (F044599 -> F044564, kept at +0x100), an unloaded one keeps the last;
//! - F044564: able when its people are not made yet (+0xf0) or it is the player's (faction
//!   +0x250); else of its people those not dead (+0x5bc) and up (vtable +0x30, F039191: dead,
//!   down) count; with a leader (+0xa0) and any counted: a squad of fewer than 4 (+0xa4, its
//!   template's count as made, at least 1) is able while the leader is up; a bigger one while
//!   the count is at least its size x 0.2 (x 0.4 with its leader down).
//!
//! (not ported) F044564's other ways to count a member who is down (+0x504 below 0.2, or 0.8
//! with +0x115 and a home) and its +0x70 (which drops the share to 0), F044563 (a member caged
//! or of state +0x2f8 2 makes the squad able), and the +0x115 "kept" flag F044599 and the
//! platoon's constructor set: none of these fields is traced. The port keeps a zone's people
//! when it unloads (no zone content unloading), so only a squad gone away off-screen (a world
//! squad) or deleted counts as unloaded.
use crate::sim::Sim;
use std::collections::BTreeMap;

/// Which of a cell's sets a platoon is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSet {
    /// +0x38: kept squads (F045424) and nests' squads.
    Town,
    /// +0x60: the rest, counted at +0x70.
    Roaming,
    /// +0x88: roaming squads beaten (F048126), counted at +0x98.
    Beaten,
}

/// A platoon's cell (Platoon +0x120) and the set it is in there (none once F048126 dropped it;
/// it stays the platoon's cell until it moves on).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellEntry {
    pub zone: (i32, i32),
    pub set: Option<CellSet>,
    /// F044612's last answer (ActivePlatoon -> Platoon +0x100).
    pub able: bool,
    /// Whether the platoon is loaded (+0x1d8 set).
    pub loaded: bool,
}

/// The cells' squads: each platoon's entry, by platoon number.
#[derive(Debug, Clone, Default)]
pub struct Cells {
    pub of: BTreeMap<usize, CellEntry>,
}

impl Cells {
    /// A cell's +0x70 (roaming) and +0x98 (beaten) counts.
    pub fn counts(&self, z: (i32, i32)) -> (usize, usize) {
        let mut n = (0, 0);
        for e in self.of.values().filter(|e| e.zone == z) {
            match e.set {
                Some(CellSet::Roaming) => n.0 += 1,
                Some(CellSet::Beaten) => n.1 += 1,
                _ => {}
            }
        }
        n
    }

    /// F048140's measure of a cell: +0x70 + +0x98 x 0.5.
    pub fn squads(&self, z: (i32, i32)) -> f32 {
        let (a, c) = self.counts(z);
        a as f32 + c as f32 * 0.5
    }
}

impl Sim {
    /// Each platoon's people, in the order they were made.
    fn platoon_members(&self) -> BTreeMap<usize, Vec<usize>> {
        let mut of: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for p in &self.folk {
            if let Some(pid) = p.platoon {
                of.entry(pid).or_default().push(p.id);
            }
        }
        of
    }

    /// A platoon's position (+0x48) and whether it is loaded (+0x1d8): a loaded one (any of its
    /// people here, not away) at its leader's place (`platoon_leader`); one away at its world
    /// squad's place.
    fn platoon_place(&self, members: &[usize]) -> Option<([f32; 3], bool)> {
        if let Some(m) = self.platoon_leader(members) {
            return Some((self.folk[m].mover.position_units(), true));
        }
        members.iter().find_map(|&m| self.folk[m].world_squad).and_then(|k| self.world.squads.get(k)).map(|s| (s.pos, false))
    }

    /// The platoon's leader (ActivePlatoon +0xa0): its first person here of the leader's role,
    /// else its first person here (an animals' squad's).
    fn platoon_leader(&self, members: &[usize]) -> Option<usize> {
        let here = |m: &usize| !self.folk[*m].away;
        members.iter().copied().filter(here).find(|&m| self.folk[m].role == 5).or_else(|| members.iter().copied().find(here))
    }

    /// @fn F045424
    ///
    /// Whether the platoon is kept in its cell's town set: on one of its faction's missions not
    /// done (the port's faction missions: its world squad marching on a campaign).
    fn platoon_kept(&self, members: &[usize]) -> bool {
        members.iter().filter_map(|&m| self.folk[m].world_squad).any(|k| {
            self.world.squads.get(k).is_some_and(|s| s.alive && matches!(s.goal, crate::world::Goal::Campaign { .. }))
        })
    }

    /// Whether the platoon's home (+0x148 +0x30) is a nest: a nest's people, or a homeless
    /// squad homed at one (F048137).
    fn platoon_nest_home(&self, members: &[usize]) -> bool {
        if self.nests.iter().any(|n| members.iter().any(|m| n.members.contains(m))) {
            return true;
        }
        self.population.as_ref().is_some_and(|pop| members.iter().any(|m| matches!(pop.home_town.get(m), Some(Err(_)))))
    }

    /// @fn F044564
    ///
    /// Whether a loaded platoon can still fight (see the module's spec).
    pub(crate) fn platoon_able(&self, members: &[usize]) -> bool {
        let here: Vec<usize> = members.iter().copied().filter(|&m| !self.folk[m].away).collect();
        if here.is_empty() || here.iter().any(|&m| self.folk[m].player) {
            return true;
        }
        let is_up = |m: usize| !self.folk[m].dead && !self.folk[m].down;
        let up = here.iter().filter(|&&m| is_up(m)).count();
        let Some(leader) = self.platoon_leader(members) else { return false };
        if up == 0 {
            return false;
        }
        let size = members.len().max(1);
        if size < 4 {
            return is_up(leader);
        }
        let share = if is_up(leader) { 0.2 } else { 0.4 };
        size as f32 * share <= up as f32
    }

    /// @fn F045409
    ///
    /// Every platoon's cell kept up (F048125, F048118), the deleted ones' dropped.
    pub fn cells_update(&mut self) {
        let platoons = self.platoon_members();
        let gone: Vec<usize> = self.cells.of.keys().copied().filter(|k| !platoons.contains_key(k)).collect();
        for k in gone {
            self.cells.of.remove(&k);
        }
        for (&pid, members) in &platoons {
            let Some((at, loaded)) = self.platoon_place(members) else {
                self.cells.of.remove(&pid);
                continue;
            };
            let zone = kenshi_world::zone_of(at[0], at[2]);
            // F044612: a loaded platoon works it out again, an unloaded one keeps the last
            let able = if loaded { self.platoon_able(members) } else { self.cells.of.get(&pid).is_none_or(|e| e.able) };
            if let Some(e) = self.cells.of.get_mut(&pid) {
                e.able = able;
                e.loaded = loaded;
                if e.zone == zone {
                    continue;
                }
            }
            let set = if self.platoon_kept(members) || self.platoon_nest_home(members) { CellSet::Town } else { CellSet::Roaming };
            self.cells.of.insert(pid, CellEntry { zone, set: Some(set), able, loaded });
        }
    }

    /// @fn F048126
    ///
    /// A cell's sets kept: its roaming squads no longer able beaten; the first of its town set
    /// not able out; the first of its beaten set unloaded out.
    pub fn cell_tick(&mut self, z: (i32, i32)) {
        let here: Vec<usize> = self.cells.of.iter().filter(|(_, e)| e.zone == z).map(|(&k, _)| k).collect();
        for &k in &here {
            let e = self.cells.of.get_mut(&k).unwrap();
            if e.set == Some(CellSet::Roaming) && !e.able {
                e.set = Some(CellSet::Beaten);
            }
        }
        if let Some(&k) = here.iter().find(|k| self.cells.of[k].set == Some(CellSet::Town) && !self.cells.of[k].able) {
            self.cells.of.get_mut(&k).unwrap().set = None;
        }
        if let Some(&k) = here.iter().find(|k| self.cells.of[k].set == Some(CellSet::Beaten) && !self.cells.of[k].loaded) {
            self.cells.of.get_mut(&k).unwrap().set = None;
        }
    }
}
