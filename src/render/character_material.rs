//! Kenshi's character materials (port_map `render`): data/materials/deferred/character.hlsl
//! main_fs for skin (body and head), skin.hlsl main_fs for skinned clothing (glossMult 0: the
//! game never sets it on the Skinned material, only F046402 on objects; the colour map's
//! COLOURING and metalness when the piece is painted, `ClothPaint`, else colormap_default.dds:
//! none) and character.hlsl
//! hair_fs for hair (its colour composed into the image; specularMult the hair's "specular", F037428), lit by
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
    /// x: 1 with a vest (painted-on clothing) to lay over the skin; y: 1 painted clothing.
    pub vest: Vec4,
    /// Painted clothing's color1 and color2 (rgb, paint factor).
    pub paint1: Vec4,
    pub paint2: Vec4,
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
    /// A skin's painted-on clothing: its texture and normal map (the normal map's alpha lays it
    /// over the skin).
    #[texture(13)]
    pub vest: Handle<Image>,
    #[texture(14)]
    pub vest_normal: Handle<Image>,
    /// Painted clothing's colour map (Skinned's colorMap: trilinear).
    #[texture(15)]
    pub color_map: Handle<Image>,
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
        _images: &mut Assets<Image>,
        surface: &Surface,
        origin: Vec3,
    ) -> KenshiCharacterMaterial {
        let flat = "data/materials/flat.dds";
        let white = "data/materials/white.dds";
        let none = "data/materials/nothing.dds";
        let mut params = CharacterParams::default();
        let mut vest: Option<(Handle<Image>, Handle<Image>)> = None;
        let mut color_map = None;
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
            Surface::Vested(t, n, vd, vn) => {
                vest = Some((self.image_with(assets, vd, skin), self.image_with(assets, vn, skin)));
                params.vest = Vec4::new(1.0, 0.0, 0.0, 0.0);
                (0.0, self.image_with(assets, t.as_deref().unwrap_or(white), skin), self.image_with(assets, n.as_deref().unwrap_or(flat), skin))
            }
            Surface::Creature(t, n) => (
                3.0,
                self.image_with(assets, t.as_deref().unwrap_or(white), Sampling::Trilinear),
                self.image_with(assets, n.as_deref().unwrap_or(flat), Sampling::Trilinear),
            ),
            Surface::Cloth(t, n, paint) => (
                1.0,
                {
                    if let Some(pt) = paint {
                        color_map = Some(self.image_with(assets, &pt.color_map, Sampling::Trilinear));
                        params.vest.y = 1.0;
                        params.paint1 = Vec4::from(pt.colours[0]);
                        params.paint2 = Vec4::from(pt.colours[1]);
                    }
                    self.image_with(assets, t.as_deref().unwrap_or(white), Sampling::Trilinear)
                },
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
        // y: glossMult, never set on the Skinned material (0)
        params.kind = Vec4::new(kind, 0.0, 0.4, WATER_HEIGHT);
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
            vest: vest.as_ref().map_or_else(|| self.image_with(assets, none, Sampling::Border), |v| v.0.clone()),
            vest_normal: vest.map_or_else(|| self.image_with(assets, none, Sampling::Border), |v| v.1),
            color_map: color_map.unwrap_or_else(|| self.image_with(assets, "data/items/colormap_default.dds", Sampling::Trilinear)),
        }
    }
}
