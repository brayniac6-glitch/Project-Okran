//! Kenshi's moons (port_map `sky`): two planet01.mesh entities in the sky, materials "Moon" and
//! "Moon2" (data/materials/forward/moon.material, moon.hlsl). See kenshi_moon.wgsl.
//!
//! Spec: F040804 (the planets: planet01.mesh; "Moon" toward (1, 0.3, -1) with size 35 and turn 1;
//! "Moon2" toward (1, 0.14, -0.7) with size 10 and turn 4.731), F040777 (a planet: its direction
//! normalised, size, turn / 1440 per game minute), F040808 (each frame: at the camera plus the
//! direction x d, d = (near + far) / 2 x (1 - tan 0.01 deg); scaled by size x d x tan 0.01 deg;
//! turned about Y by the game's minutes ((day x 24 + hour) x 60 + minute) x turn / 1440 rad),
//! F040806 (the planets are updated each frame). (assumption) The game's day count starts at 0.

use crate::terrain_material::MainLight;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct MoonParams {
    pub sun_real: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct KenshiMoonMaterial {
    #[uniform(0)]
    pub params: MoonParams,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl Material for KenshiMoonMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_moon.wgsl".into()
    }

    // drawn after the scene (moon.material: depth_write off), alpha 1
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
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // depth_write off; the shader puts it at the far plane, behind everything drawn
        if let Some(d) = descriptor.depth_stencil.as_mut() {
            d.depth_write_enabled = Some(false);
            d.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        Ok(())
    }
}

/// One of the planets (F040777).
#[derive(Component, Debug, Clone, Copy)]
pub struct Planet {
    pub direction: Vec3,
    pub size: f32,
    /// Radians per game minute.
    pub turn: f32,
}

/// tan(0.01 degrees) (F040808).
fn tan_size() -> f32 {
    0.01f32.to_radians().tan()
}

/// Spawn the two moons (F040804).
pub fn spawn_moons(
    commands: &mut Commands,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<KenshiMoonMaterial>,
    install: &std::path::Path,
) {
    let Ok(m) = kenshi_io::ogre_mesh::read(&install.join("data/materials/planet01.mesh")) else {
        return;
    };
    let parts: Vec<Handle<Mesh>> = crate::to_bevy(&m)
        .into_iter()
        .map(|(_, mesh)| meshes.add(mesh))
        .collect();
    for (texture, direction, size, turn) in [
        ("data/materials/moon_HI.dds", Vec3::new(1.0, 0.3, -1.0), 35.0, 1.0),
        ("data/materials/moon2_HI.dds", Vec3::new(1.0, 0.14, -0.7), 10.0, 4.731),
    ] {
        let mat = materials.add(KenshiMoonMaterial {
            params: MoonParams::default(),
            texture: assets
                .load_builder()
                .with_settings(|s: &mut bevy::image::ImageLoaderSettings| s.is_srgb = false)
                .load(texture.to_string()),
        });
        let planet = Planet {
            direction: direction.normalize(),
            size,
            turn: turn / 1440.0,
        };
        let root = commands
            .spawn((planet, Transform::default(), Visibility::default()))
            .id();
        for mesh in &parts {
            let e = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat.clone()),
                    bevy::light::NotShadowCaster,
                    bevy::light::NotShadowReceiver,
                    bevy::camera::visibility::NoFrustumCulling,
                ))
                .id();
            commands.entity(root).add_child(e);
        }
    }
}

/// F040808 for every planet: `minutes` is the game's minute count, `near`/`far` the camera's
/// clip distances.
pub fn place_planets(
    planets: &mut Query<(&Planet, &mut Transform), Without<Camera3d>>,
    camera: Vec3,
    near: f32,
    far: f32,
    minutes: f32,
) {
    let f = tan_size();
    let d = (near + far) * 0.5;
    let d = d - f * d;
    for (p, mut t) in planets.iter_mut() {
        t.translation = camera + p.direction * d;
        t.scale = Vec3::splat(p.size * d * f);
        t.rotation = Quat::from_rotation_y(minutes * p.turn);
    }
}

/// Light the moons with the sun.
pub fn relight_moons(light: &MainLight, materials: &mut Assets<KenshiMoonMaterial>) {
    for (_, m) in materials.iter_mut() {
        m.params.sun_real = light.sun_real.extend(0.0);
    }
}
