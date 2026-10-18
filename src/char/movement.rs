//! How a character moves toward a goal: its footprint and acceleration from race data, and the
//! mover that turns a desired direction into velocity and position.
//!
//! The original hands the path itself to Havok AI (navmesh search, corridor following, local
//! avoidance), a third-party library the rewrite replaces. What is ported here is Kenshi's own side:
//! the settings it gives the mover, the unit conversion (Havok works in metres, Kenshi in 10 cm
//! units), its bounded-acceleration steering, braking on arrival, and which goal requests it ignores.
//! The path between goal and character comes from the rewrite's own pathfinding (not here yet): the
//! mover only needs the next point to head for.
//!
//! Specs: sheet rows F040657 CharMovement::init, F024360 the mover's constructor, F024340
//! setMaxSpeed, F024364 steer, F024365 step, F040653 CharMovement::goTo, F040650 the per-tick update,
//! F033689 the race's "water avoidance".

use crate::medical::lerp;
use kenshi_world::navmesh::{FaceRef, GatePath, NavWorld, PathManager, PathResult};

/// Havok metres per Kenshi unit.
pub const TO_METRES: f32 = 0.1;

/// The race fields movement reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaceMovement {
    /// "pathfind footprint radius" (units; 4 for people, 40 for leviathans).
    pub footprint_radius: f32,
    /// "pathfind acceleration" (m/s²; 20 for most races).
    pub acceleration: f32,
    /// "water avoidance": how much the path search avoids water.
    pub water_avoidance: f32,
    /// "combat move speed mult".
    pub combat_speed_mult: f32,
}

impl RaceMovement {
    /// The race record's fields (absent ones take the values most races have).
    pub fn from_record(r: &kenshi_data::GameData) -> Self {
        let f = |k: &str, d: f32| {
            r.floats
                .get(k)
                .copied()
                .or_else(|| r.ints.get(k).map(|v| *v as f32))
                .unwrap_or(d)
        };
        Self {
            footprint_radius: f("pathfind footprint radius", 4.0),
            acceleration: f("pathfind acceleration", 20.0),
            water_avoidance: f("water avoidance", 0.0),
            combat_speed_mult: f("combat move speed mult", 1.0),
        }
    }

    /// @fn F033689
    ///
    /// The path cost multiplier for water: 1 + avoidance, or 1 / (1 - avoidance) when negative
    /// (a race that prefers water).
    pub fn water_cost(&self) -> f32 {
        if self.water_avoidance >= 0.0 {
            self.water_avoidance + 1.0
        } else {
            (1.0 / (1.0 - self.water_avoidance as f64)) as f32
        }
    }
}

/// A character's mover (the original's wrapper around a Havok AI character), in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mover {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// m (+0x70)
    pub radius: f32,
    /// +0x74
    pub water_cost: f32,
    /// m/s² (+0x78)
    pub acceleration: f32,
    /// m/s (+0x7c)
    pub max_speed: f32,
    /// Reached the goal on the last step (+0x80).
    pub arrived: bool,
    /// The deepest neighbour's push found last tick (F024363): the normal away from it and the
    /// depth (m), which the next steer bends the velocity by. (assumption) Kenshi finds it inside
    /// steer; here the crowd / arena pass after the step sets it for the next one.
    pub avoid: Option<([f32; 3], f32)>,
}

/// A neighbour as the push test sees it: position and radius (m).
#[derive(Debug, Clone, Copy)]
pub struct Neighbour {
    pub position: [f32; 3],
    pub radius: f32,
}

/// @fn F024363
///
/// The push a mover at `me` (radius `radius`, m) gets from its neighbours, if any: the deepest
/// overlap on the ground plane (the other's position taken at its height) of their radii
/// together R, its depth (R - d) x (0.5 + (R_other - R_me) / R) so the smaller one gives way
/// more, along the line from it; in combat mode (Character +0xe0) a fiftieth of that. Exactly on
/// another (within 0.001): a random direction (Kenshi's (r - 0.5, r - 0.5, r x 0.5), the
/// height dropped here) as deep as max(own radius m, the other's in units) x 0.1, at once.
/// `rand` gives numbers in 0..1. Returns the normal and depth; None under 0.001.
/// (not ported) A path-following mover whose goal the other stands on (within its radius) has
/// its path state +0x88 set to 1; the rewrite's path state has no traced equivalent.
pub fn neighbour_push(me: [f32; 3], radius: f32, others: &[Neighbour], combat: bool, mut rand: impl FnMut() -> f32) -> Option<([f32; 3], f32)> {
    let mut best: Option<([f32; 3], f32)> = None;
    for o in others {
        let (dx, dz) = (me[0] - o.position[0], me[2] - o.position[2]);
        if dx.abs() <= 0.001 && dz.abs() <= 0.001 {
            let (r1, r2, r3) = (rand(), rand(), rand());
            let _ = r2;
            let n = [r3 - 0.5, 0.0, r1 * 0.5];
            let depth = radius.max(o.radius * 10.0) * 0.1;
            return Some((n, depth));
        }
        let sum = radius + o.radius;
        let d2 = dx * dx + dz * dz;
        if d2 >= sum * sum {
            continue;
        }
        let d = d2.sqrt();
        let depth = ((sum - d) * ((o.radius - radius) / sum + 0.5)).max(0.0);
        if best.is_none_or(|b| depth > b.1) {
            best = Some(([dx / d, 0.0, dz / d], depth));
        }
    }
    let (n, depth) = best.filter(|b| b.1 >= 0.001)?;
    Some((n, if combat { depth * 0.02 } else { depth }))
}

impl Mover {
    /// @fn F024360
    ///
    /// `position` in units; `radius` in units, `acceleration` already in m/s².
    pub fn new(position: [f32; 3], radius: f32, acceleration: f32) -> Self {
        Self {
            position: position.map(|v| v * TO_METRES),
            velocity: [0.0; 3],
            radius: (radius as f64 * 0.1) as f32,
            water_cost: 1.0,
            acceleration,
            max_speed: 0.0,
            arrived: false,
            avoid: None,
        }
    }

    /// @fn F040657
    ///
    /// The mover CharMovement creates for a character of this race: the footprint scales with the
    /// character's size (Character vtable 0x390); young animals (maturity < 1) accelerate up to
    /// three times as hard; `halved_water` (the character's +0x250 link is set) halves the water cost.
    pub fn for_character(
        race: &RaceMovement,
        position: [f32; 3],
        size: f32,
        maturity: f32,
        halved_water: bool,
    ) -> Self {
        let m = maturity.clamp(0.0, 1.0);
        let accel = lerp(m, race.acceleration * 3.0, race.acceleration);
        let mut mover = Self::new(position, race.footprint_radius * size, accel);
        mover.water_cost = if halved_water { 0.5 } else { 1.0 } * race.water_cost();
        mover
    }

    /// @fn F024340
    ///
    /// `speed` in units per second.
    pub fn set_max_speed(&mut self, speed: f32) {
        self.max_speed = (speed as f64 * 0.1) as f32;
    }

    /// Position in units.
    pub fn position_units(&self) -> [f32; 3] {
        self.position.map(|v| v * 10.0)
    }

    /// @fn F024339
    ///
    /// Drive by a desired velocity (units/s) instead of a goal, as CharMovement does while it
    /// steers in formation (mode 1): the velocity is taken as given and the position follows.
    pub fn drive(&mut self, velocity: [f32; 3], dt: f32) {
        self.arrived = false;
        self.velocity = velocity.map(|v| (v as f64 * 0.1) as f32);
        for (p, v) in self.position.iter_mut().zip(self.velocity) {
            *p += v * dt;
        }
    }

    /// @fn F024364
    ///
    /// Turn the velocity toward `direction` (unit length, or zero to stop) at full speed, changing
    /// it by at most acceleration * dt, the desired velocity first bent by the neighbour push in
    /// `avoid` (F024363, Kenshi's own, not Havok's). (not ported) Kenshi also moves the position
    /// by the push here; the crowd / arena pass does that instead.
    pub fn steer(&mut self, direction: [f32; 3], dt: f32) {
        let mut desired = direction.map(|d| d * self.max_speed);
        // pushed by a neighbour (F024363): heading within ~37 degrees of straight into it, half
        // the heading turned aside is added (toward the side it already leans); then the push
        // normal times max(depth, 0.01)
        if let Some((n, depth)) = self.avoid.take() {
            let dot = desired[0] * n[0] + desired[1] * n[1] + desired[2] * n[2];
            let len = length(desired);
            if dot < 0.0 && len > 0.0 && dot / len < -0.8 {
                let k = if desired[2] * n[0] - desired[0] * n[2] < 0.0 { -0.5 } else { 0.5 };
                let side = [desired[2], 0.0, -desired[0]];
                for (v, s) in desired.iter_mut().zip(side) {
                    *v += k * s;
                }
            }
            for (v, c) in desired.iter_mut().zip(n) {
                *v += depth.max(0.01) * c;
            }
        }
        let mut dv: [f32; 3] = std::array::from_fn(|k| desired[k] - self.velocity[k]);
        let len = length(dv);
        let limit = dt * self.acceleration;
        if limit < len {
            let s = limit / len;
            dv = dv.map(|v| v * s);
        }
        for (v, d) in self.velocity.iter_mut().zip(dv) {
            *v += d;
        }
    }

    /// @fn F024365
    ///
    /// One tick toward `goal` (metres): head for it, braking so the character stops on it (once
    /// the distance left, less 0.1 m, is under v² / 2a), then move; a step that would pass the goal
    /// lands on it and stops.
    pub fn step(&mut self, goal: [f32; 3], dt: f32) {
        self.arrived = false;
        let to = [goal[0] - self.position[0], 0.0, goal[2] - self.position[2]];
        let dist = length(to);
        let dir = if dist > 0.0 {
            to.map(|v| v / dist)
        } else {
            [0.0; 3]
        };
        let max = self.max_speed;
        let speed = length(self.velocity);
        if speed > 0.0
            && ((dist as f64) - 0.1) < ((speed * speed) / (self.acceleration * 2.0)) as f64
        {
            // the speed from which the mover can still stop in the distance left
            let brake = (2.0 * self.acceleration * dist).sqrt();
            if brake < max {
                self.max_speed = brake;
            }
        }
        self.steer(dir, dt);
        self.max_speed = max;
        let step = self.velocity.map(|v| v * dt);
        let s2 = step[0] * step[0] + step[1] * step[1] + step[2] * step[2];
        if dist * dist < s2 {
            self.arrived = true;
            self.position[0] = goal[0];
            self.position[2] = goal[2];
            self.velocity = [0.0; 3];
            return;
        }
        for (p, d) in self.position.iter_mut().zip(step) {
            *p += d;
        }
    }
}

/// A mover on a found path (CharMovement mode 0): the gates still ahead, and the navmesh face
/// underfoot (the Havok character tracks it; heights come from it).
#[derive(Debug, Clone, PartialEq)]
pub struct PathFollow {
    pub path: GatePath,
    pub face: FaceRef,
}

/// @fn F024365
///
/// One tick along a path: the heading is pulled through the gates from where the mover stands
/// (F024361); while gates are ahead it steers that way at full speed, on the last segment it
/// takes the braking step onto the goal. The height follows the face underfoot.
pub fn follow(mover: &mut Mover, f: &mut PathFollow, nav: &NavWorld, dt: f32) {
    let before = mover.position;
    let crossed = f.path.next;
    let target = f.path.target(before);
    // through a gate: onto the face it leads into (replacement for Havok's corridor tracking)
    let mut on_corridor = false;
    if f.path.next > crossed
        && let Some(&face) = f.path.faces.get(f.path.next - 1)
    {
        f.face = face;
        on_corridor = true;
    }
    if f.path.on_last_segment() {
        mover.step(f.path.goal, dt);
    } else {
        mover.arrived = false;
        let to = [target[0] - before[0], 0.0, target[2] - before[2]];
        let dist = length(to);
        let dir = if dist > 0.0 {
            to.map(|v| v / dist)
        } else {
            [0.0; 3]
        };
        mover.steer(dir, dt);
        for (p, v) in mover.position.iter_mut().zip(mover.velocity) {
            *p += v * dt;
        }
    }
    // track the face underfoot (not on the tick a gate hands over the next corridor face);
    // where the walk loses it, the corridor's faces round the next gate first, then the nearest
    if !on_corridor {
        let n = f.path.next;
        let found = nav.walk(f.face, before, mover.position).or_else(|| {
            [n.wrapping_sub(1), n]
                .iter()
                .filter_map(|&i| f.path.faces.get(i))
                .copied()
                .find(|&c| nav.is_over(c, mover.position))
                .or_else(|| nav.closest_point(mover.position, 5.0).map(|(face, _)| face))
        });
        if let Some(face) = found {
            f.face = face;
        }
    }
    mover.position[1] = nav.height_on(f.face, mover.position);
}

/// @fn F040653
///
/// Whether a go-to request reaches the mover: not when the character already stands within a unit
/// of it, nor (while it is moving there) when it is within 2 units of the goal it already has.
pub fn accepts_goal(position: [f32; 3], goal: [f32; 3], current_goal: Option<[f32; 3]>) -> bool {
    if dist2(goal, position) < 1.0 {
        return false;
    }
    if let Some(g) = current_goal
        && dist2(g, goal) < 4.0
    {
        return false;
    }
    true
}

/// @fn F040650
///
/// One tick of CharMovement mode 2 (direct velocity, set by Task_FollowLeader): steer along
/// `dir` (F024364), then move, the tick cut short so the step does not pass `dist`. The original
/// compares `dist` (units, +0x398) with the mover's step in metres, so the cut only bites within
/// a fraction of a unit; kept as it is.
pub fn direct(mover: &mut Mover, dir: [f32; 3], dist: f32, dt: f32) {
    mover.arrived = false;
    mover.steer(dir, dt);
    let speed = length(mover.velocity);
    let t = if dist < dt * speed { dist / speed } else { dt };
    for (p, v) in mover.position.iter_mut().zip(mover.velocity) {
        *p += v * t;
    }
}

/// @fn F040646
///
/// CharMovement::stop: the goal becomes where the character stands, velocity zero, path and
/// target dropped.
pub fn stop(mover: &mut Mover, pathing: &mut Pathing) {
    mover.velocity = [0.0; 3];
    pathing.last_goal = Some(mover.position_units());
    pathing.follow = None;
}

/// Ground below which a character that keeps out of water does not take an area point (units).
pub const AREA_MIN_GROUND: f32 = 95.0;
/// F050534's answer where there is no ground.
pub const NO_GROUND: f32 = -99.0;

/// @fn F040635
///
/// CharMovement::order for an area (order mode 2, and mode 1): up to 10 tries at a random point in
/// the square of side `radius` round `centre` (units), each taken unless its ground (`ground`,
/// F050534: `NO_GROUND` where there is none) lies below `AREA_MIN_GROUND` for a character that
/// keeps out of water (`avoid_water`, the race flag at +0x2e0 +0x79). The point then goes to goTo
/// (vtable +0x90, `Pathing::go_to`) at its ground height. None when every try failed.
///
/// Not ported: the building test of each point (F032174 within 300 units: a building the
/// character may not enter rejects it) and order mode 3 (a point near an object).
pub fn area_point(
    centre: [f32; 3],
    radius: f32,
    avoid_water: bool,
    mut rand01: impl FnMut() -> f32,
    mut ground: impl FnMut(f32, f32) -> f32,
) -> Option<[f32; 3]> {
    for _ in 0..10 {
        let x = centre[0] + (rand01() * radius - radius * 0.5);
        let z = centre[2] + (rand01() * radius - radius * 0.5);
        let h = ground(x, z);
        if h == NO_GROUND || h >= AREA_MIN_GROUND || !avoid_water {
            return Some([x, h, z]);
        }
    }
    None
}

/// The radius F039100 gives a move order: the target object's (Town::radius, vtable +0x2a0)
/// times 0.5, else 1250 without a target, never below 500.
/// @fn F039100
pub fn order_radius(target_radius: Option<f32>) -> f32 {
    target_radius.map_or(1250.0, |r| r * 0.5).max(500.0)
}

/// CharMovement's path state (mode 0): the last goal asked for (+0xdc), whether the mover waits
/// on the navmesh manager (mover state +0x90 4/5), and the path it follows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pathing {
    /// Units.
    pub last_goal: Option<[f32; 3]>,
    pub pending: bool,
    pub follow: Option<PathFollow>,
    /// Which shut gates this character may pass (its faction's, kenshi_world::navmesh::Pass).
    pub pass: kenshi_world::navmesh::Pass,
    /// (stand-in) The goal of the last request that found no path, and how many times it has
    /// been asked for again since: not sent again for 60 asks (a second of a task asking each
    /// tick) unless the goal moves, so one who can't get somewhere doesn't hold up everyone
    /// else's paths (the manager answers one a tick).
    pub failed: Option<([f32; 3], u16)>,
}

impl Pathing {
    /// @fn F040653
    ///
    /// goTo: a destination (units) the mover takes (`accepts_goal`) is sent on as a path request
    /// (F024356 setGoal: start = the mover's position, its radius and water cost; urgent for
    /// mode 2). Returns whether it was taken.
    pub fn go_to<O: PartialEq + Clone>(
        &mut self,
        owner: O,
        mover: &Mover,
        dest: [f32; 3],
        urgent: bool,
        manager: &mut PathManager<O>,
    ) -> bool {
        if let Some((g, n)) = &mut self.failed
            && (g[0] - dest[0]).hypot(g[2] - dest[2]) < 20.0
            && *n < 60
        {
            *n += 1;
            return false;
        }
        let moving = self.pending || self.follow.is_some();
        if !accepts_goal(
            mover.position_units(),
            dest,
            self.last_goal.filter(|_| moving),
        ) {
            return false;
        }
        manager.submit_as(owner, mover.position, dest.map(|v| v * TO_METRES), mover.radius, mover.water_cost, urgent, self.pass);
        self.pending = true;
        self.last_goal = Some(dest);
        true
    }

    /// The manager answered: follow the path, or stand (no path).
    pub fn answer(&mut self, result: PathResult) {
        self.pending = false;
        self.failed = match &result {
            PathResult::Found(_) => None,
            _ => self.last_goal.map(|g| (g, 0)),
        };
        self.follow = match result {
            PathResult::Found(f) => Some(PathFollow {
                path: f.follow,
                face: f.face,
            }),
            _ => None,
        };
    }

    /// One tick of mode 0: along the path if there is one; arriving ends it.
    pub fn tick(&mut self, mover: &mut Mover, nav: &NavWorld, dt: f32) {
        if let Some(f) = &mut self.follow {
            follow(mover, f, nav, dt);
            if mover.arrived {
                self.follow = None;
            }
        }
    }
}

/// How fast a character is told to go (the FCS editor's MoveSpeed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveSpeed {
    Walk = 0,
    Jog = 1,
    Run = 2,
    /// Keep to the group's pace (its speed is worked out each tick).
    Grouped = 3,
    NoSpeedChange = 4,
}

impl MoveSpeed {
    /// @fn F040700
    ///
    /// The speed cap a mode sets (CharMovement +0xbc): walking holds to `walk_speed`
    /// (CharMovement +0xc0, 15 unless set), jogging to 55, running and grouped are uncapped
    /// (999); `None` for NoSpeedChange, which keeps the current cap.
    pub fn cap(self, walk_speed: f32) -> Option<f32> {
        match self {
            MoveSpeed::Walk => Some(walk_speed),
            MoveSpeed::Jog => Some(55.0),
            MoveSpeed::Run | MoveSpeed::Grouped => Some(999.0),
            MoveSpeed::NoSpeedChange => None,
        }
    }
}

/// @fn F040650
///
/// The speed the mover is limited to this tick: the character's own (its move speed, F047064,
/// through the animation's cap F037131) or the mode's cap if lower; when grouped, `group` (the
/// group's pace) replaces the cap.
pub fn speed_limit(own: f32, cap: f32, group: Option<f32>) -> f32 {
    let cap = group.unwrap_or(cap);
    if cap <= own { cap } else { own }
}

/// @fn F040650
///
/// The measured velocity (units per second) after a tick that moved the character from `before`
/// to `after`, and its speed; a tick without horizontal movement drops the vertical part too.
pub fn measured_velocity(before: [f32; 3], after: [f32; 3], dt: f32) -> ([f32; 3], f32) {
    let mut v = [
        after[0] - before[0],
        after[1] - before[1],
        after[2] - before[2],
    ];
    if v[0] == 0.0 && v[2] == 0.0 {
        v[1] = 0.0;
    }
    if dt > 0.0 {
        v = v.map(|x| x / dt);
    }
    (v, length(v))
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (x, z) = (a[0] - b[0], a[2] - b[2]);
    let y = a[1] - b[1];
    x * x + y * y + z * z
}

#[cfg(test)]
mod tests {
    use super::*;

    fn race() -> RaceMovement {
        RaceMovement {
            footprint_radius: 4.0,
            acceleration: 20.0,
            water_avoidance: 0.0,
            combat_speed_mult: 1.0,
        }
    }

    #[test]
    fn settings_from_race() {
        let m = Mover::for_character(&race(), [100.0, 0.0, 0.0], 1.0, 1.0, false);
        assert_eq!(m.position, [10.0, 0.0, 0.0]);
        assert!((m.radius - 0.4).abs() < 1e-6);
        assert_eq!(m.acceleration, 20.0);
        // a newborn animal accelerates three times as hard
        let young = Mover::for_character(&race(), [0.0; 3], 1.0, 0.0, false);
        assert_eq!(young.acceleration, 60.0);
        let r = RaceMovement {
            water_avoidance: -1.0,
            ..race()
        };
        assert_eq!(r.water_cost(), 0.5);
        let r = RaceMovement {
            water_avoidance: 3.0,
            ..race()
        };
        assert_eq!(r.water_cost(), 4.0);
    }

    #[test]
    fn accelerates_runs_and_stops_on_the_goal() {
        let mut m = Mover::for_character(&race(), [0.0; 3], 1.0, 1.0, false);
        m.set_max_speed(70.0);
        let goal = [20.0, 0.0, 0.0]; // 200 units away
        let dt = 1.0 / 60.0;
        m.step(goal, dt);
        assert!((length(m.velocity) - 20.0 * dt).abs() < 1e-4);
        let mut top = 0.0f32;
        let mut t = 0.0;
        while !m.arrived && t < 10.0 {
            m.step(goal, dt);
            top = top.max(length(m.velocity));
            t += dt;
        }
        assert!(m.arrived, "never arrived");
        assert!((top - 7.0).abs() < 1e-3, "top speed {top}");
        assert_eq!(m.position_units(), [200.0, 0.0, 0.0]);
        // 20 m at 7 m/s plus a little to speed up and brake
        assert!(t > 20.0 / 7.0 && t < 20.0 / 7.0 + 0.6, "took {t}");
    }

    #[test]
    fn area_points() {
        // the square round the centre, half the radius each way
        let mut r = [0.0f32, 1.0].into_iter().cycle();
        let p = area_point(
            [100.0, 0.0, 100.0],
            1000.0,
            true,
            || r.next().unwrap(),
            |_, _| 500.0,
        );
        assert_eq!(p, Some([-400.0, 500.0, 600.0]));
        // under water: no point for one who keeps out of it, any for one who does not
        assert_eq!(
            area_point([0.0; 3], 1000.0, true, || 0.5, |_, _| 50.0),
            None
        );
        assert!(area_point([0.0; 3], 1000.0, false, || 0.5, |_, _| 50.0).is_some());
        assert_eq!(order_radius(Some(2500.0)), 1250.0);
        assert_eq!(order_radius(Some(400.0)), 500.0);
        assert_eq!(order_radius(None), 1250.0);
    }

    #[test]
    fn go_to_queues_one_request_per_goal() {
        let mut manager: PathManager<u8> = PathManager::default();
        let mut p = Pathing::default();
        let m = Mover::new([0.0; 3], 4.0, 20.0);
        assert!(p.go_to(1, &m, [50.0, 0.0, 0.0], true, &mut manager));
        // the same goal again while the request is pending: ignored
        assert!(!p.go_to(1, &m, [51.0, 0.0, 0.0], true, &mut manager));
        assert!(p.go_to(1, &m, [90.0, 0.0, 0.0], false, &mut manager));
        // no navmesh loaded: one answer (the later request replaced the first), no path
        let (owner, r) = manager.update(&NavWorld::default()).unwrap();
        assert_eq!((owner, r), (1, PathResult::NoStart));
        assert!(manager.update(&NavWorld::default()).is_none());
        p.answer(PathResult::NoStart);
        assert!(!p.pending && p.follow.is_none());
    }

    #[test]
    fn goal_requests() {
        assert!(!accepts_goal([0.0; 3], [0.5, 0.0, 0.0], None));
        assert!(accepts_goal([0.0; 3], [5.0, 0.0, 0.0], None));
        assert!(!accepts_goal(
            [0.0; 3],
            [5.0, 0.0, 0.0],
            Some([6.0, 0.0, 0.0])
        ));
        assert_eq!(speed_limit(70.0, 999.0, Some(45.0)), 45.0);
        assert_eq!(speed_limit(30.0, 999.0, Some(45.0)), 30.0);
        let walk = MoveSpeed::Walk.cap(15.0).unwrap();
        assert_eq!(speed_limit(95.0, walk, None), 15.0);
        assert_eq!(
            speed_limit(95.0, MoveSpeed::Jog.cap(15.0).unwrap(), None),
            55.0
        );
        assert_eq!(
            speed_limit(95.0, MoveSpeed::Run.cap(15.0).unwrap(), None),
            95.0
        );
        let (v, s) = measured_velocity([0.0; 3], [3.0, 0.0, 4.0], 0.5);
        assert_eq!((v, s), ([6.0, 0.0, 8.0], 10.0));
    }
}
