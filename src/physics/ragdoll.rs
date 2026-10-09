//! Ragdolls: the rigid bodies and joints of a Scythe `.phs` file (F025944), simulated.
//!
//! (stand-in) Kenshi hands these to PhysX 2.8; this solves the same bodies, shapes, joint limits
//! and damping with extended position-based dynamics (sub-stepped rigid bodies, one solve per
//! sub-step). Bodies collide with the ground and with each other unless jointed (as PhysX
//! joints disable their pair's contacts), on PhysX's default material (static and dynamic
//! friction 0.5, no bounce); no buildings yet. (approximation) Against each other, a box is the
//! capsule along its longest side as thick as its thinnest.
use glam::{Mat3, Quat, Vec3};
use crate::world::Surroundings;
use kenshi_io::phs::{Frame, JointKind, Phs, ShapeKind};

/// The physics scene's gravity (units / s^2; the -98.1 in the physics module's data, F045103's
/// neighbourhood: 9.81 m/s^2 in decimetres).
pub const GRAVITY: Vec3 = Vec3::new(0.0, -98.1, 0.0);
const SUBSTEPS: usize = 12;
/// PhysX 2.8's NX_MAX_ANGULAR_VELOCITY (its default, also in Kenshi's collision files): no
/// body turns faster (radians / s).
const MAX_ANGULAR: f32 = 7.0;
/// (stand-in for PhysX's penalty depenetration) a contact pushes a body out at most this fast
/// (units / s), so starting deep inside something does not fling it.
const MAX_DEPENETRATION: f32 = 40.0;
/// A box's corners collide as spheres this big (units) set inside them.
const CORNER: f32 = 0.3;
const STATIC_FRICTION: f32 = 0.5;
const DYNAMIC_FRICTION: f32 = 0.5;
/// (stand-in for PhysX's sleep test) a ragdoll whose bodies all stay within this distance
/// (units) and turn (radians) over `SLEEP_TIME` seconds sleeps.
const SLEEP_DRIFT: f32 = 0.15;
const SLEEP_TURN: f32 = 0.1;
const SLEEP_TIME: f32 = 0.4;
/// (stand-in) once every body is slower than these (units / s, radians / s), the ragdoll is
/// damped this much more (per second) so it comes to rest instead of creeping.
const SETTLE_SPEED: f32 = 3.0;
const SETTLE_TURN: f32 = 1.0;
const SETTLE_DAMPING: f32 = 4.0;

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
    inv_mass: f32,
    inv_inertia: Mat3,
    damping: [f32; 2],
    colliders: Vec<Collider>,
    /// Capsules (ends in the body's frame, radius) for contacts between bodies.
    hulls: Vec<(Vec3, Vec3, f32)>,
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
}

/// A ragdoll as loaded: bodies in bind space and the joints between them.
#[derive(Debug, Clone)]
pub struct Template {
    pub bodies: Vec<BodyDef>,
    joints: Vec<JointDef>,
    /// The body pairs that collide (not jointed).
    pairs: Vec<(usize, usize)>,
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
            // (approximation) as a cylinder of the capsule's full length
            let r = s.x * 0.5;
            let side = (3.0 * r * r + s.y * s.y) / 12.0;
            Vec3::new(side, 0.5 * r * r, side)
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
            let density = if a.density > 0.0 { a.density } else { 1.0 };
            let parts: Vec<(ShapeKind, Vec3, Vec3, Quat, f32)> = a
                .shapes
                .iter()
                .map(|s| {
                    let size = Vec3::from(s.size);
                    (s.kind, size, Vec3::from(s.frame.pos), rot(&s.frame), density * volume(s.kind, size))
                })
                .collect();
            let mass: f32 = parts.iter().map(|p| p.4).sum();
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
            let hulls = colliders
                .iter()
                .map(|c| match *c {
                    Collider::Sphere { c, r } => (c, c, r),
                    Collider::Capsule { a, b, r } => (a, b, r),
                    Collider::Box { c, rot, half } => {
                        let axes = [Vec3::X, Vec3::Y, Vec3::Z];
                        let h = half.to_array();
                        let long = (0..3).max_by(|&i, &j| h[i].total_cmp(&h[j])).unwrap_or(0);
                        let r = (0..3).filter(|&i| i != long).map(|i| h[i]).fold(f32::MAX, f32::min);
                        let d = rot * axes[long] * (h[long] - r).max(0.0);
                        (c - d, c + d, r)
                    }
                })
                .collect();
            let (inv_mass, inv_inertia) = if mass > 0.0 && inertia.determinant().abs() > 1e-12 {
                (1.0 / mass, inertia.inverse())
            } else {
                (0.0, Mat3::ZERO)
            };
            bodies.push(BodyDef {
                bone: a.bone.clone(),
                bind: com,
                inv_mass,
                inv_inertia,
                damping: a.damping,
                colliders,
                hulls,
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
                })
            })
            .collect::<Vec<JointDef>>();
        let n = bodies.len();
        let pairs = (0..n)
            .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
            .filter(|&(i, j)| !joints.iter().any(|jd| (jd.a, jd.b) == (i, j) || (jd.a, jd.b) == (j, i)))
            .collect();
        Template { bodies, joints, pairs }
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
}

/// A ragdoll in the world.
#[derive(Debug, Clone)]
pub struct Ragdoll {
    s: Vec<State>,
    /// The sub-step being solved (seconds).
    h: f32,
    /// Bodies held at poses (index, centre of mass, rotation).
    pinned: Vec<(usize, Vec3, Quat)>,
    /// This sub-step's ground contacts: body, point (body frame), normal, depth corrected.
    contacts: Vec<(usize, Vec3, Vec3, f32)>,
    still: f32,
    /// The poses the stillness test measures from.
    rest: Vec<(Vec3, Quat)>,
    pub asleep: bool,
}

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
            })
            .collect();
        Ragdoll { s, h: 1.0 / 360.0, pinned: Vec::new(), contacts: Vec::new(), still: 0.0, rest: poses.to_vec(), asleep: false }
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

    pub fn wake(&mut self) {
        self.asleep = false;
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
        self.wake();
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
                if let Some(d) = past_limit(phi, j.low, j.high) {
                    self.angular(t, a, b, n * d);
                }
            }
            JointKind::Ball => {
                // swing: the twist axes (first rows) stay within the cone
                let ua = axes(self, j.fa, a, Vec3::X);
                let ub = axes(self, j.fb, b, Vec3::X);
                let angle = ua.dot(ub).clamp(-1.0, 1.0).acos();
                if angle > j.swing {
                    let n = ua.cross(ub).normalize_or_zero();
                    self.angular(t, a, b, n * (angle - j.swing));
                }
                // twist about that axis
                let rel = (self.s[a].q * j.fa).inverse() * (self.s[b].q * j.fb);
                let tw = 2.0 * rel.x.atan2(rel.w);
                let n = axes(self, j.fb, b, Vec3::X);
                if let Some(d) = past_limit(tw, j.low, j.high) {
                    self.angular(t, a, b, n * d);
                }
            }
            JointKind::Other(_) => {}
        }
    }

    /// Bodies `i` and `j` pushed apart where their hulls overlap.
    fn touch(&mut self, t: &Template, i: usize, j: usize) {
        for &(a1, b1, r1) in &t.bodies[i].hulls {
            for &(a2, b2, r2) in &t.bodies[j].hulls {
                let (si, sj) = (self.s[i], self.s[j]);
                let (c1, c2) = closest(si.x + si.q * a1, si.x + si.q * b1, sj.x + sj.q * a2, sj.x + sj.q * b2);
                let d = c1 - c2;
                let dist = d.length();
                let depth = r1 + r2 - dist;
                if depth <= 0.0 || dist < 1e-6 {
                    continue;
                }
                let n = d / dist;
                let (p1, p2) = (c1 - n * r1, c2 + n * r2);
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

    /// Body `i` pushed out along `n` by `depth` at the contact point `c`, then held by friction.
    fn push_out(&mut self, t: &Template, i: usize, c: Vec3, n: Vec3, depth: f32) {
        let depth = depth.min(MAX_DEPENETRATION * self.h);
        let s = self.s[i];
        self.positional(t, i, None, n * depth, c - s.x, Vec3::ZERO);
        self.contacts.push((i, s.q.inverse() * (c - s.x), n, depth));
        // friction: the contact point's slide this sub-step undone, up to the friction cone
        let s = self.s[i];
        let local = s.q.inverse() * (c - s.x);
        let before = s.px + s.pq * local;
        let now = s.x + s.q * local;
        let d = now - before;
        let slide = d - n * d.dot(n);
        let len = slide.length();
        if len > 1e-7 {
            let undo = if len < STATIC_FRICTION * depth { 1.0 } else { (DYNAMIC_FRICTION * depth / len).min(1.0) };
            self.positional(t, i, None, -slide * undo, now - s.x, Vec3::ZERO);
        }
    }

    /// Advance `dt` seconds among `world`'s ground and solids.
    pub fn step(&mut self, t: &Template, dt: f32, world: &dyn Surroundings) {
        if self.asleep || dt <= 0.0 {
            return;
        }
        let h = dt.min(1.0 / 30.0) / SUBSTEPS as f32;
        self.h = h;
        let mut found = Vec::new();
        // a pinned body goes to its pose in even steps
        let pins: Vec<(usize, Vec3, Quat, Vec3, Quat)> = self.pinned.iter().map(|&(i, x, q)| (i, self.s[i].x, self.s[i].q, x, q)).collect();
        for k in 0..SUBSTEPS {
            for (i, s) in self.s.iter_mut().enumerate() {
                s.px = s.x;
                s.pq = s.q;
                if let Some(&(_, x0, q0, x1, q1)) = pins.iter().find(|p| p.0 == i) {
                    let f = (k + 1) as f32 / SUBSTEPS as f32;
                    s.x = x0.lerp(x1, f);
                    s.q = q0.slerp(q1, f);
                    continue;
                }
                if t.bodies[i].inv_mass == 0.0 {
                    continue;
                }
                s.v += GRAVITY * h;
                s.x += s.v * h;
                s.q = add_rotation(s.q, s.w * h);
            }
            for j in &t.joints {
                self.joint(t, j);
            }
            for &(i, j) in &t.pairs {
                self.touch(t, i, j);
            }
            for i in 0..self.s.len() {
                let (x, q) = (self.s[i].x, self.s[i].q);
                for c in t.bodies[i].colliders.clone() {
                    match c {
                        Collider::Sphere { c, r } => self.contact(t, i, x + q * c, r, world, &mut found),
                        Collider::Capsule { a, b, r } => {
                            for p in [a, b, (a + b) * 0.5] {
                                self.contact(t, i, x + q * p, r, world, &mut found);
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
                                self.contact(t, i, x + q * (c + rot * corner), r, world, &mut found);
                            }
                        }
                    }
                }
            }
            for s in &mut self.s {
                s.v = (s.x - s.px) / h;
                let dq = s.q * s.pq.inverse();
                s.w = Vec3::new(dq.x, dq.y, dq.z) * (2.0 / h);
                if dq.w < 0.0 {
                    s.w = -s.w;
                }
            }
            // the contacts' velocities: no bounce, sliding slowed by dynamic friction up to the
            // normal correction's speed
            for (i, local, n, depth) in std::mem::take(&mut self.contacts) {
                let s = self.s[i];
                let r = s.q * local;
                let v = s.v + s.w.cross(r);
                let vn = v.dot(n);
                let vt = v - n * vn;
                let mut dv = Vec3::ZERO;
                let lt = vt.length();
                if lt > 1e-7 {
                    dv -= vt / lt * (DYNAMIC_FRICTION * depth / h).min(lt);
                }
                if vn < 0.0 {
                    dv -= n * vn;
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
            let settling = self.s.iter().all(|s| s.v.length() < SETTLE_SPEED && s.w.length() < SETTLE_TURN);
            let extra = if settling { SETTLE_DAMPING } else { 0.0 };
            for (i, s) in self.s.iter_mut().enumerate() {
                let [lin, ang] = t.bodies[i].damping;
                s.v *= (1.0 - (lin + extra) * h).max(0.0);
                s.w *= (1.0 - (ang + extra) * h).max(0.0);
                s.w = s.w.clamp_length_max(MAX_ANGULAR);
            }
        }
        let moving = self
            .s
            .iter()
            .zip(&self.rest)
            .any(|(s, &(x, q))| s.x.distance(x) > SLEEP_DRIFT || s.q.angle_between(q) > SLEEP_TURN);
        if moving {
            self.still = 0.0;
            self.rest = self.s.iter().map(|s| (s.x, s.q)).collect();
        } else {
            self.still += dt;
        }
        if self.still >= SLEEP_TIME {
            self.asleep = true;
            for s in &mut self.s {
                s.v = Vec3::ZERO;
                s.w = Vec3::ZERO;
            }
        }
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
            bone: name.into(),
            bone_frame: frame(at),
            offsets: Vec::new(),
            shapes: vec![Shape { frame: frame(at), kind: ShapeKind::Capsule, size: [1.0, 4.0, 1.0] }],
        }
    }

    #[test]
    fn falls_and_rests_on_the_ground() {
        let p = Phs { version: 1.4, actors: vec![capsule("a", [0.0, 10.0, 0.0])], joints: vec![] };
        let t = Template::new(&p);
        let mut r = Ragdoll::new(&[(t.bodies[0].bind, Quat::IDENTITY)], &[]);
        for _ in 0..300 {
            r.step(&t, 1.0 / 60.0, &crate::world::Ground(|_, _| 0.0));
        }
        let (x, _) = r.poses().next().unwrap();
        // upright on its end: the centre half the length up
        assert!((x.y - 2.0).abs() < 0.2, "{x}");
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
            }],
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
