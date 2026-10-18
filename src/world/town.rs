//! Towns (port_map `ai`): where the level files place them, how far they reach, and which one a
//! position is in.
//!
//! Spec: F048904 Town::setup (the record's "type" at +0xd8, "town radius mult" at +0x520: below
//! 0.1 it is 1, then clamped to 0.1..5), F048768 Town::radius (vtable +0x2a0), F048764
//! Town::contains, F048796 TownManager::nearest, F038707 AI::updateTown (the character's current
//! town, +0x88).
//!
//! The level files (`leveldata.level` in each `ZONE_FOLDERS` folder, later ones win) place each
//! TOWN record once, as an instance in world units.

use crate::ZONE_FOLDERS;
use kenshi_data::{GameDataContainer, ItemType};
use std::collections::HashMap;
use std::path::Path;

/// One placed town.
#[derive(Debug, Clone, PartialEq)]
pub struct Town {
    /// The TOWN record's string id.
    pub record: String,
    pub name: String,
    /// World units.
    pub position: [f32; 3],
    /// The record's "type" (+0xd8): 8 is not a town to be in (F048764, F048796), 2, 3, 4 and 9
    /// have their own radius (F048768).
    pub kind: i32,
    /// "town radius mult" (+0x520), as F048904 stores it.
    pub scale: f32,
    /// "no-foliage range" (+0x384, F048904; 0 for type 8): foliage that avoids towns keeps
    /// this far from the town's centre (F041802).
    pub no_foliage: f32,
    /// The TOWN record's "faction".
    pub faction: Option<String>,
    /// The TOWN record's "is public".
    pub public: bool,
}

/// The town radius a type gives, times the town's scale (F048768); player towns use their own
/// radius (+0x51c) instead and are not handled here.
/// @fn F048768
pub fn radius(kind: i32, scale: f32) -> f32 {
    scale
        * match kind {
            2 => 2500.0,
            3 => 937.5,
            4 => 875.0,
            9 => 437.5,
            8 | 10 => 0.0,
            _ => 1250.0,
        }
}

/// F048904's clamp of "town radius mult".
fn scale_of(mult: f32) -> f32 {
    let m = if (mult as f64) < 0.1 { 1.0 } else { mult };
    m.clamp(0.1, 5.0)
}

impl Town {
    pub fn radius(&self) -> f32 {
        radius(self.kind, self.scale)
    }

    /// Whether a position is within the town's radius times `mult`, across (F048764).
    /// @fn F048764
    pub fn contains(&self, p: [f32; 3], mult: f32) -> bool {
        if self.kind == 8 {
            return false;
        }
        let r = self.radius() * mult;
        (self.position[0] - p[0]).powi(2) + (self.position[2] - p[2]).powi(2) < r * r
    }
}

/// Every town the level files place, keyed by record (the last folder wins).
pub fn towns(install: &Path, data: &GameDataContainer) -> Vec<Town> {
    let mut by: HashMap<String, Town> = HashMap::new();
    for f in ZONE_FOLDERS {
        let p = install
            .join("data/newland/leveldata")
            .join(f)
            .join("leveldata.level");
        let Ok(level) = kenshi_io::fcs::read(&p) else {
            continue;
        };
        for r in &level.records {
            for i in &r.instances {
                let Some(t) = data.get(&i.target) else {
                    continue;
                };
                if t.item_type() != Some(ItemType::Town) {
                    continue;
                }
                let mult = t
                    .floats
                    .get("town radius mult")
                    .copied()
                    .or_else(|| t.ints.get("town radius mult").map(|&v| v as f32))
                    .unwrap_or(1.0);
                by.insert(
                    i.target.clone(),
                    Town {
                        record: i.target.clone(),
                        name: t.name.clone(),
                        faction: t.refs("faction").first().map(|(f, _)| f.clone()),
                        public: t.bools.get("is public").copied().unwrap_or(false),
                        position: i.position,
                        kind: *t.ints.get("type").unwrap_or(&0),
                        scale: scale_of(mult),
                        no_foliage: if *t.ints.get("type").unwrap_or(&0) == 8 {
                            0.0
                        } else {
                            t.floats
                                .get("no-foliage range")
                                .copied()
                                .or_else(|| t.ints.get("no-foliage range").map(|&v| v as f32))
                                .unwrap_or(0.0)
                        },
                    },
                );
            }
        }
    }
    let mut v: Vec<Town> = by.into_values().collect();
    v.sort_by(|a, b| a.record.cmp(&b.record));
    v
}

/// Every placement of a nest marker (a TOWN of type 8) in the level, with its position: the
/// markers biomes with "nests at fixed markers only" raise their nests at (F048808). [`towns`]
/// keeps one of each record; a marker record is placed many times.
pub fn nest_markers(install: &Path, data: &GameDataContainer) -> Vec<[f32; 3]> {
    let mut out = vec![];
    for f in ZONE_FOLDERS {
        let p = install.join("data/newland/leveldata").join(f).join("leveldata.level");
        let Ok(level) = kenshi_io::fcs::read(&p) else {
            continue;
        };
        for r in &level.records {
            for i in &r.instances {
                let Some(t) = data.get(&i.target) else {
                    continue;
                };
                if t.item_type() == Some(ItemType::Town) && t.ints.get("type") == Some(&8) && !out.contains(&i.position) {
                    out.push(i.position);
                }
            }
        }
    }
    out
}

/// The nearest town to a position across, of a type (10 = any, but type 8 only when asked
/// for), skipping `except` (F048796; the faction filters are not taken).
/// @fn F048796
pub fn nearest(towns: &[Town], p: [f32; 3], kind: i32, except: Option<usize>) -> Option<usize> {
    let mut best = None;
    let mut d = f32::MAX;
    for (i, t) in towns.iter().enumerate() {
        if Some(i) == except || (kind != 10 && kind != t.kind) || (t.kind == 8 && kind != 8) {
            continue;
        }
        let e = (t.position[0] - p[0]).powi(2) + (t.position[2] - p[2]).powi(2);
        if e < d {
            best = Some(i);
            d = e;
        }
    }
    best
}

/// The town at a position: the nearest within its radius (F038707's town part for something
/// in no town before, outside every building; `character_town`).
pub fn current_town(towns: &[Town], p: [f32; 3], was: Option<usize>) -> Option<usize> {
    character_town(towns, p, was, &[], None)
}

/// How near one of its town's gateways a character keeps that town: 300 units, across and up
/// (F038707's 90000 squared).
pub const GATE_REACH: f32 = 300.0;

/// The character's current town after a think (F038707, its town part):
/// - the town it was in (else the nearest, F048796) while the position is within its radius,
///   1.5 times for a character already in a town, once otherwise (F048764);
/// - or while it stands within `GATE_REACH` of that town's nearest gateway (Town +0x308,
///   F048826: the town's buildings that are a GatewayBuilding, +0x3d8 F038246, by their
///   position, Building +0x38 = +0x40); `gateways` holds each town's gateways;
/// - else, inside a building (Character::insideBuilding F039242), that building's town
///   (F023168: the town nearest the building, at any distance); `inside` is its position.
///
/// (not taken) The player's own towns first, within 1.25 times (the player's outposts are not
/// towns here); the manager's second list (F048792, (assumption) the nests) tried before the
/// building; Town +0x230 (F048831, the town's ground loaded) is taken as true.
/// @fn F038707
pub fn character_town(towns: &[Town], p: [f32; 3], was: Option<usize>, gateways: &[(usize, [f32; 3])], inside: Option<[f32; 3]>) -> Option<usize> {
    let mult = if was.is_some() { 1.5 } else { 1.0 };
    let by_gate = |c: usize| {
        gateways
            .iter()
            .filter(|(t, _)| *t == c)
            .any(|(_, g)| (g[0] - p[0]).powi(2) + (g[1] - p[1]).powi(2) + (g[2] - p[2]).powi(2) < GATE_REACH * GATE_REACH)
    };
    was.or_else(|| nearest(towns, p, 10, None))
        .filter(|&c| towns[c].contains(p, mult) || by_gate(c))
        .or_else(|| inside.and_then(|b| nearest(towns, b, 10, None)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn town(kind: i32, x: f32) -> Town {
        Town {
            faction: None,
            public: false,
            record: String::new(),
            name: String::new(),
            position: [x, 0.0, 0.0],
            kind,
            scale: 1.0,
            no_foliage: 0.0,
        }
    }

    #[test]
    fn radius_by_type() {
        assert_eq!(radius(2, 1.0), 2500.0);
        assert_eq!(radius(0, 2.0), 2500.0);
        assert_eq!(radius(8, 1.0), 0.0);
        assert_eq!(scale_of(0.0), 1.0);
        assert_eq!(scale_of(9.0), 5.0);
    }

    #[test]
    fn gateways_and_buildings_keep_their_town() {
        let ts = [town(2, 0.0), town(2, 10_000.0)];
        // 2600 out: beyond the radius (2500), but 250 from the first town's gateway
        let p = [2600.0, 0.0, 0.0];
        assert_eq!(character_town(&ts, p, None, &[], None), None);
        assert_eq!(character_town(&ts, p, None, &[(0, [2600.0, 0.0, 250.0])], None), Some(0));
        assert_eq!(character_town(&ts, p, None, &[(0, [2600.0, 0.0, 350.0])], None), None);
        // up counts too
        assert_eq!(character_town(&ts, p, None, &[(0, [2600.0, 400.0, 0.0])], None), None);
        // inside a building: the town nearest it, however far
        assert_eq!(character_town(&ts, [4000.0, 0.0, 0.0], None, &[], Some([4000.0, 0.0, 0.0])), Some(0));
        assert_eq!(character_town(&ts, [6000.0, 0.0, 0.0], None, &[], Some([6000.0, 0.0, 0.0])), Some(1));
    }

    #[test]
    fn staying_in_town_reaches_further() {
        let ts = [town(2, 0.0), town(2, 10_000.0)];
        assert_eq!(current_town(&ts, [3000.0, 0.0, 0.0], None), None);
        assert_eq!(current_town(&ts, [2000.0, 0.0, 0.0], None), Some(0));
        // once in, 1.5 times the radius
        assert_eq!(current_town(&ts, [3000.0, 0.0, 0.0], Some(0)), Some(0));
        // out of it: no town that think (F038707 tries only the town it was in), the nearest
        // the next
        assert_eq!(current_town(&ts, [8000.0, 0.0, 0.0], Some(0)), None);
        assert_eq!(current_town(&ts, [8000.0, 0.0, 0.0], None), Some(1));
        assert!(!town(8, 0.0).contains([0.0; 3], 1.0));
    }
}
