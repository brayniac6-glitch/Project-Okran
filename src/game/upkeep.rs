//! The player's buildings after they are built (port_map `build`): damage, repair and
//! dismantling, and the doors of the player's gates.
//!
//! A building's health is its construction (Building +0x164, in its materials' worth): a wall
//! (WallBuilding, "links together") hit takes min(max(blunt, 10), 60) x 0.003 off it (F029385;
//! the blow's cut counts nothing) and, at none, is destroyed ("Break"); other buildings take no
//! damage (Building's takeDamage does nothing, F022605). A damaged building is repaired as it
//! was built (Task_Build, F031256): its builders work it back up to its materials' total, a
//! material for each material's worth the damage took. A gate's door is repaired the same way,
//! a "Building Material" a unit.
//!
//! Dismantling (Task_Dismantle, F031298 / F029409): a dismantler takes down
//! lerp(0.05, 4, engineering / 100) x 4 of its construction a second; not while furniture
//! stands in it ("I have to dismantle the furniture first."); at none it is gone
//! ("Thing_Complete").
//! (assumption) The materials put into it come back (to the dismantler, what doesn't fit on the
//! ground at their feet); Kenshi's own refund is not read.
use crate::base::{BUILDING_MATERIAL, builder_rate};
use crate::sim::{DoorState, Sim};
use kenshi_char::StatsEnumerated;

impl Sim {
    /// Whether the player's building `b` (built) has been damaged: its construction short of
    /// its materials' total, or one of its doors hurt.
    pub fn needs_repair(&self, b: usize) -> bool {
        let Some(pb) = self.buildings.get(b) else { return false };
        pb.done && (pb.progress < pb.total() as f32 - 1e-3 || self.door_states.iter().any(|d| d.player_building == Some(b) && !d.gone && d.health < d.full - 1e-3))
    }

    /// "Repair" (order 95, Repairing: Task_Build on a damaged building): an order on the task
    /// system's list for each of `ids`, planned (crate::jobs).
    pub fn order_repair(&mut self, ids: &[usize], b: usize) {
        if !self.needs_repair(b) {
            return;
        }
        let at = self.buildings[b].at;
        let ids: Vec<usize> = ids.iter().copied().filter(|&id| self.folk.get(id).is_some_and(|p| !p.down)).collect();
        self.order_task(&ids, crate::jobs::REPAIRING, crate::sim::Target::Building(b), at);
    }

    /// One repairer's work on building `b` this tick (Task_Build's on a built building): its
    /// doors first, then its construction. Whether it is whole again.
    pub(crate) fn repair_work(&mut self, id: usize, b: usize, dt: f32) -> bool {
        let hour = self.hour_secs as f32;
        let rate = builder_rate(self.arena.f[id].level(StatsEnumerated::StatEngineering)) * self.buildings[b].speed * dt / hour;
        if let Some(d) = self.door_states.iter().position(|d| d.player_building == Some(b) && !d.gone && d.health < d.full - 1e-3) {
            if self.door_states[d].health >= self.door_states[d].paid {
                if !self.take_material(id, BUILDING_MATERIAL) {
                    self.missing_material = Some(BUILDING_MATERIAL.to_string());
                    return false;
                }
                self.door_states[d].paid += 1.0;
            }
            let s = &mut self.door_states[d];
            s.health = (s.health + rate).min(s.paid).min(s.full);
            if s.health >= s.full - 1e-3 {
                s.health = s.full;
                s.broken = false;
            }
            return false;
        }
        let pb = &self.buildings[b];
        if pb.progress >= pb.used as f32 {
            let Some(m) = pb.next_material().map(str::to_string) else { return true };
            if !self.take_material(id, &m) {
                self.missing_material = Some(m);
                return false;
            }
            self.buildings[b].used += 1;
        }
        let pb = &mut self.buildings[b];
        pb.progress = (pb.progress + rate).min(pb.used as f32);
        if pb.progress >= pb.total() as f32 - 1e-4 {
            pb.progress = pb.total() as f32;
            return !self.needs_repair(b);
        }
        false
    }

    /// @fn F029385
    ///
    /// A blow on the player's building `b`: a wall's construction goes down by
    /// min(max(blunt, 10), 60) x 0.003 (cut counts nothing); at none it is destroyed. Other
    /// buildings take no damage. Whether it was destroyed.
    pub fn damage_building(&mut self, b: usize, blunt: f32) -> bool {
        let Some(pb) = self.buildings.get_mut(b) else { return false };
        let wall = self.data.get(&pb.record).is_some_and(|r| r.bools.get("links together").copied().unwrap_or(false));
        if !wall || !pb.done {
            return false;
        }
        pb.progress -= (blunt * 2.0 * 0.5).clamp(10.0, 60.0) * 0.003;
        pb.used = pb.used.min(pb.progress.max(0.0).ceil() as u32);
        if pb.progress <= 0.0 {
            self.messages.push(format!("{} destroyed", self.data.get(&self.buildings[b].record).map_or("Building", |r| r.name.as_str())));
            self.remove_building(b);
            return true;
        }
        false
    }

    /// Order characters to dismantle building `b` (Task_Dismantle, F031298).
    pub fn order_dismantle(&mut self, ids: &[usize], b: usize) {
        let Some(pb) = self.buildings.get(b) else { return };
        if !pb.confirmed {
            return;
        }
        let (at, r) = (pb.at, pb.radius);
        for &id in ids {
            if self.folk.get(id).is_some_and(|p| !p.down) {
                let me = self.folk[id].mover.position_units();
                let (dx, dz) = (me[0] - at[0], me[2] - at[2]);
                let d = dx.hypot(dz).max(1e-3);
                self.order_move(&[id], [at[0] + dx / d * r, at[1], at[2] + dz / d * r]);
                self.folk[id].build = None;
                self.folk[id].dismantle = Some(b);
            }
        }
    }

    /// Furniture of the player's standing inside building `b` (its interior furniture).
    fn furniture_inside(&self, b: usize) -> bool {
        let pb = &self.buildings[b];
        let has_interior = self.data.get(&pb.record).is_some_and(|r| !r.refs("interior").is_empty());
        has_interior
            && self.buildings.iter().enumerate().any(|(k, o)| {
                k != b
                    && o.confirmed
                    && self.data.get(&o.record).is_some_and(|r| r.bools.get("is interior furniture").copied().unwrap_or(false))
                    && (o.at[0] - pb.at[0]).hypot(o.at[2] - pb.at[2]) < pb.radius
            })
    }

    /// @fn F031298
    ///
    /// Dismantlers at their building take it down (F029409); at none it is gone and its
    /// materials come back.
    pub fn dismantle_tick(&mut self, dt: f32) {
        for id in 0..self.folk.len() {
            let Some(b) = self.folk[id].dismantle else { continue };
            if self.folk[id].down || b >= self.buildings.len() {
                self.folk[id].dismantle = None;
                continue;
            }
            let (at, r) = (self.buildings[b].at, self.buildings[b].radius);
            let me = self.folk[id].mover.position_units();
            if (me[0] - at[0]).hypot(me[2] - at[2]) > r + 15.0 {
                continue;
            }
            if self.furniture_inside(b) {
                self.messages.push("I have to dismantle the furniture first.".to_string());
                self.folk[id].dismantle = None;
                continue;
            }
            let eng = self.arena.f[id].level(StatsEnumerated::StatEngineering).clamp(0.0, 100.0);
            let rate = 0.05 + (4.0 - 0.05) * eng * 0.01;
            let pb = &mut self.buildings[b];
            if !pb.done {
                // a site taken down: its work first
                pb.progress = pb.progress.min(pb.used as f32);
            }
            pb.progress -= dt * rate * 4.0 * pb.speed;
            self.arena.f[id].skills.train(StatsEnumerated::StatEngineering, dt * 0.05, 1.0);
            if pb.progress <= 0.0 {
                // the materials put in, back to the dismantler (else at their feet)
                let mut back = Vec::new();
                let mut left = pb.used;
                for (m, n) in &pb.materials {
                    let k = left.min(*n);
                    left -= k;
                    if k > 0 {
                        back.push((m.clone(), k));
                    }
                }
                self.remove_building(b);
                for (m, n) in back {
                    let Some(rec) = self.data.get(&m).cloned() else { continue };
                    for _ in 0..n {
                        let item = kenshi_char::inventory::Item::from_record(&rec, 1);
                        if !self.folk[id].inventory.add(item.clone(), false) {
                            self.drop_item(id, item);
                        }
                    }
                }
                self.dismantled += 1;
            }
        }
    }

    /// Building `b` gone (dismantled, destroyed): those working on it stop, its doors go, and
    /// the buildings after it move down one.
    pub fn remove_building(&mut self, b: usize) {
        if b >= self.buildings.len() {
            return;
        }
        self.buildings.remove(b);
        let fix = |v: &mut Option<usize>| {
            *v = match *v {
                Some(k) if k == b => None,
                Some(k) if k > b => Some(k - 1),
                other => other,
            }
        };
        for p in &mut self.folk {
            fix(&mut p.build);
            fix(&mut p.operate);
            fix(&mut p.dismantle);
        }
        self.jobs_building_gone(b);
        for d in &mut self.door_states {
            if d.player_building == Some(b) {
                d.gone = true;
                d.player_building = None;
            } else {
                fix(&mut d.player_building);
            }
        }
        self.buildings_changed = true;
    }

    /// The player's built buildings with doors (their gates) given door states, as a placed
    /// building's are (closed, unlocked).
    pub(crate) fn player_doors_sync(&mut self) {
        for b in 0..self.buildings.len() {
            let pb = &self.buildings[b];
            if !pb.done || self.door_states.iter().any(|d| d.player_building == Some(b)) {
                continue;
            }
            let Some(rec) = self.data.get(&pb.record) else { continue };
            if rec.refs("doors").is_empty() {
                continue;
            }
            let place = kenshi_world::Placement {
                building: pb.record.clone(),
                position: pb.at,
                rotation: [(pb.yaw * 0.5).cos(), 0.0, (pb.yaw * 0.5).sin(), 0.0],
                world_y: Some(pb.at[1]),
                interior: None,
                exterior: None,
                town: None,
                squad: None,
                residents: None,
                locks: Vec::new(),
                door_saved: Vec::new(),
                destroyed: false,
                power_on: None,
            };
            let data = self.data.clone();
            for door in kenshi_world::interior::doors(&data, &self.res, std::slice::from_ref(&place)) {
                let inside = door.position;
                let mut s = DoorState::new(&data, &place, door, false, 0, inside);
                s.player_building = Some(b);
                s.door.placement = usize::MAX;
                self.door_states.push(s);
            }
        }
    }
}
