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
//! (assumption) The "effect" (LightEffect PULSE, FLICKER, SHIMMER) is set on the Ogre light as a
//! custom parameter the game's shaders as shipped do not read back; here PULSE swells and fades
//! over two seconds, FLICKER wavers like a flame, SHIMMER trembles slightly. A light's variance
//! is drawn from -variance to +variance.
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
    pub phase: f32,
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
    let local = LocalLight { colour, power, effect, phase: hash(at, 2) * std::f32::consts::TAU };
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

/// The lights' effects over time (see the module notes).
pub fn animate(time: Res<Time>, mut points: Query<(&LocalLight, &mut PointLight), Without<SpotLight>>, mut spots: Query<(&LocalLight, &mut SpotLight)>) {
    let t = time.elapsed_secs();
    let scale = |l: &LocalLight| -> Option<f32> {
        match l.effect {
            EFFECT_PULSE => Some(0.75 + 0.25 * (t * std::f32::consts::PI + l.phase).sin()),
            EFFECT_FLICKER => {
                let w = (t * 11.0 + l.phase).sin() * 0.5 + (t * 23.7 + l.phase * 1.7).sin() * 0.3 + (t * 5.3 + l.phase * 0.3).sin() * 0.2;
                Some(0.85 + 0.15 * w)
            }
            EFFECT_SHIMMER => Some(0.95 + 0.05 * (t * 31.0 + l.phase).sin()),
            _ => None,
        }
    };
    for (l, mut p) in &mut points {
        if let Some(s) = scale(l) {
            let c = l.colour * l.power * s;
            p.color = Color::linear_rgb(c.x, c.y, c.z);
        }
    }
    for (l, mut p) in &mut spots {
        if let Some(s) = scale(l) {
            let c = l.colour * l.power * s;
            p.color = Color::linear_rgb(c.x, c.y, c.z);
        }
    }
}
