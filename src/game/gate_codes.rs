//! The gate codes in the simulation (kenshi_world::gates): the doors gathered as gates
//! (F030340), the codes worked out whenever the navmesh changes (F030360, Kenshi's navmesh
//! thread) and handed out (F030361: each door its two sides', each building and character the
//! code where it stands), each door's opening, shutting and locking passed on to the pairs
//! (F030352), and the code a character stands in (F044467) and finds another in (F044464), which
//! the aggression flag's nav-area test reads (F038557).

use crate::sim::{DOOR_OPEN, Sim};
use kenshi_world::gates::{Gate, PAIR_LOCKED, PAIR_OPEN, PAIR_SHUT};
use kenshi_world::navmesh::METRES;

/// How far each door point is set from the door's centre along its navmesh axis (units,
/// F029383's 15.0 at 0x14168a258), and the least the inside and outside lie behind and in front
/// of it once dropped (10 units, when nearer than -5 and 5).
const DOOR_SIDE: f32 = 15.0;
const DOOR_SIDE_MIN: f32 = 10.0;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    a.map(|v| v * k)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

impl Sim {
    /// @fn F030363
    ///
    /// Door `d`'s gate state: 1 when broken (+0x3ec) or open (+0x380 1), 3 when its lock is on
    /// and of some level (+0x370: its +0x20 set, F029312 not broken, its level above 0), else 2.
    pub fn door_gate_state(&self, d: usize) -> u8 {
        let s = &self.door_states[d];
        if s.broken || s.phase == DOOR_OPEN {
            PAIR_OPEN
        } else if s.locked && s.level >= 1 {
            PAIR_LOCKED
        } else {
            PAIR_SHUT
        }
    }

    /// @fn F029383
    ///
    /// Door `d`'s inside (+0x3c8) and outside (+0x3bc) points (units): 15 units back and forth
    /// from its centre along its navmesh axis x up, each dropped to the floor under it (masks
    /// 0x200 / 0x204), then the inside set at least 10 units behind the centre and the outside 10
    /// in front of it along the axis x the building's up. (stand-in for Kenshi's raycast
    /// against the ground's and buildings' collision, which the headless simulation lacks: the
    /// navmesh's floors met by a ray straight down within 500 m, else the terrain.)
    fn door_sides(&mut self, d: usize) -> ([f32; 3], [f32; 3]) {
        let (c, axis, placement) = {
            let door = &self.door_states[d].door;
            (door.centre, door.navmesh_axis, door.placement)
        };
        let n = cross(axis, [0.0, 1.0, 0.0]);
        let inside = self.drop_to_floor(sub(c, scale(n, DOOR_SIDE)));
        let outside = self.drop_to_floor(add(c, scale(n, DOOR_SIDE)));
        let (mut inside, mut outside) = (inside, outside);
        // the axis x the building's up (its quaternion's y axis)
        let [w, x, y, z] = self.placed.get(placement).map_or([1.0, 0.0, 0.0, 0.0], |p| p.rotation);
        let up = [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)];
        let side = cross(axis, up);
        let (a, b) = (dot(sub(inside, c), side), dot(sub(outside, c), side));
        if -5.0 < a {
            inside = sub(inside, scale(side, a + DOOR_SIDE_MIN));
        }
        if b < 5.0 {
            outside = add(outside, scale(side, DOOR_SIDE_MIN - b));
        }
        (inside, outside)
    }

    /// A point (units) dropped straight down to the floor under it: the navmesh's within 500 m,
    /// else the terrain (part of `door_sides`' stand-in).
    fn drop_to_floor(&mut self, p: [f32; 3]) -> [f32; 3] {
        let at = p.map(|v| v * METRES);
        if let Some((q, _)) = self.nav.raycast(at, [0.0, -1.0, 0.0], 500.0, &|_| true) {
            return q.map(|v| v / METRES);
        }
        match self.terrain.height_at(p[0], p[2]) {
            Ok(g) if g < p[1] => [p[0], g, p[2]],
            _ => p,
        }
    }

    /// @fn F030340
    ///
    /// The gates: every door (not gone) with points half a door-depth past each side (metres),
    /// its two codes as they stand and its state.
    fn gather_gates(&mut self) -> Vec<Gate> {
        (0..self.door_states.len())
            .filter(|&d| !self.door_states[d].gone)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|d| {
                let (i, o) = self.door_sides(d);
                let inner = add(i, scale(sub(i, o), 0.5)).map(|v| v * METRES);
                let outer = add(o, scale(sub(o, i), 0.5)).map(|v| v * METRES);
                let (outside, inside) = self.door_codes.get(d).copied().unwrap_or((-1, 0xffff));
                Gate { door: d, inner, outer, inside, outside, state: self.door_gate_state(d) }
            })
            .collect()
    }

    /// The gate codes kept up: worked out afresh when the navmesh has changed since they last
    /// were (F032235 runs the gates' pass after its sections change, F030360, and the main thread
    /// hands the codes out, F030361), else each door whose state changed passed on (F030352).
    pub fn gate_codes_tick(&mut self) {
        if self.door_codes.len() < self.door_states.len() {
            self.door_codes.resize(self.door_states.len(), (-1, 0xffff));
            self.gate_states.resize(self.door_states.len(), 0);
        }
        if self.nav.generation() != self.gate_generation {
            self.update_gate_codes();
            return;
        }
        for d in 0..self.door_states.len() {
            let now = self.door_gate_state(d);
            if self.gate_states[d] == now {
                continue;
            }
            self.gate_states[d] = now;
            let (outside, inside) = self.door_codes[d];
            let codes = &self.door_codes;
            let states = &self.gate_states;
            self.gate_codes.door_changed(outside, inside, now, &|g| codes.get(g).map(|&(o, i)| (o, i, states[g])));
        }
    }

    /// @fn F030361
    ///
    /// The gates' pass run (F030360, the agent width the race "17-gamedata.quack"'s "pathfind
    /// footprint radius" x 0.2 the first time) and its codes handed out: each door its inside's
    /// (+0x378) and its building's outside's; every building the code where it stands (F032176,
    /// none kept as it was); every character likewise (F030357).
    pub fn update_gate_codes(&mut self) {
        if self.gate_codes.width == 0.0 {
            let r = self.data.get("17-gamedata.quack").and_then(|r| r.floats.get("pathfind footprint radius").copied()).unwrap_or(0.0);
            self.gate_codes.width = r * 0.2;
        }
        let started = std::time::Instant::now();
        let mut gates = self.gather_gates();
        let debug = std::env::var("KENSHI_GATE_DEBUG").is_ok();
        let mut log = |s: String| {
            if debug {
                eprintln!("{s}");
            }
        };
        let mut codes = std::mem::take(&mut self.gate_codes);
        codes.update(&mut self.nav, &mut gates, &mut log);
        self.gate_codes = codes;
        for g in &gates {
            self.door_codes[g.door] = (g.outside, g.inside);
            self.gate_states[g.door] = g.state;
        }
        if self.building_codes.len() < self.placed_at.len() {
            self.building_codes.resize(self.placed_at.len(), -1);
        }
        for b in 0..self.placed_at.len() {
            let c = self.nav.code_at(self.placed_at[b].map(|v| v * METRES));
            if c != -1 {
                self.building_codes[b] = c;
            }
        }
        for i in 0..self.folk.len() {
            let c = self.nav.code_at(self.folk[i].mover.position);
            self.folk[i].area = c;
        }
        self.gate_generation = self.nav.generation();
        if debug {
            for g in &gates {
                eprintln!("gate door {} inside {} outside {} state {}", g.door, g.inside, g.outside, g.state);
            }
            eprintln!("Gates - {} gates, {:.0} ms", gates.len(), started.elapsed().as_secs_f64() * 1000.0);
        }
    }

    /// The building whose own navmesh face `id` stands on (its section's placement).
    /// (stand-in for CharMovement's ground probe, F040642: the building its raycast down meets,
    /// +0x338; the headless simulation has no collision to cast against, so the floor under
    /// the mover's face stands for it.)
    fn ground_building(&self, id: usize) -> Option<usize> {
        let f = self.folk[id].pathing.face?;
        if !self.nav.on_building(f) {
            return None;
        }
        let uid = if self.nav.sections.contains_key(&f.section) { f.section } else { f.section.wrapping_sub(kenshi_world::navcut::CUT_UID) };
        let info = self.nav.sections.get(&uid)?.info.as_ref()?;
        let at: Vec<f32> = info.split(';').skip(1).filter_map(|v| v.parse().ok()).collect();
        if at.len() != 3 {
            return None;
        }
        (0..self.placed_at.len()).find(|&b| {
            let p = self.placed_at[b];
            (p[0] - at[0]).abs() < 1.0 && (p[1] - at[1]).abs() < 1.0 && (p[2] - at[2]).abs() < 1.0
        })
    }

    /// @fn F044467
    ///
    /// The gate code character `id` stands in (Character vtable 0x118): the building it is inside
    /// (F039242), else the one it walks on, gives its code (Building F038479: its +0xa0) and
    /// that is kept (+0xa0); else the code it was last given.
    pub fn area(&self, id: usize) -> i32 {
        let p = &self.folk[id];
        match p.inside.or_else(|| self.ground_building(id)) {
            Some(b) => self.building_codes.get(b).copied().unwrap_or(p.area),
            None => p.area,
        }
    }

    /// The codes characters stand in kept (+0xa0), as F044467 keeps them when asked.
    pub fn keep_areas(&mut self) {
        for i in 0..self.folk.len() {
            let a = self.area(i);
            self.folk[i].area = a;
        }
    }

    /// @fn F044464
    ///
    /// The gate code character `me` finds character `other` in (another character's own,
    /// F044467).
    pub fn area_toward(&self, _me: usize, other: usize) -> i32 {
        self.area(other)
    }

    /// @fn F038529
    ///
    /// The use test for character `other` as `me`'s target: passed when their gate codes are
    /// the same or either is missing, or the pair is openly joined (F030342 answering 1).
    /// (Building targets, which also pass through a door of theirs openly joined to mine or a
    /// parent building in my code, are the AI's finders' to ask.)
    pub fn can_use(&self, me: usize, other: usize) -> bool {
        let (a, b) = (self.area(me), self.area_toward(me, other));
        a == b || a == -1 || b == -1 || self.gate_codes.state(a, b) == PAIR_OPEN as i32
    }

    /// @fn F038557
    ///
    /// Whether `other` stands in a region cut off from `me`'s: both codes valid and different,
    /// and the pair not openly joined (F030342 not 1).
    pub fn cut_off(&self, me: usize, other: usize) -> bool {
        let (a, b) = (self.area(me), self.area_toward(me, other));
        a != b && a != -1 && b != -1 && self.gate_codes.state(a, b) != PAIR_OPEN as i32
    }
}
