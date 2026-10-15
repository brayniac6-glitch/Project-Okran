//! Kenshi's water reflection (main.compositor Water_Reflection, F050963, F050956, F050965,
//! F050971, F032930): a "ReflectionCamera" mirrored in the water plane renders a 512 x 512
//! A8R8G8B8 target (cleared, then the scene's render queues the "water reflection" option picks),
//! and the water multiplies its environment reflection by the target's red channel
//! (forward/water.hlsl, see kenshi_water.wgsl).
//!
//! At "water reflection" 1 (the default) the queues are 5 - 8 (SkyX's sky dome and clouds,
//! F040804) and 25 (the terrain, F046700): the sky in its colour and the terrain as its G-buffer's
//! first target, the albedo's luma (gbuffer.hlsl writeAlbedo, see kenshi_terrain.wgsl). Entities
//! in those queues carry `reflected()`.
//!
//! The camera (F050963): near 5, the main camera's orientation, field of view and aspect, its far
//! clip the view distance x 10 x the "reflection range" (F032930); reflected in the plane at the
//! water height and its near plane clipped to it (F050956).
//!
//! Harness stand-ins: Bevy cannot cull a mirrored view's faces the other way round, so the camera
//! is the unmirrored one at the reflected place (looking along the reflected direction, up
//! reflected) and the water reads its image flipped left to right; the near plane's clip at the
//! water is the terrain's discard under the water for views from under it; the target is drawn
//! every frame (Kenshi only while more than 100 of the water's pixels pass its occlusion query,
//! F050971).
//!
//! The levels' queues (F050965): 1 adds 5 - 8 and 25; 2 also 50 (Ogre's main queue: the building
//! parts, objects and characters, `reflect_main_queue`, drawn as their albedo's luma); 3 also 20
//! and the town manager's call with 1 (not ported: what goes in queue 20 is not found).

use bevy::asset::RenderAssetUsages;
use bevy::camera::{CameraProjection, RenderTarget, SubCameraView, visibility::RenderLayers};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::math::Vec3A;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

/// The reflection camera's render layer (the layer the reflected entities join).
pub const REFLECTION_LAYER: usize = 1;
/// The target's side (Water_Reflection: 512 x 512).
pub const TARGET_SIZE: u32 = 512;
/// The camera's near clip (F050963).
pub const NEAR: f32 = 5.0;

/// The reflection: the "water reflection" level (0 off), the camera's far clip, the target.
#[derive(Resource, Clone)]
pub struct WaterReflection {
    pub level: i32,
    pub far: f32,
    pub water_height: f32,
    pub image: Handle<Image>,
}

/// Marks the reflection camera.
#[derive(Component)]
pub struct ReflectionCamera;

/// A camera drawing a character's portrait (not the main view).
#[derive(Component)]
pub struct PortraitCamera;

/// The render layers of an entity the reflection draws (its queue is one the level picks).
pub fn reflected() -> RenderLayers {
    RenderLayers::from_layers(&[0, REFLECTION_LAYER])
}

/// The main camera's perspective with its aspect kept (Kenshi copies it onto the square target).
#[derive(Debug, Clone)]
pub struct MirrorProjection(pub PerspectiveProjection);

impl CameraProjection for MirrorProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.0.get_clip_from_view()
    }
    fn get_clip_from_view_for_sub(&self, sub_view: &SubCameraView) -> Mat4 {
        self.0.get_clip_from_view_for_sub(sub_view)
    }
    fn update(&mut self, _width: f32, _height: f32) {}
    fn far(&self) -> f32 {
        self.0.far
    }
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        self.0.get_frustum_corners(z_near, z_far)
    }
}

/// The reflection's target and camera, for "water reflection" `level` and "reflection range"
/// `range` with the "view distance" `view_distance`; the water at `water_height` (scene units).
pub fn spawn_reflection(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    level: i32,
    view_distance: f32,
    range: f32,
    water_height: f32,
) -> WaterReflection {
    let mut image = Image::new_fill(
        Extent3d {
            width: TARGET_SIZE,
            height: TARGET_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    // the water's texture unit: filtering bilinear, tex_address_mode clamp
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    let image = images.add(image);
    let r = WaterReflection {
        level,
        far: view_distance * 10.0 * range,
        water_height,
        image: image.clone(),
    };
    if level > 0 {
        commands.spawn((
            ReflectionCamera,
            Camera3d::default(),
            Camera {
                order: -1,
                // pass clear {}: black
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::Image(image.into()),
            Msaa::Off,
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            bevy::core_pipeline::tonemapping::DebandDither::Disabled,
            Projection::custom(MirrorProjection(PerspectiveProjection {
                near: NEAR,
                far: r.far,
                ..default()
            })),
            RenderLayers::layer(REFLECTION_LAYER),
            Transform::default(),
        ));
    }
    r
}

/// The reflection camera on the main camera each frame: its place and directions reflected in the
/// water plane, the main camera's field of view and aspect (F050963).
#[allow(clippy::type_complexity)]
pub fn follow_main_camera(
    reflection: Option<Res<WaterReflection>>,
    main: Query<(&Transform, &Projection), (With<Camera3d>, Without<ReflectionCamera>, Without<PortraitCamera>, Without<crate::interior_mask::MaskCamera>)>,
    mut cams: Query<(&mut Transform, &mut Projection), With<ReflectionCamera>>,
) {
    let Some(reflection) = reflection else { return };
    let Some((t, p)) = main.iter().next() else { return };
    let h = reflection.water_height;
    let mirror = |v: Vec3| Vec3::new(v.x, -v.y, v.z);
    for (mut rt, mut rp) in &mut cams {
        let at = Vec3::new(t.translation.x, 2.0 * h - t.translation.y, t.translation.z);
        *rt = Transform::from_translation(at).looking_to(mirror(*t.forward()), mirror(*t.up()));
        if let (Projection::Perspective(main), Projection::Custom(c)) = (p, &mut *rp)
            && let Some(m) = c.get_mut::<MirrorProjection>()
        {
            m.0.fov = main.fov;
            m.0.aspect_ratio = main.aspect_ratio;
        }
    }
}

/// "water reflection" 2 and up (F050965 adds queue 50): the building parts, objects and
/// characters are drawn in the reflection too.
#[allow(clippy::type_complexity)]
pub fn reflect_main_queue(
    mut commands: Commands,
    reflection: Option<Res<WaterReflection>>,
    added: Query<
        Entity,
        (
            Without<RenderLayers>,
            Or<(
                Added<MeshMaterial3d<crate::object_material::KenshiObjectMaterial>>,
                Added<MeshMaterial3d<crate::character_material::KenshiCharacterMaterial>>,
            )>,
        ),
    >,
) {
    if reflection.is_none_or(|r| r.level < 2) {
        return;
    }
    for e in &added {
        commands.entity(e).insert(reflected());
    }
}
