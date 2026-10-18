//! Kenshi's fog volumes (port_map `effects`): the EFFECT_FOG_VOLUMEs an effect carries (its
//! "fog volumes"), drawn as data/materials/post/fog.material's FogSphere and FogBeam. See
//! kenshi_fog_volume.wgsl.
//!
//! Spec: F023263 (an EFFECT_FOG_VOLUME: its "type" (EffectVolumeType SPHERE 0, CYLINDER 1), its
//! offset ("position x/y/z") from the effect, a cylinder's other end ("position 2 x/y/z"),
//! "radius", "distance", "colour", "alpha" (clamped to 0 .. 1), "additive colour"; the volume a
//! child of the effect's node), F023586 (FogSphere: unit_sphere.mesh scaled by radius x 2),
//! F023587 / F023539 (FogBeam: cylinder.mesh from the offset to the other end, its y along them,
//! scaled (radius x 2, their distance, radius x 2); edgeBlur 1 / 300, F023566), F023565 (the
//! density: 1 / "distance", 1 for none), F023531 (the alpha with the colour; hidden at 0),
//! F023552 (additive: source alpha + one; else alpha blending), F023246 / F023247 / F023243
//! (the effect's "fog fade in duration" and "fog fade out duration" fade the volumes' alpha).
//! "ground movement" (F023263 keeps it beside the volume, its holder's +8) is not read by the
//! effect and fog volume code (F023240 - F023569 read no such flag), so a volume keeps its
//! offset from the effect.
//!
//! The map's fog features (kenshi_world::fog_features: the swamps' mists and the other standing
//! fogs) are drawn the same way, and their plane volumes as FogPlaneVolume (F023580 / F023581,
//! F023573: the hull of the seven planes, where each three meet inside the rest; fog.hlsl
//! fog_planes_fs: the ray's span between the planes, its edges softened by edgeBlur, brightened
//! by the sun's height, and beyond 12000 fogged by the atmosphere and the global fog).

use crate::terrain_material::MainLight;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction, Face, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

/// EffectVolumeType.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeShape {
    Sphere,
    Cylinder,
}

/// An EFFECT_FOG_VOLUME record (F023263).
#[derive(Debug, Clone)]
pub struct FogVolumeDef {
    pub shape: VolumeShape,
    /// Offset from the effect, and a cylinder's other end (units).
    pub offset: Vec3,
    pub offset2: Vec3,
    pub radius: f32,
    /// The density distance ("distance"): density 1 / it.
    pub distance: f32,
    /// Linear colour and alpha.
    pub colour: Vec4,
    pub additive: bool,
}

impl FogVolumeDef {
    pub fn from_record(r: &kenshi_data::GameData) -> FogVolumeDef {
        let f = |k: &str, d: f32| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|&v| v as f32)).unwrap_or(d);
        let argb = r.ints.get("colour").copied().unwrap_or(0xFFFFFF) as u32;
        let c = |s: u32| ((argb >> s) & 0xff) as f32 / 255.0;
        FogVolumeDef {
            shape: if r.ints.get("type").copied().unwrap_or(1) == 0 { VolumeShape::Sphere } else { VolumeShape::Cylinder },
            offset: Vec3::new(f("position x", 0.0), f("position y", 0.0), f("position z", 0.0)),
            offset2: Vec3::new(f("position 2 x", 0.0), f("position 2 y", 0.0), f("position 2 z", 0.0)),
            radius: f("radius", 1.0),
            distance: f("distance", 1.0),
            // ColourValue::setAsARGB: the record's colour as it is (no gamma)
            colour: Vec4::new(c(16), c(8), c(0), f("alpha", 1.0).clamp(0.0, 1.0)),
            additive: r.bools.get("additive colour").copied().unwrap_or(false),
        }
    }
}

/// One volume to draw this frame: its record, the effect's position (scene space) and the
/// fade (0 .. 1).
#[derive(Debug, Clone)]
pub struct FogVolumeDraw {
    pub def: FogVolumeDef,
    pub at: Vec3,
    pub fade: f32,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct FogVolumeParams {
    /// xyz: the sphere's centre or the beam's start (scene space); w: the radius the shader
    /// takes (FogSphere 0.48 x scale, FogBeam 0.4 x scale).
    pub start: Vec4,
    /// xyz: the beam's axis (unit); w: its length (0 for a sphere).
    pub axis: Vec4,
    /// rgb x light, alpha x fade.
    pub colour: Vec4,
    /// x: density; y: edgeBlur; z: additive (1); w: 1 a plane volume.
    pub consts: Vec4,
    /// xyz: the scene origin in world units (the sphere's break-up reads world positions).
    pub origin: Vec4,
    /// A plane volume's planes (world units: normal, w; inside where normal . p < w).
    pub planes: [Vec4; 7],
    /// The light a plane volume reads: sunDirectionReal (xyz) and sunColour.w; the horizon,
    /// fog parameters and colour (SharedSkyParams, SkyXFogParams, SharedFogParams).
    pub sun_real: Vec4,
    pub horizon: Vec4,
    pub fog: Vec4,
    pub fog_colour: Vec4,
}

impl FogVolumeParams {
    /// A plane volume's light (fog_planes_fs: sunDirectionReal, sunColour.w, the atmosphere's
    /// horizon and fog parameters, the global fog's colour).
    pub fn set_light(&mut self, light: &MainLight) {
        self.sun_real = light.sun_real.extend(light.sun_colour.w);
        self.horizon = light.horizon;
        self.fog = light.fog;
        self.fog_colour = light.fog_colour;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FogVolumeKey {
    additive: bool,
}

impl From<&KenshiFogVolumeMaterial> for FogVolumeKey {
    fn from(m: &KenshiFogVolumeMaterial) -> Self {
        FogVolumeKey { additive: m.params.consts.z > 0.5 }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(FogVolumeKey)]
pub struct KenshiFogVolumeMaterial {
    #[uniform(0)]
    pub params: FogVolumeParams,
}

impl KenshiFogVolumeMaterial {
    /// A fog feature's plane volume (F023580 / F023573) with its colour, density and edge,
    /// lit by `light`, and its hull (scene space): None when the planes close nothing.
    pub fn for_planes(colour: [f32; 4], distance: f32, edge: f32, planes: &[[f32; 4]; 7], light: &MainLight, origin: Vec3) -> Option<(KenshiFogVolumeMaterial, Mesh)> {
        let (corners, faces) = kenshi_world::fog_features::plane_hull(planes);
        let mut positions: Vec<[f32; 3]> = Vec::new();
        for f in faces.iter().filter(|f| f.len() >= 3) {
            for k in 1..f.len() - 1 {
                for c in [f[0], f[k], f[k + 1]] {
                    positions.push((Vec3::from(corners[c]) - origin).to_array());
                }
            }
        }
        if positions.is_empty() {
            return None;
        }
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        let mut params = FogVolumeParams {
            colour: Vec4::from(colour),
            // F023565: 1 / the distance; F023566: edgeBlur 1 / the edge
            consts: Vec4::new(if distance > 0.0 { 1.0 / distance } else { 1.0 }, if edge != 0.0 { 1.0 / edge } else { 0.0 }, 0.0, 1.0),
            origin: origin.extend(0.0),
            planes: planes.map(Vec4::from),
            ..default()
        };
        params.set_light(light);
        Some((KenshiFogVolumeMaterial { params }, mesh))
    }

    /// The material and the hull's transform (scene space) for `d`, lit by `light`.
    /// The hull is the volume's mesh as Kenshi scales it, a little larger so that the
    /// shader's exact shape stays inside it.
    pub fn for_volume(d: &FogVolumeDraw, light: &MainLight, origin: Vec3) -> (KenshiFogVolumeMaterial, Transform) {
        let v = &d.def;
        let density = if v.distance > 0.0 { 1.0 / v.distance } else { 1.0 };
        // fogValue's light: sunColour.w (SharedSkyParams)
        let lit = light.sun_colour.w;
        let colour = Vec4::new(v.colour.x * lit, v.colour.y * lit, v.colour.z * lit, v.colour.w * d.fade.clamp(0.0, 1.0));
        let start = d.at + v.offset;
        let (params, transform) = match v.shape {
            VolumeShape::Sphere => {
                let scale = v.radius * 2.0;
                (
                    FogVolumeParams {
                        start: start.extend(0.48 * scale),
                        axis: Vec4::ZERO,
                        colour,
                        consts: Vec4::new(density, 0.0, if v.additive { 1.0 } else { 0.0 }, 0.0),
                        origin: origin.extend(0.0),
                        ..default()
                    },
                    Transform::from_translation(start).with_scale(Vec3::splat(0.5 * scale * 1.05)),
                )
            }
            VolumeShape::Cylinder => {
                let end = d.at + v.offset2;
                let len = start.distance(end).max(1e-3);
                let axis = (end - start) / len;
                let scale = v.radius * 2.0;
                let rot = Quat::from_rotation_arc(Vec3::Y, axis);
                (
                    FogVolumeParams {
                        start: start.extend(0.4 * scale),
                        axis: axis.extend(len),
                        colour,
                        consts: Vec4::new(density, 1.0 / 300.0, if v.additive { 1.0 } else { 0.0 }, 0.0),
                        origin: origin.extend(0.0),
                        ..default()
                    },
                    Transform::from_translation((start + end) * 0.5)
                        .with_rotation(rot)
                        .with_scale(Vec3::new(0.5 * scale * 1.05, len * 1.02, 0.5 * scale * 1.05)),
                )
            }
        };
        (KenshiFogVolumeMaterial { params }, transform)
    }
}

impl Material for KenshiFogVolumeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_fog_volume.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// The hull's far side, without the depth test: the shader measures the volume against the
    /// scene's depth itself, so the volume shows from outside and from within.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(Face::Front);
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_compare = Some(CompareFunction::Always);
            ds.depth_write_enabled = Some(false);
        }
        if key.bind_group_data.additive
            && let Some(fragment) = descriptor.fragment.as_mut()
        {
            for t in fragment.targets.iter_mut().flatten() {
                t.blend = Some(BlendState {
                    color: BlendComponent { src_factor: BlendFactor::SrcAlpha, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
                    alpha: BlendComponent::OVER,
                });
            }
        }
        Ok(())
    }
}

/// Registers the fog volumes' material and its shader.
pub struct KenshiFogVolumePlugin;

impl Plugin for KenshiFogVolumePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "kenshi_fog_volume.wgsl");
        app.add_plugins(MaterialPlugin::<KenshiFogVolumeMaterial>::default());
    }
}
