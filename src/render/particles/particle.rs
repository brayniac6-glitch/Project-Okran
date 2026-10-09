//! ParticleUniverse's particles and their pool (ParticleUniverseParticle, VisualParticle,
//! ParticlePool, Pool / PoolMap): every emitted thing is a Particle; a technique's pool keeps
//! them in "locked" (free) and "released" (live) lists, released in emission order (emitters by
//! name), iterated visuals first, then emitters, then techniques, with PU's lockLatestElement
//! (an expired first element makes the walk skip the one after it, as in PU).

use super::math::Quat;
use bevy::math::{Vec3, Vec4};

pub const PEF_EXPIRED: u32 = 1 << 0;
pub const PEF_EMITTED: u32 = 1 << 1;

pub const DEFAULT_TTL: f32 = 10.0;
pub const DEFAULT_MASS: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PType {
    #[default]
    Visual,
    Technique,
    Emitter,
    Affector,
    System,
}

/// An emitter of the technique a particle lives in: one of its own or a pooled (emitted) one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitterId {
    Own(usize),
    Pooled(usize),
}

/// A particle in a technique's pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PRef {
    Visual(u32),
    Emitter(u32),
    Technique(u32),
}

/// Particle and VisualParticle's attributes.
#[derive(Debug, Clone)]
pub struct Particle {
    pub ptype: PType,
    pub marked_for_emission: bool,
    pub position: Vec3,
    pub derived_position: Vec3,
    pub direction: Vec3,
    pub time_to_live: f32,
    pub total_time_to_live: f32,
    pub time_fraction: f32,
    pub mass: f32,
    pub flags: u32,
    pub enabled: bool,
    pub freezed: bool,
    pub original_enabled: bool,
    pub original_enabled_set: bool,
    pub original_position: Vec3,
    pub latest_position: Vec3,
    pub original_direction: Vec3,
    pub original_direction_length: f32,
    pub original_scaled_direction_length: f32,
    pub original_velocity: f32,
    pub parent_emitter: Option<EmitterId>,
    // VisualParticle
    pub colour: Vec4,
    pub original_colour: Vec4,
    pub z_rotation: f32,
    pub z_rotation_speed: f32,
    pub orientation: Quat,
    pub original_orientation: Quat,
    pub rotation_speed: f32,
    pub rotation_axis: Vec3,
    pub own_dimensions: bool,
    pub width: f32,
    pub height: f32,
    pub depth: f32,
    pub radius: f32,
    pub texture_coords_current: u16,
    /// SlaveBehaviour's master particle (in the master technique).
    pub slave_master: Option<(usize, PRef)>,
}

impl Particle {
    pub fn new(ptype: PType) -> Particle {
        Particle {
            ptype,
            marked_for_emission: ptype == PType::Visual,
            position: Vec3::ZERO,
            derived_position: Vec3::ZERO,
            direction: Vec3::ZERO,
            time_to_live: DEFAULT_TTL,
            total_time_to_live: DEFAULT_TTL,
            time_fraction: 0.0,
            mass: DEFAULT_MASS,
            flags: 0,
            enabled: true,
            freezed: false,
            original_enabled: true,
            original_enabled_set: false,
            original_position: Vec3::ZERO,
            latest_position: Vec3::ZERO,
            original_direction: Vec3::ZERO,
            original_direction_length: 0.0,
            original_scaled_direction_length: 0.0,
            original_velocity: 0.0,
            parent_emitter: None,
            colour: Vec4::ONE,
            original_colour: Vec4::ONE,
            z_rotation: 0.0,
            z_rotation_speed: 0.0,
            orientation: Quat::IDENTITY,
            original_orientation: Quat::IDENTITY,
            rotation_speed: 0.0,
            rotation_axis: Vec3::Z,
            own_dimensions: false,
            width: 1.0,
            height: 1.0,
            depth: 1.0,
            radius: 0.87,
            texture_coords_current: 0,
            slave_master: None,
        }
    }

    pub fn has(&self, f: u32) -> bool {
        self.flags & f != 0
    }

    /// Particle::setEnabled: the first call also fixes the original state.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !self.original_enabled_set {
            self.original_enabled = enabled;
            self.original_enabled_set = true;
        }
    }

    /// Particle::_initForEmission (VisualParticle's resets the texture animation, unused).
    pub fn init_for_emission(&mut self) {
        self.flags = PEF_EMITTED;
        self.time_fraction = 0.0;
        self.freezed = false;
    }

    /// Particle::_process: the time fraction, then the behaviours (SlaveBehaviour, applied by
    /// the technique).
    pub fn process_time_fraction(&mut self) {
        self.time_fraction = (self.total_time_to_live - self.time_to_live) / self.total_time_to_live;
    }

    pub fn calculate_velocity(&self) -> f32 {
        if self.original_scaled_direction_length != 0.0 {
            self.original_velocity * (self.direction.length() / self.original_scaled_direction_length)
        } else {
            self.original_velocity * self.direction.length()
        }
    }

    /// VisualParticle::setOwnDimensions: zero keeps the current value.
    pub fn set_own_dimensions(&mut self, w: f32, h: f32, d: f32) {
        self.own_dimensions = true;
        if w != 0.0 {
            self.width = w;
        }
        if h != 0.0 {
            self.height = h;
        }
        if d != 0.0 {
            self.depth = d;
        }
        self.calculate_bounding_sphere_radius();
    }

    pub fn calculate_bounding_sphere_radius(&mut self) {
        self.radius = 0.5 * self.depth.max(self.width.max(self.height));
    }
}

const NIL: u32 = u32::MAX;

/// Two std::lists (released and locked) over a fixed arena of indices, with PU's iterator.
#[derive(Debug, Clone, Default)]
pub struct SubPool {
    next: Vec<u32>,
    prev: Vec<u32>,
    released: bool_list::Ends,
    locked: bool_list::Ends,
    pub released_len: usize,
    cursor: u32,
    /// PoolMap's keys (emitters, techniques): released and locked kept ordered by key.
    keys: Option<Vec<String>>,
}

mod bool_list {
    #[derive(Debug, Clone, Copy)]
    pub struct Ends {
        pub head: u32,
        pub tail: u32,
    }
    impl Default for Ends {
        fn default() -> Self {
            Ends { head: super::NIL, tail: super::NIL }
        }
    }
}

impl SubPool {
    pub fn keyed() -> SubPool {
        SubPool { keys: Some(Vec::new()), cursor: NIL, ..Default::default() }
    }

    pub fn plain() -> SubPool {
        SubPool { cursor: NIL, ..Default::default() }
    }

    pub fn len_all(&self) -> usize {
        self.next.len()
    }

    /// addElement: a new (locked) element; returns its index.
    pub fn add(&mut self, key: Option<String>) -> u32 {
        let i = self.next.len() as u32;
        self.next.push(NIL);
        self.prev.push(NIL);
        if let Some(keys) = &mut self.keys {
            keys.push(key.unwrap_or_default());
        }
        self.insert_locked(i);
        i
    }

    fn unlink(&mut self, released: bool, i: u32) {
        let (p, n) = (self.prev[i as usize], self.next[i as usize]);
        let ends = if released { &mut self.released } else { &mut self.locked };
        if p == NIL {
            ends.head = n;
        } else {
            self.next[p as usize] = n;
        }
        if n == NIL {
            ends.tail = p;
        } else {
            self.prev[n as usize] = p;
        }
        self.prev[i as usize] = NIL;
        self.next[i as usize] = NIL;
    }

    /// Link `i` after `after` (NIL: at the head).
    fn link_after(&mut self, released: bool, i: u32, after: u32) {
        let ends = if released { &mut self.released } else { &mut self.locked };
        let n = if after == NIL { ends.head } else { self.next[after as usize] };
        self.prev[i as usize] = after;
        self.next[i as usize] = n;
        if after == NIL {
            ends.head = i;
        } else {
            self.next[after as usize] = i;
        }
        if n == NIL {
            ends.tail = i;
        } else {
            self.prev[n as usize] = i;
        }
    }

    /// Where `i` goes: the end (Pool), or after the last element with a key not above its own
    /// (PoolMap, std::multimap insertion).
    fn insert_pos(&self, released: bool, i: u32) -> u32 {
        let ends = if released { self.released } else { self.locked };
        let Some(keys) = &self.keys else { return ends.tail };
        let k = &keys[i as usize];
        let mut at = ends.tail;
        while at != NIL && keys[at as usize] > *k {
            at = self.prev[at as usize];
        }
        at
    }

    fn insert_locked(&mut self, i: u32) {
        let at = self.insert_pos(false, i);
        self.link_after(false, i, at);
    }

    /// releaseElement: the first locked element (with the key, for PoolMap).
    pub fn release(&mut self, key: Option<&str>) -> Option<u32> {
        let mut i = self.locked.head;
        if let (Some(keys), Some(k)) = (&self.keys, key) {
            while i != NIL && keys[i as usize] != k {
                i = self.next[i as usize];
            }
        }
        if i == NIL {
            return None;
        }
        self.unlink(false, i);
        let at = self.insert_pos(true, i);
        self.link_after(true, i, at);
        self.released_len += 1;
        Some(i)
    }

    pub fn reset(&mut self) {
        self.cursor = self.released.head;
    }

    pub fn first(&mut self) -> Option<u32> {
        self.reset();
        (self.cursor != NIL).then_some(self.cursor)
    }

    pub fn next_el(&mut self) -> Option<u32> {
        if self.cursor == NIL {
            return None;
        }
        self.cursor = self.next[self.cursor as usize];
        (self.cursor != NIL).then_some(self.cursor)
    }

    pub fn at_end(&self) -> bool {
        self.cursor == NIL
    }

    /// lockLatestElement.
    pub fn lock_latest(&mut self) {
        let i = self.cursor;
        if i == NIL {
            return;
        }
        let n = self.next[i as usize];
        self.unlink(true, i);
        self.released_len -= 1;
        self.insert_locked(i);
        self.cursor = n;
        if self.cursor != self.released.head && self.cursor != NIL {
            self.cursor = self.prev[self.cursor as usize];
        }
    }

    pub fn lock_all(&mut self) {
        while self.released.head != NIL {
            let i = self.released.head;
            self.unlink(true, i);
            self.insert_locked(i);
        }
        self.released_len = 0;
        self.cursor = self.released.head;
    }

    /// The released elements in order.
    pub fn released(&self) -> Vec<u32> {
        let mut v = Vec::with_capacity(self.released_len);
        let mut i = self.released.head;
        while i != NIL {
            v.push(i);
            i = self.next[i as usize];
        }
        v
    }

    /// Reorders the released list (the renderer's sort).
    pub fn set_released_order(&mut self, order: &[u32]) {
        let mut prev = NIL;
        self.released.head = order.first().copied().unwrap_or(NIL);
        for &i in order {
            self.prev[i as usize] = prev;
            if prev != NIL {
                self.next[prev as usize] = i;
            }
            prev = i;
        }
        if prev != NIL {
            self.next[prev as usize] = NIL;
        }
        self.released.tail = prev;
        self.cursor = self.released.head;
    }
}

/// ParticlePool: visual particles, emitted emitters and emitted techniques (emitted affectors
/// and systems are not used by Kenshi's scripts).
#[derive(Debug, Clone)]
pub struct Pool {
    pub visual: SubPool,
    pub emitter: SubPool,
    pub technique: SubPool,
    latest: Option<PRef>,
}

impl Default for Pool {
    fn default() -> Self {
        Pool {
            visual: SubPool::plain(),
            emitter: SubPool::keyed(),
            technique: SubPool::keyed(),
            latest: None,
        }
    }
}

impl Pool {
    pub fn is_empty(&self) -> bool {
        self.visual.released_len == 0
            && self.emitter.released_len == 0
            && self.technique.released_len == 0
    }

    pub fn size(&self) -> usize {
        self.visual.released_len + self.emitter.released_len + self.technique.released_len
    }

    fn reset_iterator(&mut self) {
        self.visual.reset();
        self.emitter.reset();
        self.technique.reset();
        self.latest = None;
    }

    fn first_from(&mut self, from: u8) -> Option<PRef> {
        if from == 0
            && let Some(i) = self.visual.first()
        {
            return Some(PRef::Visual(i));
        }
        if from <= 1
            && let Some(i) = self.emitter.first()
        {
            return Some(PRef::Emitter(i));
        }
        self.technique.first().map(PRef::Technique)
    }

    /// ParticlePool::getFirst.
    pub fn first(&mut self) -> Option<PRef> {
        self.reset_iterator();
        self.latest = self.first_from(0);
        self.latest
    }

    /// ParticlePool::getNext.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<PRef> {
        let latest = self.latest?;
        self.latest = match latest {
            PRef::Visual(_) => match self.visual.next_el() {
                Some(i) => Some(PRef::Visual(i)),
                None => self.first_from(1),
            },
            PRef::Emitter(_) => match self.emitter.next_el() {
                Some(i) => Some(PRef::Emitter(i)),
                None => self.first_from(2),
            },
            PRef::Technique(_) => self.technique.next_el().map(PRef::Technique),
        };
        self.latest
    }

    /// ParticlePool::end.
    pub fn at_end(&self) -> bool {
        self.visual.at_end() && self.emitter.at_end() && self.technique.at_end()
    }

    /// lockLatestParticle.
    pub fn lock_latest(&mut self) {
        match self.latest {
            Some(PRef::Visual(_)) => self.visual.lock_latest(),
            Some(PRef::Emitter(_)) => self.emitter.lock_latest(),
            Some(PRef::Technique(_)) => self.technique.lock_latest(),
            None => {}
        }
    }

    pub fn lock_all(&mut self) {
        self.visual.lock_all();
        self.emitter.lock_all();
        self.technique.lock_all();
        self.reset_iterator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_latest_skips_after_an_expired_head() {
        let mut p = SubPool::plain();
        for _ in 0..4 {
            p.add(None);
        }
        for _ in 0..4 {
            p.release(None);
        }
        // expire the first: the walk resumes past the new head (PU's behaviour)
        let mut seen = vec![p.first().unwrap()];
        p.lock_latest();
        while let Some(i) = p.next_el() {
            seen.push(i);
        }
        assert_eq!(seen, vec![0, 2, 3]);
        assert_eq!(p.released(), vec![1, 2, 3]);
        // expire a middle one: no skip
        let mut seen = vec![p.first().unwrap()];
        let i = p.next_el().unwrap();
        seen.push(i);
        p.lock_latest();
        while let Some(i) = p.next_el() {
            seen.push(i);
        }
        assert_eq!(seen, vec![1, 2, 3]);
        assert_eq!(p.released(), vec![1, 3]);
        // released again in locked order (FIFO)
        assert_eq!(p.release(None), Some(0));
    }
}
