//! What Kenshi adds to a particle system it runs as an effect (F033084, the effect's wrapper):
//! the camera wrap extern (F023397), the ground colour handler (F033108), the wind handler
//! (F033083 -> F033081) and the light affector (F033093). The system holds them in
//! `System::kenshi`; the effect layer (`effects`) fills them in each frame.

use super::dynamic::Dyn;
use super::system::Technique;
use bevy::math::{Vec3, Vec4};
use std::sync::Arc;

/// Kenshi's per-system state the hooks read.
#[derive(Debug, Clone, Default)]
pub struct KenshiHooks {
    /// The camera wrap extern's box (F023372: its centre, and the effect's size; the box's
    /// half side is size / 1.5).
    pub wrap: Option<(Vec3, f32)>,
    /// The "distant" map the ground colour handler samples (F033101).
    pub ground: Option<Arc<GroundMap>>,
    /// The wind the wind handler sets on emitted techniques (F033082's state).
    pub wind: Option<Wind>,
    /// The sky manager's sun colour (F040805's +0x74: the main light's colour, alpha 1) the
    /// light affector takes.
    pub sun_colour: Vec3,
    /// World minus scene positions (the ground map is in world positions).
    pub origin: Vec3,
    /// The global extern (F023380): the handler's position, the effect's size, its max
    /// altitude, and the way the camera's target last moved (F023387).
    pub global: Option<(Vec3, f32, f32, Vec3)>,
}

/// An image's width and RGBA bytes (for the overlay maps the footsteps read).
pub fn load_rgba(path: &std::path::Path) -> Option<(u32, Vec<u8>)> {
    let img = image::open(path).ok()?.to_rgba8();
    Some((img.width(), img.into_raw()))
}

/// @fn F023399
///
/// The global extern's _interface: a particle further than the size from the handler is
/// moved 1.5 sizes the way the target moved; if still 1.5 sizes away (squared), put back at a
/// random place round the handler at its height. Then no higher than the max altitude (when
/// set).
pub fn global_wrap(p: Vec3, g: (Vec3, f32, f32, Vec3), rng: &mut super::math::Rand) -> Vec3 {
    let (centre, size, max_alt, moved) = g;
    let mut p = p;
    let size_sq = size * size;
    if (p - centre).length_squared() > size_sq {
        p += moved * (size * 1.5);
        if size_sq * 1.5 <= (p - centre).length_squared() {
            p.x = rng.range(-size, size) + centre.x;
            p.y = centre.y;
            p.z = rng.range(-size, size) + centre.z;
        }
    }
    if max_alt > 0.0 {
        p.y = p.y.min(max_alt);
    }
    p
}

/// The wrapper's wind (F033082): the force on the "Wind" affectors, the direction, and the
/// emission rate factor; and what of the effect it reads.
#[derive(Debug, Clone)]
pub struct Wind {
    pub force: Vec3,
    pub direction: Vec3,
    pub factor: f32,
    pub direction_emission: bool,
    pub span_max: f32,
    /// The LinearForce affectors whose name starts "Wind" (any case).
    pub affectors: Arc<Vec<String>>,
    /// The visual emitters' emission rates as the template has them (by emitter name).
    pub rates: Arc<Vec<(String, Dyn)>>,
}

/// @fn F023397
///
/// The camera wrap extern's _interface: a position wrapped into the box round `centre`.
pub fn wrap(p: Vec3, centre: Vec3, size: f32) -> Vec3 {
    let half = size / 1.5;
    let side = half * 2.0;
    let lo = centre - Vec3::splat(half);
    let f = |v: f32, l: f32| v - ((v - l) / side).floor() * side;
    Vec3::new(f(p.x, lo.x), f(p.y, lo.y), f(p.z, lo.z))
}

/// The "distant" overlay map (data/newland/land/overlaymaps/distant.png) over the world
/// (F033101: from -147456 on both axes).
#[derive(Debug)]
pub struct GroundMap {
    pub width: i32,
    pub height: i32,
    /// The bytes as Ogre has them in memory (3 per pixel).
    pub data: Vec<u8>,
    pub x0: f32,
    pub z0: f32,
    pub w: f32,
    pub h: f32,
}

impl GroundMap {
    /// (assumption) the PNG is decoded the way Ogre's FreeImage codec leaves a 24-bit image in
    /// memory: B, G, R per pixel (PF_BYTE_BGR on little-endian). (assumption) the map covers
    /// the whole world, 294912 on a side.
    pub fn load(install: &std::path::Path) -> Option<GroundMap> {
        let img = image::open(install.join("data/newland/land/overlaymaps/distant.png")).ok()?.to_rgb8();
        let (width, height) = (img.width() as i32, img.height() as i32);
        let mut data = Vec::with_capacity(img.as_raw().len());
        for px in img.pixels() {
            data.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        Some(GroundMap { width, height, data, x0: -147456.0, z0: -147456.0, w: 294912.0, h: 294912.0 })
    }

    /// @fn F033102
    ///
    /// The pixel bytes at (`a`, `b`): row `a`, column `b` (0 outside).
    fn pixel(&self, a: i32, b: i32) -> u32 {
        if a < self.width && b < self.height && a >= 0 && b >= 0 {
            let i = ((b + a * self.width) * 3) as usize;
            self.data[i] as u32 | (self.data[i + 1] as u32) << 8 | (self.data[i + 2] as u32) << 16
        } else {
            0
        }
    }

    /// @fn F033108
    ///
    /// The ground colour handler: the particle's original colour times the map's colour where
    /// it is (its latest position).
    pub fn tint(&self, at: Vec3, original: &mut Vec4) {
        let mut a = ((at.x - self.x0) / (self.w / self.width as f32)) as i32;
        let mut b = ((at.z - self.z0) / (self.h / self.height as f32)) as i32;
        if at.x < self.x0 {
            a = 0;
        }
        if self.x0 + self.w < at.x {
            a = self.width - 1;
        }
        if at.z < self.z0 {
            b = 0;
        }
        if self.z0 + self.h < at.z {
            b = self.height - 1;
        }
        let c = self.pixel(a, b);
        original.x *= (c & 0xff) as f32 / 255.0;
        original.y *= (c >> 8 & 0xff) as f32 / 255.0;
        original.z *= (c >> 16 & 0xff) as f32 / 255.0;
    }
}

/// @fn F033078
///
/// An emission rate scaled from the template's: fixed and random ones only.
fn scale_rate(original: &Dyn, rate: &mut Dyn, factor: f32) {
    match (original, rate) {
        (Dyn::Fixed(v), r @ Dyn::Fixed(_)) => *r = Dyn::Fixed(v * factor),
        (Dyn::Random { min, max }, Dyn::Random { min: m0, max: m1 }) => {
            *m0 = min * factor;
            *m1 = max * factor;
        }
        _ => {}
    }
}

/// @fn F033081
///
/// The wind on a technique: its emitters pointed down the wind (when "wind direction emission"),
/// its "Wind" affectors' force, and its visual emitters' emission rates by the wind factor.
pub fn apply_wind(t: &mut Technique, w: &Wind) {
    if w.direction_emission {
        for e in &mut t.emitters {
            let d = Vec3::new(w.direction.x, e.original_particle_direction.y, w.direction.z);
            e.set_particle_direction(super::math::normalise(d));
        }
    }
    for name in w.affectors.iter() {
        // getAffector by name: the first of that name
        if let Some(a) = t.affectors.iter_mut().find(|a| &a.name == name) {
            a.set_force_vector(w.force);
        }
    }
    if w.span_max > 0.0 {
        for (name, original) in w.rates.iter() {
            if let Some(e) = t.emitters.iter_mut().find(|e| &e.name == name) {
                scale_rate(original, &mut e.dyn_emission_rate, w.factor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_keeps_particles_in_the_camera_box() {
        // size 150: the box's half side is 100
        let c = Vec3::new(1000.0, 50.0, -400.0);
        let p = wrap(Vec3::new(1150.0, 50.0, -400.0), c, 150.0);
        assert!((p.x - 950.0).abs() < 1e-3);
        let q = wrap(Vec3::new(1000.0, 50.0, -400.0), c, 150.0);
        assert_eq!(q, Vec3::new(1000.0, 50.0, -400.0));
    }
}
