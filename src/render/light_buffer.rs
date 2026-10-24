//! The main light's shader constants shared by the terrain, object, character and water
//! materials (Kenshi sets them as the materials' shared parameters: sunDirection, sunColour,
//! envColour, ambientParams, exposure, sunDirectionReal, horizonClouds, pFogParams, fogColour,
//! wetness, rainAmount, dustAmount, gameTime; F040805 each frame).
//!
//! Harness: one storage buffer (`LIGHT`) bound in each of those materials (binding `BINDING`),
//! written when the light changes. Bevy prepares a material and specialises its meshes again
//! whenever the material changes, so the light kept in every material made each game minute's
//! relight (and the water's clock each frame) cost a frame of 20 - 50 ms; the buffer is written
//! in place (same size, COPY_DST: Bevy keeps the GPU buffer) and the materials stay as they are.
//! The values are the same as the materials held.
use crate::terrain_material::MainLight;
use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::render::storage::ShaderBuffer;

/// The shared buffer's handle (its asset made by `LightBufferPlugin`).
pub const LIGHT: Handle<ShaderBuffer> = uuid_handle!("6f1d3c52-7a4e-4b8e-9f0e-2c5d8a1b7e33");

/// The materials' binding of `LIGHT` (KenshiLight in kenshi_lighting.wgsl).
pub const BINDING: u32 = 70;

/// The shared constants (kenshi_lighting.wgsl KenshiLight).
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct KenshiLight {
    pub sun_direction: Vec4,
    pub sun_colour: Vec4,
    pub env_colour: Vec4,
    pub ambient_params: Vec4,
    pub sun_real: Vec4,
    pub horizon: Vec4,
    pub fog: Vec4,
    pub fog_colour: Vec4,
    /// x exposure, y wetness, z rain amount, w gameTime (the game's hours, the water's clock).
    pub misc: Vec4,
    pub dust: Vec4,
}

impl KenshiLight {
    pub fn from_light(light: &MainLight, game_hours: f32) -> KenshiLight {
        KenshiLight {
            sun_direction: light.sun_direction.extend(0.0),
            sun_colour: light.sun_colour,
            env_colour: light.env_colour,
            ambient_params: light.ambient_params,
            sun_real: light.sun_real.extend(0.0),
            horizon: light.horizon,
            fog: light.fog,
            fog_colour: light.fog_colour,
            misc: Vec4::new(light.exposure, light.wetness, light.rain_amount, game_hours),
            dust: light.dust.extend(0.0),
        }
    }
}

/// The game's hours for the water's clock (set each frame by the game), and what the buffer
/// last got.
#[derive(Resource, Default)]
pub struct SharedLight {
    pub game_hours: f32,
    written: Option<(MainLight, f32)>,
}

pub struct LightBufferPlugin;

impl Plugin for LightBufferPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SharedLight>().add_systems(PostUpdate, write_light);
    }

    fn finish(&self, app: &mut App) {
        if let Some(mut buffers) = app.world_mut().get_resource_mut::<Assets<ShaderBuffer>>() {
            let _ = buffers.insert(&LIGHT, ShaderBuffer::from(KenshiLight::default()));
        }
    }
}

/// The buffer written when the light (`TerrainLook`'s, relit each game minute) or the clock
/// moved.
fn write_light(look: Option<Res<crate::terrain_material::TerrainLook>>, mut shared: ResMut<SharedLight>, mut buffers: ResMut<Assets<ShaderBuffer>>) {
    let Some(look) = look else { return };
    let now = (look.light, shared.game_hours);
    if shared.written == Some(now) {
        return;
    }
    if let Some(mut b) = buffers.get_mut(&LIGHT) {
        b.set_data(KenshiLight::from_light(&now.0, now.1));
        shared.written = Some(now);
    }
}
