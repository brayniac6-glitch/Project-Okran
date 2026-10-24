//! People push one another as Kenshi's AI mover does (F024363, `kenshi_char::movement::
//! neighbour_push_at`; crate::combat's arena `separate` does it for fighters): each stepping
//! mover takes its share of the deepest overlap with a neighbour, the smaller one more, along the
//! line between them, and its steer bends round it (F024364, `Mover::push_steer`). A mover
//! following a path that overlaps someone standing on its goal ends that path there.
//!
//! The neighbours a mover tests are its sensor's proximity list (the AICharacter's +0x100 is the
//! character's CharSensor, Character vtable 0x68 = AI +0x650 + 0x28; the list at its +0x68),
//! which CharSensor::update (F046548) rebuilds on each sensor tick (F046551, the AI's update):
//! from the characters gathered within the vision range, everyone but itself not carried and not
//! a ragdoll (F044938: Character +0x3d4, or the animation's ragdoll +0x59 / +0x5a, AnimationClass
//! flags 1 / 0x800, F038930), not lying in a bed or a cage (+0x2f8) and not seated on a chair or
//! throne (the state object +0x1a0, +0xb8, set by Task_OperateMachine::update F031350 for
//! BF_CHAIR / BF_THRONE), standing within the two radii and 40 units on the ground plane and
//! within 20 units of its height. One seated or in a bed or a cage has an empty list.
//!
//! Then at each step (F024363's own test of each listed one, as it stands now): anyone not seated
//! on a chair or throne and not carried pushes; every character is active (+0x1a9) here. Who is
//! pushed: every mover that steps; one held in place (operating a machine, in bed, caged,
//! carried, down, in a fight the arena runs) does not step.
//!
//! The random way out for two on the same spot is rand01 (F050500, the C runtime's
//! rand() / 32768), drawn from the simulation's one generator as every ported rand01 is.

use kenshi_char::movement::{neighbour_push_at, Neighbour};
use std::collections::HashMap;

/// One character as its own and others' sensors see it (Kenshi units).
#[derive(Debug, Clone, Copy)]
pub struct Sensed {
    /// units
    pub position: [f32; 3],
    /// Its radius (F039126: the movement object's footprint radius x its scale), units.
    pub radius: f32,
    /// Put on others' lists (F046548): not carried, not a ragdoll (F044938), not in a bed or a
    /// cage (+0x2f8 == 0), not seated (+0x1a0 +0xb8).
    pub listed: bool,
    /// Keeps a list of its own: not in a bed or a cage, not seated.
    pub lists: bool,
}

/// @fn F046548
///
/// The proximity list character `me` keeps (CharSensor +0x68) from those gathered within its
/// vision range `range` (units, F046475; the gather F044287 adds nothing more): everyone else
/// listed within the two radii and 40 units on the ground plane and within 20 units of its
/// height.
pub fn proximity(me: usize, all: &[Sensed], range: f32) -> Vec<usize> {
    let mine = &all[me];
    if !mine.lists {
        return Vec::new();
    }
    let p = mine.position;
    all.iter()
        .enumerate()
        .filter(|&(k, o)| {
            if k == me || !o.listed {
                return false;
            }
            let (dx, dy, dz) = (p[0] - o.position[0], p[1] - o.position[1], p[2] - o.position[2]);
            let reach = mine.radius + o.radius + 40.0;
            dx.hypot(dz) <= range && dx * dx + dz * dz < reach * reach && dy.abs() < 20.0
        })
        .map(|(k, _)| k)
        .collect()
}

/// One person as the movers see it as they step.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub id: usize,
    /// metres
    pub position: [f32; 3],
    pub radius: f32,
    /// Pushes those that step into it (F024363's own test: not seated, not carried).
    pub pushes: bool,
}

/// Everyone as the movers step: their places by id.
#[derive(Debug, Default)]
pub struct Crowd {
    bodies: HashMap<usize, Body>,
}

impl Crowd {
    pub fn new(bodies: impl IntoIterator<Item = Body>) -> Crowd {
        Crowd { bodies: bodies.into_iter().map(|b| (b.id, b)).collect() }
    }

    /// Person `id` now stands at `position`.
    pub fn moved(&mut self, id: usize, position: [f32; 3]) {
        if let Some(b) = self.bodies.get_mut(&id) {
            b.position = position;
        }
    }

    /// @fn F024363
    ///
    /// The push the mover at `position` (radius `radius`, m) gets as it steps from those on its
    /// proximity list `near` (ids) as they stand now, and, for one following a path to `goal`,
    /// whether a neighbour it overlaps stands on that goal. `rand` gives rand01.
    pub fn push(&self, position: [f32; 3], radius: f32, goal: Option<[f32; 3]>, near: &[usize], rand: impl FnMut() -> f32) -> (Option<([f32; 3], f32)>, bool) {
        let others: Vec<Neighbour> = near
            .iter()
            .filter_map(|o| self.bodies.get(o))
            .filter(|b| b.pushes)
            .map(|b| Neighbour { position: b.position, radius: b.radius })
            .collect();
        neighbour_push_at(position, radius, &others, false, goal, rand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(id: usize, x: f32, pushes: bool) -> Body {
        Body { id, position: [x, 0.0, 0.0], radius: 0.4, pushes }
    }

    fn roll() -> impl FnMut() -> f32 {
        || 0.75
    }

    #[test]
    fn overlapping_people_part() {
        // overlapping by 0.3: each takes half (equal radii), away from the other
        let c = Crowd::new([body(0, 0.0, true), body(1, 0.5, true)]);
        let (p, _) = c.push([0.0; 3], 0.4, None, &[1], roll());
        let (n, d) = p.unwrap();
        assert!((n[0] + 1.0).abs() < 1e-5 && (d - 0.15).abs() < 1e-5);
        // not on my list: nothing
        assert!(c.push([0.0; 3], 0.4, None, &[], roll()).0.is_none());
        // one on a chair does not push
        let c = Crowd::new([body(0, 0.0, true), body(1, 0.5, false)]);
        assert!(c.push([0.0; 3], 0.4, None, &[1], roll()).0.is_none());
        // the smaller gives way more: 0.2 against 0.6 overlapping by 0.4
        let mut small = body(0, 0.0, true);
        small.radius = 0.2;
        let mut big = body(1, 0.4, true);
        big.radius = 0.6;
        let c = Crowd::new([small, big]);
        assert!((c.push([0.0; 3], 0.2, None, &[1], roll()).0.unwrap().1 - 0.4).abs() < 1e-5);
        // on the same spot: a random way, as deep as the other's radius
        let c = Crowd::new([body(0, 0.0, true), body(1, 0.0, true)]);
        assert!((c.push([0.0; 3], 0.4, None, &[1], roll()).0.unwrap().1 - 0.4).abs() < 1e-5);
        // apart already: nothing
        let c = Crowd::new([body(0, 0.0, true), body(1, 0.9, true)]);
        assert!(c.push([0.0; 3], 0.4, None, &[1], roll()).0.is_none());
        // someone standing on my goal that I touch: the path ends
        let c = Crowd::new([body(0, 0.0, true), body(1, 0.5, true)]);
        assert!(c.push([0.0; 3], 0.4, Some([0.6, 0.0, 0.1]), &[1], roll()).1);
        assert!(!c.push([0.0; 3], 0.4, Some([2.0, 0.0, 0.0]), &[1], roll()).1);
        // moved away: no longer touching
        let mut c = Crowd::new([body(0, 0.0, true), body(1, 0.5, true)]);
        c.moved(1, [5.0, 0.0, 0.0]);
        assert!(c.push([0.0; 3], 0.4, None, &[1], roll()).0.is_none());
    }

    fn sensed(x: f32, y: f32) -> Sensed {
        Sensed { position: [x, y, 0.0], radius: 4.0, listed: true, lists: true }
    }

    #[test]
    fn the_proximity_list() {
        // within the radii and 40 units, under 20 units up or down
        let all = [sensed(0.0, 0.0), sensed(47.9, 0.0), sensed(48.1, 0.0), sensed(10.0, 19.9), sensed(10.0, -20.0)];
        assert_eq!(proximity(0, &all, 1500.0), vec![1, 3]);
        // past the vision range: not gathered
        assert_eq!(proximity(0, &all, 30.0), vec![3]);
        // a ragdoll, one carried, in bed or seated: not on the list
        let mut all = [sensed(0.0, 0.0), sensed(5.0, 0.0)];
        all[1].listed = false;
        assert!(proximity(0, &all, 1500.0).is_empty());
        // one in bed or seated keeps none
        let mut all = [sensed(0.0, 0.0), sensed(5.0, 0.0)];
        all[0].lists = false;
        assert!(proximity(0, &all, 1500.0).is_empty());
    }
}
