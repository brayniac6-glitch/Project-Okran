//! Building upgrades and downgrades (port_map `build`): a building of the player's turned into
//! the record its "upgrades to" names (Building::upgrade, F029413) or, while that upgrade is
//! being built, back into the record that upgrades to it (Building::downgrade, F029414). Either
//! way the building is set up again as its new record (vtable +0x458 / +0x450) and stands under
//! construction (Building +0x160 cleared, its pause too), its construction made for the new
//! record keeping the materials put in (F037832: each material the record needs as much as it
//! needs, what is in of it carried), its work held to 50 (an upgrade) or 80 (a downgrade)
//! materials' worth. An upgrade puts in at once the materials the building holds itself; a
//! downgrade whose materials are all in is built there and then (F037818 with FLT_MAX).
//!
//! (assumption) The sim keeps a building's materials in one ordered list, worked through in
//! turn: the construction made is laid out as what is in of each material first, then what is
//! still wanted of each. Materials put in beyond the new record's need are kept aside
//! (`PlayerBuilding::spare`), as Kenshi keeps them on the construction's list with no need. The
//! building's doors (its "doors"
//! record, F029413 / F029414 setting it to the new record's first) go with the old record and
//! come again once the new one is built, as the sim makes a building's doors when it is built
//! (`Sim::player_doors_sync`); its footprint stays as it was laid out.
use crate::base::{PlayerBuilding, unlocked};
use crate::sim::Sim;
use kenshi_data::{GameData, ItemType};

/// Work held to on an upgrade (F029413) and a downgrade (F029414), in materials' worth.
pub const UPGRADE_WORK: f32 = 50.0;
pub const DOWNGRADE_WORK: f32 = 80.0;

/// The materials put in a building, by kind (its construction's entries' have: F037703 sums
/// each one's min(have, need)), in the order its list first names them, those kept aside last.
pub fn materials_in(pb: &PlayerBuilding) -> Vec<(String, u32)> {
    let mut left = pb.used;
    let mut out: Vec<(String, u32)> = Vec::new();
    let put = |out: &mut Vec<(String, u32)>, m: &String, have: u32| match out.iter_mut().find(|o| o.0 == *m) {
        Some(o) => o.1 += have,
        None => out.push((m.clone(), have)),
    };
    for (m, n) in &pb.materials {
        let have = left.min(*n);
        left -= have;
        put(&mut out, m, have);
    }
    for (m, n) in &pb.spare {
        put(&mut out, m, *n);
    }
    out
}

/// @fn F037832
///
/// A construction for record `r` keeping the materials `had` put in: each material `r` needs
/// ("construction", else "build materials" of Building Materials) as much as it needs, and what
/// is in of it (no more than that); its need less what is in each one's cost. As the sim's list:
/// what is in first, then what is wanted, with how many are in.
/// Those put in beyond the need, kept aside.
pub fn construction_for(r: &GameData, had: &[(String, u32)]) -> (Vec<(String, u32)>, u32, Vec<(String, u32)>) {
    let need = PlayerBuilding::new(r, [0.0; 3], 0.0).materials;
    let (mut first, mut rest, mut used) = (Vec::new(), Vec::new(), 0);
    let mut left: Vec<(String, u32)> = Vec::new();
    for (m, k) in had {
        match left.iter_mut().find(|o| o.0 == *m) {
            Some(o) => o.1 += k,
            None => left.push((m.clone(), *k)),
        }
    }
    for (m, n) in need {
        let have = left.iter_mut().find(|o| o.0 == m).map_or(0, |o| {
            let h = o.1.min(n);
            o.1 -= h;
            h
        });
        if have > 0 {
            first.push((m.clone(), have));
            used += have;
        }
        if n > have {
            rest.push((m, n - have));
        }
    }
    first.extend(rest);
    left.retain(|o| o.1 > 0);
    (first, used, left)
}

impl Sim {
    /// @fn F029376
    ///
    /// The record building `b` upgrades to: its record's first "upgrades to", for one of the
    /// player's (its owner's +0x250), once the player's research has unlocked it (F046226).
    pub fn upgrade_target(&self, b: usize) -> Option<&GameData> {
        let rec = self.data.get(&self.buildings.get(b)?.record)?;
        let target = self.data.get(&rec.refs("upgrades to").first()?.0)?;
        unlocked(&self.data, &self.research_done).iter().any(|u| u.string_id == target.string_id).then_some(target)
    }

    /// @fn F029415
    ///
    /// The record building `b` downgrades to: the first BUILDING record whose first "upgrades
    /// to" is building `b`'s record, for one of the player's.
    pub fn downgrade_target(&self, b: usize) -> Option<&GameData> {
        let record = &self.buildings.get(b)?.record;
        self.data.of_type(ItemType::Building).find(|r| r.refs("upgrades to").first().is_some_and(|(t, _)| t == record))
    }

    /// Building `b` set up again as record `r` and under construction, keeping the materials
    /// put in, its work held to `work` (F029413 / F029414).
    fn rebuild_as(&mut self, b: usize, r: &GameData, work: f32) {
        let pb = &mut self.buildings[b];
        let had = materials_in(pb);
        let (materials, used, spare) = construction_for(r, &had);
        let fresh = PlayerBuilding::new(r, pb.at, pb.yaw);
        pb.record = r.string_id.clone();
        pb.materials = materials;
        pb.used = used;
        pb.spare = spare;
        pb.speed = fresh.speed;
        pb.done = false;
        pb.paused = false;
        pb.progress = pb.progress.min(work);
        // its inventory and machine parts made again for the new record once built
        pb.containers_made = false;
        pb.made.clear();
        pb.taken.clear();
        pb.full = false;
        for d in &mut self.door_states {
            if d.player_building == Some(b) {
                d.gone = true;
                d.player_building = None;
            }
        }
        self.buildings_changed = true;
    }

    /// @fn F029413
    ///
    /// Building::upgrade: building `b` turned into the record it upgrades to (F029376), its own
    /// inventory's materials then put into the construction, one at a time while wanted.
    /// Whether there was one.
    pub fn upgrade_building(&mut self, b: usize) -> bool {
        let Some(target) = self.upgrade_target(b).cloned() else { return false };
        self.rebuild_as(b, &target, UPGRADE_WORK);
        let pb = &mut self.buildings[b];
        loop {
            let Some(m) = pb.next_material().map(str::to_string) else { break };
            if !pb.storage.as_mut().is_some_and(|inv| crate::production::take_one(inv, &m)) {
                break;
            }
            pb.used += 1;
        }
        true
    }

    /// @fn F029414
    ///
    /// Building::downgrade: building `b` turned back into the record that upgrades to it
    /// (F029415); with every material it needs in, built at once. Whether there was one.
    pub fn downgrade_building(&mut self, b: usize) -> bool {
        let Some(source) = self.downgrade_target(b).cloned() else { return false };
        self.rebuild_as(b, &source, DOWNGRADE_WORK);
        let pb = &mut self.buildings[b];
        if pb.total() <= pb.used {
            pb.progress = pb.total() as f32;
            pb.done = true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(construction: &[(&str, i32)]) -> GameData {
        let refs = construction.iter().map(|(m, n)| (m.to_string(), [*n, 0, 0])).collect();
        GameData { references: vec![("construction".to_string(), refs)], ..GameData::default() }
    }

    #[test]
    fn materials_carry_over() {
        let old = record(&[("bm", 5)]);
        let mut pb = PlayerBuilding::new(&old, [0.0; 3], 0.0);
        pb.used = 5;
        assert_eq!(materials_in(&pb), vec![("bm".to_string(), 5)]);
        let new = record(&[("bm", 8), ("iron", 2)]);
        let (list, used, spare) = construction_for(&new, &materials_in(&pb));
        assert_eq!(list, vec![("bm".to_string(), 5), ("bm".to_string(), 3), ("iron".to_string(), 2)]);
        assert_eq!((used, spare), (5, vec![]));
        // back down: more in than needed, all of it there, the rest kept aside
        let (list, used, spare) = construction_for(&old, &[("bm".to_string(), 8), ("iron".to_string(), 1)]);
        assert_eq!((list, used), (vec![("bm".to_string(), 5)], 5));
        assert_eq!(spare, vec![("bm".to_string(), 3), ("iron".to_string(), 1)]);
        // a record needing other materials keeps the old ones aside, for the way back
        let other = record(&[("fruit", 4)]);
        let (list, used, spare) = construction_for(&other, &[("bm".to_string(), 5)]);
        assert_eq!((list, used, spare.clone()), (vec![("fruit".to_string(), 4)], 0, vec![("bm".to_string(), 5)]));
        let (list, used, spare) = construction_for(&old, &spare);
        assert_eq!((list, used, spare), (vec![("bm".to_string(), 5)], 5, vec![]));
    }
}
