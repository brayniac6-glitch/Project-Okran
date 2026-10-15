//! SkyX's skydome (port_map `sky`): data/materials/SkyX/SkyX_Skydome.hlsl on a sphere round the
//! camera at SkyX's skydome radius (0.95 x the far clip). See kenshi_sky.wgsl.

use crate::terrain_material::MainLight;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct SkyParams {
    /// xyz: toward the sun; w: SkyX's uTime (SkyX::update: half its time offset, see
    /// `drift_stars`).
    pub light: Vec4,
    /// x: the HDR composite's exposure.
    pub exposure: Vec4,
    /// The cloud layer: zenithLight, sunColour, uWindDirection (xy).
    pub zenith: Vec4,
    pub sun_colour: Vec4,
    pub wind: Vec4,
    /// CloudLayer::Options (F040804): (Height, Scale, DistanceAttenuation, DetailAttenuation),
    /// (HeightVolume, VolumetricDisplacement, DensityMultiplier, DensityOffset), (Darkness).
    pub cloud_a: Vec4,
    pub cloud_b: Vec4,
    pub cloud_c: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct KenshiSkyMaterial {
    #[uniform(0)]
    pub params: SkyParams,
    #[texture(1)]
    #[sampler(2)]
    pub starfield: Handle<Image>,
    #[texture(3)]
    pub clouds: Handle<Image>,
    #[texture(4)]
    pub clouds_normal: Handle<Image>,
    #[texture(5)]
    pub clouds_tile: Handle<Image>,
}

impl KenshiSkyMaterial {
    pub fn set_light(&mut self, light: &MainLight) {
        self.params.light = light.sun_real.extend(self.params.light.w);
        self.params.exposure.x = light.exposure;
        self.params.zenith = light.zenith.extend(0.0);
        self.params.cloud_b.z = light.clouds.x;
        self.params.cloud_b.w = light.clouds.y;
        self.params.cloud_c.x = light.clouds.z;
        self.params.sun_colour = light.sun_colour;
    }
}

/// SkyX's time (SkyX_x64 SkyX::update: while its time multiplier is not 0 the offset grows by it
/// x the frame time and uTime is half the offset): F040805 sets the multiplier each frame to
/// 0.0091667 x the game speed (0x142134810), 0 while paused. `offset` is kept by the caller.
pub fn drift_stars(offset: &mut f32, dt: f32, speed: f32, paused: bool, skies: &mut Assets<KenshiSkyMaterial>) {
    let mult = if paused { 0.0 } else { speed * 0.009_166_667 };
    if mult == 0.0 {
        return;
    }
    *offset += mult * dt;
    for (_, m) in skies.iter_mut() {
        m.params.light.w = *offset * 0.5;
    }
}

impl Material for KenshiSkyMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_sky.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // seen from inside; SkyX's dome writes no depth (the moons sit behind it at the far
        // plane)
        descriptor.primitive.cull_mode = None;
        if let Some(d) = descriptor.depth_stencil.as_mut() {
            d.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// Marks the skydome, which stays centred on the camera.
#[derive(Component)]
pub struct SkyDome;

/// Spawn the skydome for a camera whose far clip is `far`.
pub fn spawn_sky_dome(
    commands: &mut Commands,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<KenshiSkyMaterial>,
    light: &MainLight,
    far: f32,
) {
    let starfield: Handle<Image> = assets
        .load_builder()
        .with_settings(|s: &mut bevy::image::ImageLoaderSettings| s.is_srgb = false)
        .load("data/materials/SkyX/SkyX_Starfield.dds");
    let raw = |path: &str| -> Handle<Image> {
        assets
            .load_builder()
            .with_settings(|s: &mut bevy::image::ImageLoaderSettings| s.is_srgb = false)
            .load(path.to_string())
    };
    // the cloud layer's options as F040804 adds it (clear weather: unchanged since)
    let mut m = KenshiSkyMaterial {
        params: SkyParams {
            cloud_a: Vec4::new(100.0, 0.001, 0.05, 1.0),
            cloud_b: Vec4::new(0.25, 0.01, 3.0, -0.8),
            cloud_c: Vec4::ZERO,
            ..default()
        },
        starfield,
        clouds: raw("data/materials/SkyX/Clouds.dds"),
        clouds_normal: raw("data/materials/SkyX/CloudsNormal.dds"),
        clouds_tile: raw("data/materials/SkyX/CloudsTile.dds"),
    };
    m.set_light(light);
    commands.spawn((
        SkyDome,
        Mesh3d(meshes.add(Sphere::new(far * 0.95).mesh().uv(64, 32))),
        MeshMaterial3d(materials.add(m)),
        Transform::default(),
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
        bevy::camera::visibility::NoFrustumCulling,
        // render queue 5: in the water reflection
        crate::reflection::reflected(),
    ));
}

/// The main camera (not the water reflection's).
type MainCamera = (With<Camera3d>, Without<SkyDome>, Without<crate::reflection::ReflectionCamera>, Without<crate::interior_mask::MaskCamera>);

/// Keep the skydome on the camera.
pub fn follow_camera(
    cams: Query<&Transform, MainCamera>,
    mut domes: Query<&mut Transform, With<SkyDome>>,
) {
    let Some(cam) = cams.iter().next() else {
        return;
    };
    for mut t in &mut domes {
        t.translation = cam.translation;
    }
}

pub struct KenshiSkyPlugin;

impl Plugin for KenshiSkyPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "kenshi_sky.wgsl");
        app.add_plugins(MaterialPlugin::<KenshiSkyMaterial>::default())
            .add_systems(PostUpdate, follow_camera.before(TransformSystems::Propagate));
    }
}
