//! Kenshi's local lights (port_map `rendering`): the lamps, torches, campfires and building lights
//! the game places (LIGHT records as instances on buildings, their parts and their furniture,
//! `zone::placed_lights`), made as Kenshi makes them (F045912) and lit as its deferred light
//! pass lights them (data/materials/deferred/deferred.hlsl light_fs, kenshi::lighting
//! `local_lights`).
//!
//! Each becomes a Bevy point or spot light (so Bevy's clustering finds those near each pixel):
//! its "radius" the range; its "diffuse" (0xRRGGBB) times its power ("brightness", varied by up
//! to "variance" per light, randRange F050502) the colour the shader reads back
//! (color x intensity / 4 pi, so the intensity is 4 pi); a spot's cone from its "inner" and
//! "outer" (full angles, as Ogre's setSpotlightRange takes them). No shadows (F045912).
//!
//! The "effect" (LightEffect) is set on the Ogre light as its custom parameter's z (F045912),
//! which the deferred light pass reads each frame (F030155) and scales the light's power by:
//! PULSE sin(4 T) x 0.5 + 0.5 (not drawn at 0), SHIMMER sin(30 T) x sin(17 T) x 0.05 + 0.95,
//! anything else (FLICKER too) 1; T is the sky's game hours (day x 24 + hour, Sky +0xa0) x 60,
//! the same for every light. A light's variance is drawn from -variance to +variance.
//! Not ported: the parameter's x (F045912's third argument, from the building's town handle,
//! F037795) keeps a light dark by day (daylight over 0.1, F040781).
use bevy::prelude::*;
use kenshi_data::{GameData, GameDataContainer};

/// LightEffect.
pub const EFFECT_PULSE: i32 = 1;
pub const EFFECT_FLICKER: i32 = 2;
pub const EFFECT_SHIMMER: i32 = 3;

/// A light made from a LIGHT record: its colour at full power and its effect.
#[derive(Component, Clone, Debug)]
pub struct LocalLight {
    pub colour: Vec3,
    pub power: f32,
    pub effect: i32,
}

/// The sky's game hours (Sky +0xa0: day x 24 + hour) the light effects run on; the game sets it
/// each frame.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct LightClock {
    pub hours: f64,
}

/// A small hash for a light's own variance and phase (from where it stands).
fn hash(p: Vec3, k: u32) -> f32 {
    let mut h = (p.x * 73.0) as i32 as u32 ^ ((p.y * 19.0) as i32 as u32).rotate_left(11) ^ ((p.z * 37.0) as i32 as u32).rotate_left(22) ^ k.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// The colour a LIGHT record's "diffuse" (0xRRGGBB) gives.
fn colour(r: &GameData) -> Vec3 {
    let c = r.ints.get("diffuse").copied().unwrap_or(0xffffff) as u32;
    Vec3::new(((c >> 16) & 255) as f32, ((c >> 8) & 255) as f32, (c & 255) as f32) / 255.0
}

/// @fn F045912
///
/// The Bevy light (and its `LocalLight`) for LIGHT record `light` at `at` (scene units) turned
/// `rot`.
pub fn spawn_light(commands: &mut Commands, data: &GameDataContainer, light: &str, at: Vec3, rot: Quat) -> Option<Entity> {
    let r = data.get(light)?;
    let f = |k: &str, d: f32| r.floats.get(k).copied().unwrap_or(d);
    let range = f("radius", 100.0).max(1.0);
    let variance = f("variance", 0.0);
    let power = (f("brightness", 1.0) + (hash(at, 1) * 2.0 - 1.0) * variance).max(0.0);
    let colour = colour(r);
    let effect = r.ints.get("effect").copied().unwrap_or(0);
    let local = LocalLight { colour, power, effect };
    let c = colour * power;
    let tint = Color::linear_rgb(c.x, c.y, c.z);
    let place = Transform::from_translation(at).with_rotation(rot);
    let e = if r.ints.get("type").copied().unwrap_or(0) == 1 {
        let (inner, outer) = (f("inner", 45.0), f("outer", 40.0));
        let half = |deg: f32| (deg * 0.5).to_radians().clamp(0.01, std::f32::consts::FRAC_PI_2 - 0.01);
        let (a, b) = (half(inner.min(outer)), half(inner.max(outer)));
        commands.spawn((
            SpotLight { color: tint, intensity: 4.0 * std::f32::consts::PI, range, radius: 0.0, shadow_maps_enabled: false, inner_angle: a, outer_angle: b, ..default() },
            place,
            local,
        ))
    } else {
        commands.spawn((PointLight { color: tint, intensity: 4.0 * std::f32::consts::PI, range, radius: 0.0, shadow_maps_enabled: false, ..default() }, place, local))
    };
    Some(e.id())
}

/// @fn F030155
///
/// The lights' effects over time (the deferred light pass's power scale, see the module notes).
pub fn animate(
    clock: Option<Res<LightClock>>,
    mut points: Query<(&LocalLight, &mut PointLight), Without<SpotLight>>,
    mut spots: Query<(&LocalLight, &mut SpotLight)>,
) {
    let t = (clock.map_or(0.0, |c| c.hours) * 60.0) as f32;
    let scale = |l: &LocalLight| -> Option<f32> {
        match l.effect {
            EFFECT_PULSE => Some(((t * 4.0).sin() as f64 * 0.5 + 0.5) as f32),
            EFFECT_SHIMMER => Some((((t * 30.0).sin() * (t * 17.0).sin()) as f64 * 0.05 + 0.95) as f32),
            _ => None,
        }
    };
    for (l, mut p) in &mut points {
        if let Some(s) = scale(l) {
            let c = l.colour * l.power * s.max(0.0);
            p.color = Color::linear_rgb(c.x, c.y, c.z);
        }
    }
    for (l, mut p) in &mut spots {
        if let Some(s) = scale(l) {
            let c = l.colour * l.power * s.max(0.0);
            p.color = Color::linear_rgb(c.x, c.y, c.z);
        }
    }
}
