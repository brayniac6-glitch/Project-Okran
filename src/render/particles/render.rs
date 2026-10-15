//! Particle systems in the scene: `ParticleEffect` entities hold a running system (updated by
//! the game); each frame they are told the camera (LOD, sorting) and each technique's particles
//! become a mesh drawn with its particle material (render queue 84: after the scene and the
//! atmosphere fog, blended, not writing depth, F033084).
//!
//! (stand-in) Ogre sorts the queue's transparent renderables by their node's distance; the
//! techniques of one system share it and keep their order: here each technique's mesh sits a
//! hair nearer the camera than the one before. Water (Kenshi's queues 81-82) is not kept
//! before particles.

use super::billboard::{self, Camera, Geometry};
use super::library::{Blend, ParticleLibrary};
use super::system::{RendererKind, System};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction,
    RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct ParticleParams {
    pub colour_mult: Vec4,
    pub flags: Vec4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParticleKey {
    blend: u8,
    depth_check: bool,
    depth_write: bool,
    bias: i32,
    slope_bias: i32,
}

impl From<&KenshiParticleMaterial> for ParticleKey {
    fn from(m: &KenshiParticleMaterial) -> Self {
        ParticleKey {
            blend: m.blend as u8,
            depth_check: m.depth_check,
            depth_write: m.depth_write,
            bias: m.depth_bias.0 as i32,
            slope_bias: m.depth_bias.1 as i32,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ParticleKey)]
pub struct KenshiParticleMaterial {
    #[uniform(0)]
    pub params: ParticleParams,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
    pub blend: Blend,
    pub depth_check: bool,
    pub depth_write: bool,
    pub depth_bias: (f32, f32),
    pub ambient: bool,
}

impl Material for KenshiParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://kenshi_render/render/particles/kenshi_particle.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/particles/kenshi_particle.wgsl".into()
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

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let k = key.bind_group_data;
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        // scene_blend (the same factors for alpha)
        let comp = |src, dst| BlendComponent { src_factor: src, dst_factor: dst, operation: BlendOperation::Add };
        let blend = match k.blend {
            b if b == Blend::Add as u8 => Some(BlendState {
                color: comp(BlendFactor::One, BlendFactor::One),
                alpha: comp(BlendFactor::One, BlendFactor::One),
            }),
            b if b == Blend::Alpha as u8 => Some(BlendState {
                color: comp(BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha),
                alpha: comp(BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha),
            }),
            b if b == Blend::Colour as u8 => Some(BlendState {
                color: comp(BlendFactor::Src, BlendFactor::OneMinusSrc),
                alpha: comp(BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha),
            }),
            b if b == Blend::Modulate as u8 => Some(BlendState {
                color: comp(BlendFactor::Dst, BlendFactor::Zero),
                alpha: comp(BlendFactor::DstAlpha, BlendFactor::Zero),
            }),
            _ => None,
        };
        if let Some(f) = descriptor.fragment.as_mut() {
            for t in f.targets.iter_mut().flatten() {
                t.blend = blend;
            }
        }
        if let Some(d) = descriptor.depth_stencil.as_mut() {
            d.depth_write_enabled = Some(k.depth_write);
            if !k.depth_check {
                d.depth_compare = Some(CompareFunction::Always);
            }
            // depth_bias pulls toward the camera (reverse depth here)
            d.bias.constant = k.bias;
            d.bias.slope_scale = k.slope_bias as f32;
        }
        Ok(())
    }
}

/// A running particle system in the scene, at its node (position and orientation, render
/// space); the game updates `system`.
#[derive(Component)]
pub struct ParticleEffect {
    pub system: System,
    /// The meshes drawing its techniques.
    draws: Vec<(Entity, Handle<Mesh>)>,
}

impl ParticleEffect {
    pub fn new(system: System) -> ParticleEffect {
        ParticleEffect { system, draws: Vec::new() }
    }

    /// The entities drawing it.
    pub fn draw_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.draws.iter().map(|(e, _)| *e)
    }

    /// Its draw entities, let go (to be despawned).
    pub fn take_draws(&mut self) -> Vec<Entity> {
        self.draws.drain(..).map(|(e, _)| e).collect()
    }
}

/// The install's particle templates and materials, their GPU materials, and the light the
/// ambient particles take.
#[derive(Resource)]
pub struct Particles {
    pub library: Arc<ParticleLibrary>,
    pub resources: Option<Arc<kenshi_io::resources::Resources>>,
    materials: HashMap<String, Handle<KenshiParticleMaterial>>,
    /// sunDirection's y (the sun, y clamped to 0).
    pub sun_y: f32,
    applied_sun_y: f32,
    /// The camera's far clip.
    pub far: f32,
    /// World minus scene positions (the effects' systems are placed in world positions).
    pub origin: Vec3,
    geometry: Geometry,
}

impl Particles {
    pub fn new(library: Arc<ParticleLibrary>, resources: Option<Arc<kenshi_io::resources::Resources>>) -> Particles {
        Particles {
            library,
            resources,
            materials: HashMap::new(),
            sun_y: 1.0,
            applied_sun_y: f32::NAN,
            far: 60000.0,
            origin: Vec3::ZERO,
            geometry: Geometry::default(),
        }
    }

    /// A system from a template, its node at `position` (stopped: start it).
    pub fn instance(&self, template: &str, seed: u32) -> Option<System> {
        self.library.templates.get(template).map(|t| System::instance(t, seed))
    }

    fn material(
        &mut self,
        name: &str,
        assets: &AssetServer,
        materials: &mut Assets<KenshiParticleMaterial>,
    ) -> Handle<KenshiParticleMaterial> {
        if let Some(h) = self.materials.get(name) {
            return h.clone();
        }
        let m = self.library.materials.get(name).cloned();
        let texture = m.as_ref().and_then(|m| m.texture.clone()).and_then(|t| {
            let path = match &self.resources {
                Some(r) => r.find(&t)?,
                None => format!("data/particles/textures/{t}"),
            };
            let (clamp, point) = (m.as_ref().is_some_and(|m| m.clamp), m.as_ref().is_some_and(|m| m.point_filtering));
            Some(assets.load_builder().with_settings(move |s: &mut bevy::image::ImageLoaderSettings| {
                s.is_srgb = false;
                let mode = if clamp {
                    bevy::image::ImageAddressMode::ClampToEdge
                } else {
                    bevy::image::ImageAddressMode::Repeat
                };
                let filter = if point { bevy::image::ImageFilterMode::Nearest } else { bevy::image::ImageFilterMode::Linear };
                s.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                    address_mode_u: mode,
                    address_mode_v: mode,
                    address_mode_w: mode,
                    mag_filter: filter,
                    min_filter: filter,
                    mipmap_filter: filter,
                    anisotropy_clamp: if point { 1 } else { 16 },
                    ..Default::default()
                });
            })
            .load(path))
        });
        let m = m.unwrap_or(super::library::ParticleMaterial {
            name: name.to_string(),
            texture: None,
            blend: Blend::Replace,
            depth_check: true,
            depth_write: true,
            depth_bias: (0.0, 0.0),
            ambient: false,
            depth_blend: false,
            clamp: false,
            point_filtering: false,
            coloured: false,
        });
        let texture = texture.unwrap_or_else(|| {
            assets.load_builder().with_settings(|s: &mut bevy::image::ImageLoaderSettings| s.is_srgb = false).load(
                match &self.resources {
                    Some(r) => r.find("white.dds").unwrap_or_else(|| "data/materials/white.dds".into()),
                    None => "data/materials/white.dds".into(),
                },
            )
        });
        let h = materials.add(KenshiParticleMaterial {
            params: ParticleParams {
                colour_mult: Vec4::ONE,
                flags: Vec4::new(
                    if m.coloured { 1.0 } else { 0.0 },
                    if m.depth_blend { 1.0 } else { 0.0 },
                    self.far,
                    0.0,
                ),
            },
            texture,
            blend: m.blend,
            depth_check: m.depth_check,
            depth_write: m.depth_write,
            depth_bias: m.depth_bias,
            ambient: m.ambient,
        });
        self.materials.insert(name.to_string(), h.clone());
        self.applied_sun_y = f32::NAN;
        h
    }
}

/// Tells the systems the camera and builds their meshes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn draw_particles(
    mut commands: Commands,
    mut particles: Option<ResMut<Particles>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<KenshiParticleMaterial>>,
    cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::reflection::ReflectionCamera>, Without<crate::interior_mask::MaskCamera>)>,
    mut effects: Query<&mut ParticleEffect>,
    mut fx: Option<ResMut<super::effects::Effects>>,
    mut draws: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<KenshiParticleMaterial>), Without<ParticleEffect>>,
) {
    let Some(particles) = particles.as_mut() else { return };
    let Some(cam) = cams.iter().next() else { return };
    let cam = Camera { position: cam.translation(), orientation: cam.to_scale_rotation_translation().1 };
    let cam_dir = cam.orientation * Vec3::NEG_Z;
    // the ambient particles' multiplier: clamp(sunDirection.y x 5 + 0.2, 0.1, 1)
    if particles.applied_sun_y != particles.sun_y {
        let mult = (particles.sun_y * 5.0 + 0.2).clamp(0.1, 1.0);
        for h in particles.materials.values() {
            if let Some(mut m) = materials.get_mut(h) {
                let a = if m.ambient { mult } else { 1.0 };
                m.params.colour_mult = Vec4::new(a, a, a, 1.0);
            }
        }
        particles.applied_sun_y = particles.sun_y;
    }
    let origin = particles.origin;
    let mut one = |effect: &mut ParticleEffect| {
        let sys = &mut effect.system;
        if sys.state == super::system::State::Started {
            sys.notify_camera(cam.position, cam_dir);
        }
        let node = sys.position;
        let toward = (cam.position - node).normalize_or_zero();
        let mut k = 0;
        let mut list: Vec<(String, Handle<Mesh>)> = Vec::new();
        let visible = sys.state == super::system::State::Started;
        if visible {
            sys.for_each_visible(|t, visuals, order| {
                let Some(r) = &t.renderer else { return };
                if order.is_empty() {
                    return;
                }
                let at = node + toward * (k as f32 * 0.01);
                let g = &mut particles.geometry;
                g.clear();
                match &r.kind {
                    RendererKind::Billboard(b) => billboard::billboards(
                        b,
                        Vec2::new(r.default_dims.x, r.default_dims.y),
                        visuals,
                        order,
                        &cam,
                        at,
                        g,
                    ),
                    RendererKind::Sphere => billboard::spheres(0.5 * r.default_dims.x, visuals, order, at, g),
                }
                let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, std::mem::take(&mut g.positions));
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, std::mem::take(&mut g.uvs));
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, std::mem::take(&mut g.colours));
                mesh.insert_indices(Indices::U32(std::mem::take(&mut g.indices)));
                let h = match effect.draws.get(k) {
                    Some((_, h)) => {
                        if let Some(mut m) = meshes.get_mut(h) {
                            *m = mesh;
                        }
                        h.clone()
                    }
                    None => meshes.add(mesh),
                };
                list.push((t.material.clone(), h));
                k += 1;
            });
        }
        // the draws: one entity per technique drawn, in order
        for (k, (material, mesh)) in list.into_iter().enumerate() {
            let at = node + toward * (k as f32 * 0.01);
            let mat = particles.material(&material, &assets, &mut materials);
            match effect.draws.get(k) {
                Some((e, _)) => {
                    if let Ok((mut tr, mut vis, mut mm)) = draws.get_mut(*e) {
                        tr.translation = at;
                        *vis = Visibility::Visible;
                        if mm.0 != mat {
                            mm.0 = mat;
                        }
                    }
                }
                None => {
                    let e = commands
                        .spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(mat),
                            Transform::from_translation(at),
                            NoFrustumCulling,
                            bevy::light::NotShadowCaster,
                            Visibility::Visible,
                        ))
                        .id();
                    effect.draws.push((e, mesh));
                }
            }
        }
        for (e, _) in effect.draws.iter().skip(k) {
            if let Ok((_, mut vis, _)) = draws.get_mut(*e) {
                *vis = Visibility::Hidden;
            }
        }
    };
    for mut e in &mut effects {
        one(&mut e);
    }
    // the weathers' effects (`effects`), and the draws of those dropped
    if let Some(fx) = fx.as_mut() {
        fx.for_each(origin, &mut one);
        for e in std::mem::take(&mut fx.dead_draws) {
            commands.entity(e).despawn();
        }
    }
}

/// Kenshi's particles.
pub struct KenshiParticlesPlugin;

impl Plugin for KenshiParticlesPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "kenshi_particle.wgsl");
        app.add_plugins(MaterialPlugin::<KenshiParticleMaterial>::default()).add_systems(
            PostUpdate,
            draw_particles.after(bevy::transform::TransformSystems::Propagate),
        );
    }
}
