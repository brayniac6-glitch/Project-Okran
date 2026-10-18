//! Kenshi's interior mask (main.compositor InteriorMask, common/interiormask.material and
//! interiormask.hlsl; the mask entities made by F037793 Building::setupInteriorMask from each
//! building's "interior mask" BUILDING_PART mesh, placed with the building): the render queues
//! 16 - 17 are drawn into rt_depth_range (PF_FLOAT16_GR, the screen's size) from the main camera,
//! cleared, first in the material scheme "back" (cull_hardware clockwise, colour_write g), then
//! in "front" (cull_hardware anticlockwise, colour_write r), each face writing its view depth + 1.
//! The masks are modelled facing inwards, so "back" keeps the far side of the nearest volume
//! and "front" its near side, in front of it by the depth test. The terrain, the map features'
//! terrain material and the foliage and grass (CLIP_INTERIOR in terrainfp4.hlsl, mapfeature.hlsl,
//! foliage.hlsl) clip a fragment nearer than the far side and farther than the near side
//! (`interior_clipped` in the shaders), so the ground and plants inside a building are not seen.
//!
//! Harness: Bevy's two passes cannot share the depth buffer, so each channel keeps its nearest
//! face by a min blend (cleared to `CLEAR`, no depth test) and the readers take a near side behind
//! the far side as none (0), as Kenshi's depth test leaves it when the camera is inside a volume.
//! The target follows the window's size; the readers clip only in a view of its size (not the
//! water reflection's). (Not ported: the clip on building parts, see
//! object_material.rs.) The masks are always drawn: F037793 keeps the mask
//! entity's load request (Building +0x340) until the building goes (F037860), nothing hides it.

use crate::reflection::MirrorProjection;
use crate::zone::{BuildingView, ZoneTag};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::image::ImageSampler;
use bevy::light::NotShadowCaster;
use bevy::mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorWrites,
    CompareFunction, Extent3d, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::shader::ShaderRef;

/// The mask camera's render layer (the masks' own).
pub const MASK_LAYER: usize = 2;
/// What the target is cleared to: no face (the min blend keeps any face nearer).
pub const CLEAR: f32 = 60000.0;

/// The mask's target and its two materials, and the scene origin the masks are placed from.
#[derive(Resource, Clone)]
pub struct InteriorMask {
    pub image: Handle<Image>,
    pub origin: Vec3,
    back: Handle<InteriorMaskMaterial>,
    front: Handle<InteriorMaskMaterial>,
}

/// Marks the mask camera (also in the render world: Kenshi's HDR chain skips its view).
#[derive(Component, Clone, bevy::render::extract_component::ExtractComponent)]
pub struct MaskCamera;

/// The pipeline key: the "back" scheme (else "front").
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaskKey {
    back: bool,
}

impl From<&InteriorMaskMaterial> for MaskKey {
    fn from(m: &InteriorMaskMaterial) -> Self {
        MaskKey { back: m.scheme.x > 0.5 }
    }
}

/// InteriorMask's material: x 1 for its "back" scheme, 0 for "front".
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(MaskKey)]
pub struct InteriorMaskMaterial {
    #[uniform(0)]
    pub scheme: Vec4,
}

impl Material for InteriorMaskMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://kenshi_render/render/kenshi_interior_mask.wgsl".into()
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
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let back = key.bind_group_data.back;
        // "back": cull_hardware clockwise (Ogre's default, the back faces culled), colour_write g;
        // "front": cull_hardware anticlockwise, colour_write r
        descriptor.primitive.cull_mode = Some(if back { Face::Back } else { Face::Front });
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_compare = Some(CompareFunction::Always);
            ds.depth_write_enabled = Some(false);
        }
        let min = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Min,
        };
        if let Some(f) = descriptor.fragment.as_mut() {
            for t in f.targets.iter_mut().flatten() {
                t.blend = Some(BlendState { color: min, alpha: min });
                t.write_mask = if back { ColorWrites::GREEN } else { ColorWrites::RED };
            }
        }
        Ok(())
    }
}

/// Registers the mask's material and shader and its systems.
pub struct InteriorMaskPlugin;

impl Plugin for InteriorMaskPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "kenshi_interior_mask.wgsl");
        app.add_plugins(MaterialPlugin::<InteriorMaskMaterial>::default());
        app.add_plugins(bevy::render::extract_component::ExtractComponentPlugin::<MaskCamera>::default());
        app.add_systems(PostUpdate, (add_masks, follow_window, follow_main_camera.before(bevy::transform::TransformSystems::Propagate)));
    }
}

/// The mask's target (`size`: the window's, in pixels) and camera; `origin` the scene's.
pub fn spawn_interior_mask(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    materials: &mut Assets<InteriorMaskMaterial>,
    size: UVec2,
    origin: Vec3,
) -> InteriorMask {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::nearest();
    let image = images.add(image);
    commands.spawn((
        MaskCamera,
        Camera3d::default(),
        Camera {
            // before the main view
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::LinearRgba(LinearRgba::new(CLEAR, CLEAR, CLEAR, CLEAR))),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        // raw floats through to the target
        bevy::camera::Hdr,
        Msaa::Off,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Projection::custom(MirrorProjection(PerspectiveProjection::default())),
        RenderLayers::layer(MASK_LAYER),
        Transform::default(),
    ));
    InteriorMask {
        image,
        origin,
        back: materials.add(InteriorMaskMaterial { scheme: Vec4::new(1.0, 0.0, 0.0, 0.0) }),
        front: materials.add(InteriorMaskMaterial { scheme: Vec4::ZERO }),
    }
}

/// The mask camera on the main camera each frame: its place, field of view, aspect and clips.
#[allow(clippy::type_complexity)]
fn follow_main_camera(
    main: Query<(&Transform, &Projection), (With<Camera3d>, Without<MaskCamera>, Without<crate::reflection::ReflectionCamera>, Without<crate::reflection::PortraitCamera>)>,
    mut cams: Query<(&mut Transform, &mut Projection), With<MaskCamera>>,
) {
    let Some((t, p)) = main.iter().next() else { return };
    let Projection::Perspective(main) = p else { return };
    for (mut mt, mut mp) in &mut cams {
        *mt = *t;
        if let Projection::Custom(c) = &mut *mp
            && let Some(m) = c.get_mut::<MirrorProjection>()
        {
            m.0 = main.clone();
        }
    }
}

/// rt_depth_range is a compositor texture the viewport's size (main.compositor), made again when
/// the viewport changes: the target follows the window's size (a mask of another size than the
/// view clips nothing, so a resized window lost the clip: the ground filled sunken buildings).
fn follow_window(
    mask: Option<Res<InteriorMask>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(mask), Ok(w)) = (mask, windows.single()) else { return };
    let want = UVec2::new(w.physical_width().max(1), w.physical_height().max(1));
    let Some(have) = images.get(&mask.image).map(|i| i.size()) else { return };
    if have != want
        && let Some(mut i) = images.get_mut(&mask.image)
    {
        i.resize(Extent3d { width: want.x, height: want.y, depth_or_array_layers: 1 });
    }
}

/// Each building with an interior gets its mask (F037793), drawn in both schemes; unloaded with
/// its zone.
fn add_masks(
    mut commands: Commands,
    mask: Option<Res<InteriorMask>>,
    views: Query<&BuildingView, Added<BuildingView>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(mask) = mask else { return };
    for v in &views {
        spawn_mask(&mut commands, &mut meshes, &mask, &v.interior.triangles, ZoneTag(v.zone));
    }
}

/// A mask (world placed triangles) drawn in both schemes, its entities carrying `tag`.
pub fn spawn_mask(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mask: &InteriorMask,
    triangles: &[[[f32; 3]; 3]],
    tag: impl Bundle + Clone,
) {
    let positions: Vec<[f32; 3]> = triangles
        .iter()
        .flatten()
        .map(|p| (Vec3::from(*p) - mask.origin).to_array())
        .collect();
    if positions.is_empty() {
        return;
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    let h = meshes.add(m);
    for mat in [&mask.back, &mask.front] {
        commands.spawn((
            tag.clone(),
            Mesh3d(h.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::IDENTITY,
            RenderLayers::layer(MASK_LAYER),
            NotShadowCaster,
        ));
    }
}
