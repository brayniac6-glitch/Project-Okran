//! Towns (port_map `ai`): where the level files place them, how far they reach, and which one a
//! position is in.
//!
//! Spec: F048904 Town::setup (the record's "type" at +0xd8, "town radius mult" at +0x520: below
//! 0.1 it is 1, then clamped to 0.1..5), F048768 Town::radius (vtable +0x2a0), F048764
//! Town::contains, F048796 TownManager::nearest, F038707 AI::updateTown (the character's current
//! town, +0x88).
//!
//! The level files (`leveldata.level` in each `ZONE_FOLDERS` folder, merged in folder order,
//! `read_level`) place the towns: each instance of a TOWN record in the town list is a town of
//! its own (TownManager makes a Town for each, F048904), so a record placed several times (the
//! Holy Mines, the Hive Villages, the nest markers) is that many towns.

use kenshi_data::{GameDataContainer, ItemType};
use std::path::Path;

/// One placed town.
#[derive(Debug, Clone, PartialEq)]
pub struct Town {
    /// The level instance that places it (its id in the town list): the town's own key.
    pub id: String,
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

/// Every town the level files place: each TOWN instance of the merged level data (`read_level`),
/// in the level's order (the order TownManager makes them in).
pub fn towns(install: &Path, data: &GameDataContainer) -> Vec<Town> {
    let Some(level) = crate::read_level(install) else { return Vec::new() };
    let mut v: Vec<Town> = Vec::new();
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
            let kind = *t.ints.get("type").unwrap_or(&0);
            v.push(Town {
                id: i.id.clone(),
                record: i.target.clone(),
                name: t.name.clone(),
                faction: t.refs("faction").first().map(|(f, _)| f.clone()),
                public: t.bools.get("is public").copied().unwrap_or(false),
                position: i.position,
                kind,
                scale: scale_of(mult),
                no_foliage: if kind == 8 {
                    0.0
                } else {
                    t.floats
                        .get("no-foliage range")
                        .copied()
                        .or_else(|| t.ints.get("no-foliage range").map(|&v| v as f32))
                        .unwrap_or(0.0)
                },
            });
        }
    }
    v
}

/// Every placement of a nest marker (a TOWN of type 8) in the level, with its position: the
/// markers biomes with "nests at fixed markers only" raise their nests at (F048808): the towns
/// of kind 8 (`towns`), one a position.
pub fn nest_markers(install: &Path, data: &GameDataContainer) -> Vec<[f32; 3]> {
    let mut out: Vec<[f32; 3]> = vec![];
    for t in towns(install, data).into_iter().filter(|t| t.kind == 8) {
        if !out.contains(&t.position) {
            out.push(t.position);
        }
    }
    out
}

/// @fn F022648
///
/// A hand's hash (Kenshi's hash for its object handles): boost's hash_combine over its
/// container, index, serial and container serial, then its type (64-bit); a type 11 hand (no
/// object) hashes to 0x9e3779c4. `h` is [type, container, container serial, index, serial], as
/// a zone state keeps them ("handleTYPE", "handleC", "handleCS", "handleI", "handleS").
pub fn hand_hash(h: [i32; 5]) -> u64 {
    const K: u64 = 0x9e37_79b9;
    let [ty, c, cs, i, s] = h;
    if ty == 11 {
        return 0x9e37_79c4;
    }
    let mut seed = (c as u32 as u64).wrapping_add(K);
    for v in [i, s, cs] {
        seed ^= (seed << 6).wrapping_add(v as u32 as u64).wrapping_add(K).wrapping_add(seed >> 2);
    }
    seed ^ (seed >> 2).wrapping_add(seed << 6).wrapping_add(K).wrapping_add(ty as i64 as u64)
}

/// @fn F029224
/// @fn F029218
/// @fn F029196
/// @fn F029221
///
/// The order Kenshi's hash map of hands (boost::unordered, all its nodes on one list from a
/// start node) gives keys inserted in the order of `keys`: each key's hash (`hand_hash`) mixed
/// (x * 0x200000 + !x, then the shifts and multiplies of F029224), its bucket the low bits; a
/// key into an empty bucket goes to the front of the list (the old first node's bucket now
/// pointing at it), one into a bucket in use right after that bucket's previous node; a key
/// already in is not added. 16 buckets to start, load factor 1: inserting past the buckets
/// grows them (F029221: to the power of two above 1.5 x the size, F029196) and the list is
/// walked again (F029218: a node whose bucket is empty stays and marks it, another moves
/// behind its bucket's previous node). Returns indices into `keys`, in the map's order.
pub fn hand_map_order(keys: &[[i32; 5]]) -> Vec<usize> {
    fn mix(mut h: u64) -> u64 {
        h = h.wrapping_mul(0x20_0000).wrapping_add(!h);
        h = ((h >> 24) ^ h).wrapping_mul(0x109);
        h = ((h >> 14) ^ h).wrapping_mul(0x15);
        ((h >> 28) ^ h).wrapping_mul(0x8000_0001)
    }
    // F029196: the buckets for a size (load factor 1): the power of two above it, at least 4
    fn buckets_for(n: usize) -> usize {
        let v = n + 1;
        if v < 5 { 4 } else { v.next_power_of_two() }
    }
    // nodes: (key index, hash, next); NONE ends the list; START is the start node
    const NONE: usize = usize::MAX;
    const START: usize = usize::MAX - 1;
    let mut nodes: Vec<(usize, u64, usize)> = Vec::new();
    let mut first = NONE;
    let mut count: usize = 16;
    // each bucket's previous node (START or a node), when in use
    let mut buckets: Vec<Option<usize>> = vec![None; count];
    let next_of = |nodes: &Vec<(usize, u64, usize)>, first: usize, p: usize| if p == START { first } else { nodes[p].2 };
    for (k, &key) in keys.iter().enumerate() {
        let h = mix(hand_hash(key));
        if nodes.iter().any(|n| keys[n.0] == key) {
            continue;
        }
        // F029221: grow past the load
        if nodes.len() + 1 > count {
            let want = buckets_for((nodes.len() + nodes.len() / 2).max(nodes.len() + 1));
            if want != count {
                count = want;
                buckets = vec![None; count];
                let mut prev = START;
                loop {
                    let n = next_of(&nodes, first, prev);
                    if n == NONE {
                        break;
                    }
                    let b = (nodes[n].1 & (count as u64 - 1)) as usize;
                    match buckets[b] {
                        None => {
                            buckets[b] = Some(prev);
                            prev = n;
                        }
                        Some(bp) => {
                            // unlink n, put it after its bucket's previous node
                            let after = nodes[n].2;
                            if prev == START { first = after } else { nodes[prev].2 = after }
                            let bn = next_of(&nodes, first, bp);
                            nodes[n].2 = bn;
                            if bp == START { first = n } else { nodes[bp].2 = n }
                        }
                    }
                }
            }
        }
        let i = nodes.len();
        let b = (h & (count as u64 - 1)) as usize;
        match buckets[b] {
            None => {
                if first != NONE {
                    let fb = (nodes[first].1 & (count as u64 - 1)) as usize;
                    buckets[fb] = Some(i);
                }
                buckets[b] = Some(START);
                nodes.push((k, h, first));
                first = i;
            }
            Some(bp) => {
                let bn = next_of(&nodes, first, bp);
                nodes.push((k, h, bn));
                if bp == START { first = i } else { nodes[bp].2 = i }
            }
        }
    }
    let mut out = Vec::new();
    let mut n = first;
    while n != NONE {
        out.push(nodes[n].0);
        n = nodes[n].2;
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

/// How far a building outside every town looks for one to belong to (F038257: 1e8 squared).
pub const BUILDING_TOWN_REACH: f32 = 10_000.0;

/// @fn F048806
///
/// TownManager::findTown: the town a position is in. The manager keeps its towns on a 64 x 64
/// grid of the zones; of the towns in the 3 x 3 cells about the position's (the zone it is in
/// and those round it), none a nest marker (type 8, `Town::kind`), none its vtable +0x270 marks
/// (0 for every Town, F049332), none of the player's (`player`) when `fresh`, the nearest whose
/// radius (F048768) takes it in, across. None (Kenshi's TownNull, +0x180f8) when there is none.
/// A town is kept in the one cell its centre is in (TownManager's removal, F048847, takes it out
/// of the cell of its position, F051545).
pub fn find_town(towns: &[Town], p: [f32; 3], fresh: bool, player: impl Fn(usize) -> bool) -> Option<usize> {
    let (zx, zy) = crate::zone_of(p[0], p[2]);
    let mut best = None;
    let mut d = f32::MAX;
    for dx in -1..=1 {
        for dy in -1..=1 {
            let cell = (zx + dx, zy + dy);
            if !(0..64).contains(&cell.0) || !(0..64).contains(&cell.1) {
                continue;
            }
            for (i, t) in towns.iter().enumerate() {
                if t.kind == 8 || crate::zone_of(t.position[0], t.position[2]) != cell || (fresh && player(i)) {
                    continue;
                }
                let r = t.radius();
                let e = (t.position[2] - p[2]).powi(2) + (t.position[0] - p[0]).powi(2);
                if e <= r * r && e < d {
                    best = Some(i);
                    d = e;
                }
            }
        }
    }
    best
}

/// @fn F038257
///
/// The town a building a zone file places belongs to (the building's creation, its town
/// part), in a fresh zone (ZoneMapContent +0xa8: the shipped zone, not a saved one):
/// - the zone state's "town" handle (townTYPE / townC / townCS) is kept only for a town of the
///   player's, or for one whose vtable +0x270 is set and within 13824 units: never for an NPC
///   town (Town's +0x270 is 0, F049332), so the shipped handles (some stale: the Spider
///   Factory's Ancient Factory names a town 295 of another serial) are not read;
/// - the town it stands in (`find_town`, F048806);
/// - for a building made in game (its instance's record, +0x18, of no file: +0x48 -1; (assumption)
///   the player's own builds, `crate::base`'s, never a zone file's), the player's town nearest
///   unless the building "creates player town";
/// - in none, a building with an "interior" (F021903: the reference list not empty), or one
///   inside another ("isInsideBuilding"), mounted or put up for an owner (`inside`): the
///   nearest town of all, nest markers too, not the player's, within `BUILDING_TOWN_REACH`
///   (3D, the town's position against the building's).
///
/// None: the wilderness (TownNull). A saved zone's building keeps its handle's town (Building
/// +0x1d0, saved by F037788): the town this gave it as its zone first loaded, so a world the
/// rewrite rebuilds from the shipped zones (crate::save) gets the same.
pub fn building_town(towns: &[Town], p: [f32; 3], interior: bool, inside: bool, player: impl Fn(usize) -> bool) -> Option<usize> {
    if let Some(t) = find_town(towns, p, true, &player) {
        return Some(t);
    }
    if !interior && !inside {
        return None;
    }
    let mut best = None;
    let mut d = BUILDING_TOWN_REACH * BUILDING_TOWN_REACH;
    for (i, t) in towns.iter().enumerate() {
        if player(i) {
            continue;
        }
        let e = (t.position[0] - p[0]).powi(2) + (t.position[1] - p[1]).powi(2) + (t.position[2] - p[2]).powi(2);
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
/// towns here); the manager's second list (F048792: the nests, TownList::add F048868) tried before the
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

    #[test]
    fn a_hand_map_keeps_each_key_once_in_one_order_through_growing() {
        let keys: Vec<[i32; 5]> = (0..40).map(|k| [0, 1619, 11111, 300 + k, k * 7919]).collect();
        let mut with_twice = keys.clone();
        with_twice.push(keys[3]);
        let order = hand_map_order(&with_twice);
        let mut seen = order.clone();
        seen.sort();
        assert_eq!(seen, (0..40).collect::<Vec<_>>());
        // the order is the hashes' and the insertion's, the same each time
        assert_eq!(order, hand_map_order(&keys));
        // a key into an empty bucket of a fresh map goes to the front
        let two = hand_map_order(&keys[..2]);
        let b = |k: [i32; 5]| {
            let mut h = hand_hash(k);
            h = h.wrapping_mul(0x20_0000).wrapping_add(!h);
            h = ((h >> 24) ^ h).wrapping_mul(0x109);
            h = ((h >> 14) ^ h).wrapping_mul(0x15);
            ((h >> 28) ^ h).wrapping_mul(0x8000_0001) & 15
        };
        if b(keys[0]) != b(keys[1]) {
            assert_eq!(two, vec![1, 0]);
        }
        assert_eq!(hand_hash([11, 1, 2, 3, 4]), 0x9e37_79c4);
    }

    fn town(kind: i32, x: f32) -> Town {
        Town {
            id: String::new(),
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

    #[test]
    fn buildings_take_the_town_they_stand_in_else_the_nearest_with_an_interior() {
        // the Spider Factory (type 4, 875 units) and its Ancient Factory 1328 units off
        let ts = [town(4, 0.0), town(8, -3000.0), town(2, 30_000.0)];
        let none = |_: usize| false;
        assert_eq!(building_town(&ts, [500.0, 0.0, 0.0], false, false, none), Some(0));
        // outside every radius: none without an interior, else the nearest within 10000
        assert_eq!(building_town(&ts, [1328.0, 0.0, 0.0], false, false, none), None);
        assert_eq!(building_town(&ts, [1328.0, 0.0, 0.0], true, false, none), Some(0));
        assert_eq!(building_town(&ts, [1328.0, 0.0, 0.0], false, true, none), Some(0));
        // nest markers are taken by the nearest but never stood in
        assert_eq!(building_town(&ts, [-2900.0, 0.0, 0.0], false, false, none), None);
        assert_eq!(building_town(&ts, [-2900.0, 0.0, 0.0], true, false, none), Some(1));
        // over 10000 units from every town: the wilderness
        assert_eq!(building_town(&ts, [15_000.0, 0.0, 0.0], true, false, none), None);
        // the player's towns are left out
        assert_eq!(building_town(&ts, [500.0, 0.0, 0.0], true, false, |t| t == 0), Some(1));
    }
}
