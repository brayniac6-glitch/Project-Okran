//! Kenshi's static object material (port_map `render`): data/materials/deferred/objects.hlsl's
//! pixel shader with the deferred main light, for building parts. See kenshi_object.wgsl.
//!
//! Spec: F046402 (the object material from a MATERIAL_SPEC: diffuseMap "texture map" (flat.dds /
//! white.dds when missing), normalMap "normal map", metalnessMap "metalness map" (black.dds),
//! tiling ("tile X", "tile Y"; (1, 1) when either is 0), glossMult "specular mult", threshold
//! "alpha threshold" / 255 (default 128); shader defines from flags: 1 COLOURING, 2 DUAL_TEXTURE,
//! 4 CONSTRUCTION, 8 TRANSPARENCY, 0x10 DOUBLESIDED, 0x20 EMISSIVE, 0x100 INTERIOR, 0x200
//! CLIP_INTERIOR), F046403 getObjectMaterial (a kind picks the shader and defines: 1
//! Triplanar, 3 DUAL_TEXTURE (with vertex colours), 4 TRANSPARENCY | DOUBLESIDED, 5 Triplanar
//! with DUAL_TEXTURE, 6 EMISSIVE alone; vertex colours add COLOURING; always CLIP_INTERIOR),
//! F041794 (a building part asks with DUST, except type 2; the dust colour is the "ground colour"
//! of the BIOMES at the part, F046402; the dust noise Turbulent.dds), CONSTRUCTION
//! (construction.material, objects.hlsl: a building being built is drawn with the scaffold
//! texture construction.dds at 10x its UVs blended in by the scaffold's alpha; above 1.2 x its
//! progress of its height only the scaffold's bars (alpha 0.7 on) are left; over its last fifth
//! the scaffold clears from the top down). CLIP_INTERIOR (kenshi_render::interior_mask) only on
//! grass and foliage: (stand-in) Kenshi's building parts carry it too (F046403 adds 0x200,
//! F046402 the define and the rt_interiormask unit), but how a building's own floors and
//! furniture inside its mask escape the clip is not found, so parts are not clipped.
//! (assumption) The biome at a point is the blend cell's slot weighing most there.

use crate::terrain_material::MainLight;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

/// The object shader's parameters (ObjectParams in kenshi_object.wgsl).
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct ObjectParams {
    pub tiling_gloss: Vec4,
    pub flags: Vec4,
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
    /// DUAL_TEXTURE, DUST, COLOURING (1 on).
    pub modes: Vec4,
    pub dust_colour: Vec4,
    pub dust_amount: Vec4,
    /// CONSTRUCTION: (constructionState, upperPos (the building's height), scaffoldTiling, the
    /// building's base height in the scene); off while the height is 0.
    pub construction: Vec4,
}

impl ObjectParams {
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
        self.flags.z = light.wetness;
        self.dust_amount = light.dust.extend(0.0);
    }
}

/// A MATERIAL_SPEC's object material settings (F046402).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectLook {
    pub texture: Option<String>,
    pub normal: Option<String>,
    pub metal: Option<String>,
    pub texture2: Option<String>,
    pub normal2: Option<String>,
    pub metal2: Option<String>,
    pub tiling: [f32; 2],
    pub gloss_mult: f32,
    pub threshold: f32,
    pub kind: i32,
    /// Kenshi's Grass material (foliage.hlsl grass_fs): the sprite's alpha cut at the
    /// threshold, the normal straight up, no gloss.
    pub grass: bool,
    /// A FOLIAGE_MESH's (placed by the foliage, not a building part: no DUST).
    pub foliage: bool,
}

impl ObjectLook {
    /// @fn F046402
    pub fn from_record(m: Option<&kenshi_data::GameData>, find: impl Fn(&str) -> Option<String>) -> ObjectLook {
        let file = |k: &str| {
            let name = m.and_then(|m| m.files.get(k)).filter(|s| !s.is_empty())?;
            let found = find(name);
            if found.is_none() && std::env::var("KENSHI_LOOK_DEBUG").is_ok() {
                eprintln!("look: {} {k} {name:?} not found", m.map_or("", |m| m.name.as_str()));
            }
            found
        };
        let f = |k: &str| m.and_then(|m| m.floats.get(k).copied());
        let (x, y) = (f("tile X").unwrap_or(1.0), f("tile Y").unwrap_or(1.0));
        let tiling = if x == 0.0 || y == 0.0 { [1.0, 1.0] } else { [x, y] };
        let threshold = m
            .and_then(|m| m.ints.get("alpha threshold").copied())
            .unwrap_or(0x80) as f32
            / 255.0;
        ObjectLook {
            texture: file("texture map"),
            normal: file("normal map"),
            metal: file("metalness map"),
            texture2: file("texture map 2"),
            normal2: file("normal map 2"),
            metal2: file("metalness map 2"),
            tiling,
            gloss_mult: f("specular mult").unwrap_or(1.0),
            threshold,
            kind: m.and_then(|m| m.ints.get("material type").copied()).unwrap_or(0),
            grass: false,
            foliage: false,
        }
    }

    /// A building part's material: its MATERIAL_SPEC "material type" is the BuildingShader
    /// (fcs.def: 0 default, 1 alpha "uses normal map alpha for transparency", 2 foliage
    /// "double sided foliage shader with alpha", 3 dual "blends between two texture sets", 4
    /// emissive), put into the shader kinds getObjectMaterial takes (F046403, MapFeatureMode):
    /// default 0, alpha 7 (TRANSPARENCY alone, UV mapped), foliage 4, dual 3, emissive 6.
    /// (Kenshi's own walls and gates, alpha, show their UV mapped detail, not the triplanar
    /// projection kind 1 would give.)
    pub fn for_building(mut self) -> Self {
        self.kind = match self.kind {
            1 => 7,
            2 => 4,
            3 => 3,
            4 => 6,
            _ => 0,
        };
        self
    }

    /// F046403: the Triplanar shader (types 1 and 5).
    pub fn triplanar(&self) -> bool {
        self.kind == 1 || self.kind == 5
    }
    /// TRANSPARENCY (type 4 with DOUBLESIDED; 7 a building's alpha shader alone).
    pub fn alpha(&self) -> bool {
        self.kind == 4 || self.kind == 7
    }
    pub fn double_sided(&self) -> bool {
        self.kind == 4
    }
    /// EMISSIVE alone (type 6).
    pub fn emissive(&self) -> bool {
        self.kind == 6
    }
    /// DUAL_TEXTURE (types 3 and 5, with vertex colours).
    pub fn dual(&self, vertex_colours: bool) -> bool {
        (self.kind == 3 || self.kind == 5) && vertex_colours
    }
    /// DUST on a building part (F041794: all but type 2; type 6 sets EMISSIVE alone). Grass
    /// (grass_fs) and foliage are not building parts.
    pub fn dust(&self) -> bool {
        self.kind != 2 && self.kind != 6 && !self.grass && !self.foliage
    }
    /// COLOURING (vertex colours; not with type 6).
    pub fn colouring(&self, vertex_colours: bool) -> bool {
        vertex_colours && self.kind != 6
    }
}

/// The pipeline key: double sided parts are drawn without culling.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectKey {
    double_sided: bool,
}

impl From<&KenshiObjectMaterial> for ObjectKey {
    fn from(m: &KenshiObjectMaterial) -> Self {
        ObjectKey {
            double_sided: m.params.flags.y > 0.5,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ObjectKey)]
pub struct KenshiObjectMaterial {
    #[uniform(0)]
    pub params: ObjectParams,
    #[texture(1)]
    #[sampler(2)]
    pub base: Handle<Image>,
    #[texture(3)]
    pub normal: Handle<Image>,
    #[texture(4)]
    #[sampler(13)]
    pub metal: Handle<Image>,
    #[texture(5)]
    #[sampler(14)]
    pub ambient: Handle<Image>,
    #[texture(6, dimension = "cube")]
    #[sampler(8)]
    pub irradiance: Handle<Image>,
    #[texture(7, dimension = "cube")]
    pub specularity: Handle<Image>,
    #[texture(9)]
    pub base2: Handle<Image>,
    #[texture(10)]
    pub normal2: Handle<Image>,
    #[texture(11)]
    pub metal2: Handle<Image>,
    #[texture(12)]
    pub dust_noise: Handle<Image>,
    /// The CSM shadows' jitter map (kenshi::lighting::sun_shadow).
    #[texture(15)]
    pub shadow_jitter: Handle<Image>,
    /// CONSTRUCTION's scaffold (grid_map, construction.dds).
    #[texture(16)]
    pub grid: Handle<Image>,
    /// The interior mask (CLIP_INTERIOR on grass and foliage, kenshi_render::interior_mask).
    #[texture(17)]
    pub interior_mask: Handle<Image>,
}

impl Material for KenshiObjectMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_object.wgsl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_cutout_prepass.wgsl".into()
    }

    // TRANSPARENCY parts cut pixels away: their prepass (depth, shadows) must cut them too
    fn alpha_mode(&self) -> AlphaMode {
        if self.params.tiling_gloss.w > 0.0 || self.params.construction.y > 0.0 {
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
        // back faces culled in every pass (the prepass too: an interior's inward ceiling must
        // leave no depth seen from above), both sides for DOUBLESIDED
        descriptor.primitive.cull_mode = if key.bind_group_data.double_sided {
            None
        } else {
            Some(bevy::render::render_resource::Face::Back)
        };
        Ok(())
    }
}
