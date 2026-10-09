//! Kenshi's character materials (port_map `render`): data/materials/deferred/character.hlsl
//! main_fs for skin (body and head), skin.hlsl main_fs for skinned clothing (glossMult 1, no
//! colouring: (assumption) the clothing's colour map is not read, metalness 0) and character.hlsl
//! hair_fs for hair (its colour composed into the image; specularMult (assumption) 0), lit by
//! the deferred main light. See kenshi_character.wgsl.
//!
//! Harness stand-ins: the body and head are drawn as their split meshes (Kenshi samples the head
//! tile in the same pass); blood, part masks, muscle blending and glowing eyes are not drawn.

use crate::character::Surface;
use crate::terrain_material::{MainLight, TerrainLook};
use crate::water_material::WATER_HEIGHT;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct CharacterParams {
    pub kind: Vec4,
    pub sun_direction: Vec4,
    pub sun_colour: Vec4,
    pub env_colour: Vec4,
    pub ambient_params: Vec4,
    pub origin_exposure: Vec4,
    pub world: Vec4,
    pub sun_real: Vec4,
    pub horizon: Vec4,
    pub fog: Vec4,
    pub fog_colour: Vec4,
    pub hair_mult: Vec4,
    pub hair_alpha: Vec4,
    pub beard_alpha: Vec4,
    pub hair_colour: Vec4,
}

impl CharacterParams {
    pub fn set_light(&mut self, light: &MainLight) {
        self.sun_direction = light.sun_direction.extend(0.0);
        self.sun_colour = light.sun_colour;
        self.env_colour = light.env_colour;
        self.ambient_params = light.ambient_params;
        self.origin_exposure.w = light.exposure;
        self.sun_real = light.sun_real.extend(0.0);
        self.horizon = light.horizon;
        self.fog = light.fog;
        self.fog_colour = light.fog_colour;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CharacterKey {
    double_sided: bool,
}

impl From<&KenshiCharacterMaterial> for CharacterKey {
    fn from(m: &KenshiCharacterMaterial) -> Self {
        CharacterKey {
            double_sided: (1.5..2.5).contains(&m.params.kind.x),
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(CharacterKey)]
pub struct KenshiCharacterMaterial {
    #[uniform(0)]
    pub params: CharacterParams,
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Handle<Image>,
    #[texture(3)]
    pub normal: Handle<Image>,
    #[texture(4)]
    #[sampler(11)]
    pub ambient: Handle<Image>,
    #[texture(5, dimension = "cube")]
    #[sampler(7)]
    pub irradiance: Handle<Image>,
    #[texture(6, dimension = "cube")]
    pub specularity: Handle<Image>,
    #[texture(8)]
    #[sampler(10)]
    pub hair_map: Handle<Image>,
    #[texture(9)]
    pub beard_map: Handle<Image>,
    /// The CSM shadows' jitter map (kenshi::lighting::sun_shadow).
    #[texture(12)]
    pub shadow_jitter: Handle<Image>,
}

impl Material for KenshiCharacterMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_character.wgsl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_cutout_prepass.wgsl".into()
    }

    // clothing and hair cut pixels away: their prepass (depth, shadows) must cut them too
    fn alpha_mode(&self) -> AlphaMode {
        if (0.5..2.5).contains(&self.params.kind.x) {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Opaque
        }
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.double_sided {
            descriptor.primitive.cull_mode = None;
        }
        // the prepass shader's layout
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader_defs.push("CHARACTER".into());
        }
        Ok(())
    }
}

impl TerrainLook {
    /// A character surface's material, placed relative to `origin`.
    pub fn character(
        &mut self,
        assets: &AssetServer,
        images: &mut Assets<Image>,
        surface: &Surface,
        origin: Vec3,
    ) -> KenshiCharacterMaterial {
        let flat = "data/materials/flat.dds";
        let white = "data/materials/white.dds";
        let none = "data/materials/nothing.dds";
        let mut params = CharacterParams::default();
        // samplers as the texture units: Skinned's diffuseMap / normalMap trilinear (diffuse
        // mipmap_bias -2 in the shader), Character's diffuse, normal, hairMap and beardMap with
        // border addressing (colour 0 0 0 0)
        use crate::terrain_material::Sampling;
        let skin = Sampling::TrilinearBorder;
        let (mut hair_map, mut beard_map) = (
            self.image_with(assets, none, Sampling::Border),
            self.image_with(assets, none, Sampling::Border),
        );
        if let Surface::Head(_, _, scalps) = surface {
            for s in scalps {
                params.hair_colour = Vec3::from(s.colour).extend(1.0);
                if s.beard {
                    beard_map = self.image_with(assets, &s.map, Sampling::Border);
                    params.beard_alpha = Vec4::from(s.alpha);
                } else {
                    hair_map = self.image_with(assets, &s.map, Sampling::Border);
                    params.hair_mult = Vec4::from(s.mult);
                    params.hair_alpha = Vec4::from(s.alpha);
                }
            }
        }
        let (kind, diffuse, normal) = match surface {
            Surface::Skin(t, n) | Surface::Head(t, n, _) => (
                0.0,
                self.image_with(assets, t.as_deref().unwrap_or(white), skin),
                self.image_with(assets, n.as_deref().unwrap_or(flat), skin),
            ),
            Surface::Painted(img, n) => {
                let mut img = img.clone();
                // Kenshi samples raw: the composed image's bytes are taken as they are
                img.texture_descriptor.format = bevy::render::render_resource::TextureFormat::Rgba8Unorm;
                if let Some(d) = skin.descriptor() {
                    img.sampler = bevy::image::ImageSampler::Descriptor(d);
                }
                (0.0, images.add(img), self.image_with(assets, n.as_deref().unwrap_or(flat), skin))
            }
            Surface::Creature(t, n) => (
                3.0,
                self.image_with(assets, t.as_deref().unwrap_or(white), Sampling::Trilinear),
                self.image_with(assets, n.as_deref().unwrap_or(flat), Sampling::Trilinear),
            ),
            Surface::Cloth(t, n) => (
                1.0,
                self.image_with(assets, t.as_deref().unwrap_or(white), Sampling::Trilinear),
                self.image_with(assets, n.as_deref().unwrap_or(flat), Sampling::Trilinear),
            ),
            Surface::Hair(_, card) => {
                params.hair_mult = Vec4::from(card.diffuse);
                params.hair_alpha = Vec4::from(card.alpha);
                params.hair_colour = Vec3::from(card.colour).extend(card.specular);
                params.beard_alpha.x = card.threshold;
                (
                    2.0,
                    self.image_with(
                        assets,
                        card.texture.as_deref().unwrap_or(white),
                        Sampling::Trilinear,
                    ),
                    self.image_with(assets, flat, Sampling::Trilinear),
                )
            }
        };
        params.kind = Vec4::new(kind, 1.0, 0.4, WATER_HEIGHT);
        params.origin_exposure = origin.extend(0.0);
        params.world = Vec4::new(kenshi_world::WORLD_SIZE, 0.0, 0.0, 0.0);
        params.set_light(&self.light);
        KenshiCharacterMaterial {
            params,
            diffuse,
            normal,
            ambient: self.image_with(
                assets,
                "data/newland/land/overlaymaps/ambientmap.png",
                Sampling::Clamp,
            ),
            irradiance: self.image(assets, "data/materials/mp_irradiance.dds"),
            specularity: self.image(assets, "data/materials/mp_specularity.dds"),
            hair_map,
            beard_map,
            shadow_jitter: self.image(assets, "data/materials/white-noise.png"),
        }
    }
}
