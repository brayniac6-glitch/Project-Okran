//! Kenshi's terrain material (port_map `terrain`): the terrain shader of
//! data/materials/deferred/terrainfp4.hlsl with the deferred main light of deferred.hlsl, fed per
//! terrain piece from BIOMES records (`kenshi_world::terrain_biome`). See kenshi_terrain.wgsl.

use crate::object_material::{KenshiObjectMaterial, ObjectLook, ObjectParams};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use kenshi_world::terrain_biome::{BlendInfo, TerrainBiome, terrain_biomes};
use std::collections::HashMap;

/// The terrain shader's parameters (see TerrainParams in kenshi_terrain.wgsl).
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct TerrainParams {
    pub scales_a: [Vec4; 4],
    pub scales_b: [Vec4; 4],
    pub scales_c: [Vec4; 4],
    pub slope_min: [Vec4; 4],
    pub slope_max: [Vec4; 4],
    pub slope_blend: [Vec4; 4],
    pub overlay_mult: [Vec4; 4],
    pub texture_fade: [Vec4; 4],
    pub absorb0: [Vec4; 4],
    pub absorb1: [Vec4; 4],
    pub brightness_fix: Vec4,
    pub distortion0: Vec4,
    pub distortion1: Vec4,
    pub channel: Vec4,
    pub count_origin: Vec4,
    pub sun_direction: Vec4,
    pub sun_colour: Vec4,
    pub env_colour: Vec4,
    pub ambient_params: Vec4,
    pub world: Vec4,
    pub sun_real: Vec4,
    pub horizon: Vec4,
    pub fog: Vec4,
    pub fog_colour: Vec4,
    /// The blood on the ground (TerrainBlood): x, y its area's corner (world x, z), z one over
    /// its width; w 1 when there is a blood map.
    pub blood: Vec4,
}

impl TerrainParams {
    /// The parameters of up to four biomes (F051631), each with the blend map channel its
    /// weight comes from (0..3 R, G, B, A; 4 the rest).
    pub fn new(biomes: &[(&TerrainBiome, u32)], origin: Vec3, world_size: f32) -> TerrainParams {
        let mut p = TerrainParams::default();
        let mut dist = [0.0f32; 8];
        let mut channel = [4.0f32; 4];
        let mut fix = [1.0f32; 4];
        for (k, (b, ch)) in biomes.iter().take(4).enumerate() {
            p.scales_a[k] = Vec4::from(b.scales_a);
            p.scales_b[k] = Vec4::from(b.scales_b);
            p.scales_c[k] = Vec4::from(b.scales_c);
            p.slope_min[k] = Vec4::from(b.slope_min);
            p.slope_max[k] = Vec4::from(b.slope_max);
            p.slope_blend[k] = Vec4::from(b.slope_blend);
            p.overlay_mult[k] = Vec4::from(b.overlay_mult);
            p.texture_fade[k] = Vec4::from(b.texture_fade);
            p.absorb0[k] = Vec4::from(b.absorbance[0]);
            p.absorb1[k] = Vec4::from(b.absorbance[1]);
            dist[2 * k] = b.distortion[0];
            dist[2 * k + 1] = b.distortion[1];
            channel[k] = *ch as f32;
            fix[k] = b.brightness_fix;
        }
        p.brightness_fix = Vec4::from(fix);
        p.distortion0 = Vec4::new(dist[0], dist[1], dist[2], dist[3]);
        p.distortion1 = Vec4::new(dist[4], dist[5], dist[6], dist[7]);
        p.channel = Vec4::from(channel);
        p.count_origin = Vec4::new(biomes.len().min(4) as f32, origin.x, origin.y, origin.z);
        p.world = Vec4::new(world_size, 0.0, 0.0, 0.0);
        p
    }
}

/// One terrain piece's material: its params, the maps and up to four biomes' textures.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct KenshiTerrainMaterial {
    #[uniform(0)]
    pub params: TerrainParams,
    #[texture(1)]
    #[sampler(2)]
    pub overlay: Handle<Image>,
    #[texture(3)]
    pub colour: Handle<Image>,
    #[texture(4)]
    pub blend: Handle<Image>,
    #[texture(5)]
    #[sampler(58)]
    pub ambient: Handle<Image>,
    /// The CSM shadows' jitter map (kenshi::lighting::sun_shadow).
    #[texture(59)]
    pub shadow_jitter: Handle<Image>,
    /// The blood on the ground (see `TerrainParams::blood`).
    #[texture(60)]
    #[sampler(61)]
    pub blood: Handle<Image>,
    /// The interior mask (CLIP_INTERIOR, kenshi_render::interior_mask).
    #[texture(62)]
    pub interior_mask: Handle<Image>,
    #[texture(6, dimension = "cube")]
    #[sampler(8)]
    pub irradiance: Handle<Image>,
    #[texture(7, dimension = "cube")]
    pub specularity: Handle<Image>,
    #[texture(10)]
    #[sampler(9)]
    pub d00: Handle<Image>,
    #[texture(11)]
    pub d01: Handle<Image>,
    #[texture(12)]
    pub d02: Handle<Image>,
    #[texture(13)]
    pub d03: Handle<Image>,
    #[texture(14)]
    pub d04: Handle<Image>,
    #[texture(15)]
    pub d05: Handle<Image>,
    #[texture(16)]
    pub n00: Handle<Image>,
    #[texture(17)]
    pub n01: Handle<Image>,
    #[texture(18)]
    pub n02: Handle<Image>,
    #[texture(19)]
    pub n03: Handle<Image>,
    #[texture(20)]
    pub n04: Handle<Image>,
    #[texture(21)]
    pub n05: Handle<Image>,
    #[texture(22)]
    pub d10: Handle<Image>,
    #[texture(23)]
    pub d11: Handle<Image>,
    #[texture(24)]
    pub d12: Handle<Image>,
    #[texture(25)]
    pub d13: Handle<Image>,
    #[texture(26)]
    pub d14: Handle<Image>,
    #[texture(27)]
    pub d15: Handle<Image>,
    #[texture(28)]
    pub n10: Handle<Image>,
    #[texture(29)]
    pub n11: Handle<Image>,
    #[texture(30)]
    pub n12: Handle<Image>,
    #[texture(31)]
    pub n13: Handle<Image>,
    #[texture(32)]
    pub n14: Handle<Image>,
    #[texture(33)]
    pub n15: Handle<Image>,
    #[texture(34)]
    pub d20: Handle<Image>,
    #[texture(35)]
    pub d21: Handle<Image>,
    #[texture(36)]
    pub d22: Handle<Image>,
    #[texture(37)]
    pub d23: Handle<Image>,
    #[texture(38)]
    pub d24: Handle<Image>,
    #[texture(39)]
    pub d25: Handle<Image>,
    #[texture(40)]
    pub n20: Handle<Image>,
    #[texture(41)]
    pub n21: Handle<Image>,
    #[texture(42)]
    pub n22: Handle<Image>,
    #[texture(43)]
    pub n23: Handle<Image>,
    #[texture(44)]
    pub n24: Handle<Image>,
    #[texture(45)]
    pub n25: Handle<Image>,
    #[texture(46)]
    pub d30: Handle<Image>,
    #[texture(47)]
    pub d31: Handle<Image>,
    #[texture(48)]
    pub d32: Handle<Image>,
    #[texture(49)]
    pub d33: Handle<Image>,
    #[texture(50)]
    pub d34: Handle<Image>,
    #[texture(51)]
    pub d35: Handle<Image>,
    #[texture(52)]
    pub n30: Handle<Image>,
    #[texture(53)]
    pub n31: Handle<Image>,
    #[texture(54)]
    pub n32: Handle<Image>,
    #[texture(55)]
    pub n33: Handle<Image>,
    #[texture(56)]
    pub n34: Handle<Image>,
    #[texture(57)]
    pub n35: Handle<Image>,
}

impl Material for KenshiTerrainMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_terrain_vertex.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_terrain.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_terrain_prepass.wgsl".into()
    }
    // CLIP_INTERIOR discards: the prepass must too (its fragment stage runs for a material
    // that may discard), or the ground inside a building would leave its depth over the floor
    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_terrain_prepass.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Mask(0.5)
    }
}

/// Registers the terrain material and its embedded shader.
pub struct KenshiTerrainPlugin;

impl Plugin for KenshiTerrainPlugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "kenshi_lighting.wgsl");
        bevy::shader::load_shader_library!(app, "kenshi_interior.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_terrain.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_terrain_vertex.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_terrain_prepass.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_object.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_water.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_character.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_cutout_prepass.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_moon.wgsl");
        app.add_plugins((crate::mipmaps::MipmapPlugin, crate::texture_quality::TextureQualityPlugin));
        app.add_plugins(crate::hdr::KenshiHdrPlugin);
        app.add_plugins(crate::particles::render::KenshiParticlesPlugin);
        app.add_plugins(MaterialPlugin::<crate::moons::KenshiMoonMaterial>::default());
        app.add_plugins(MaterialPlugin::<KenshiTerrainMaterial>::default());
        app.add_plugins(MaterialPlugin::<KenshiObjectMaterial>::default());
        app.add_plugins(MaterialPlugin::<crate::character_material::KenshiCharacterMaterial>::default());
        app.add_plugins(MaterialPlugin::<crate::water_material::KenshiWaterMaterial>::default());
        app.add_plugins(crate::fog_volume::KenshiFogVolumePlugin);
        app.add_plugins(crate::interior_mask::InteriorMaskPlugin);
        app.add_systems(PostUpdate, crate::reflection::reflect_main_queue);
    }
}

/// The light the terrain is drawn under (deferred.hlsl main_fs uniforms).
#[derive(Debug, Clone, Copy)]
pub struct MainLight {
    /// Toward the sun.
    pub sun_direction: Vec3,
    /// rgb and its strength (w).
    pub sun_colour: Vec4,
    /// w: the environment light's strength.
    pub env_colour: Vec4,
    /// w: the ambient multiplier.
    pub ambient_params: Vec4,
    /// The HDR composite's exposure: EXPOSURE_KEY (0.55) over the adapted scene luminance
    /// (clamped to MIN_LUMINANCE 0.8 .. MAX_LUMINANCE 0.96, data/materials/post/hdr.material).
    pub exposure: f32,
    /// The sun itself (sunDirectionReal; below the horizon at night).
    pub sun_real: Vec3,
    /// horizonClouds: the colour the atmosphere fog tends to (rgb) and how far (a).
    pub horizon: Vec4,
    /// pFogParams (view distance x 10, fog start, fog end) and (w) the global fog's density.
    pub fog: Vec4,
    /// The global fog's colour (fogColour).
    pub fog_colour: Vec4,
    /// zenithLight (F040805).
    pub zenith: Vec3,
    /// The cloud layer's DensityMultiplier, DensityOffset, Darkness.
    pub clouds: Vec3,
    /// The weather's SharedWaterParams: wetness, rainAmount, dustAmount (F051194, F050957,
    /// F051193).
    pub wetness: f32,
    pub rain_amount: f32,
    pub dust: Vec3,
}

impl TerrainParams {
    fn set_light(&mut self, light: &MainLight) {
        self.sun_direction = light.sun_direction.extend(0.0);
        self.sun_colour = light.sun_colour;
        self.env_colour = light.env_colour;
        self.ambient_params = light.ambient_params;
        self.world.z = light.exposure;
        self.sun_real = light.sun_real.extend(0.0);
        self.horizon = light.horizon;
        self.fog = light.fog;
        self.fog_colour = light.fog_colour;
        self.world.y = light.wetness;
    }
}

/// How a texture unit samples (Ogre material scripts' filtering and tex_address_mode). Kenshi sets
/// the default filtering to TFO_ANISOTROPIC at anisotropy 16 (F045916: MaterialManager
/// setDefaultTextureFiltering(3), setDefaultAnisotropy(16)), the default addressing wraps; the
/// image plugin's default sampler is that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sampling {
    /// Anisotropic x16, wrapping.
    Default,
    /// `filtering trilinear`, wrapping.
    Trilinear,
    /// `filtering trilinear`, `tex_address_mode border`, `tex_border_colour 0 0 0 0`.
    TrilinearBorder,
    /// Anisotropic x16, `tex_address_mode border`, `tex_border_colour 0 0 0 0`.
    Border,
    /// Anisotropic x16, `tex_address_mode clamp`.
    Clamp,
}

impl Sampling {
    /// The sampler, None for the image plugin's default.
    pub fn descriptor(self) -> Option<bevy::image::ImageSamplerDescriptor> {
        use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSamplerBorderColor};
        let linear = bevy::image::ImageSamplerDescriptor {
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..Default::default()
        };
        let address = |mode: ImageAddressMode, d: bevy::image::ImageSamplerDescriptor| {
            bevy::image::ImageSamplerDescriptor {
                address_mode_u: mode,
                address_mode_v: mode,
                address_mode_w: mode,
                border_color: (mode == ImageAddressMode::ClampToBorder)
                    .then_some(ImageSamplerBorderColor::TransparentBlack),
                ..d
            }
        };
        let aniso = bevy::image::ImageSamplerDescriptor {
            anisotropy_clamp: 16,
            ..linear.clone()
        };
        match self {
            Sampling::Default => None,
            // (enhancement) Kenshi's "filtering trilinear", made anisotropic x 16: sharper at
            // grazing angles
            Sampling::Trilinear => Some(address(ImageAddressMode::Repeat, aniso.clone())),
            Sampling::TrilinearBorder => Some(address(ImageAddressMode::ClampToBorder, linear)),
            Sampling::Border => Some(address(ImageAddressMode::ClampToBorder, aniso)),
            Sampling::Clamp => Some(address(ImageAddressMode::ClampToEdge, aniso)),
        }
    }
}

/// The terrain's looks: the BIOMES sets, which cover each blend cell, and the textures they use.
#[derive(Resource)]
pub struct TerrainLook {
    pub biomes: Vec<TerrainBiome>,
    pub blend: BlendInfo,
    pub light: MainLight,
    textures: HashMap<String, Handle<Image>>,
    /// The install's resource index, to find textures by file name.
    pub resources: Option<std::sync::Arc<kenshi_io::resources::Resources>>,
    /// The blend map on the CPU (the biome at a point).
    pub blend_map: Option<kenshi_world::terrain_biome::BlendMap>,
    /// The water reflection's target (kenshi_render::reflection), when it is on.
    pub reflection: Option<Handle<Image>>,
    /// The blood on the ground and its area (`TerrainParams::blood`), when there is any.
    pub blood: Option<(Handle<Image>, Vec4)>,
    /// The interior mask's target (kenshi_render::interior_mask), once it is made.
    pub interior_mask: Option<Handle<Image>>,

}

impl TerrainLook {
    pub fn new(
        install: &std::path::Path,
        data: &kenshi_data::GameDataContainer,
        light: MainLight,
    ) -> Option<TerrainLook> {
        Some(TerrainLook {
            biomes: terrain_biomes(data),
            blend: BlendInfo::load(install)?,
            light,
            textures: HashMap::new(),
            resources: None,
            blend_map: kenshi_world::terrain_biome::BlendMap::load(install),
            reflection: None,
            blood: None,
            interior_mask: None,
        })
    }

    /// A texture sampled as a texture unit with Kenshi's default filtering (see `Sampling`).
    pub(crate) fn image(&mut self, assets: &AssetServer, path: &str) -> Handle<Image> {
        self.image_with(assets, path, Sampling::Default)
    }

    /// A texture sampled as a texture unit set up by `sampling`.
    pub(crate) fn image_with(
        &mut self,
        assets: &AssetServer,
        path: &str,
        sampling: Sampling,
    ) -> Handle<Image> {
        let key = format!("{path}#{sampling:?}");
        if let Some(h) = self.textures.get(&key) {
            return h.clone();
        }
        // Ogre finds a texture by its file name in its resource folders: (assumption) a missing
        // path's name is looked for in data/materials first (the materials' own group), then
        // anywhere
        let path = match &self.resources {
            Some(r) => {
                let name = path.rsplit('/').next().unwrap_or(path);
                let exact = |p: &str| {
                    let want = kenshi_io::resources::normalize(p);
                    r.find(p).filter(|f| f.eq_ignore_ascii_case(&want))
                };
                exact(path)
                    .or_else(|| exact(&format!("data/materials/{name}")))
                    .or_else(|| r.find(name))
                    .unwrap_or_else(|| path.to_string())
            }
            None => path.to_string(),
        };
        let path = path.as_str();
        // Kenshi samples its textures raw (no sRGB decoding)
        let sampler = sampling.descriptor();
        let h: Handle<Image> = assets
            .load_builder()
            .with_settings(move |s: &mut bevy::image::ImageLoaderSettings| {
                s.is_srgb = false;
                if let Some(d) = &sampler {
                    s.sampler = bevy::image::ImageSampler::Descriptor(d.clone());
                }
            })
            .load(path.to_string());
        self.textures.insert(key, h.clone());
        h
    }

    /// The material of a terrain piece in colour tile `tile` and blend cell `cell`: up to four of
    /// the cell's biomes ((assumption) the first four of its slots with a biome), each weighted by
    /// its slot's blend map channel.
    /// "DistantTerrain" (deferred/terrain.material, terrain.hlsl simple_fs): the colour of
    /// overlaymaps/distant.png over the whole world (clamped), gloss 0.2, no metalness, the
    /// normal clamped to positive components; world.w = 1 selects it in kenshi_terrain.wgsl.
    /// (assumption) the "map" parameter spans the world, as the ground effects read distant.png.
    pub fn distant_material(&mut self, assets: &AssetServer, origin: Vec3) -> KenshiTerrainMaterial {
        let mut m = self.material(assets, (0, 0), (0, 0), origin);
        m.colour = self.image_with(assets, "data/newland/land/overlaymaps/distant.png", Sampling::Clamp);
        m.params.world.w = 1.0;
        m
    }

    pub fn material(
        &mut self,
        assets: &AssetServer,
        tile: (u32, u32),
        cell: (u32, u32),
        origin: Vec3,
    ) -> KenshiTerrainMaterial {
        let slots = self.blend.slots(
            (cell.0 as usize).min(self.blend.n - 1),
            (cell.1 as usize).min(self.blend.n - 1),
        );
        let mut chosen: Vec<(usize, u32)> = vec![];
        for (k, &index) in slots.iter().enumerate() {
            if index == 0 || chosen.len() == 4 {
                continue;
            }
            if let Some(b) = self.biomes.iter().position(|b| b.index == index) {
                chosen.push((b, k as u32));
            }
        }
        if chosen.is_empty()
            && let Some(b) = self.biomes.iter().position(|b| !b.textures[0].0.is_empty())
        {
            chosen.push((b, 4));
        }
        let pairs: Vec<(&TerrainBiome, u32)> =
            chosen.iter().map(|&(b, k)| (&self.biomes[b], k)).collect();
        let mut params = TerrainParams::new(&pairs, origin, kenshi_world::WORLD_SIZE);
        params.set_light(&self.light);
        if let Some((_, area)) = &self.blood {
            params.blood = *area;
        }
        // per biome slot: its twelve textures (a missing biome repeats the first)
        let mut tex: Vec<Handle<Image>> = vec![];
        for k in 0..4 {
            let (b, _) = chosen[k.min(chosen.len() - 1)];
            let layers = self.biomes[b].textures.clone();
            for (d, _) in &layers {
                tex.push(self.image(assets, d));
            }
            for (_, n) in &layers {
                tex.push(self.image(assets, n));
            }
        }
        let t = |i: usize| tex[i].clone();
        KenshiTerrainMaterial {
            params,
            overlay: self.image(
                assets,
                &format!(
                    "data/newland/land/overlaymaps/new_overlay.{}.{}.png",
                    tile.0, tile.1
                ),
            ),
            colour: self.image(assets, &kenshi_world::colour_tile(tile.0, tile.1)),
            blend: self.image(assets, "data/newland/land/blendmap.png"),
            ambient: self.image_with(
                assets,
                "data/newland/land/overlaymaps/ambientmap.png",
                Sampling::Clamp,
            ),
            irradiance: self.image(assets, "data/materials/mp_irradiance.dds"),
            specularity: self.image(assets, "data/materials/mp_specularity.dds"),
            shadow_jitter: self.image(assets, "data/materials/white-noise.png"),
            blood: match self.blood.as_ref().map(|(h, _)| h.clone()) {
                Some(h) => h,
                None => self.image(assets, "data/materials/black.dds"),
            },
            interior_mask: self.interior_mask_image(assets),
            d00: t(0),
            d01: t(1),
            d02: t(2),
            d03: t(3),
            d04: t(4),
            d05: t(5),
            n00: t(6),
            n01: t(7),
            n02: t(8),
            n03: t(9),
            n04: t(10),
            n05: t(11),
            d10: t(12),
            d11: t(13),
            d12: t(14),
            d13: t(15),
            d14: t(16),
            d15: t(17),
            n10: t(18),
            n11: t(19),
            n12: t(20),
            n13: t(21),
            n14: t(22),
            n15: t(23),
            d20: t(24),
            d21: t(25),
            d22: t(26),
            d23: t(27),
            d24: t(28),
            d25: t(29),
            n20: t(30),
            n21: t(31),
            n22: t(32),
            n23: t(33),
            n24: t(34),
            n25: t(35),
            d30: t(36),
            d31: t(37),
            d32: t(38),
            d33: t(39),
            d34: t(40),
            d35: t(41),
            n30: t(42),
            n31: t(43),
            n32: t(44),
            n33: t(45),
            n34: t(46),
            n35: t(47),
        }
    }
}

impl TerrainLook {
    /// The object material of a building part (F046403 / F046402) placed relative to `origin`;
    /// a missing map is white.dds (diffuse), flat.dds (normal) or black.dds (metalness), a missing
    /// second map black.dds; `vertex_colours` whether its mesh has them; `dust_colour` the ground
    /// colour of the biome at the part.
    pub fn object(
        &mut self,
        assets: &AssetServer,
        o: &ObjectLook,
        origin: Vec3,
        vertex_colours: bool,
        dust_colour: [f32; 3],
    ) -> KenshiObjectMaterial {
        let mut params = ObjectParams {
            tiling_gloss: Vec4::new(
                o.tiling[0],
                o.tiling[1],
                o.gloss_mult,
                if o.alpha() { o.threshold.max(1e-3) } else { 0.0 },
            ),
            flags: Vec4::new(
                if o.emissive() { 1.0 } else { 0.0 },
                if o.double_sided() { 1.0 } else { 0.0 },
                0.0,
                if o.triplanar() { 1.0 } else { 0.0 },
            ),
            modes: Vec4::new(
                if o.dual(vertex_colours) { 1.0 } else { 0.0 },
                if o.dust() { 1.0 } else { 0.0 },
                if o.colouring(vertex_colours) { 1.0 } else { 0.0 },
                if o.grass { 1.0 } else { 0.0 },
            ),
            dust_colour: Vec3::from(dust_colour).extend(1.0),
            origin_exposure: origin.extend(0.0),
            // y: CLIP_INTERIOR (foliage.material's grass and foliage)
            world: Vec4::new(kenshi_world::WORLD_SIZE, if o.grass || o.foliage { 1.0 } else { 0.0 }, 0.0, 0.0),
            ..default()
        };
        params.set_light(&self.light);
        let or = |p: &Option<String>, d: &str| p.clone().unwrap_or_else(|| d.to_string());
        KenshiObjectMaterial {
            params,
            base: self.image_with(
                assets,
                &or(&o.texture, "data/materials/white.dds"),
                Sampling::Trilinear,
            ),
            normal: self.image_with(
                assets,
                &or(&o.normal, "data/materials/flat.dds"),
                Sampling::Trilinear,
            ),
            metal: self.image(assets, &or(&o.metal, "data/materials/black.dds")),
            base2: self.image(assets, &or(&o.texture2, "data/materials/black.dds")),
            normal2: self.image(assets, &or(&o.normal2, "data/materials/flat.dds")),
            metal2: self.image(assets, &or(&o.metal2, "data/materials/black.dds")),
            dust_noise: self.image(assets, "data/materials/Turbulent.dds"),
            shadow_jitter: self.image(assets, "data/materials/white-noise.png"),
            grid: self.image(assets, "data/buildings/materials/construction.dds"),
            interior_mask: self.interior_mask_image(assets),
            ambient: self.image_with(
                assets,
                "data/newland/land/overlaymaps/ambientmap.png",
                Sampling::Clamp,
            ),
            irradiance: self.image(assets, "data/materials/mp_irradiance.dds"),
            specularity: self.image(assets, "data/materials/mp_specularity.dds"),
        }
    }

    /// The interior mask's target, or (none made) black: nothing clipped.
    fn interior_mask_image(&mut self, assets: &AssetServer) -> Handle<Image> {
        match self.interior_mask.clone() {
            Some(h) => h,
            None => self.image(assets, "data/materials/black.dds"),
        }
    }

    /// The BIOMES record at world (x, z): the blend cell's slot weighing most there (F051563,
    /// as the ground and water effects key it).
    pub fn biome_at(&self, x: f32, z: f32) -> Option<&str> {
        let (cx, cy) = self.blend.cell_at(x, z);
        let slots = self.blend.slots(cx, cy);
        let slot = self.blend_map.as_ref().map_or(0, |m| m.dominant_slot(x, z));
        let index = slots[slot.min(4)];
        self.biomes.iter().find(|b| b.index == index).map(|b| b.record.as_str())
    }

    /// The "ground colour" of the BIOMES at world (x, z): the blend cell's slot weighing most
    /// there (white without one).
    pub fn ground_colour(&self, x: f32, z: f32) -> [f32; 3] {
        let (cx, cy) = self.blend.cell_at(x, z);
        let slots = self.blend.slots(cx, cy);
        let slot = self.blend_map.as_ref().map_or(0, |m| m.dominant_slot(x, z));
        let index = slots[slot.min(4)];
        self.biomes
            .iter()
            .find(|b| b.index == index)
            .map_or([1.0; 3], |b| [b.texture_fade[0], b.texture_fade[1], b.texture_fade[2]])
    }

    /// Light every terrain and object material with `light` (the sky's main light changes with
    /// the hour).
    /// The blood map (`TerrainParams::blood`) given to every terrain material.
    pub fn set_blood(&mut self, image: Handle<Image>, area: Vec4, materials: &mut Assets<KenshiTerrainMaterial>) {
        for (_, m) in materials.iter_mut() {
            m.params.blood = area;
            m.blood = image.clone();
        }
        self.blood = Some((image, area));
    }

    pub fn relight(
        &mut self,
        light: MainLight,
        materials: &mut Assets<KenshiTerrainMaterial>,
        objects: &mut Assets<KenshiObjectMaterial>,
    ) {
        self.light = light;
        for (_, m) in objects.iter_mut() {
            m.params.set_light(&light);
        }
        for (_, m) in materials.iter_mut() {
            m.params.set_light(&light);
        }
    }
}
