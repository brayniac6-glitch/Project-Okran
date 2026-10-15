//! Ogre's maths as ParticleUniverse uses it (OgreQuaternion, OgreVector3, OgreMath): quaternions
//! stored w, x, y, z and never normalised behind the caller's back (scripts give unnormalised
//! ones), and Ogre's random numbers (rand() / RAND_MAX).

use bevy::math::Vec3;

/// Ogre::Quaternion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Quat {
    pub const IDENTITY: Quat = Quat { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(w: f32, x: f32, y: f32, z: f32) -> Quat {
        Quat { w, x, y, z }
    }

    /// FromAngleAxis (the axis is taken as it is, unit length assumed).
    pub fn from_angle_axis(angle: f32, axis: Vec3) -> Quat {
        let half = 0.5 * angle;
        let s = half.sin();
        Quat { w: half.cos(), x: s * axis.x, y: s * axis.y, z: s * axis.z }
    }

    pub fn dot(self, o: Quat) -> f32 {
        self.w * o.w + self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// normalise: scaled by 1 / sqrt(norm).
    pub fn normalised(self) -> Quat {
        let len = self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z;
        let f = 1.0 / len.sqrt();
        Quat { w: self.w * f, x: self.x * f, y: self.y * f, z: self.z * f }
    }

    pub fn inverse(self) -> Quat {
        let norm = self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z;
        if norm > 0.0 {
            let inv = 1.0 / norm;
            Quat { w: self.w * inv, x: -self.x * inv, y: -self.y * inv, z: -self.z * inv }
        } else {
            Quat { w: 0.0, x: 0.0, y: 0.0, z: 0.0 }
        }
    }

    /// Quaternion::nlerp.
    pub fn nlerp(t: f32, p: Quat, q: Quat, shortest_path: bool) -> Quat {
        let cos = p.dot(q);
        let r = if cos < 0.0 && shortest_path {
            Quat {
                w: p.w + t * (-q.w - p.w),
                x: p.x + t * (-q.x - p.x),
                y: p.y + t * (-q.y - p.y),
                z: p.z + t * (-q.z - p.z),
            }
        } else {
            Quat {
                w: p.w + t * (q.w - p.w),
                x: p.x + t * (q.x - p.x),
                y: p.y + t * (q.y - p.y),
                z: p.z + t * (q.z - p.z),
            }
        };
        r.normalised()
    }

    /// operator* (Vector3): the nVidia SDK form Ogre uses.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let q = Vec3::new(self.x, self.y, self.z);
        let uv = q.cross(v) * (2.0 * self.w);
        let uuv = q.cross(q.cross(v)) * 2.0;
        v + uv + uuv
    }
}

impl std::ops::Mul for Quat {
    type Output = Quat;
    fn mul(self, r: Quat) -> Quat {
        Quat {
            w: self.w * r.w - self.x * r.x - self.y * r.y - self.z * r.z,
            x: self.w * r.x + self.x * r.w + self.y * r.z - self.z * r.y,
            y: self.w * r.y + self.y * r.w + self.z * r.x - self.x * r.z,
            z: self.w * r.z + self.z * r.w + self.x * r.y - self.y * r.x,
        }
    }
}

impl From<Quat> for bevy::math::Quat {
    fn from(q: Quat) -> Self {
        bevy::math::Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}

impl From<bevy::math::Quat> for Quat {
    fn from(q: bevy::math::Quat) -> Self {
        Quat { w: q.w, x: q.x, y: q.y, z: q.z }
    }
}

/// Vector3::normalise: unchanged when shorter than 1e-08.
pub fn normalise(v: Vec3) -> Vec3 {
    let len = v.length();
    if len > 1e-08 { v * (1.0 / len) } else { v }
}

/// Vector3::perpendicular.
pub fn perpendicular(v: Vec3) -> Vec3 {
    let mut p = v.cross(Vec3::X);
    if p.length_squared() < 1e-06 * 1e-06 {
        p = v.cross(Vec3::Y);
    }
    normalise(p)
}

/// Vector3::randomDeviant: `v` turned by `angle` about a random axis perpendicular to it.
pub fn random_deviant(v: Vec3, angle: f32, up: Vec3, rng: &mut Rand) -> Vec3 {
    let up = if up == Vec3::ZERO { perpendicular(v) } else { up };
    let q = Quat::from_angle_axis(rng.unit() * std::f32::consts::TAU, v);
    let new_up = q.rotate(up);
    Quat::from_angle_axis(angle, new_up).rotate(v)
}

/// Vector3::getRotationTo (unit vectors' shortest arc).
pub fn rotation_to(from: Vec3, dest: Vec3, fallback: Vec3) -> Quat {
    let v0 = normalise(from);
    let v1 = normalise(dest);
    let d = v0.dot(v1);
    if d >= 1.0 {
        return Quat::IDENTITY;
    }
    if d < 1e-6 - 1.0 {
        if fallback != Vec3::ZERO {
            return Quat::from_angle_axis(std::f32::consts::PI, fallback);
        }
        let mut axis = Vec3::X.cross(from);
        if axis.length() < 1e-6 {
            axis = Vec3::Y.cross(from);
        }
        return Quat::from_angle_axis(std::f32::consts::PI, normalise(axis));
    }
    let s = ((1.0 + d) * 2.0).sqrt();
    let invs = 1.0 / s;
    let c = v0.cross(v1);
    Quat { w: s * 0.5, x: c.x * invs, y: c.y * invs, z: c.z * invs }.normalised()
}

/// Ogre's Math::UnitRandom: the CRT's rand() / RAND_MAX. (Stand-in) rand() is MSVC's linear
/// congruential generator from a fixed seed: the same 15-bit steps, not the same sequence (the
/// original shares one generator with the whole game).
#[derive(Debug, Clone)]
pub struct Rand(u32);

impl Default for Rand {
    fn default() -> Self {
        Rand(1)
    }
}

impl Rand {
    pub fn new(seed: u32) -> Rand {
        Rand(seed)
    }

    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        (self.0 >> 16) & 0x7fff
    }

    /// The CRT's rand(): 0 .. 32767.
    pub fn rand(&mut self) -> u32 {
        self.next()
    }

    pub fn unit(&mut self) -> f32 {
        self.next() as f32 / 32767.0
    }

    /// Math::RangeRandom.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        (b - a) * self.unit() + a
    }

    /// Math::SymmetricRandom.
    pub fn symmetric(&mut self) -> f32 {
        2.0 * self.unit() - 1.0
    }
}
