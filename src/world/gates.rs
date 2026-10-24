//! Gate codes (Kenshi's GateCodes, the navmesh manager's "Gates" pass, F030339 .. F030363): the
//! navmesh cut into regions by its doorways (faces of data 4, an open door's, and 5, a shut
//! one's, which the doors flip, F032197), each region behind a door numbered with a code kept in
//! the upper bits of its faces' data (data >> 4), the ground outside 0; and, for each pair of
//! codes, how their doors join them: 1 open, 2 shut, 3 locked, 0 not at all.
//!
//! The pass (F030360, on the navmesh's thread whenever its sections change) takes the gates the
//! main thread gathered (F030340: every door, a point half a door-depth past each side), finds
//! which doors part their two sides ("Enabled test": no way from one side to the other but
//! through a doorway, F030349), gives each such door's inside a code, the same for insides that
//! reach one another ("Interior region test"), floods each inside region with its code
//! (F030354), takes each outside's code from where it stands (F032176) or numbers the outsides
//! anew ("Update codes", the first of them the ground's 0), floods those ("External code fix",
//! one region that cannot be flooded swapped with 0), and builds the pairs (F030358 / F030346).
//! The main thread then gives each door, building and character its code (F030361).
//!
//! The searches and floods are Havok's (hkaiPathfindingUtil's A* and its flood fill) with the
//! GateFilter (F030396: faces of data 4 and 5 shut) and the agent width Kenshi reads from the
//! race "17-gamedata.quack" ("pathfind footprint radius" x 0.2 m): here the navmesh's own search
//! (`NavWorld::search_with`) and a flood over the faces crossed the same way. (not ported) The
//! buffer sizes Havok gives a search or a flood that sets none (Kenshi logs a search cut short
//! as "Iteration limit", "Open set full", "Search state full"): Havok's defaults are not in its
//! documentation; the search here stops at its own iteration cap, a flood not at all.

use crate::navmesh::{FaceRef, NavWorld, Pass};
use std::collections::{BTreeMap, HashSet};

/// A doorway's face data, open (an open door's: costly, passable) and shut.
pub const DATA_DOOR_OPEN: i32 = 4;
pub const DATA_DOOR_SHUT: i32 = 5;

/// How a pair of codes is joined (GateCodes::State).
pub const PAIR_NONE: u8 = 0;
pub const PAIR_OPEN: u8 = 1;
pub const PAIR_SHUT: u8 = 2;
pub const PAIR_LOCKED: u8 = 3;

/// GateFilter::isEnabled (F030396's vtable slot 7, 0x1402f4730): a doorway, open or shut, is
/// passed by no gate search.
pub fn gate_enabled(data: i32) -> bool {
    data != DATA_DOOR_OPEN && data != DATA_DOOR_SHUT
}

/// One gate (GateCodes::GateData, 0x48 bytes, F030340).
#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    /// The caller's door.
    pub door: usize,
    /// Half a door-depth past its inside point (door +0x3c8 + (+0x3c8 - +0x3bc) x 0.5) and past
    /// its outside point (+0x2c), metres.
    pub inner: [f32; 3],
    pub outer: [f32; 3],
    /// The codes of its two sides (+0x38 the door's +0x378, +0x3c the building's).
    pub inside: i32,
    pub outside: i32,
    /// Its door's state (F030363): 1 open or broken, 2 shut, 3 locked.
    pub state: u8,
}

/// A pair's entry (+0x1c state, +0x1d steps, +0x1e joined by a door of its own).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Pair {
    state: u8,
    steps: u8,
    direct: bool,
}

/// The pairs' key: the larger code << 8 | the smaller (F030342).
fn key(a: i32, b: i32) -> i32 {
    if a < b { b << 8 | a } else { a << 8 | b }
}

/// The gate codes (GateCodes, the singleton at 0x14212e368).
#[derive(Debug, Clone, Default)]
pub struct GateCodes {
    /// +0x08: the pairs by key.
    pairs: BTreeMap<i32, Pair>,
    /// +0x28 / +0x30: each code's gates (doors).
    by_code: BTreeMap<i32, Vec<usize>>,
    /// +0x90: the searches' agent width (m).
    pub width: f32,
}

impl NavWorld {
    /// @fn F032176
    ///
    /// NavMeshManager's gate code at `p` (m): the face a vertical ray finds within 500 m, else
    /// the nearest point within 4 m of a ground section's face not shut (the InteriorFilter,
    /// 0x1416ca358); none: -1. A ground face's code: its data >> 4, 0 for none (-1), -1 for a
    /// doorway; a building's: its code, -1 for 0 or none.
    pub fn code_at(&self, p: [f32; 3]) -> i32 {
        let face = self.vertical_face(p, 500.0).map(|(f, _)| f).or_else(|| self.closest_interior(p));
        let Some(f) = face else { return -1 };
        let data = self.face(f).kind;
        if f.section <= 0xffff {
            if data == -1 {
                0
            } else if !gate_enabled(data) {
                -1
            } else {
                data >> 4
            }
        } else if data == -1 || data & !0xf > 0 {
            data >> 4
        } else {
            -1
        }
    }

    /// The query mediator's getClosestPoint within the box of half extent 4 m round `p` with the
    /// InteriorFilter (0x1416ca358): ground sections (uid up to 0xffff) only, shut faces (data 5)
    /// left out.
    fn closest_interior(&self, p: [f32; 3]) -> Option<FaceRef> {
        self.closest_in_box(p, 4.0, &|f| f.section <= 0xffff && self.face(f).kind != DATA_DOOR_SHUT).map(|(f, _)| f)
    }

    /// @fn F030350
    ///
    /// The codes taken off every face (data over 0xf back to its low four bits).
    pub fn clear_codes(&mut self) {
        for s in self.sections.values_mut() {
            for f in &mut s.faces {
                if f.kind > 0xf {
                    f.kind &= 0xf;
                }
            }
        }
    }

    /// @fn F030349
    ///
    /// Whether a gate search finds a way from `a` to `b` (m): each placed on the face a vertical
    /// ray finds within 500 m (F030335 / F032169; none: no way), then Havok's A* with the
    /// GateFilter for an agent of `radius`.
    pub fn gate_reach(&self, a: [f32; 3], b: [f32; 3], radius: f32) -> bool {
        let (Some((fa, _)), Some((fb, _))) = (self.vertical_face(a, 500.0), self.vertical_face(b, 500.0)) else {
            return false;
        };
        self.search_with(fa, a, fb, b, radius, 1.0, Pass::ANYONE, true, gate_enabled).is_some()
    }

    /// @fn F030339
    ///
    /// Whether face `f` lies on the loaded world's edge: a ground section's face on the section's
    /// streaming border (one of its edges has a streaming link, the instance's face flag 4) with
    /// none of its edges leading into a loaded section, and a boundary edge at least 2 m long
    /// lying along the tile's side (both ends within 0.001 m of x or z 0, or past 460.79 m, in
    /// the tile's own frame). GateFillCallback (F0302fa30) leaves such a face out of a flood.
    fn world_edge(&self, f: FaceRef) -> bool {
        if f.section > 0xffff {
            return false;
        }
        let Some(s) = self.sections.get(&f.section) else { return false };
        let edges = self.face_edges(f);
        if !edges.clone().any(|e| s.links.contains_key(&e)) {
            return false;
        }
        if edges.clone().any(|e| s.links.get(&e).is_some_and(|l| l.iter().any(|&(o, _, _)| self.sections.contains_key(&o)))) {
            return false;
        }
        let (tx, ty) = s.tile;
        let half = crate::WORLD_SIZE * crate::navmesh::METRES / 2.0;
        let size = crate::ZONE_SIZE * crate::navmesh::METRES;
        let origin = [tx as f32 * size - half, ty as f32 * size - half];
        edges.into_iter().any(|e| {
            if s.edges[e as usize].opposite_face >= 0 {
                return false;
            }
            let (a, b) = self.edge_ends(f, e);
            let l2 = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
            if l2 < 4.0 {
                return false;
            }
            let (ax, az, bx, bz) = (a[0] - origin[0], a[2] - origin[1], b[0] - origin[0], b[2] - origin[1]);
            ((ax as f64) < 0.001 && (bx as f64) < 0.001)
                || ((az as f64) < 0.001 && (bz as f64) < 0.001)
                || (ax > 460.79 && bx > 460.79)
                || (az > 460.79 && bz > 460.79)
        })
    }

    /// The faces Havok's flood fill reaches from `seed` (crossing as a gate search crosses: the
    /// GateFilter and the agent's `radius`), the GateFillCallback taking every face but those on
    /// the loaded world's edge (F030339), which it does not go past.
    fn gate_flood(&self, seed: FaceRef, radius: f32) -> Vec<FaceRef> {
        let mut out = Vec::new();
        let mut seen: HashSet<FaceRef> = HashSet::from([seed]);
        let mut open = vec![seed];
        while let Some(f) = open.pop() {
            if self.world_edge(f) {
                continue;
            }
            out.push(f);
            for e in self.face_edges(f) {
                for (g, (a, b)) in self.across(f, e) {
                    if !gate_enabled(self.face(g).kind) || self.clearance(f, e, g, a, b, radius).most < radius {
                        continue;
                    }
                    if seen.insert(g) {
                        open.push(g);
                    }
                }
            }
        }
        out
    }

    /// F030354's writing of a code: every flooded face but a doorway takes `code` in its upper
    /// bits, its own low four kept (none: 0).
    fn write_code(&mut self, faces: &[FaceRef], code: i32) {
        for &f in faces {
            let Some(face) = self.sections.get_mut(&f.section).and_then(|s| s.faces.get_mut(f.face as usize)) else { continue };
            if !gate_enabled(face.kind) {
                continue;
            }
            let low = if face.kind < 0 { 0 } else { face.kind & 0xf };
            face.kind = code << 4 | low;
        }
    }
}

impl GateCodes {
    /// @fn F030342
    ///
    /// How codes `a` and `b` are joined: 0 for the same code, -1 for a missing one (below 0),
    /// else the pair's state (none kept: 0).
    pub fn state(&self, a: i32, b: i32) -> i32 {
        if a == b {
            return 0;
        }
        if a < 0 || b < 0 {
            return -1;
        }
        self.pairs.get(&key(a, b)).map_or(0, |p| p.state as i32)
    }

    /// @fn F030343
    ///
    /// The number of doors between codes `a` and `b` along the way the state was found (0 for
    /// the same code).
    pub fn steps(&self, a: i32, b: i32) -> u8 {
        if a == b {
            return 0;
        }
        self.pairs.get(&key(a, b)).map_or(0, |p| p.steps)
    }

    /// F04c799: whether codes `a` and `b` have a door of their own between them (+0x1e).
    pub fn direct(&self, a: i32, b: i32) -> bool {
        self.pairs.get(&key(a, b)).is_some_and(|p| p.direct)
    }

    /// The gates (doors) of code `c` (+0x30).
    pub fn gates_of(&self, c: i32) -> &[usize] {
        self.by_code.get(&c).map_or(&[], |v| v.as_slice())
    }

    /// @fn F030354
    ///
    /// Each code of the gates' insides (`mode` bit 1; 0xffff left out) and outsides (bit 2), the
    /// first gate's point of each seeding it, flooded with it in code order when above 0 and its
    /// seed's face (the vertical ray's within 500 m) is not coded already. A seed with no face is
    /// logged; a flood Havok cuts short is a failure, its code in `fails`.
    pub fn flood(&self, nav: &mut NavWorld, gates: &[Gate], mode: u32, fails: &mut Vec<i32>, log: &mut dyn FnMut(String)) {
        if gates.is_empty() || mode == 0 {
            return;
        }
        let mut seeds: BTreeMap<i32, [f32; 3]> = BTreeMap::new();
        if mode & 1 != 0 {
            for g in gates {
                if g.inside != 0xffff {
                    seeds.entry(g.inside).or_insert(g.inner);
                }
            }
        }
        if mode & 2 != 0 {
            for g in gates {
                seeds.entry(g.outside).or_insert(g.outer);
            }
        }
        let radius = self.width * 0.5;
        for (&code, &p) in &seeds {
            if code <= 0 {
                continue;
            }
            let Some((seed, _)) = nav.vertical_face(p, 500.0) else {
                log("Gates - Gatecode fill start face not found".to_string());
                continue;
            };
            if nav.face(seed).kind >= 0x10 {
                continue;
            }
            let faces = nav.gate_flood(seed, radius);
            nav.write_code(&faces, code);
            log(format!("Gates - Filled code {code}: {} faces", faces.len()));
        }
        let _ = fails;
    }

    /// @fn F030360
    ///
    /// The gate codes worked out afresh for `gates` (their inside and outside codes set), the
    /// faces coded and the pairs built. Kenshi's log lines go to `log`.
    pub fn update(&mut self, nav: &mut NavWorld, gates: &mut [Gate], log: &mut dyn FnMut(String)) {
        if gates.is_empty() {
            return;
        }
        let radius = self.width * 0.5;
        let count = gates.len() as i32;
        // Enabled test: a door whose sides meet other than through a doorway parts nothing
        let mut n = 0;
        let mut codes: Vec<i32> = gates
            .iter()
            .map(|g| {
                if nav.gate_reach(g.inner, g.outer, radius) {
                    -1
                } else {
                    n += 1;
                    n
                }
            })
            .collect();
        // Interior region test: insides that reach one another share the first one's code
        for i in 0..gates.len() {
            if codes[i] <= 0 {
                continue;
            }
            for j in i + 1..gates.len() {
                if codes[j] > 0 && nav.gate_reach(gates[i].inner, gates[j].inner, radius) {
                    codes[j] = codes[i];
                }
            }
        }
        for (g, &c) in gates.iter_mut().zip(&codes) {
            g.inside = c;
        }
        nav.clear_codes();
        let mut fails = Vec::new();
        self.flood(nav, gates, 1, &mut fails, log);
        // the outsides: the code where they stand
        for g in gates.iter_mut() {
            let c = nav.code_at(g.outer).max(0);
            g.outside = c;
            if g.inside == -1 {
                g.inside = c;
            }
        }
        // Update codes: the outsides on the ground numbered by region, the first region 0
        let first = count + 1;
        let mut next = first;
        for i in 0..gates.len() {
            if gates[i].outside != 0 {
                continue;
            }
            gates[i].outside = next;
            for j in i + 1..gates.len() {
                if gates[j].outside == 0 && nav.gate_reach(gates[i].outer, gates[j].outer, radius) {
                    gates[j].outside = next;
                }
            }
            next += 1;
        }
        for g in gates.iter_mut() {
            if g.inside == first {
                g.inside = 0;
            }
            if g.outside == first {
                g.outside = 0;
            }
        }
        if count + 2 < next {
            log("Gates - Info: Multiple gatecode 0 regions".to_string());
        }
        for g in gates.iter_mut() {
            if g.inside == 0xffff {
                g.inside = g.outside;
            }
        }
        // External code fix
        let mut fails = Vec::new();
        self.flood(nav, gates, 2, &mut fails, log);
        if fails.len() == 1 {
            log("Gates - Single gatecode fail - Attempting to swap with code 0".to_string());
            for g in gates.iter_mut() {
                if g.inside == 0 {
                    g.inside = next;
                }
                if g.outside == 0 {
                    g.outside = next;
                }
            }
            let bad = fails[0];
            for g in gates.iter_mut() {
                if g.inside == bad {
                    g.inside = 0;
                }
                if g.outside == bad {
                    g.outside = 0;
                }
            }
            fails.clear();
            if let Some(g) = gates.iter().find(|g| g.inside == next || g.outside == next).cloned() {
                let mode = if g.inside != next { 2 } else { 1 };
                self.flood(nav, std::slice::from_ref(&g), mode, &mut fails, log);
            }
        }
        if !fails.is_empty() {
            log(format!("Gates - Code fill failed for {} codes", fails.len()));
            for &bad in &fails {
                for g in gates.iter_mut() {
                    if g.inside == bad {
                        g.inside = 0;
                    }
                    if g.outside == bad {
                        g.outside = 0;
                    }
                }
            }
        }
        self.build(gates);
    }

    /// @fn F030358
    ///
    /// The pairs built from the gates: each gate's two codes joined by its door's state (the
    /// least shut of their doors), one step, a door of their own; every other pair of their codes
    /// not joined; each code's gates listed; then the ways through other codes (F030346).
    pub fn build(&mut self, gates: &[Gate]) {
        self.pairs.clear();
        let mut codes: Vec<i32> = Vec::new();
        for g in gates {
            let k = key(g.inside, g.outside);
            match self.pairs.get_mut(&k) {
                None => {
                    self.pairs.insert(k, Pair { state: g.state, steps: 1, direct: true });
                }
                Some(p) => {
                    if g.state < p.state {
                        p.state = g.state;
                    }
                }
            }
            for c in [g.outside, g.inside] {
                if !codes.contains(&c) {
                    codes.push(c);
                }
            }
        }
        for i in 0..codes.len() {
            for j in i + 1..codes.len() {
                self.pairs.entry(key(codes[i], codes[j])).or_default();
            }
        }
        self.by_code.clear();
        for g in gates {
            self.by_code.entry(g.outside).or_default().push(g.door);
            self.by_code.entry(g.inside).or_default().push(g.door);
        }
        self.close();
    }

    /// @fn F030346
    ///
    /// The ways through other codes: every pair joined by more than one step forgotten (state 0),
    /// all of them one step; then, until nothing changes, each pair not openly joined takes, over
    /// every third code, the more shut of its two halves when both are joined, if that is less
    /// shut than it was (or as shut in fewer steps), or it was not joined.
    fn close(&mut self) {
        for p in self.pairs.values_mut() {
            if p.steps > 1 {
                p.state = PAIR_NONE;
            }
            p.steps = 1;
        }
        let codes: Vec<i32> = self.by_code.keys().copied().collect();
        loop {
            let mut changed = false;
            let keys: Vec<i32> = self.pairs.keys().copied().collect();
            for k in keys {
                if self.pairs[&k].state == PAIR_OPEN {
                    continue;
                }
                let (lo, hi) = (k & 0xff, k >> 8);
                for &c in &codes {
                    if c == lo || c == hi {
                        continue;
                    }
                    let one = self.pairs.get(&key(lo, c)).copied().unwrap_or_default();
                    let two = self.pairs.get(&key(hi, c)).copied().unwrap_or_default();
                    if one.state == 0 || two.state == 0 {
                        continue;
                    }
                    let state = one.state.max(two.state);
                    let steps = one.steps.wrapping_add(two.steps);
                    let p = self.pairs.get_mut(&k).unwrap();
                    if p.state == 0 || state < p.state || (state == p.state && steps < p.steps) {
                        p.state = state;
                        p.steps = steps;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// @fn F030352
    ///
    /// A door's state changed (`now`: 1 open or broken, 2 shut, 3 locked) between codes `outside`
    /// (its building's) and `inside` (the door's +0x378; -1 none): growing more shut, the pair
    /// takes the least shut of the doors between them (F030345, `state_of` each gate of
    /// `outside`'s that joins `inside`); less shut, the door's own; then the ways through other
    /// codes again.
    pub fn door_changed(&mut self, outside: i32, inside: i32, now: u8, joins: &dyn Fn(usize) -> Option<(i32, i32, u8)>) {
        if outside == inside || inside == -1 {
            return;
        }
        let k = key(outside, inside);
        let p = self.pairs.entry(k).or_default();
        if p.state < now {
            // F030345: the least shut door among `outside`'s gates whose building or door
            // stands on `inside`
            let mut best = 0u8;
            for &g in self.by_code.get(&outside).map_or(&[][..], |v| v.as_slice()) {
                if let Some((b, d, s)) = joins(g)
                    && (b == inside || d == inside)
                    && (best == 0 || s < best)
                {
                    best = s;
                }
            }
            self.pairs.get_mut(&k).unwrap().state = best;
        } else {
            p.state = now;
        }
        self.close();
    }

    /// @fn F030347
    ///
    /// The gate to make for, from `at`, to go from code `from` to code `to` (not openly
    /// joined): with a door of their own, the nearest of `from`'s gates not broken whose door is
    /// not open and that joins `to`; else, searching outward from `to` over the codes in order,
    /// the gate on the way from the first code openly joined to `from`, or one with a door of
    /// its own to `from`. `at` and the gates' positions in units: none past 10000 units
    /// (1e8 squared). `gate` answers a gate's (position, building code, door code, gone,
    /// broken or open).
    pub fn route(&self, at: [f32; 3], from: i32, to: i32, gate: &dyn Fn(usize) -> Option<([f32; 3], i32, i32, bool)>) -> Option<usize> {
        if self.state(from, to) == PAIR_OPEN as i32 {
            return None;
        }
        if self.direct(from, to) {
            let mut best: Option<(f32, usize)> = None;
            let mut limit = 1e8f32;
            for &g in self.gates_of(from) {
                let Some((p, b, d, passable)) = gate(g) else { continue };
                if passable || (b != to && d != to) {
                    continue;
                }
                let d2 = (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2);
                if d2 < limit {
                    limit = d2;
                    best = Some((d2, g));
                }
            }
            return best.map(|(_, g)| g);
        }
        let mut list = vec![to];
        let mut start = 0;
        while start < list.len() {
            let end = list.len();
            for &c in self.by_code.keys() {
                if list.contains(&c) {
                    continue;
                }
                for k in start..end {
                    let here = list[k];
                    if !self.direct(here, c) {
                        continue;
                    }
                    if self.state(from, c) == PAIR_OPEN as i32 {
                        return self.route(at, c, here, gate);
                    }
                    if self.direct(from, c) {
                        return self.route(at, from, c, gate);
                    }
                    list.push(c);
                    break;
                }
            }
            start = end;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(door: usize, inside: i32, outside: i32, state: u8) -> Gate {
        Gate { door, inner: [0.0; 3], outer: [0.0; 3], inside, outside, state }
    }

    #[test]
    fn pairs_and_ways_through() {
        // the ground 0, a house 1 behind a shut door, its back room 2 behind an open one, a
        // locked vault 3 off the back room
        let mut c = GateCodes::default();
        c.build(&[gate(0, 1, 0, PAIR_SHUT), gate(1, 2, 1, PAIR_OPEN), gate(2, 3, 2, PAIR_LOCKED)]);
        assert_eq!(c.state(1, 0), 2);
        assert_eq!(c.state(2, 1), 1);
        // through the house: as shut as its front door, two steps
        assert_eq!(c.state(2, 0), 2);
        assert_eq!(c.steps(2, 0), 2);
        // the vault from the ground: locked
        assert_eq!(c.state(3, 0), 3);
        assert_eq!(c.state(0, 0), 0);
        assert_eq!(c.state(-1, 0), -1);
        // a second front door, open: the least shut wins
        c.build(&[gate(0, 1, 0, PAIR_SHUT), gate(3, 1, 0, PAIR_OPEN), gate(1, 2, 1, PAIR_OPEN)]);
        assert_eq!(c.state(2, 0), 1);
        // the door opens: the pair takes it
        let mut c = GateCodes::default();
        c.build(&[gate(0, 1, 0, PAIR_SHUT)]);
        c.door_changed(0, 1, PAIR_OPEN, &|_| None);
        assert_eq!(c.state(0, 1), 1);
        // and locks: the least shut of the doors between (only this one)
        c.door_changed(0, 1, PAIR_LOCKED, &|g| (g == 0).then_some((0, 1, PAIR_LOCKED)));
        assert_eq!(c.state(0, 1), 3);
    }

    #[test]
    fn the_way_to_a_door() {
        let mut c = GateCodes::default();
        c.build(&[gate(0, 1, 0, PAIR_SHUT), gate(1, 2, 1, PAIR_SHUT)]);
        let at = |g: usize| Some(([g as f32 * 10.0, 0.0, 0.0], [0, 1][g], [1, 2][g], false));
        // from the ground to the back room: the house's front door first
        assert_eq!(c.route([0.0; 3], 0, 2, &at), Some(0));
        // from the house to the back room: its own door
        assert_eq!(c.route([0.0; 3], 1, 2, &at), Some(1));
    }
}
