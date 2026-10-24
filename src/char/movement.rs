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
    /// The deepest neighbour's push for this step (F024363): the normal away from it and the
    /// depth (m), which the steer moves the mover by and bends the velocity with. Kenshi finds
    /// it inside steer: crate::crowd finds it as each mover steps, the arena for its fighters.
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
/// more, held to 0 .. 1 m, along the line from it; in combat mode (Character +0xe0) a fiftieth
/// of that. Exactly on another (within 0.001): Kenshi's random direction (r - 0.5, r - 0.5,
/// r x 0.5) as deep as max(own radius m, the other's in units) x 0.1, at once. `rand` gives
/// numbers in 0..1. Returns the normal and depth; None under 0.001.
pub fn neighbour_push(me: [f32; 3], radius: f32, others: &[Neighbour], combat: bool, rand: impl FnMut() -> f32) -> Option<([f32; 3], f32)> {
    neighbour_push_at(me, radius, others, combat, None, rand).0
}

/// @fn F024363
///
/// `neighbour_push` for a mover following a path to `goal` (m; path state +0x88 = 2): also
/// whether a neighbour it overlaps stands on that goal (within the neighbour's radius on the
/// ground plane), which ends the path where the mover is (+0x88 = 1).
pub fn neighbour_push_at(me: [f32; 3], radius: f32, others: &[Neighbour], combat: bool, goal: Option<[f32; 3]>, mut rand: impl FnMut() -> f32) -> (Option<([f32; 3], f32)>, bool) {
    let mut best: Option<([f32; 3], f32)> = None;
    let mut taken = false;
    for o in others {
        let (dx, dz) = (me[0] - o.position[0], me[2] - o.position[2]);
        if dx.abs() <= 0.001 && dz.abs() <= 0.001 {
            let (r1, r2, r3) = (rand(), rand(), rand());
            let n = [r3 - 0.5, r2 - 0.5, (r1 as f64 * 0.5) as f32];
            let depth = (radius.max(o.radius * 10.0) as f64 * 0.1) as f32;
            return (Some((n, depth)), taken);
        }
        let sum = radius + o.radius;
        let d2 = dx * dx + dz * dz;
        if d2 >= sum * sum {
            continue;
        }
        let d = d2.sqrt();
        let depth = ((sum - d) * ((o.radius - radius) / sum + 0.5)).min(1.0).max(0.0);
        if best.is_none_or(|b| depth > b.1) {
            best = Some(([dx / d, 0.0, dz / d], depth));
        }
        if let Some(g) = goal {
            let (gx, gz) = (o.position[0] - g[0], o.position[2] - g[2]);
            if gx * gx + gz * gz < o.radius * o.radius {
                taken = true;
            }
        }
    }
    let Some((n, depth)) = best.filter(|b| b.1 >= 0.001) else { return (None, taken) };
    (Some((n, if combat { depth * 0.02 } else { depth })), taken)
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
    /// AICharacter::steer with the push found for this step (F024363, in `avoid`) as Kenshi takes it: the
    /// mover moved by all of the push first (the walls unheeded: Kenshi sweeps the push and
    /// does not use the answer), then the desired velocity bent by it.
    pub fn push_steer(&mut self, direction: [f32; 3], dt: f32) {
        if let Some((n, depth)) = self.avoid {
            for (p, c) in self.position.iter_mut().zip(n) {
                *p += c * depth;
            }
        }
        self.steer(direction, dt);
    }

    /// @fn F024364
    ///
    /// Turn the velocity toward `direction` (unit length, or zero to stop) at full speed, changing
    /// it by at most acceleration * dt, the desired velocity first bent by the neighbour push in
    /// `avoid` (F024363, Kenshi's own, not Havok's). The push's own move is `push_steer`'s (the
    /// arena moves its fighters by theirs itself).
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

/// A mover on a found path (CharMovement mode 0; AICharacter's path state +0x88 = 2): the edges
/// still ahead, the time spent off the path's corridor (+0x84), and whether the mover asks for
/// its path again (its state +0x90 = 6).
#[derive(Debug, Clone, PartialEq)]
pub struct PathFollow {
    pub path: GatePath,
    pub off_corridor: f32,
    pub repath: bool,
}

impl PathFollow {
    pub fn new(path: GatePath) -> Self {
        PathFollow { path, off_corridor: 0.0, repath: false }
    }
}

/// How far up and down AICharacter::step looks for the face under the mover after it moves
/// (F032166's ray, 4 m).
pub const FACE_TRACK: f32 = 4.0;

/// @fn F032166
///
/// The mover's face (+0x14) after it moved: the coherent vertical ray of
/// `NavWorld::track_face` over `range` (the face it stood on first); found, the mover is set
/// onto it; none: no face (-1).
pub fn track(mover: &mut Mover, nav: &NavWorld, face: &mut Option<FaceRef>, range: f32) {
    match nav.track_face(mover.position, *face, range) {
        Some((f, y)) => {
            *face = Some(f);
            mover.position[1] = y;
        }
        None => *face = None,
    }
}

/// How long a mover may be off its path's corridor before it asks for the path again (s).
pub const OFF_CORRIDOR: f32 = 5.0;

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// @fn F024365
///
/// AICharacter::step along a path (path state 2):
/// - the heading from F024361 (with the corner fans rebuilt as it goes);
/// - within stopping distance of the point the heading is judged by (its way along the
///   velocity, less 0.1 m, under v^2 / 2a), the turn that heading asks for decides: when the
///   speed squared is above 2a x the heading's part across the velocity, the heading at
///   sqrt(that) (slowing round the turn, to a stop where it lies straight ahead); else square
///   across the velocity toward the heading at full speed;
/// - steer (F024364); heading for the goal, a step reaching it lands on it and stops;
/// - the step moved against the walls (`step_piece`), split where it is over 3 m into as many
///   pieces as it holds 3 m (rounded down);
/// - after each piece the face underfoot, the time off the corridor (on it: the start face
///   before any edge is passed, or the face any of the edges from the last passed to the next
///   five leads into; more than 5 s off it asks for the path again) and the edges passed
///   (F024347);
/// - within 0.1 m of the goal the path is done (F024349): `arrived`.
pub fn follow(mover: &mut Mover, f: &mut PathFollow, face: &mut Option<FaceRef>, walls: &mut Walls, nav: &NavWorld, dt: f32) {
    mover.arrived = false;
    // the Havok character stands on the navmesh (the body is put on the ground after)
    if let Some(g) = *face {
        mover.position[1] = nav.height_on(g, mover.position);
    }
    let h = f.path.heading(mover.position, mover.velocity, mover.radius, true, Some(nav));
    let to_goal = h.index == f.path.edges.len();
    let mut desired = h.dir;
    let v = [mover.velocity[0], 0.0, mover.velocity[2]];
    let speed = (v[0] * v[0] + v[2] * v[2]).sqrt();
    if speed > 0.0 {
        let vh = [v[0] / speed, 0.0, v[2] / speed];
        let to = [h.to[0], 0.0, h.to[2]];
        let stop = speed * speed / (mover.acceleration * 2.0);
        if stop as f64 > dot3(to, vh) as f64 - 0.1 {
            let n = [vh[2], 0.0, -vh[0]];
            let across = dot3(n, h.dir);
            let turn = ((mover.acceleration * 2.0 * across) as f64).abs() as f32;
            let max = mover.max_speed;
            desired = if max * max > turn {
                h.dir.map(|c| c * (turn.sqrt() / max))
            } else if across < 0.0 {
                n.map(|c| -c)
            } else {
                n
            };
        }
    }
    steer(mover, nav, *face, walls, desired, dt);
    let mut step = mover.velocity.map(|c| c * dt);
    let mut moved = false;
    if to_goal {
        let d = [mover.position[0] - f.path.goal[0], mover.position[1] - f.path.goal[1], mover.position[2] - f.path.goal[2]];
        if dot3(d, d) < dot3(step, step) {
            moved = true;
            mover.position = f.path.goal;
            mover.velocity = [0.0; 3];
            step = [0.0; 3];
        }
    }
    let mut pieces = 1;
    let s2 = dot3(step, step);
    if s2 > 9.0 {
        pieces = ((s2.sqrt() / 3.0).floor() as usize).max(1);
        step = step.map(|c| c / pieces as f32);
    }
    for _ in 0..pieces {
        if step != [0.0; 3] {
            step_piece(mover, nav, *face, walls, step, dt);
            moved = true;
        }
        if !moved {
            continue;
        }
        moved = false;
        track(mover, nav, face, FACE_TRACK);
        let on = face.is_some_and(|g| {
            (f.path.next == 0 && f.path.start == Some(g)) || (f.path.next.saturating_sub(1)..f.path.edges.len().min(f.path.next + 5)).any(|k| f.path.edges[k].face == g)
        });
        if on {
            f.off_corridor = 0.0;
        } else {
            f.off_corridor += dt;
        }
        if f.off_corridor > OFF_CORRIDOR {
            f.repath = true;
        }
        if let Some(g) = f.path.advance(mover.position) {
            *face = Some(g);
        }
    }
    if at_goal(mover.position, *face, f) {
        mover.arrived = true;
    }
}

/// How far round the mover its walls are gathered: radius + 6 m (F024350's callers).
pub const WALL_REACH: f32 = 6.0;

/// The walls a mover keeps round it (AICharacter +0x150 .. +0x170): where and how far round they
/// were gathered, whether one of them has a face across it, and the walls themselves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Walls {
    /// m (+0x150); never gathered: 0.
    pub centre: [f32; 3],
    /// m (+0x160)
    pub reach: f32,
    /// A wall with a face across it (+0x164): they are gathered again each time.
    pub again: bool,
    /// Each wall's ends (at their heights) and its normal into the walkable side.
    pub list: Vec<([f32; 3], [f32; 3], [f32; 3])>,
}

impl Walls {
    /// @fn F024350
    ///
    /// The walls round `at` (m) within `reach`: kept while none has a face across it and `at`
    /// lies within (0.8 x the reach they were gathered with) of where they were, as a squared
    /// distance (Kenshi compares the squared distance with 0.8 x the reach itself); else gathered
    /// again from the face the mover stands on (F032187), which on no face, or one not loaded,
    /// leaves them as they were.
    pub fn gather(&mut self, nav: &NavWorld, face: Option<FaceRef>, at: [f32; 3], reach: f32) {
        if !self.again && dist2(self.centre, at) <= (self.reach as f64 * 0.8) as f32 {
            return;
        }
        let Some(face) = face.filter(|&f| nav.face_valid(f)) else { return };
        self.centre = at;
        self.reach = reach;
        self.again = nav.walls_near(face, at, reach, &mut self.list);
    }

    /// F024364's gather: only when they never were (the centre still at 0).
    pub fn first(&mut self, nav: &NavWorld, face: Option<FaceRef>, at: [f32; 3], reach: f32) {
        if self.centre == [0.0; 3] {
            self.gather(nav, face, at, reach);
        }
    }
}

/// @fn F024365
///
/// One piece of a step against the walls (the walls gathered round the mover, F024350): swept
/// (F024351) to the first touch, the rest of the piece along that wall (on its tangent) swept
/// again; a slide that cannot move at all is the stuck case Kenshi logs, which moves the mover
/// 0.1 m out along the first touch normal (swept) instead. Otherwise, after a touch, the move
/// is the new velocity (over `dt`), and a mover that began within its radius of the touch point
/// goes out to the radius along that normal (not swept). The walls are those kept round the
/// mover, gathered again first if the piece's middle is off from where they were (F024350).
pub fn step_piece(mover: &mut Mover, nav: &NavWorld, face: Option<FaceRef>, walls: &mut Walls, d: [f32; 3], dt: f32) {
    let start = mover.position;
    walls.gather(nav, face, [start[0] + d[0] * 0.5, start[1] + d[1] * 0.5, start[2] + d[2] * 0.5], mover.radius + WALL_REACH);
    let walls = &walls.list;
    let (t1, hit) = NavWorld::sweep_wall(start, d, mover.radius, walls);
    let mut at = start;
    let (mut moved, mut t, mut touched) = (d, t1, hit);
    if let Some((n1, _)) = hit {
        at = [at[0] + d[0] * t1, at[1] + d[1] * t1, at[2] + d[2] * t1];
        let rest = d.map(|c| c * (1.0 - t1));
        let tangent = [n1[2], 0.0, -n1[0]];
        let along = dot3(rest, tangent);
        moved = tangent.map(|c| c * along);
        let (t2, _) = NavWorld::sweep_wall(at, moved, mover.radius, walls);
        t = t2;
        if t2 == 0.0 {
            kenshi_world::navmesh::STUCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if std::env::var("KENSHI_SWEEP_DEBUG").is_ok() {
                eprintln!("stuck at {:?}", at.map(|v| (v * 10.0).round()));
            }
            moved = n1.map(|c| c * 0.1);
            let (t3, _) = NavWorld::sweep_wall(at, moved, mover.radius, walls);
            t = t3;
            touched = None;
        }
    }
    at = [at[0] + moved[0] * t, at[1] + moved[1] * t, at[2] + moved[2] * t];
    kenshi_world::navmesh::SWEEPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Some((n1, w1)) = touched {
        if dt > 0.0 {
            mover.velocity = [(at[0] - start[0]) / dt, (at[1] - start[1]) / dt, (at[2] - start[2]) / dt];
        }
        if w1 < mover.radius {
            at = [at[0] + n1[0] * (mover.radius - w1), at[1], at[2] + n1[2] * (mover.radius - w1)];
        }
    }
    mover.position = at;
}

/// @fn F024349
///
/// Whether a path follower has reached its goal: within 0.1 m of it on the ground plane, and on
/// the face the path's last edge leads into (the start face for a path with none) or within
/// 1 m of the goal's height.
pub fn at_goal(pos: [f32; 3], face: Option<FaceRef>, f: &PathFollow) -> bool {
    let g = f.path.goal;
    let (dx, dz) = (pos[0] - g[0], pos[2] - g[2]);
    if dx * dx + dz * dz > 0.01 {
        return false;
    }
    let goal_face = if f.path.edges.is_empty() { f.path.start } else { f.path.edges.last().map(|e| e.face) };
    (face.is_some() && goal_face == face) || (pos[1] - g[1]).abs() < 1.0
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
/// CharMovement mode 2 (direct velocity, set by Task_FollowLeader): steer along `dir` (F024364,
/// with its push), and the tick the step then takes (`paused_step`), cut short so it does not
/// pass `dist`. The original compares `dist` (units, +0x398) with the mover's step in metres, so
/// the cut only bites within a fraction of a unit; kept as it is.
pub fn direct(mover: &mut Mover, nav: &NavWorld, face: Option<FaceRef>, walls: &mut Walls, dir: [f32; 3], dist: f32, dt: f32) -> f32 {
    mover.arrived = false;
    steer(mover, nav, face, walls, dir, dt);
    let speed = length(mover.velocity);
    if dist < dt * speed { dist / speed } else { dt }
}

/// @fn F024365
///
/// AICharacter::step for a held mover (path state 3, set by CharMovement modes 1 and 2 through
/// F024341): the neighbour push found as it steps (F024363) moves it by all of it, the walls
/// unheeded (Kenshi gathers them, F024350, sweeps the push and does not use the answer), then
/// its velocity moves it against the walls (`step_piece`) over `dt`.
pub fn paused_step(mover: &mut Mover, nav: &NavWorld, face: &mut Option<FaceRef>, walls: &mut Walls, push: Option<([f32; 3], f32)>, dt: f32) {
    if let Some((n, depth)) = push {
        walls.gather(nav, *face, mover.position, mover.radius + WALL_REACH);
        for (p, c) in mover.position.iter_mut().zip(n) {
            *p += c * depth;
        }
    }
    walk_velocity(mover, nav, face, walls, dt);
}

/// @fn F024364
///
/// The steer (`Mover::push_steer`) as the mover takes it: with a push, the walls gathered first
/// if they never were (Kenshi sweeps the push against them and moves by all of it).
pub fn steer(mover: &mut Mover, nav: &NavWorld, face: Option<FaceRef>, walls: &mut Walls, desired: [f32; 3], dt: f32) {
    if mover.avoid.is_some() {
        walls.first(nav, face, mover.position, mover.radius + WALL_REACH);
    }
    mover.push_steer(desired, dt);
}

/// @fn F024365
///
/// AICharacter::step for a mover without a path (path state 0 or 1): no heading, so the steer
/// (with its push, F024364) only slows it, then its velocity moves it against the walls.
pub fn idle_step(mover: &mut Mover, nav: &NavWorld, face: &mut Option<FaceRef>, walls: &mut Walls, dt: f32) {
    mover.arrived = false;
    steer(mover, nav, *face, walls, [0.0; 3], dt);
    walk_velocity(mover, nav, face, walls, dt);
}

/// The velocity's step over `dt` against the walls (`walk_step`).
fn walk_velocity(mover: &mut Mover, nav: &NavWorld, face: &mut Option<FaceRef>, walls: &mut Walls, dt: f32) {
    let step = mover.velocity.map(|c| c * dt);
    walk_step(mover, nav, face, walls, step, dt);
}

/// @fn F024365
///
/// A mover's step of `step` (m) over `dt` against the walls, in pieces as F024365 cuts a long
/// one, the face it stands on (+0x14) tracked after each (F032166).
pub fn walk_step(mover: &mut Mover, nav: &NavWorld, face: &mut Option<FaceRef>, walls: &mut Walls, step: [f32; 3], dt: f32) {
    if let Some(g) = *face {
        mover.position[1] = nav.height_on(g, mover.position);
    }
    let mut step = step;
    let mut pieces = 1;
    let s2 = dot3(step, step);
    if s2 > 9.0 {
        pieces = ((s2.sqrt() / 3.0).floor() as usize).max(1);
        step = step.map(|c| c / pieces as f32);
    }
    for _ in 0..pieces {
        if step == [0.0; 3] {
            break;
        }
        step_piece(mover, nav, *face, walls, step, dt);
        track(mover, nav, face, FACE_TRACK);
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
    /// The last answer found no path.
    pub failed: bool,
    /// The mover's goal (AICharacter +0x40, m): the last one sent to the manager, then the
    /// answer's.
    pub goal: Option<[f32; 3]>,
    /// The mover asks for its path to `goal` again at its next step (state +0x90 = 6).
    pub again: bool,
    /// The last answer found no path (+0x180): the next step tries to free the mover (F024358).
    pub unstick: bool,
    /// The face the mover stands on (AICharacter +0x14), none for -1.
    pub face: Option<FaceRef>,
    /// The walls the mover keeps round it (+0x150 .. +0x170).
    pub walls: Walls,
    /// Where the mover's own last step left it (x, z), none before its first: one found
    /// elsewhere was moved by the simulation, as Kenshi moves it with setPosition (F024357).
    pub last: Option<[f32; 2]>,
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
        let moving = self.pending || self.follow.is_some();
        if !accepts_goal(
            mover.position_units(),
            dest,
            self.last_goal.filter(|_| moving),
        ) {
            return false;
        }
        self.set_goal(owner, mover, dest.map(|v| v * TO_METRES), urgent, manager);
        self.last_goal = Some(dest);
        true
    }

    /// @fn F024356
    ///
    /// AICharacter::setGoal: the goal (m) sent to the manager from where the mover stands, with
    /// its radius and water cost; the mover waits on the answer.
    pub fn set_goal<O: PartialEq + Clone>(&mut self, owner: O, mover: &Mover, goal: [f32; 3], urgent: bool, manager: &mut PathManager<O>) {
        manager.submit_from(owner, mover.position, self.face, goal, mover.radius, mover.water_cost, urgent, self.pass);
        self.pending = true;
        self.again = false;
        self.goal = Some(goal);
    }

    /// @fn F024352
    ///
    /// The manager answered. A path found from another face than the mover now stands on (it
    /// moved while it waited) is taken from the edge after the one, of its first ten, that
    /// leads onto that face; with none of them the answer is dropped and the path asked for
    /// again. The edges from there on that end exactly where the mover stands are passed, and a
    /// mover already at the goal is done (F024349). No path: the mover stops following, and its
    /// next step tries to free it (F024358).
    pub fn answer(&mut self, result: PathResult, mover: &Mover, nav: &NavWorld) {
        self.pending = false;
        self.failed = !matches!(result, PathResult::Found(_));
        match result {
            PathResult::Found(found) => {
                let mut path = found.follow;
                // the face the mover stands on now (F032165, else F032166 over 4 m)
                if !self.face.is_some_and(|f| nav.face_valid(f)) {
                    self.face = nav.track_face(mover.position, self.face, FACE_TRACK).map(|(f, _)| f);
                }
                let mut next = 0;
                if self.face != Some(found.face) {
                    match path.edges.iter().take(10).position(|e| Some(e.face) == self.face) {
                        Some(i) => next = i + 1,
                        None => {
                            self.again = true;
                            return;
                        }
                    }
                }
                let pos = mover.position;
                while next < path.edges.len() && (path.edges[next].a == pos || path.edges[next].b == pos) {
                    next += 1;
                }
                path.next = next;
                path.start = Some(found.face);
                self.goal = Some(path.goal);
                let f = PathFollow::new(path);
                self.follow = if at_goal(pos, self.face, &f) { None } else { Some(f) };
            }
            _ => {
                self.follow = None;
                self.unstick = true;
            }
        }
    }

    /// The start of the mover's step (F024365): a mover whose last path failed is freed if it
    /// can be (F024358, `visible` telling whether a point is in the camera's view), and one that
    /// asks for its path again sends its goal to the manager (setGoal, not urgent).
    pub fn prelude<O: PartialEq + Clone>(&mut self, owner: O, mover: &mut Mover, nav: &NavWorld, manager: &mut PathManager<O>, visible: &dyn Fn([f32; 3]) -> bool) {
        self.settle(mover, nav);
        self.prelude_requests(owner, mover, nav, manager, visible);
    }

    /// The mover as the simulation hands it to its step: a new one put on the navmesh
    /// (AICharacter's constructor, F024360), one the simulation moved found anew (setPosition,
    /// F024357).
    pub fn settle(&mut self, mover: &mut Mover, nav: &NavWorld) {
        match self.last {
            // a new mover: AICharacter's constructor (F024360)
            None => place(mover, nav, &mut self.face),
            // moved by the simulation: setPosition (F024357), the face found anew by the
            // vertical ray over 500 m
            Some(l) if l != [mover.position[0], mover.position[2]] => {
                self.face = None;
                track(mover, nav, &mut self.face, kenshi_world::navmesh::START_SEARCH);
            }
            _ => {}
        }
        self.last = Some([mover.position[0], mover.position[2]]);
    }

    fn prelude_requests<O: PartialEq + Clone>(&mut self, owner: O, mover: &mut Mover, nav: &NavWorld, manager: &mut PathManager<O>, visible: &dyn Fn([f32; 3]) -> bool) {
        if self.unstick {
            self.unstick = false;
            if let Some(goal) = self.goal
                && unstick(mover, nav, self.face, &mut self.walls, goal, visible)
            {
                self.again = true;
            }
        }
        if self.again
            && let Some(goal) = self.goal
        {
            // state 6: the face found again over 500 m (F032166), then setGoal
            track(mover, nav, &mut self.face, kenshi_world::navmesh::START_SEARCH);
            self.set_goal(owner, mover, goal, false, manager);
        }
    }

    /// One tick of mode 0 (F024365): the step's start (`prelude`), then along the path if there
    /// is one (arriving ends it), else the step of a mover without one (`idle_step`).
    pub fn tick<O: PartialEq + Clone>(&mut self, owner: O, mover: &mut Mover, nav: &NavWorld, manager: &mut PathManager<O>, visible: &dyn Fn([f32; 3]) -> bool, dt: f32) {
        self.prelude(owner, mover, nav, manager, visible);
        if let Some(f) = &mut self.follow {
            follow(mover, f, &mut self.face, &mut self.walls, nav, dt);
            if f.repath {
                f.repath = false;
                f.off_corridor = 0.0;
                if !self.pending {
                    self.again = true;
                }
            }
            if mover.arrived {
                self.follow = None;
            }
        } else {
            idle_step(mover, nav, &mut self.face, &mut self.walls, dt);
        }
        self.last = Some([mover.position[0], mover.position[2]]);
    }

    /// After a step taken outside `tick` (a fighter's, or mode 2's): where it left the mover.
    pub fn stepped(&mut self, mover: &Mover) {
        self.last = Some([mover.position[0], mover.position[2]]);
    }
}

/// @fn F024360
///
/// The new mover put on the navmesh: setPosition (F024357: the face found by the coherent
/// vertical ray over 500 m, the mover set onto it); on none, the nearest point of the mesh
/// within 3 m (F032167, the query mediator's getClosestPoint) and setPosition there.
pub fn place(mover: &mut Mover, nav: &NavWorld, face: &mut Option<FaceRef>) {
    *face = None;
    track(mover, nav, face, kenshi_world::navmesh::START_SEARCH);
    if face.is_none()
        && let Some((_, q)) = nav.closest_point(mover.position, 3.0)
    {
        mover.position = q;
        track(mover, nav, face, kenshi_world::navmesh::START_SEARCH);
    }
}

/// @fn F024358
///
/// A mover whose path request failed (+0x180 = 2): out to its radius + 0.01 from the nearest
/// wall it is too near (F024354, the walls it keeps, gathered within radius + 6 m, F024350:
/// one on no face keeps those it had); then, stuck where it stands
/// (F032186), 0.5 m toward its goal if that point is in the camera's view (F041469), else 3 m.
/// Whether it moved (it then asks for its path again, +0x90 = 6). (The other case, +0x180 = 1,
/// follows an answer of status 0 that found no path, which the manager here never gives.)
pub fn unstick(mover: &mut Mover, nav: &NavWorld, face: Option<FaceRef>, walls: &mut Walls, goal: [f32; 3], visible: &dyn Fn([f32; 3]) -> bool) -> bool {
    let mut again = false;
    walls.gather(nav, face, mover.position, mover.radius + WALL_REACH);
    if let Some(out) = wall_escape(mover.position, (mover.radius as f64 + 0.01) as f32, &walls.list) {
        for (p, c) in mover.position.iter_mut().zip(out) {
            *p += c;
        }
        again = true;
    }
    if nav.stuck(mover.position, mover.radius, 10.0) {
        let d = [goal[0] - mover.position[0], goal[1] - mover.position[1], goal[2] - mover.position[2]];
        let l = length(d);
        let k = if visible(mover.position) { 0.5 } else { 3.0 };
        if l > 0.0 {
            for (p, c) in mover.position.iter_mut().zip(d) {
                *p += c / l * k;
            }
        }
        again = true;
    }
    again
}

/// @fn F024354
///
/// The way out from the walls for a mover at `pos` to stand `reach` off them (on the ground
/// plane): of the walls within that reach, the nearest the mover stands in front of goes before
/// any behind it (behind one, over its length, it is taken only when nearer than the nearest
/// so far, and moved through to the front); the move puts the mover `reach` in front of that
/// wall's nearest point. None with no wall within reach.
pub fn wall_escape(pos: [f32; 3], reach: f32, walls: &[([f32; 3], [f32; 3], [f32; 3])]) -> Option<[f32; 3]> {
    let r2 = reach * reach;
    let (mut limit, mut best) = (r2, r2);
    let mut out = None;
    for &(a, b, _) in walls {
        let e = [b[0] - a[0], 0.0, b[2] - a[2]];
        let a = [a[0], pos[1], a[2]];
        let d = [pos[0] - a[0], 0.0, pos[2] - a[2]];
        let ee = e[0] * e[0] + e[2] * e[2];
        let t = if ee > 0.0 { ((d[0] * e[0] + d[2] * e[2]) / ee).min(1.0).max(0.0) } else { 0.0 };
        let diff = [pos[0] - (a[0] + e[0] * t), 0.0, pos[2] - (a[2] + e[2] * t)];
        let dist2 = diff[0] * diff[0] + diff[2] * diff[2];
        let mut next = limit;
        if dist2 < limit || dist2 < best {
            let n = [e[2], 0.0, -e[0]];
            let mut k = reach;
            next = dist2;
            if n[0] * diff[0] + n[2] * diff[2] < 0.0 && 0.0 < t && t < 1.0 {
                next = limit;
                if best <= dist2 {
                    limit = next;
                    continue;
                }
                k = -reach;
            }
            let v = if dist2 == 0.0 { n } else { diff };
            let l = length(v);
            let dist = dist2.sqrt();
            out = Some(if l > 0.0 { v.map(|c| c / l * (k - dist)) } else { [0.0; 3] });
            best = dist2;
        }
        limit = next;
    }
    out
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
        p.answer(PathResult::NoStart, &m, &NavWorld::default());
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
