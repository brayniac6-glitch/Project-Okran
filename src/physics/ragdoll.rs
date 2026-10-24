//! Ragdolls: the rigid bodies and joints of a Scythe `.phs` file (F025944), simulated.
//!
//! (stand-in: PhysX 2.8's solver is closed source and its SDK documents only the parameters,
//! so the solver itself cannot be ported) Kenshi hands these to PhysX 2.8; this solves the same
//! bodies, shapes, joints and settings with extended position-based dynamics (sub-stepped rigid
//! bodies, one solve per sub-step), on the values Kenshi gives PhysX: the scene (F045103), the bodies' descriptors
//! (F025969, then F045312 for a ragdoll's parts), the shapes' (F025966) and the joints'
//! (F025967 / F025968), stepped once a frame at most 0.04 s (F045111). Bodies collide with the
//! ground, the buildings and with each other unless jointed (as PhysX joints disable their pair's
//! contacts unless NX_JF_COLLISION_ENABLED). Against each other the shapes meet as they are
//! (spheres, capsules and boxes: `overlap`).
use glam::{Mat3, Quat, Vec3};
use crate::world::Surroundings;
use kenshi_io::phs::{Frame, JointKind, Phs, ShapeKind};

/// The physics scene's gravity (units / s^2: 9.81 m/s^2 in decimetres; NxSceneDesc, F045103).
pub const GRAVITY: Vec3 = Vec3::new(0.0, -98.1, 0.0);
/// A physics step: the frame's time, at most this (s). Kenshi's scene steps variably
/// (NX_TIMESTEP_VARIABLE): one step per frame (F045111, F045103).
pub const MAX_STEP: f32 = 0.04;
/// The least solver iteration count a body gets (F025969 raises the file's to 16); a ragdoll
/// solves with its bodies' most, one sub-step per iteration here. (assumption: the SDK documents
/// the count per body, NxBodyDesc::solverIterationCount, and not how the bodies of one island
/// share it) An island is solved in the highest count among its bodies.
const MIN_ITERATIONS: u32 = 16;
/// PhysX 2.8's NX_MAX_ANGULAR_VELOCITY default (Kenshi leaves the scene's and the bodies' at it,
/// F045103 / F025969: -1): no body turns faster (radians / s).
const MAX_ANGULAR: f32 = 7.0;
/// (stand-in: PhysX 2.8 has no setting for it, NxBodyDesc has no depenetration cap, and how its
/// closed solver pulls penetration out over its iterations is undocumented) a contact pushes a
/// body out at most this fast (units / s), so starting deep inside something does not fling it.
const MAX_DEPENETRATION: f32 = 40.0;
/// Against the ground and the buildings a box's corners collide as spheres this big (units) set
/// inside them (a box meets a plane at its corners; the spheres round them by this much).
const CORNER: f32 = 0.3;
/// The scene's skin width (NX_SKIN_WIDTH 0.25, F045103): shapes rest this far (units) inside
/// what they touch. A shape's own (its file's, F025966) when not negative; (assumption: the SDK
/// documents a skin per shape, NxShapeDesc::skinWidth, not how a touching pair's two combine) a
/// pair rests the more of its two skins deep.
const SKIN: f32 = 0.25;
/// Every material Kenshi makes and its default material (F045103): static friction 20, dynamic
/// 1, restitution 0.05, averaged between the two (NX_CM_AVERAGE, so as given), with the scene's
/// friction scaled 4 times (NX_STA_FRICT_SCALING / NX_DYN_FRICT_SCALING 4).
const STATIC_FRICTION: f32 = 20.0 * 4.0;
const DYNAMIC_FRICTION: f32 = 1.0 * 4.0;
const RESTITUTION: f32 = 0.05;
/// PhysX 2.8's NX_BOUNCE_THRESHOLD default (-2; Kenshi leaves it): contacts closing slower than
/// this (units / s) don't bounce.
const BOUNCE_THRESHOLD: f32 = 2.0;
/// A ragdoll part's damping, linear and angular (F045312 sets both on every actor that follows
/// a bone, after the file's, F025969).
const PART_DAMPING: [f32; 2] = [0.5, 0.5];
/// The bodies' sleep test (F025969: NX_BF_ENERGY_SLEEP_TEST with sleepEnergyThreshold 0.05, the
/// wake-up counter left at PhysX's 20 x 0.02 s): a body whose mass-normalised kinetic energy
/// (units^2 / s^2) stays below the threshold that long may sleep, and the ragdoll sleeps when
/// all may. (assumption: the SDK names it the mass-normalised kinetic energy without giving the
/// sum) The energy is 0.5 (v^2 + w.(I/m)w); the velocity filter F045312 turns on
/// (NX_BF_FILTER_SLEEP_VEL, undocumented beyond its name) is stood in for in `Ragdoll::settle`.
const SLEEP_ENERGY: f32 = 0.05;
const WAKE_TIME: f32 = 20.0 * 0.02;
/// A body held to a bone is woken for this long each frame (NxActor::wakeUp(9), F044974).
const HELD_WAKE: f32 = 9.0;
/// (stand-in: for this solver's own creep, which PhysX's closed solver does not have) This
/// solver's contacts on building meshes creep where PhysX's come to rest, which would keep the
/// energy test awake: a ragdoll whose bodies all stay within this distance
/// (units) and turn (radians) over the wake-up time sleeps as well.
const STILL_DRIFT: f32 = 0.15;
const STILL_TURN: f32 = 0.1;

/// A collision shape in its body's frame.
#[derive(Debug, Clone, Copy)]
enum Collider {
    Sphere { c: Vec3, r: f32 },
    Capsule { a: Vec3, b: Vec3, r: f32 },
    Box { c: Vec3, rot: Quat, half: Vec3 },
}

#[derive(Debug, Clone)]
pub struct BodyDef {
    /// The skeleton bone the body moves.
    pub bone: String,
    /// The body's frame in bind space: its centre of mass, unrotated.
    pub bind: Vec3,
    /// The actor's own frame origin from its centre of mass (bind space).
    origin: Vec3,
    inv_mass: f32,
    inv_inertia: Mat3,
    /// The inertia per unit mass (body frame), for the sleep test's energy.
    unit_inertia: Mat3,
    /// Linear and angular damping (per second).
    damping: [f32; 2],
    gravity: bool,
    /// Its shapes' skin width (units).
    skin: f32,
    /// Its solver iteration count (F025969: the file's, at least 16).
    iterations: u32,
    colliders: Vec<Collider>,
}

#[derive(Debug, Clone)]
struct JointDef {
    a: usize,
    b: usize,
    /// The anchor in each body's frame.
    ra: Vec3,
    rb: Vec3,
    /// The joint's axes in each body's frame.
    fa: Quat,
    fb: Quat,
    kind: JointKind,
    /// Radians.
    low: f32,
    high: f32,
    swing: f32,
    limited: bool,
    /// Springs (stiffness, target radians, damper): the twist (or hinge) spring and the swing
    /// spring (stiffness, damper).
    twist_spring: Option<(f32, f32, f32)>,
    swing_spring: Option<(f32, f32)>,
}

/// A ragdoll as loaded: bodies in bind space and the joints between them.
#[derive(Debug, Clone)]
pub struct Template {
    pub bodies: Vec<BodyDef>,
    joints: Vec<JointDef>,
    /// The body pairs that collide (not jointed).
    pairs: Vec<(usize, usize)>,
    /// Sub-steps per step: the bodies' highest solver iteration count.
    substeps: usize,
}

fn rot(f: &Frame) -> Quat {
    let [a, b, c] = f.rows.map(Vec3::from);
    Quat::from_mat3(&Mat3::from_cols(a, b, c).orthonormalize()).normalize()
}

trait Ortho {
    fn orthonormalize(self) -> Self;
}

impl Ortho for Mat3 {
    /// Gram-Schmidt, keeping the first two axes' directions.
    fn orthonormalize(self) -> Mat3 {
        let x = self.x_axis.normalize_or(Vec3::X);
        let y = (self.y_axis - x * x.dot(self.y_axis)).normalize_or(x.any_orthonormal_vector());
        let z = x.cross(y);
        Mat3::from_cols(x, y, z)
    }
}

/// Inertia of a solid about its centre, in its own axes, per unit mass.
fn unit_inertia(kind: ShapeKind, s: Vec3) -> Vec3 {
    match kind {
        ShapeKind::Sphere => Vec3::splat(0.4 * (s.x * 0.5).powi(2)),
        ShapeKind::Capsule => {
            // a cylinder (its height between the caps) and two half spheres, each part's share
            // of the mass by volume; a half sphere about the capsule's middle by the parallel
            // axes (its own centre 3r/8 out from the flat face)
            use std::f32::consts::PI;
            let r = s.x * 0.5;
            let h = (s.y - 2.0 * r).max(0.0);
            let (vc, vh) = (PI * r * r * h, 2.0 / 3.0 * PI * r.powi(3));
            let (mc, mh) = (vc / (vc + 2.0 * vh), vh / (vc + 2.0 * vh));
            let along = mc * 0.5 * r * r + 2.0 * mh * 0.4 * r * r;
            let side = mc * (h * h / 12.0 + r * r / 4.0) + 2.0 * mh * (0.4 * r * r + h * h / 4.0 + 3.0 * h * r / 8.0);
            Vec3::new(side, along, side)
        }
        _ => Vec3::new(s.y * s.y + s.z * s.z, s.x * s.x + s.z * s.z, s.x * s.x + s.y * s.y) / 12.0,
    }
}

fn volume(kind: ShapeKind, s: Vec3) -> f32 {
    use std::f32::consts::PI;
    match kind {
        ShapeKind::Sphere => 4.0 / 3.0 * PI * (s.x * 0.5).powi(3),
        ShapeKind::Capsule => {
            let r = s.x * 0.5;
            PI * r * r * (s.y - 2.0 * r).max(0.0) + 4.0 / 3.0 * PI * r.powi(3)
        }
        _ => s.x * s.y * s.z,
    }
}

impl Template {
    pub fn new(p: &Phs) -> Template {
        let mut bodies = Vec::with_capacity(p.actors.len());
        for a in &p.actors {
            // PhysX's mass from shapes: each shape its own mass, or its density x volume; the sum
            // then scaled to the body's mass when that is above 0, else by the actor's density.
            // Neither set (a descriptor PhysX 2.8 refuses): no body in the shipped ragdolls
            // (data/ragdoll/*.phs); (assumption, for a modded one) density 1.
            let mut parts: Vec<(ShapeKind, Vec3, Vec3, Quat, f32)> = a
                .shapes
                .iter()
                .map(|s| {
                    let size = Vec3::from(s.size);
                    let m = if s.mass > 0.0 { s.mass } else { s.density * volume(s.kind, size) };
                    (s.kind, size, Vec3::from(s.frame.pos), rot(&s.frame), m)
                })
                .collect();
            let raw: f32 = parts.iter().map(|p| p.4).sum();
            let scale = if a.mass > 0.0 && raw > 0.0 {
                a.mass / raw
            } else if a.density > 0.0 {
                a.density
            } else {
                1.0
            };
            for part in &mut parts {
                part.4 *= scale;
            }
            let mass: f32 = if a.fixed { 0.0 } else { parts.iter().map(|p| p.4).sum() };
            let com = if mass > 0.0 {
                parts.iter().map(|p| p.2 * p.4).sum::<Vec3>() / mass
            } else {
                Vec3::from(a.pos)
            };
            let mut inertia = Mat3::ZERO;
            for &(kind, size, c, q, m) in &parts {
                let r = Mat3::from_quat(q);
                let local = Mat3::from_diagonal(unit_inertia(kind, size) * m);
                let d = c - com;
                inertia += r * local * r.transpose()
                    + (Mat3::IDENTITY * d.length_squared() - Mat3::from_cols(d * d.x, d * d.y, d * d.z)) * m;
            }
            let colliders: Vec<Collider> = parts
                .iter()
                .filter_map(|&(kind, size, c, q, _)| {
                    let c = c - com;
                    Some(match kind {
                        ShapeKind::Sphere => Collider::Sphere { c, r: size.x * 0.5 },
                        ShapeKind::Capsule => {
                            let r = size.x * 0.5;
                            let half = (size.y * 0.5 - r).max(0.0);
                            let axis = q * Vec3::Y * half;
                            Collider::Capsule { a: c - axis, b: c + axis, r }
                        }
                        ShapeKind::Box => Collider::Box { c, rot: q, half: size * 0.5 },
                        ShapeKind::Other(_) => return None,
                    })
                })
                .collect();
            let (inv_mass, inv_inertia) = if mass > 0.0 && inertia.determinant().abs() > 1e-12 {
                (1.0 / mass, inertia.inverse())
            } else {
                (0.0, Mat3::ZERO)
            };
            // F025969: linear damping the file's +0x228, angular ten times its +0x224; F045312
            // then sets 0.5 and 0.5 on an actor that follows a bone
            let damping = if a.bone_slot >= 0 { PART_DAMPING } else { [a.damping[0], a.damping[1] * 10.0] };
            let skin = a.shapes.iter().map(|s| if s.skin >= 0.0 { s.skin } else { SKIN }).fold(0.0, f32::max);
            bodies.push(BodyDef {
                bone: a.bone.clone(),
                bind: com,
                origin: Vec3::from(a.pos) - com,
                inv_mass,
                inv_inertia,
                unit_inertia: if mass > 0.0 { inertia * (1.0 / mass) } else { Mat3::ZERO },
                damping,
                gravity: !a.no_gravity,
                skin: if a.shapes.is_empty() { SKIN } else { skin },
                iterations: a.iterations.max(MIN_ITERATIONS),
                colliders,
            });
        }
        let joints = p
            .joints
            .iter()
            .filter_map(|j| {
                let a = usize::try_from(j.actors[0]).ok().filter(|&i| i < bodies.len())?;
                let b = usize::try_from(j.actors[1]).ok().filter(|&i| i < bodies.len())?;
                let at = Vec3::from(j.frame.pos);
                let f = rot(&j.frame);
                let (low, high) = (j.low.min(j.high).to_radians(), j.low.max(j.high).to_radians());
                // F025968: a hinge's spring target turned to radians, a ball joint's twist
                // target multiplied by 57.3 (degrees taken for radians: Kenshi's, kept);
                // (assumption: the SDK does not say what a spherical joint's swing spring's
                // target is measured from) a swing spring pulls toward no swing, its target
                // unused
                let twist_spring = j.springs[0].map(|s| {
                    let target = if j.kind == JointKind::Hinge { s.target.to_radians() } else { s.target.to_degrees() };
                    (s.spring, target, s.damper)
                });
                let swing_spring = if j.kind == JointKind::Ball { j.springs[1].map(|s| (s.spring, s.damper)) } else { None };
                Some(JointDef {
                    a,
                    b,
                    ra: at - bodies[a].bind,
                    rb: at - bodies[b].bind,
                    fa: f,
                    fb: f,
                    kind: j.kind,
                    low,
                    high,
                    swing: j.swing.to_radians(),
                    limited: j.limited,
                    twist_spring,
                    swing_spring,
                })
            })
            .collect::<Vec<JointDef>>();
        let n = bodies.len();
        // jointed bodies don't touch unless the joint says (NX_JF_COLLISION_ENABLED, F025967)
        let apart: Vec<(usize, usize)> = p
            .joints
            .iter()
            .filter(|j| !j.collide)
            .filter_map(|j| Some((usize::try_from(j.actors[0]).ok()?, usize::try_from(j.actors[1]).ok()?)))
            .collect();
        let pairs = (0..n)
            .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
            .filter(|&(i, j)| !apart.iter().any(|&p| p == (i, j) || p == (j, i)))
            .collect();
        let substeps = bodies.iter().filter(|b| b.inv_mass > 0.0).map(|b| b.iterations).max().unwrap_or(MIN_ITERATIONS) as usize;
        Template { bodies, joints, pairs, substeps }
    }
}

impl Template {
    /// A single moving actor of an NxuStream2 collection (an item's "physics file", F043917
    /// making it with the item dropped physically): its shapes, its body's mass (the shapes
    /// share it by volume) or density, damping and solver iterations as the file gives them
    /// (PhysX 2.8's defaults where it doesn't), its frame the actor's.
    pub fn from_nxu(b: &kenshi_io::nxu::Body) -> Template {
        use kenshi_io::nxu::ShapeKind as K;
        let shapes = b
            .shapes
            .iter()
            .map(|sh| {
                // the file's rows are the matrix's rows; a .phs frame's are its columns
                let r = sh.pose.rows;
                let cols = [[r[0][0], r[1][0], r[2][0]], [r[0][1], r[1][1], r[2][1]], [r[0][2], r[1][2], r[2][2]]];
                let (kind, size) = match sh.kind {
                    K::Box(h) => (ShapeKind::Box, [h[0] * 2.0, h[1] * 2.0, h[2] * 2.0]),
                    K::Sphere(rad) => (ShapeKind::Sphere, [rad * 2.0; 3]),
                    K::Capsule { radius, height } => (ShapeKind::Capsule, [radius * 2.0, height + radius * 2.0, radius * 2.0]),
                    _ => (ShapeKind::Other(0), [0.0; 3]),
                };
                // NxShapeDesc density defaults to 1 (the mass then shared by volume)
                kenshi_io::phs::Shape { frame: Frame { pos: sh.pose.pos, rows: cols }, kind, size, density: 1.0, mass: 0.0, skin: -1.0 }
            })
            .collect();
        let actor = kenshi_io::phs::Actor {
            name: String::new(),
            pos: [0.0; 3],
            // `Template::new` takes the file's angular damping a tenth of the actor's (F025969)
            damping: [b.linear_damping, b.angular_damping / 10.0],
            density: b.density,
            mass: b.mass,
            sleep: [-1.0, -1.0],
            iterations: b.iterations,
            fixed: false,
            no_gravity: false,
            bone_slot: -1,
            bone: String::new(),
            bone_frame: Frame { pos: [0.0; 3], rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] },
            offsets: Vec::new(),
            shapes,
        };
        let mut t = Template::new(&Phs { version: 1.4, actors: vec![actor], joints: vec![], lights: vec![] });
        // the file's own solver iterations (F025969 raises a .phs body's to 16; this is not one)
        for body in &mut t.bodies {
            body.iterations = b.iterations.max(1);
        }
        t.substeps = b.iterations.max(1) as usize;
        t
    }
}

#[derive(Debug, Clone, Copy)]
struct State {
    x: Vec3,
    q: Quat,
    v: Vec3,
    w: Vec3,
    px: Vec3,
    pq: Quat,
    /// The velocities the sub-step started with (for the bounce).
    pv: Vec3,
    pw: Vec3,
}

/// A ragdoll in the world.
#[derive(Debug, Clone)]
pub struct Ragdoll {
    s: Vec<State>,
    /// The sub-step being solved (seconds).
    h: f32,
    /// Bodies held at poses (index, centre of mass, rotation).
    pinned: Vec<(usize, Vec3, Quat)>,
    /// This sub-step's ground contacts: body, point (body frame), normal, load (depth), and the
    /// way left to its skin depth.
    contacts: Vec<(usize, Vec3, Vec3, f32, f32)>,
    /// The wake-up counter (s): counts down while every body is below the sleep energy.
    wake_counter: f32,
    /// The stillness stand-in: how long every body has stayed near `rest`.
    still: f32,
    rest: Vec<(Vec3, Quat)>,
    pub asleep: bool,
    /// Each body's acceleration for the next step besides gravity (NxActor::addForce with
    /// NX_ACCELERATION; PhysX drops a body's forces after each step).
    accel: Vec<Vec3>,
    /// Kenshi's own settling (F044957): seconds since it went limp, up to `SETTLE_WARM`, and the
    /// settle timer (+0x20).
    warm: f32,
    settle_timer: f32,
}

/// @fn F044957
///
/// Kenshi's ragdoll update leaves a new ragdoll alone this long (s, +0x18)...
pub const SETTLE_WARM: f32 = 2.0;
/// ...then, its settle timer (+0x20) past this, puts it to sleep (F044952: every actor
/// putToSleep) when a ray down from 20 units over its root body (groups `SETTLE_RAY_GROUPS`)
/// meets something within 100 units of that point, the timer starting again at 3 + rand01; meeting
/// nothing, the timer starts again at 0. (Before the timer is up it sleeps once 60% of its
/// actors are asleep: a ragdoll's jointed actors are one PhysX island and sleep together, so
/// that is the energy test of `settle`.)
pub const SETTLE_AFTER: f32 = 5.0;
pub const SETTLE_REACH: f32 = 100.0;
/// F044957's ray: collision groups 2 (the terrain), 9 - 12 (floors), 15 - 18 (stairs) and
/// 19 - 22 (furniture), mask 0x7f9e04.
pub const SETTLE_RAY_GROUPS: u32 = 0x7f_9e04;
/// F044957: the ray meeting nothing reads as this height (world units, F024614's -99).
pub const SETTLE_NO_HIT: f32 = -99.0;

/// The closest points of segments `p1..q1` and `p2..q2`.
fn closest(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let (s, t) = if a <= 1e-9 && e <= 1e-9 {
        (0.0, 0.0)
    } else if a <= 1e-9 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= 1e-9 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let den = a * e - b * b;
            let mut s = if den > 1e-9 { ((b * f - c * e) / den).clamp(0.0, 1.0) } else { 0.0 };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

/// A collider placed in the world: a segment swept by a radius (a sphere's ends meet), or a
/// box (centre, rotation, half extents).
#[derive(Debug, Clone, Copy)]
enum Placed {
    Seg(Vec3, Vec3, f32),
    Box(Vec3, Quat, Vec3),
}

impl Collider {
    fn placed(&self, x: Vec3, q: Quat) -> Placed {
        match *self {
            Collider::Sphere { c, r } => Placed::Seg(x + q * c, x + q * c, r),
            Collider::Capsule { a, b, r } => Placed::Seg(x + q * a, x + q * b, r),
            Collider::Box { c, rot, half } => Placed::Box(x + q * c, (q * rot).normalize(), half),
        }
    }
}

impl Placed {
    /// A sphere holding it: centre, radius.
    fn bound(&self) -> (Vec3, f32) {
        match *self {
            Placed::Seg(a, b, r) => ((a + b) * 0.5, a.distance(b) * 0.5 + r),
            Placed::Box(c, _, h) => (c, h.length()),
        }
    }
}

/// A box's signed distance from `p` (negative inside), the nearest point on its surface and the
/// outward normal there.
fn box_distance(c: Vec3, rot: Quat, half: Vec3, p: Vec3) -> (f32, Vec3, Vec3) {
    let l = rot.inverse() * (p - c);
    let q = l.clamp(-half, half);
    if q != l {
        let d = l - q;
        let len = d.length();
        return (len, c + rot * q, rot * (d / len));
    }
    // inside: out through the nearest face
    let gap = half - l.abs();
    let k = if gap.x <= gap.y && gap.x <= gap.z { 0 } else if gap.y <= gap.z { 1 } else { 2 };
    let mut n = Vec3::ZERO;
    n[k] = if l[k] < 0.0 { -1.0 } else { 1.0 };
    let mut on = l;
    on[k] = half[k] * n[k];
    (-gap[k], c + rot * on, rot * n)
}

/// A box's corners (world).
fn corners(c: Vec3, rot: Quat, h: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|k| c + rot * Vec3::new(if k & 1 == 0 { -h.x } else { h.x }, if k & 2 == 0 { -h.y } else { h.y }, if k & 4 == 0 { -h.z } else { h.z }))
}

/// How deep two placed shapes overlap: the overlap (positive when they interpenetrate), the
/// normal pushing the first out of the second, and the contact point on each.
fn overlap(s1: Placed, s2: Placed) -> (f32, Vec3, Vec3, Vec3) {
    // apart (their bounding spheres) by more than nothing: no nearer look
    let ((o1, r1), (o2, r2)) = (s1.bound(), s2.bound());
    let gap = o1.distance(o2) - r1 - r2;
    if gap > 0.0 {
        return (-gap, Vec3::Y, o1, o2);
    }
    match (s1, s2) {
        (Placed::Seg(a1, b1, r1), Placed::Seg(a2, b2, r2)) => {
            let (c1, c2) = closest(a1, b1, a2, b2);
            let d = c1 - c2;
            let dist = d.length();
            let n = if dist > 1e-6 { d / dist } else { Vec3::Y };
            (r1 + r2 - dist, n, c1 - n * r1, c2 + n * r2)
        }
        (Placed::Seg(a, b, r), Placed::Box(c, rot, half)) => {
            // the segment's point nearest the box: its signed distance is convex along the
            // segment, so a ternary search finds it
            let f = |t: f32| box_distance(c, rot, half, a.lerp(b, t)).0;
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            if a.distance_squared(b) > 1e-12 {
                for _ in 0..40 {
                    let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                    if f(m1) <= f(m2) {
                        hi = m2;
                    } else {
                        lo = m1;
                    }
                }
            }
            let p = a.lerp(b, (lo + hi) * 0.5);
            let (d, on, n) = box_distance(c, rot, half, p);
            (r - d, n, p - n * r, on)
        }
        (Placed::Box(..), Placed::Seg(..)) => {
            let (o, n, p2, p1) = overlap(s2, s1);
            (o, -n, p1, p2)
        }
        (Placed::Box(c1, q1, h1), Placed::Box(c2, q2, h2)) => {
            // separating axes: the faces' and the edges' crossings; the least overlap
            let (u1, u2) = ([q1 * Vec3::X, q1 * Vec3::Y, q1 * Vec3::Z], [q2 * Vec3::X, q2 * Vec3::Y, q2 * Vec3::Z]);
            let mut axes: Vec<Vec3> = u1.iter().chain(u2.iter()).copied().collect();
            for x in u1 {
                for y in u2 {
                    let a = x.cross(y);
                    if a.length_squared() > 1e-8 {
                        axes.push(a.normalize());
                    }
                }
            }
            let reach = |u: &[Vec3; 3], h: Vec3, l: Vec3| (0..3).map(|k| h[k] * u[k].dot(l).abs()).sum::<f32>();
            let d = c1 - c2;
            let (mut best, mut n) = (f32::MAX, Vec3::Y);
            for l in axes {
                let o = reach(&u1, h1, l) + reach(&u2, h2, l) - d.dot(l).abs();
                if o < best {
                    best = o;
                    n = if d.dot(l) < 0.0 { -l } else { l };
                }
            }
            // the touching features: each box's corners furthest along the normal into the other
            let feature = |cs: [Vec3; 8], dir: Vec3| {
                let m = cs.iter().map(|p| p.dot(dir)).fold(f32::MIN, f32::max);
                let near: Vec<Vec3> = cs.iter().copied().filter(|p| p.dot(dir) > m - 1e-3).collect();
                near
            };
            let (f1, f2) = (feature(corners(c1, q1, h1), -n), feature(corners(c2, q2, h2), n));
            let mean = |v: &[Vec3]| v.iter().copied().sum::<Vec3>() / v.len() as f32;
            let (p1, p2) = if f1.len() == 2 && f2.len() == 2 {
                closest(f1[0], f1[1], f2[0], f2[1])
            } else if f1.len() <= f2.len() {
                let p = mean(&f1);
                (p, p + n * best)
            } else {
                let p = mean(&f2);
                (p - n * best, p)
            };
            (best, n, p1, p2)
        }
    }
}

/// An angle wrapped into -pi..pi.
fn wrap(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

/// How far (radians, signed) an angle is past the nearer end of `low..high` going round the
/// circle, if outside it.
fn past_limit(angle: f32, low: f32, high: f32) -> Option<f32> {
    let a = wrap(angle);
    if (low..=high).contains(&a) {
        return None;
    }
    let (dl, dh) = (wrap(a - low), wrap(a - high));
    Some(if dl.abs() < dh.abs() { dl } else { dh })
}

fn add_rotation(q: Quat, w: Vec3) -> Quat {
    (q + Quat::from_xyzw(w.x, w.y, w.z, 0.0) * q * 0.5).normalize()
}

impl Ragdoll {
    /// Bodies at the given world poses (centre of mass, rotation from bind) moving at the given
    /// velocities.
    pub fn new(poses: &[(Vec3, Quat)], velocities: &[Vec3]) -> Ragdoll {
        let s = poses
            .iter()
            .enumerate()
            .map(|(i, &(x, q))| State {
                x,
                q,
                v: velocities.get(i).copied().unwrap_or(Vec3::ZERO),
                w: Vec3::ZERO,
                px: x,
                pq: q,
                pv: Vec3::ZERO,
                pw: Vec3::ZERO,
            })
            .collect();
        // a new body's wake-up counter (NxBodyDesc wakeUpCounter, F025969)
        let n = poses.len();
        Ragdoll {
            s,
            h: 1.0 / 360.0,
            pinned: Vec::new(),
            contacts: Vec::new(),
            wake_counter: WAKE_TIME,
            still: 0.0,
            rest: poses.to_vec(),
            asleep: false,
            accel: vec![Vec3::ZERO; n],
            warm: 0.0,
            settle_timer: 0.0,
        }
    }

    /// Move the bodies to meet their joints and limits (a starting pose from animation may
    /// break them) without setting them moving.
    pub fn fit(&mut self, t: &Template) {
        for _ in 0..20 {
            for j in &t.joints {
                self.joint(t, j);
            }
        }
        for s in &mut self.s {
            s.px = s.x;
            s.pq = s.q;
        }
    }

    /// Each body's world pose: centre of mass and rotation from bind.
    pub fn poses(&self) -> impl Iterator<Item = (Vec3, Quat)> + '_ {
        self.s.iter().map(|b| (b.x, b.q))
    }

    /// Each body's linear and angular speed.
    pub fn body_speeds(&self) -> Vec<(f32, f32)> {
        self.s.iter().map(|s| (s.v.length(), s.w.length())).collect()
    }

    /// The fastest body's linear and angular speed.
    pub fn speeds(&self) -> (f32, f32) {
        self.s.iter().fold((0.0f32, 0.0f32), |(v, w), s| (v.max(s.v.length()), w.max(s.w.length())))
    }

    /// Wake it for PhysX's default wake-up time (NxActor::wakeUp, 20 x 0.02 s).
    pub fn wake(&mut self) {
        self.wake_for(WAKE_TIME);
    }

    fn wake_for(&mut self, t: f32) {
        self.asleep = false;
        self.wake_counter = self.wake_counter.max(t);
        self.still = 0.0;
    }

    /// Move every body by the rigid transform `(rotation, translation)` (about the origin).
    pub fn carry_by(&mut self, r: Quat, t: Vec3) {
        for s in &mut self.s {
            s.x = r * s.x + t;
            s.q = (r * s.q).normalize();
            s.v = r * s.v;
            s.w = r * s.w;
            s.px = s.x;
            s.pq = s.q;
        }
        self.wake();
    }

    /// Hold body `i` at a pose (centre of mass, rotation from bind), moved there over the next
    /// step: it pushes the others and is not pushed itself.
    pub fn pin(&mut self, i: usize, x: Vec3, q: Quat) {
        if let Some(p) = self.pinned.iter_mut().find(|p| p.0 == i) {
            p.1 = x;
            p.2 = q;
        } else {
            self.pinned.push((i, x, q));
        }
        self.wake_for(HELD_WAKE);
    }

    /// Let every pinned body go.
    pub fn unpin(&mut self) {
        self.pinned.clear();
    }

    fn is_pinned(&self, i: usize) -> bool {
        self.pinned.iter().any(|p| p.0 == i)
    }

    fn inv_mass(&self, t: &Template, i: usize) -> f32 {
        if self.is_pinned(i) { 0.0 } else { t.bodies[i].inv_mass }
    }

    fn inv_inertia(&self, t: &Template, i: usize) -> Mat3 {
        if self.is_pinned(i) {
            return Mat3::ZERO;
        }
        let r = Mat3::from_quat(self.s[i].q);
        r * t.bodies[i].inv_inertia * r.transpose()
    }

    /// Move body `a` along `corr` (and `b`, if any, against it) at the world offsets `ra` / `rb`
    /// from their centres, shared by their generalised inverse masses.
    fn positional(&mut self, t: &Template, a: usize, b: Option<usize>, corr: Vec3, ra: Vec3, rb: Vec3) {
        let c = corr.length();
        if c < 1e-7 {
            return;
        }
        let n = corr / c;
        let ia = self.inv_inertia(t, a);
        let wa = self.inv_mass(t, a) + ra.cross(n).dot(ia * ra.cross(n));
        let (ib, wb) = match b {
            Some(b) => {
                let ib = self.inv_inertia(t, b);
                (ib, self.inv_mass(t, b) + rb.cross(n).dot(ib * rb.cross(n)))
            }
            None => (Mat3::ZERO, 0.0),
        };
        if wa + wb <= 0.0 {
            return;
        }
        let p = n * (c / (wa + wb));
        let ma = self.inv_mass(t, a);
        let mb = b.map_or(0.0, |b| self.inv_mass(t, b));
        let s = &mut self.s[a];
        s.x += p * ma;
        s.q = add_rotation(s.q, ia * ra.cross(p));
        if let Some(b) = b {
            let s = &mut self.s[b];
            s.x -= p * mb;
            s.q = add_rotation(s.q, -(ib * rb.cross(p)));
        }
    }

    /// Turn body `a` by the rotation vector `dphi` and `b` against it, shared by their inverse
    /// inertias about its axis.
    fn angular(&mut self, t: &Template, a: usize, b: usize, dphi: Vec3) {
        let th = dphi.length();
        if th < 1e-7 {
            return;
        }
        let n = dphi / th;
        let ia = self.inv_inertia(t, a);
        let ib = self.inv_inertia(t, b);
        let w = n.dot(ia * n) + n.dot(ib * n);
        if w <= 0.0 {
            return;
        }
        let p = n * (th / w);
        self.s[a].q = add_rotation(self.s[a].q, ia * p);
        self.s[b].q = add_rotation(self.s[b].q, -(ib * p));
    }

    fn joint(&mut self, t: &Template, j: &JointDef) {
        let (a, b) = (j.a, j.b);
        // the anchors meet
        let pa = self.s[a].x + self.s[a].q * j.ra;
        let pb = self.s[b].x + self.s[b].q * j.rb;
        self.positional(t, a, Some(b), pb - pa, pa - self.s[a].x, pb - self.s[b].x);
        let axes = |r: &Self, f: Quat, i: usize, v: Vec3| r.s[i].q * f * v;
        match j.kind {
            JointKind::Hinge => {
                // the hinge axes (second rows) line up, then the turn about it stays in limits
                let a1 = axes(self, j.fa, a, Vec3::Y);
                let b1 = axes(self, j.fb, b, Vec3::Y);
                self.angular(t, a, b, a1.cross(b1));
                let n = axes(self, j.fa, a, Vec3::Y);
                let ua = axes(self, j.fa, a, Vec3::X);
                let ub = axes(self, j.fb, b, Vec3::X);
                let phi = ua.cross(ub).dot(n).atan2(ua.dot(ub));
                if let Some(d) = past_limit(phi, j.low, j.high).filter(|_| j.limited) {
                    self.angular(t, a, b, n * d);
                }
                if let Some((k, target, d)) = j.twist_spring {
                    self.spring(t, a, b, n * wrap(phi - target), k);
                    self.damper(t, a, b, n, d);
                }
            }
            JointKind::Ball => {
                // swing: the twist axes (first rows) stay within the cone
                let ua = axes(self, j.fa, a, Vec3::X);
                let ub = axes(self, j.fb, b, Vec3::X);
                let angle = ua.dot(ub).clamp(-1.0, 1.0).acos();
                if angle > j.swing && j.limited {
                    let n = ua.cross(ub).normalize_or_zero();
                    self.angular(t, a, b, n * (angle - j.swing));
                }
                if let Some((k, d)) = j.swing_spring {
                    let (ua, ub) = (axes(self, j.fa, a, Vec3::X), axes(self, j.fb, b, Vec3::X));
                    let angle = ua.dot(ub).clamp(-1.0, 1.0).acos();
                    let axis = ua.cross(ub).normalize_or_zero();
                    self.spring(t, a, b, axis * angle, k);
                    self.damper(t, a, b, axis, d);
                }
                // twist about that axis
                let rel = (self.s[a].q * j.fa).inverse() * (self.s[b].q * j.fb);
                let tw = 2.0 * rel.x.atan2(rel.w);
                let n = axes(self, j.fb, b, Vec3::X);
                if let Some(d) = past_limit(tw, j.low, j.high).filter(|_| j.limited) {
                    self.angular(t, a, b, n * d);
                }
                if let Some((k, target, d)) = j.twist_spring {
                    self.spring(t, a, b, n * wrap(tw - target), k);
                    self.damper(t, a, b, n, d);
                }
            }
            JointKind::Other(_) => {}
        }
    }

    /// A joint spring of stiffness `k` turning `a` by `dphi` and `b` against it, as a soft
    /// constraint (compliance 1 / k over the sub-step).
    fn spring(&mut self, t: &Template, a: usize, b: usize, dphi: Vec3, k: f32) {
        let th = dphi.length();
        if th < 1e-7 || k <= 0.0 {
            return;
        }
        let n = dphi / th;
        let w = n.dot(self.inv_inertia(t, a) * n) + n.dot(self.inv_inertia(t, b) * n);
        let compliance = 1.0 / (k * self.h * self.h);
        if w > 0.0 {
            self.angular(t, a, b, dphi * (w / (w + compliance)));
        }
    }

    /// A joint spring's damper (NxSpringDesc damper: a torque of `d` times the bodies' relative
    /// turning speed about `n`, against it), taken implicitly over the sub-step: the speed
    /// scaled by 1 / (1 + d h w), w the pair's inverse inertia about `n`.
    fn damper(&mut self, t: &Template, a: usize, b: usize, n: Vec3, d: f32) {
        if d <= 0.0 || n == Vec3::ZERO {
            return;
        }
        let w = n.dot(self.inv_inertia(t, a) * n) + n.dot(self.inv_inertia(t, b) * n);
        if w <= 0.0 {
            return;
        }
        // the joint's angle closes as `a` turns about `n` against `b`
        let rate = (self.s[b].w - self.s[a].w).dot(n);
        let x = d * self.h * w;
        self.angular(t, a, b, n * (rate * self.h * x / (1.0 + x)));
    }

    /// Bodies `i` and `j` pushed apart where their shapes overlap more than their skin.
    fn touch(&mut self, t: &Template, i: usize, j: usize) {
        let skin = t.bodies[i].skin.max(t.bodies[j].skin);
        for c1 in &t.bodies[i].colliders {
            for c2 in &t.bodies[j].colliders {
                let (si, sj) = (self.s[i], self.s[j]);
                let (o, n, p1, p2) = overlap(c1.placed(si.x, si.q), c2.placed(sj.x, sj.q));
                let depth = o - skin;
                if depth <= 0.0 {
                    continue;
                }
                self.positional(t, i, Some(j), n * depth, p1 - si.x, p2 - sj.x);
            }
        }
    }

    /// A sphere at `p` (world) of radius `r` on body `i` against the ground.
    fn contact(&mut self, t: &Template, i: usize, p: Vec3, r: f32, world: &dyn Surroundings, found: &mut Vec<(Vec3, f32)>) {
        let g = world.ground(p.x, p.z);
        if p.y - r <= g + 0.5 {
            let n = Vec3::new(
                world.ground(p.x - 1.0, p.z) - world.ground(p.x + 1.0, p.z),
                2.0,
                world.ground(p.x, p.z - 1.0) - world.ground(p.x, p.z + 1.0),
            )
            .normalize_or(Vec3::Y);
            let depth = (g - (p.y - r)) * n.y;
            if depth > 0.0 {
                self.push_out(t, i, p - n * r, n, depth);
            }
        }
        found.clear();
        world.touches(p, r, found);
        for &(n, depth) in found.iter() {
            self.push_out(t, i, p - n * r, n, depth);
        }
    }

    /// Body `i` touching along `n`, `depth` deep at the contact point `c`: pushed out to its skin,
    /// held by friction. A contact inside the skin is still one (PhysX's solver lets shapes close
    /// to skin depth, no further, and pushes out what is past it): it closes no faster than
    /// reaches the skin this sub-step and holds by friction. (stand-in: a position solver has no
    /// contact force; PhysX's is its closed solver's) The depth stands for the contact's load in
    /// the friction cone.
    fn push_out(&mut self, t: &Template, i: usize, c: Vec3, n: Vec3, depth: f32) {
        let load = depth.min(MAX_DEPENETRATION * self.h);
        if load <= 0.0 {
            return;
        }
        let out = (depth - t.bodies[i].skin).min(MAX_DEPENETRATION * self.h);
        let s = self.s[i];
        if out > 0.0 {
            self.positional(t, i, None, n * out, c - s.x, Vec3::ZERO);
        }
        self.contacts.push((i, s.q.inverse() * (c - s.x), n, load, (t.bodies[i].skin - depth).max(0.0)));
        // friction: the contact point's slide this sub-step undone, up to the friction cone
        let s = self.s[i];
        let local = s.q.inverse() * (c - s.x);
        let before = s.px + s.pq * local;
        let now = s.x + s.q * local;
        let d = now - before;
        let slide = d - n * d.dot(n);
        let len = slide.length();
        if len > 1e-7 {
            let undo = if len < STATIC_FRICTION * load { 1.0 } else { (DYNAMIC_FRICTION * load / len).min(1.0) };
            self.positional(t, i, None, -slide * undo, now - s.x, Vec3::ZERO);
        }
    }

    /// Advance `dt` seconds among `world`'s ground and solids: one PhysX step of at most
    /// `MAX_STEP` (F045111), solved in the template's sub-steps.
    pub fn step(&mut self, t: &Template, dt: f32, world: &dyn Surroundings) {
        Ragdoll::step_together(&mut [self], &[t], dt, &[world]);
    }

    /// Advance several ragdolls `dt` seconds together, each among its own surroundings: they
    /// collide with each other as well (F045103 leaves the ragdolls' collision groups, 3 and 4
    /// (F038930), colliding with themselves and each other). Ragdolls touching are one island,
    /// solved together in the most sub-steps any of them takes (as `MIN_ITERATIONS`); a sleeping
    /// one touched by an awake one is woken (PhysX 2.8 wakes a sleeping actor an awake one
    /// touches).
    pub fn step_together(dolls: &mut [&mut Ragdoll], ts: &[&Template], dt: f32, worlds: &[&dyn Surroundings]) {
        if dt <= 0.0 {
            return;
        }
        let n = dolls.len();
        // islands: the awake ragdolls and what they touch
        let mut island: Vec<usize> = (0..n).collect();
        fn root(island: &mut [usize], mut i: usize) -> usize {
            while island[i] != i {
                island[i] = island[island[i]];
                i = island[i];
            }
            i
        }
        let reach: Vec<(Vec3, f32)> = (0..n).map(|i| dolls[i].reach(ts[i])).collect();
        for i in 0..n {
            for j in i + 1..n {
                if dolls[i].asleep && dolls[j].asleep {
                    continue;
                }
                let ((ci, ri), (cj, rj)) = (reach[i], reach[j]);
                if ci.distance(cj) > ri + rj || !Ragdoll::touching(dolls[i], ts[i], dolls[j], ts[j]) {
                    continue;
                }
                for k in [i, j] {
                    if dolls[k].asleep {
                        dolls[k].wake();
                    }
                }
                let (a, b) = (root(&mut island, i), root(&mut island, j));
                island[a.max(b)] = a.min(b);
            }
        }
        for first in 0..n {
            if root(&mut island, first) != first {
                continue;
            }
            let members: Vec<usize> = (first..n).filter(|&k| root(&mut island, k) == first && !dolls[k].asleep).collect();
            if members.is_empty() {
                continue;
            }
            let dt = dt.min(MAX_STEP);
            let substeps = members.iter().map(|&k| ts[k].substeps).max().unwrap_or(1).max(1);
            let h = dt / substeps as f32;
            let mut starts = Vec::with_capacity(members.len());
            let mut pins = Vec::with_capacity(members.len());
            for &k in &members {
                let (start, pin) = dolls[k].begin(ts[k], dt, h);
                starts.push(start);
                pins.push(pin);
            }
            let mut found = Vec::new();
            for sub in 0..substeps {
                for (m, &k) in members.iter().enumerate() {
                    dolls[k].integrate(ts[k], sub, substeps, &pins[m]);
                }
                for &k in &members {
                    dolls[k].solve_inside(ts[k]);
                }
                for (x, &a) in members.iter().enumerate() {
                    for &b in &members[x + 1..] {
                        let (da, db) = two(dolls, a, b);
                        Ragdoll::touch_between(da, ts[a], db, ts[b]);
                    }
                }
                for &k in &members {
                    dolls[k].solve_world(ts[k], worlds[k], &mut found);
                    dolls[k].velocities(ts[k]);
                }
            }
            for (m, &k) in members.iter().enumerate() {
                dolls[k].settle(ts[k], dt, &starts[m]);
            }
        }
    }

    /// A sphere holding every body's shapes: centre and radius (units).
    fn reach(&self, t: &Template) -> (Vec3, f32) {
        let c = self.s.iter().map(|s| s.x).sum::<Vec3>() / self.s.len().max(1) as f32;
        let r = self
            .s
            .iter()
            .zip(&t.bodies)
            .map(|(s, b)| {
                b.colliders.iter().map(|k| {
                    let (o, r) = k.placed(s.x, s.q).bound();
                    o.distance(c) + r
                })
                .fold(0.0, f32::max)
                    + b.skin
            })
            .fold(0.0, f32::max);
        (c, r)
    }

    /// Whether any body of `a` touches any of `b`.
    fn touching(a: &Ragdoll, ta: &Template, b: &Ragdoll, tb: &Template) -> bool {
        a.s.iter().zip(&ta.bodies).any(|(sa, ba)| {
            b.s.iter().zip(&tb.bodies).any(|(sb, bb)| {
                ba.colliders.iter().any(|c1| bb.colliders.iter().any(|c2| overlap(c1.placed(sa.x, sa.q), c2.placed(sb.x, sb.q)).0 > 0.0))
            })
        })
    }

    /// The bodies of two ragdolls pushed apart where their shapes overlap more than their skin
    /// (as `touch` within one).
    fn touch_between(ra: &mut Ragdoll, ta: &Template, rb: &mut Ragdoll, tb: &Template) {
        for i in 0..ra.s.len() {
            for j in 0..rb.s.len() {
                let skin = ta.bodies[i].skin.max(tb.bodies[j].skin);
                for c1 in &ta.bodies[i].colliders {
                    for c2 in &tb.bodies[j].colliders {
                        let (si, sj) = (ra.s[i], rb.s[j]);
                        let (o, n, p1, p2) = overlap(c1.placed(si.x, si.q), c2.placed(sj.x, sj.q));
                        let depth = o - skin;
                        if depth <= 0.0 {
                            continue;
                        }
                        let (oa, ob) = (p1 - si.x, p2 - sj.x);
                        // as `positional`, the two bodies in different ragdolls
                        let (ia, ib) = (ra.inv_inertia(ta, i), rb.inv_inertia(tb, j));
                        let (ma, mb) = (ra.inv_mass(ta, i), rb.inv_mass(tb, j));
                        let wa = ma + oa.cross(n).dot(ia * oa.cross(n));
                        let wb = mb + ob.cross(n).dot(ib * ob.cross(n));
                        if wa + wb <= 0.0 {
                            continue;
                        }
                        let p = n * (depth / (wa + wb));
                        let s = &mut ra.s[i];
                        s.x += p * ma;
                        s.q = add_rotation(s.q, ia * oa.cross(p));
                        let s = &mut rb.s[j];
                        s.x -= p * mb;
                        s.q = add_rotation(s.q, -(ib * ob.cross(p)));
                    }
                }
            }
        }
    }

    /// A step's start: its sub-step `h`, the damping (once a step), and where each body and pin
    /// starts.
    #[allow(clippy::type_complexity)]
    fn begin(&mut self, t: &Template, dt: f32, h: f32) -> (Vec<(Vec3, Quat)>, Vec<(usize, Vec3, Quat, Vec3, Quat)>) {
        self.h = h;
        // (assumption: PhysX 2.8's integrator is closed and the SDK gives no formula) the
        // damping, once a step: v *= max(0, 1 - damping dt)
        for (i, s) in self.s.iter_mut().enumerate() {
            let [lin, ang] = t.bodies[i].damping;
            s.v *= (1.0 - lin * dt).max(0.0);
            s.w *= (1.0 - ang * dt).max(0.0);
        }
        let start: Vec<(Vec3, Quat)> = self.s.iter().map(|s| (s.x, s.q)).collect();
        // a pinned body goes to its pose in even steps
        let pins = self.pinned.iter().map(|&(i, x, q)| (i, self.s[i].x, self.s[i].q, x, q)).collect();
        (start, pins)
    }

    /// Sub-step `k` of `substeps`: the bodies moved on by their velocities and gravity, the
    /// pinned ones along to their poses.
    fn integrate(&mut self, t: &Template, k: usize, substeps: usize, pins: &[(usize, Vec3, Quat, Vec3, Quat)]) {
        let h = self.h;
        for (i, s) in self.s.iter_mut().enumerate() {
            s.px = s.x;
            s.pq = s.q;
            if let Some(&(_, x0, q0, x1, q1)) = pins.iter().find(|p| p.0 == i) {
                let f = (k + 1) as f32 / substeps as f32;
                s.x = x0.lerp(x1, f);
                s.q = q0.slerp(q1, f);
                continue;
            }
            if t.bodies[i].inv_mass == 0.0 {
                continue;
            }
            if t.bodies[i].gravity {
                s.v += GRAVITY * h;
            }
            s.v += self.accel[i] * h;
            s.pv = s.v;
            s.pw = s.w;
            s.x += s.v * h;
            s.q = add_rotation(s.q, s.w * h);
        }
    }

    /// The joints and the bodies' contacts with each other.
    fn solve_inside(&mut self, t: &Template) {
        for j in &t.joints {
            self.joint(t, j);
        }
        for &(i, j) in &t.pairs {
            self.touch(t, i, j);
        }
    }

    /// The bodies' contacts with the ground and the solids.
    fn solve_world(&mut self, t: &Template, world: &dyn Surroundings, found: &mut Vec<(Vec3, f32)>) {
        for i in 0..self.s.len() {
            let (x, q) = (self.s[i].x, self.s[i].q);
            for c in t.bodies[i].colliders.clone() {
                match c {
                    Collider::Sphere { c, r } => self.contact(t, i, x + q * c, r, world, found),
                    Collider::Capsule { a, b, r } => {
                        for p in [a, b, (a + b) * 0.5] {
                            self.contact(t, i, x + q * p, r, world, found);
                        }
                    }
                    Collider::Box { c, rot, half } => {
                        // its corners as small spheres just inside them
                        let r = CORNER.min(half.min_element());
                        let h = half - Vec3::splat(r);
                        for k in 0..8 {
                            let corner = Vec3::new(
                                if k & 1 == 0 { -h.x } else { h.x },
                                if k & 2 == 0 { -h.y } else { h.y },
                                if k & 4 == 0 { -h.z } else { h.z },
                            );
                            self.contact(t, i, x + q * (c + rot * corner), r, world, found);
                        }
                    }
                }
            }
        }
    }

    /// A sub-step's end: the velocities from the moves, then the contacts' (bounce, friction).
    fn velocities(&mut self, t: &Template) {
        let h = self.h;
        for s in &mut self.s {
            s.v = (s.x - s.px) / h;
            let dq = s.q * s.pq.inverse();
            s.w = Vec3::new(dq.x, dq.y, dq.z) * (2.0 / h);
            if dq.w < 0.0 {
                s.w = -s.w;
            }
        }
        // the contacts' velocities: one closing faster than the bounce threshold bounces back
        // at the restitution, others stop closing; sliding slowed by dynamic friction up to
        // the normal correction's speed
        for (i, local, n, depth, slack) in std::mem::take(&mut self.contacts) {
            let s = self.s[i];
            let r = s.q * local;
            let v = s.v + s.w.cross(r);
            let vn = v.dot(n);
            let before = (s.pv + s.pw.cross(r)).dot(n);
            let target = if before < -BOUNCE_THRESHOLD { -RESTITUTION * before } else { -slack / h };
            let vt = v - n * vn;
            let mut dv = Vec3::ZERO;
            let lt = vt.length();
            if lt > 1e-7 {
                dv -= vt / lt * (DYNAMIC_FRICTION * depth / h).min(lt);
            }
            if vn < target {
                dv += n * (target - vn);
            }
            let l = dv.length();
            if l < 1e-7 {
                continue;
            }
            let d = dv / l;
            let ii = self.inv_inertia(t, i);
            let w = self.inv_mass(t, i) + r.cross(d).dot(ii * r.cross(d));
            if w <= 0.0 {
                continue;
            }
            let p = dv / w;
            let m = self.inv_mass(t, i);
            let s = &mut self.s[i];
            s.v += p * m;
            s.w += ii * r.cross(p);
        }
        for s in &mut self.s {
            s.w = s.w.clamp_length_max(MAX_ANGULAR);
        }
    }

    /// Body `i` pushed with acceleration `a` over the next step (NX_ACCELERATION).
    /// PhysX 2.8's addForce wakes the body by default (wakeup true), as Kenshi leaves it.
    pub fn accelerate(&mut self, i: usize, a: Vec3) {
        if let Some(x) = self.accel.get_mut(i) {
            *x += a;
            if self.asleep {
                self.wake();
            }
        }
    }

    /// @fn F044957
    ///
    /// Kenshi's settling, once a frame of `dt` (s): see `SETTLE_AFTER`. `floor_below(from)` is
    /// the height a ray straight down from `from` meets (`SETTLE_RAY_GROUPS`), or
    /// `SETTLE_NO_HIT` in the caller's frame. True when it put the ragdoll to sleep: the caller
    /// then draws Kenshi's rand()/32768 for `settle_again`.
    pub fn kenshi_settle(&mut self, dt: f32, floor_below: impl FnOnce(Vec3) -> f32) -> bool {
        if self.asleep {
            return false;
        }
        if self.warm < SETTLE_WARM {
            self.warm += dt;
            return false;
        }
        self.settle_timer += dt;
        if self.settle_timer <= SETTLE_AFTER {
            return false;
        }
        let Some(root) = self.s.first().map(|s| s.x) else { return false };
        let from = root + Vec3::Y * 20.0;
        if (from.y - floor_below(from)).abs() <= SETTLE_REACH {
            self.put_to_sleep();
            true
        } else {
            self.settle_timer = 0.0;
            false
        }
    }

    /// F044957 after putting it to sleep: the settle timer starts again at 3 + `rand01`.
    pub fn settle_again(&mut self, rand01: f32) {
        self.settle_timer = rand01 + 3.0;
    }

    /// Body `i`'s actor's place (its frame's origin, NxActor::getGlobalPosition), not its
    /// centre of mass.
    pub fn actor_position(&self, t: &Template, i: usize) -> Option<Vec3> {
        let s = self.s.get(i)?;
        Some(s.x + s.q * t.bodies.get(i)?.origin)
    }

    /// F044952: every body put to sleep.
    pub fn put_to_sleep(&mut self) {
        self.asleep = true;
        self.wake_counter = 0.0;
        for s in &mut self.s {
            s.v = Vec3::ZERO;
            s.w = Vec3::ZERO;
        }
    }

    /// A step's end: the sleep test.
    fn settle(&mut self, t: &Template, dt: f32, start: &[(Vec3, Quat)]) {
        for a in &mut self.accel {
            *a = Vec3::ZERO;
        }
        // the sleep test: any body's energy at the threshold keeps the ragdoll awake for the
        // wake-up time; below it the counter runs down, and at 0 it sleeps. (stand-in for the
        // velocity filter Kenshi turns on, NX_BF_FILTER_SLEEP_VEL, F045312: the SDK says only
        // that it filters the velocities the sleep test uses) Each body's velocity here
        // is its motion over the whole step, not its last sub-step's.
        let energetic = self.s.iter().zip(&t.bodies).zip(start).any(|((s, b), &(x0, q0))| {
            let v = (s.x - x0) / dt;
            let dq = s.q * q0.inverse();
            let w = Vec3::new(dq.x, dq.y, dq.z) * (2.0 / dt) * dq.w.signum();
            let r = Mat3::from_quat(s.q);
            0.5 * (v.length_squared() + w.dot(r * b.unit_inertia * r.transpose() * w)) >= SLEEP_ENERGY
        });
        if energetic {
            self.wake_counter = self.wake_counter.max(WAKE_TIME);
        } else {
            self.wake_counter -= dt;
        }
        let moved = self
            .s
            .iter()
            .zip(&self.rest)
            .any(|(s, &(x, q))| s.x.distance(x) > STILL_DRIFT || s.q.angle_between(q) > STILL_TURN);
        if moved || !self.pinned.is_empty() {
            self.still = 0.0;
            self.rest = self.s.iter().map(|s| (s.x, s.q)).collect();
        } else {
            self.still += dt;
        }
        if self.wake_counter <= 0.0 || self.still >= WAKE_TIME {
            self.asleep = true;
            for s in &mut self.s {
                s.v = Vec3::ZERO;
                s.w = Vec3::ZERO;
            }
        }
    }
}

/// Two different ragdolls of a list, both mutable.
fn two<'a>(v: &'a mut [&mut Ragdoll], a: usize, b: usize) -> (&'a mut Ragdoll, &'a mut Ragdoll) {
    if a < b {
        let (l, r) = v.split_at_mut(b);
        (&mut *l[a], &mut *r[0])
    } else {
        let (l, r) = v.split_at_mut(a);
        (&mut *r[0], &mut *l[b])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenshi_io::phs::{Actor, Joint, Shape};

    fn frame(pos: [f32; 3]) -> Frame {
        Frame { pos, rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] }
    }

    fn capsule(name: &str, at: [f32; 3]) -> Actor {
        Actor {
            name: name.into(),
            pos: at,
            damping: [0.0, 0.05],
            density: 1.0,
            mass: 0.0,
            sleep: [-1.0, -1.0],
            iterations: 32,
            fixed: false,
            no_gravity: false,
            bone_slot: 0,
            bone: name.into(),
            bone_frame: frame(at),
            offsets: Vec::new(),
            shapes: vec![Shape {
                frame: frame(at),
                kind: ShapeKind::Capsule,
                size: [1.0, 4.0, 1.0],
                density: 1.0,
                mass: 0.0,
                skin: -1.0,
            }],
        }
    }

    #[test]
    fn falls_and_rests_on_the_ground() {
        let p = Phs { version: 1.4, actors: vec![capsule("a", [0.0, 10.0, 0.0])], joints: vec![], lights: vec![] };
        let t = Template::new(&p);
        let mut r = Ragdoll::new(&[(t.bodies[0].bind, Quat::IDENTITY)], &[]);
        for _ in 0..300 {
            r.step(&t, 1.0 / 60.0, &crate::world::Ground(|_, _| 0.0));
        }
        let (x, _) = r.poses().next().unwrap();
        // upright on its end: the centre half the length up, less the skin it rests in
        assert!((x.y - (2.0 - SKIN)).abs() < 0.1, "{x}");
        // and the energy test has put it to sleep
        assert!(r.asleep);
    }

    #[test]
    fn kenshi_settles_a_ragdoll_over_a_floor() {
        // F044957: 2 s left alone, then past 5 s on its timer it is put to sleep when the ray
        // down meets a floor within 100 units; over nothing the timer starts again
        let mut over_floor = Ragdoll::new(&[(Vec3::new(0.0, 50.0, 0.0), Quat::IDENTITY)], &[]);
        let mut over_nothing = over_floor.clone();
        let dt = 1.0 / 60.0;
        let mut slept_at = None;
        for k in 0..600 {
            if over_floor.kenshi_settle(dt, |_| 0.0) && slept_at.is_none() {
                slept_at = Some(k as f32 * dt);
            }
            assert!(!over_nothing.kenshi_settle(dt, |_| SETTLE_NO_HIT - 1000.0));
        }
        let t = slept_at.unwrap();
        assert!((t - (SETTLE_WARM + SETTLE_AFTER)).abs() < 0.05, "{t}");
        assert!(over_floor.asleep && !over_nothing.asleep);
        // a push (addForce) wakes it
        over_floor.accelerate(0, Vec3::Y);
        assert!(!over_floor.asleep);
    }

    #[test]
    fn capsule_inertia() {
        // no cylinder: a sphere's 2/5 r^2
        let i = unit_inertia(ShapeKind::Capsule, Vec3::new(2.0, 2.0, 2.0));
        assert!((i - Vec3::splat(0.4)).abs().max_element() < 1e-5, "{i}");
        // against a fine sum of thin discs along a capsule of radius 1, length 6
        let (r, len) = (1.0f32, 6.0f32);
        let (mut m, mut side, mut along) = (0.0f32, 0.0f32, 0.0f32);
        let n = 20000;
        for k in 0..n {
            let y = -len / 2.0 + (k as f32 + 0.5) * len / n as f32;
            let c = (len / 2.0 - r).max(0.0);
            let rr = if y.abs() <= c { r } else { (r * r - (y.abs() - c).powi(2)).max(0.0).sqrt() };
            let dm = rr * rr;
            m += dm;
            along += dm * rr * rr / 2.0;
            side += dm * (rr * rr / 4.0 + y * y);
        }
        let i = unit_inertia(ShapeKind::Capsule, Vec3::new(2.0 * r, len, 2.0 * r));
        assert!((i.y - along / m).abs() < 1e-3 && (i.x - side / m).abs() < 1e-3, "{i} {} {}", along / m, side / m);
    }

    #[test]
    fn spring_dampers_slow_the_joint() {
        // two capsules side by side on a hinge, the second spun about it; its damper slows it
        let spin = |damper: f32| {
            let mut a = capsule("a", [0.0, 20.0, 0.0]);
            a.fixed = true;
            let p = Phs {
                version: 1.4,
                actors: vec![a, capsule("b", [0.0, 16.0, 0.0])],
                joints: vec![Joint {
                    frame: Frame { pos: [0.0, 18.0, 0.0], rows: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]] },
                    kind: JointKind::Hinge,
                    high: 0.0,
                    low: 0.0,
                    swing: 0.0,
                    actors: [0, 1],
                    limited: false,
                    collide: false,
                    springs: [Some(kenshi_io::phs::Spring { target: 0.0, spring: 0.0, damper }), None, None],
                    projection: (0, 0.0, 0.0),
                }],
                lights: vec![],
            };
            let t = Template::new(&p);
            let poses: Vec<_> = t.bodies.iter().map(|b| (b.bind, Quat::IDENTITY)).collect();
            let mut r = Ragdoll::new(&poses, &[Vec3::ZERO, Vec3::ZERO]);
            r.s[1].w = Vec3::new(3.0, 0.0, 0.0);
            let none = crate::world::Ground(|_, _| -1000.0);
            for _ in 0..10 {
                r.step(&t, 1.0 / 60.0, &none);
            }
            r.s[1].w.length()
        };
        let (free, damped) = (spin(0.0), spin(50.0));
        assert!(damped < free * 0.8, "{free} {damped}");
    }

    #[test]
    fn shapes_overlap_as_they_are() {
        let unit = Placed::Box(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        // a sphere over the top face, 0.25 into it
        let (o, n, p1, p2) = overlap(Placed::Seg(Vec3::new(0.3, 1.75, 0.0), Vec3::new(0.3, 1.75, 0.0), 1.0), unit);
        assert!((o - 0.25).abs() < 1e-4 && n.abs_diff_eq(Vec3::Y, 1e-4), "{o} {n}");
        assert!(p1.abs_diff_eq(Vec3::new(0.3, 0.75, 0.0), 1e-3) && p2.abs_diff_eq(Vec3::new(0.3, 1.0, 0.0), 1e-3), "{p1} {p2}");
        // a capsule lying across the top, its middle nearest
        let (o, n, ..) = overlap(Placed::Seg(Vec3::new(-3.0, 1.4, 0.2), Vec3::new(3.0, 1.4, 0.2), 0.5), unit);
        assert!((o - 0.1).abs() < 1e-3 && n.abs_diff_eq(Vec3::Y, 1e-4), "{o} {n}");
        // the box first: the same, turned round
        let (o2, n2, ..) = overlap(unit, Placed::Seg(Vec3::new(-3.0, 1.4, 0.2), Vec3::new(3.0, 1.4, 0.2), 0.5));
        assert!((o2 - o).abs() < 1e-5 && n2.abs_diff_eq(-n, 1e-5));
        // a box resting a corner 0.2 into the top of another: the corner is the contact
        let tilted = Placed::Box(Vec3::new(0.0, 1.0 + 3f32.sqrt() - 0.2, 0.0), Quat::from_rotation_arc(Vec3::ONE.normalize(), Vec3::Y), Vec3::ONE);
        let (o, n, p1, _) = overlap(tilted, unit);
        assert!((o - 0.2).abs() < 1e-3 && n.abs_diff_eq(Vec3::Y, 1e-3), "{o} {n}");
        assert!(p1.abs_diff_eq(Vec3::new(0.0, 0.8, 0.0), 1e-3), "{p1}");
        // apart
        assert!(overlap(Placed::Box(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY, Vec3::ONE), unit).0 < 0.0);
    }

    #[test]
    fn ragdolls_stack_on_each_other() {
        // one capsule standing on the ground, another dropped onto it: it comes to rest on top
        let p = Phs { version: 1.4, actors: vec![capsule("a", [0.0, 0.0, 0.0])], joints: vec![], lights: vec![] };
        let t = Template::new(&p);
        let mut low = Ragdoll::new(&[(Vec3::new(0.0, 2.0 - SKIN, 0.0), Quat::IDENTITY)], &[]);
        let mut high = Ragdoll::new(&[(Vec3::new(0.0, 7.0, 0.0), Quat::IDENTITY)], &[]);
        let ground = crate::world::Ground(|_, _| 0.0);
        for _ in 0..300 {
            Ragdoll::step_together(&mut [&mut low, &mut high], &[&t, &t], 1.0 / 60.0, &[&ground, &ground]);
        }
        let (a, b) = (low.poses().next().unwrap().0, high.poses().next().unwrap().0);
        assert!(b.y - a.y > 3.0, "{a} {b}");
        // apart, each steps as alone
        let mut alone = Ragdoll::new(&[(Vec3::new(0.0, 7.0, 0.0), Quat::IDENTITY)], &[]);
        let mut far = Ragdoll::new(&[(Vec3::new(50.0, 7.0, 0.0), Quat::IDENTITY)], &[]);
        let mut near = Ragdoll::new(&[(Vec3::new(0.0, 7.0, 0.0), Quat::IDENTITY)], &[]);
        for _ in 0..60 {
            alone.step(&t, 1.0 / 60.0, &ground);
            Ragdoll::step_together(&mut [&mut near, &mut far], &[&t, &t], 1.0 / 60.0, &[&ground, &ground]);
        }
        assert_eq!(alone.poses().next(), near.poses().next());
    }

    #[test]
    fn mass_from_the_body_or_the_density() {
        // the body's mass when set (the shapes scaled to it), else density x volume
        let mut a = capsule("a", [0.0, 10.0, 0.0]);
        let free = Template::new(&Phs { version: 1.4, actors: vec![a.clone()], joints: vec![], lights: vec![] });
        let v = volume(ShapeKind::Capsule, Vec3::new(1.0, 4.0, 1.0));
        assert!((1.0 / free.bodies[0].inv_mass - v).abs() < 1e-3);
        a.density = 0.0;
        a.mass = 1.0;
        let set = Template::new(&Phs { version: 1.4, actors: vec![a], joints: vec![], lights: vec![] });
        assert!((1.0 / set.bodies[0].inv_mass - 1.0).abs() < 1e-5);
        // a ragdoll part's damping (F045312) and at least 16 iterations
        assert_eq!(set.bodies[0].damping, PART_DAMPING);
        assert_eq!(set.substeps, 32);
    }

    #[test]
    fn hinge_keeps_its_anchor_and_limit() {
        // two capsules hanging in a line, the lower hinged 0..90 degrees about X
        let p = Phs {
            version: 1.4,
            actors: vec![capsule("a", [0.0, 20.0, 0.0]), capsule("b", [0.0, 16.0, 0.0])],
            joints: vec![Joint {
                frame: Frame { pos: [0.0, 18.0, 0.0], rows: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]] },
                kind: JointKind::Hinge,
                high: 90.0,
                low: 0.0,
                swing: 0.0,
                actors: [0, 1],
                limited: true,
                collide: false,
                springs: [None; 3],
                projection: (0, 0.0, 0.0),
            }],
            lights: vec![],
        };
        let t = Template::new(&p);
        let poses: Vec<_> = t.bodies.iter().map(|b| (b.bind, Quat::IDENTITY)).collect();
        // the lower one kicked forward (+Z), against its limit
        let mut r = Ragdoll::new(&poses, &[Vec3::ZERO, Vec3::new(0.0, 0.0, 40.0)]);
        for _ in 0..30 {
            r.step(&t, 1.0 / 60.0, &crate::world::Ground(|_, _| -100.0));
        }
        let ps: Vec<_> = r.poses().collect();
        let pa = ps[0].0 + ps[0].1 * t.joints[0].ra;
        let pb = ps[1].0 + ps[1].1 * t.joints[0].rb;
        assert!(pa.distance(pb) < 0.05, "anchors apart by {}", pa.distance(pb));
        // relative turn about X stays within 0..90 (lower leg may only swing back, -Z)
        let rel = (ps[0].1.inverse() * ps[1].1).to_euler(glam::EulerRot::XYZ).0.to_degrees();
        assert!((-2.0..=92.0).contains(&rel), "{rel}");
    }
}
