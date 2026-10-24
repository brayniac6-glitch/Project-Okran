//! Kenshi's water (port_map `water`): data/materials/forward/water.hlsl on the zones' water
//! planes (one pass per biome of the zone, blended by its blend map channel) and the distant
//! plane. See kenshi_water.wgsl.
//!
//! Spec: F050961 (the water: the distant plane, 500000 units a side at the water height, drawn
//! with "waterDistant" past 4000 units; the zone planes, a zone (4608 units) a side, with the
//! zone's biomes' water material; mapBounds = the world (-147456 .. 147456); the water height
//! set to 100), F050956 (the water height: the planes' height and "waterHeight"), F050962 (a
//! biome's water pass: see `kenshi_world::terrain_biome::WaterBiome`; with more than one biome,
//! each pass blends by blendChannel = its place in the list), data/materials/forward/
//! water.material (the defaults: invStrength 1, invOpacity 0.01, glow 0, gloss 0.9, distortion
//! (120, 0.01), scale 0.004; watercolourmap.png, flowmap.png, water.png, mp_specuwatery.dds).

use crate::terrain_material::{MainLight, TerrainLook};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use kenshi_world::terrain_biome::WaterBiome;

/// The water's height (F050961 sets it to 100).
pub const WATER_HEIGHT: f32 = 100.0;
/// The zone water planes' side.
pub const ZONE_WATER_SIZE: f32 = 4608.0;
/// The distant plane's side.
pub const DISTANT_WATER_SIZE: f32 = 500_000.0;

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct WaterParams {
    pub scale: Vec4,
    pub consts: Vec4,
    pub distortion: Vec4,
    pub sun_direction: Vec4,
    pub sun_colour: Vec4,
    pub env_colour: Vec4,
    pub sun_real: Vec4,
    pub horizon: Vec4,
    pub fog: Vec4,
    pub fog_colour: Vec4,
    pub origin_time: Vec4,
    pub world: Vec4,
}

impl WaterParams {
    pub fn set_light(&mut self, light: &MainLight) {
        self.sun_direction = light.sun_direction.extend(0.0);
        self.sun_colour = light.sun_colour;
        self.env_colour = light.env_colour;
        self.sun_real = light.sun_real.extend(0.0);
        self.horizon = light.horizon;
        self.fog = light.fog;
        self.fog_colour = light.fog_colour;
        self.world.x = light.exposure;
        self.world.y = light.rain_amount;
        // KENSHI_WATER_MODE: the shader's debug views (testing)
        self.world.z = std::env::var("KENSHI_WATER_MODE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct KenshiWaterMaterial {
    #[uniform(0)]
    pub params: WaterParams,
    #[texture(1)]
    #[sampler(2)]
    pub colour: Handle<Image>,
    #[texture(3)]
    pub flow: Handle<Image>,
    #[texture(4)]
    pub turbulence: Handle<Image>,
    #[texture(5)]
    pub normal: Handle<Image>,
    #[texture(6)]
    pub scum: Handle<Image>,
    #[texture(7)]
    pub scum_normal: Handle<Image>,
    #[texture(8)]
    pub blend: Handle<Image>,
    #[texture(9, dimension = "cube")]
    #[sampler(11)]
    pub irradiance: Handle<Image>,
    #[texture(10, dimension = "cube")]
    pub specularity: Handle<Image>,
    #[texture(12)]
    pub rain: Handle<Image>,
    /// The water reflection's target (white without one: water.hlsl's NO_REFLECTION, 1).
    #[texture(13)]
    #[sampler(14)]
    pub reflection: Handle<Image>,
    pub distant: bool,
    /// The shared main light (kenshi_render::light_buffer, kenshi_lighting.wgsl KenshiLight).
    #[storage(70, read_only)]
    pub light: Handle<bevy::render::storage::ShaderBuffer>,
}

impl Material for KenshiWaterMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_water.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        if self.distant {
            AlphaMode::Opaque
        } else {
            AlphaMode::Blend
        }
    }

    // a forward pass in Kenshi: not in the depth buffer the scene is shaded from (the distant
    // plane's cut within 4000 units would not reach a prepass)
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn depth_bias(&self) -> f32 {
        // waterDistant's depth_bias -1 (against z-fighting)
        if self.distant { 1.0 } else { 0.0 }
    }
}

impl TerrainLook {
    fn water_material(
        &mut self,
        assets: &AssetServer,
        params: WaterParams,
        textures: [&str; 4],
        distant: bool,
    ) -> KenshiWaterMaterial {
        let or = |s: &str, d: &str| if s.is_empty() { d.to_string() } else { s.to_string() };
        KenshiWaterMaterial {
            light: crate::light_buffer::LIGHT,
            params,
            colour: self.image(assets, "data/newland/land/overlaymaps/watercolourmap.png"),
            flow: self.image(assets, "data/newland/land/overlaymaps/flowmap.png"),
            turbulence: self.image(assets, &or(textures[0], "data/materials/flat.dds")),
            normal: self.image(assets, &or(textures[1], "data/materials/water.png")),
            scum: self.image(assets, &or(textures[2], "data/materials/nothing.dds")),
            scum_normal: self.image(assets, &or(textures[3], "data/materials/nothing.dds")),
            blend: self.image(assets, "data/newland/land/blendmap.png"),
            irradiance: self.image(assets, "data/materials/mp_irradiance.dds"),
            specularity: self.image(assets, "data/materials/mp_specuwatery.dds"),
            rain: self.image(assets, "data/materials/rain-ripples.png"),
            reflection: match &self.reflection {
                Some(r) => r.clone(),
                None => self.image(assets, "data/materials/white.dds"),
            },
            distant,
        }
    }

    /// The water passes of the zone plane in blend cell `cell` (F050962): one per biome of the
    /// cell ((assumption) as the terrain takes them: the first four slots with a biome, each
    /// blended by its slot's channel when there is more than one).
    pub fn water(
        &mut self,
        assets: &AssetServer,
        cell: (u32, u32),
        origin: Vec3,
    ) -> Vec<KenshiWaterMaterial> {
        let slots = self.blend.slots(
            (cell.0 as usize).min(self.blend.n - 1),
            (cell.1 as usize).min(self.blend.n - 1),
        );
        let chosen: Vec<(WaterBiome, i32)> = slots
            .iter()
            .enumerate()
            .filter(|(_, i)| **i != 0)
            .filter_map(|(k, &i)| {
                self.biomes
                    .iter()
                    .find(|b| b.index == i)
                    .map(|b| (b.water.clone(), k as i32))
            })
            .take(4)
            .collect();
        let blended = chosen.len() > 1;
        chosen
            .into_iter()
            .map(|(w, k)| {
                let mut params = WaterParams {
                    scale: Vec4::from(w.scale),
                    consts: Vec4::new(w.inv_strength, w.inv_opacity, w.gloss, w.glow),
                    distortion: Vec4::new(
                        w.distortion[0],
                        w.distortion[1],
                        if blended { k as f32 } else { -1.0 },
                        0.0,
                    ),
                    origin_time: origin.extend(0.0),
                    ..default()
                };
                params.set_light(&self.light);
                let textures = [
                    w.turbulence.as_str(),
                    w.normal.as_str(),
                    w.scum.as_str(),
                    w.scum_normal.as_str(),
                ];
                self.water_material(assets, params, textures, false)
            })
            .collect()
    }

    /// The distant plane's material ("waterDistant", water.material's defaults).
    pub fn distant_water(&mut self, assets: &AssetServer, origin: Vec3) -> KenshiWaterMaterial {
        let mut params = WaterParams {
            scale: Vec4::new(0.004, 0.004, 0.0, 0.0),
            consts: Vec4::new(1.0, 0.01, 0.9, 0.0),
            distortion: Vec4::new(120.0, 0.01, -1.0, 1.0),
            origin_time: origin.extend(0.0),
            ..default()
        };
        params.set_light(&self.light);
        self.water_material(assets, params, ["", "", "", ""], true)
    }
}
