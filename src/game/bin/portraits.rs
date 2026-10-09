//! The squad's portraits (Kenshi_PortraitCharacter.layout's PortraitImage): each squad member's
//! face drawn live by a small camera of its own into an image, the characters alone (their
//! meshes also on [`PORTRAIT_LAYER`]) over a clear background (the layout's backdrop images show
//! through).
//! As Kenshi's, a portrait is a still picture: taken once, and taken again only when the
//! character's look changes (a new body: clothes, armour, hair; a lost or robotic limb).
//! (stand-in) Kenshi renders its portraits in a portrait scene of their own; the camera's place
//! (in front of the head, a little above, the face filling the frame) is fitted by eye.
use super::Game;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use kenshi_render::character::Animated;
use kenshi_render::reflection::PortraitCamera;
use kenshi_render::character_material::KenshiCharacterMaterial;
use std::collections::HashMap;

/// The render layer only portrait cameras see (with the main scene's 0 on every character mesh).
pub const PORTRAIT_LAYER: usize = 3;
/// (stand-in for the HDR chain's exposure, EXPOSURE_KEY 0.55 over the adapted luminance) the
/// portrait's colours scaled by this when shown.
pub const EXPOSURE: f32 = 0.45;
/// A portrait's backdrop quad.
#[derive(Component)]
pub struct Backdrop;

/// The portrait image's size (pixels).
const SIZE: u32 = 128;

#[derive(Resource, Default)]
pub struct Portraits {
    /// Person -> its camera and image.
    pub by: HashMap<usize, (Entity, Handle<Image>)>,
    /// Others to have a portrait of (someone being talked to).
    pub extra: Vec<usize>,
    /// Person -> the backdrop behind its head.
    backdrops: HashMap<usize, Entity>,
    /// Person -> the look the picture was taken of, and frames left to keep drawing it (the
    /// body's meshes and textures load over a few frames).
    shot: HashMap<usize, (u64, u32)>,
    /// The backdrop's mesh and material, made once.
    backdrop: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
}

/// Character meshes also on the portrait layer.
pub fn layer_characters(mut commands: Commands, added: Query<Entity, (Added<MeshMaterial3d<KenshiCharacterMaterial>>, Without<RenderLayers>)>) {
    for e in &added {
        commands.entity(e).try_insert(RenderLayers::from_layers(&[0, PORTRAIT_LAYER]));
    }
}

/// A camera and image for each squad member without one (and none for those gone); each camera
/// put before its character's face.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    mut portraits: ResMut<Portraits>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    gui: Res<kenshi_gui::GuiRes>,
    assets: Res<AssetServer>,
    game: Res<Game>,
    bodies: Query<&Animated>,
    joints: Query<&GlobalTransform>,
    roots: Query<&GlobalTransform, Without<Camera>>,
    mut cams: Query<(&mut Transform, &mut Camera), (With<PortraitCamera>, Without<Backdrop>)>,
    mut drops: Query<&mut Transform, (With<Backdrop>, Without<PortraitCamera>)>,
) {
    // (stand-in for the image's alpha, which the UI does not keep) the layout's backdrop
    // (Kenshi_PortraitImage's Background "Normal": Kenshi_UI.png 456, 335, 60 x 60) drawn in the
    // portrait's scene, behind the head, unlit, brightened by 1 / EXPOSURE
    if portraits.backdrop.is_none() {
        let mut mesh = Mesh::from(Rectangle::new(1.0, 1.0));
        if let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
            for t in uv.iter_mut() {
                t[0] = (456.0 + t[0] * 60.0) / 1920.0;
                t[1] = (335.0 + t[1] * 60.0) / 1080.0;
            }
        }
        let tex = gui.0.file("Kenshi_UI.png").map(|f| assets.load(f.to_string()));
        let mat = StandardMaterial {
            base_color: Color::linear_rgb(1.0 / EXPOSURE, 1.0 / EXPOSURE, 1.0 / EXPOSURE),
            base_color_texture: tex,
            unlit: true,
            ..default()
        };
        portraits.backdrop = Some((meshes.add(mesh), materials.add(mat)));
    }
    let mut squad: Vec<usize> = game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    squad.extend(portraits.extra.iter().copied().filter(|&e| e < game.sim.folk.len()));
    portraits.by.retain(|id, (cam, _)| {
        let keep = squad.contains(id);
        if !keep {
            commands.entity(*cam).despawn();
        }
        keep
    });
    let Portraits { backdrops, backdrop, .. } = &mut *portraits;
    backdrops.retain(|id, e| {
        let keep = squad.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    if let Some((mesh, mat)) = backdrop.clone() {
        for &id in &squad {
            backdrops.entry(id).or_insert_with(|| {
                commands
                    .spawn((Backdrop, Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), RenderLayers::layer(PORTRAIT_LAYER), Transform::default()))
                    .id()
            });
        }
    }
    for &id in &squad {
        if portraits.by.contains_key(&id) {
            continue;
        }
        let mut image = Image::new_fill(
            Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[0; 8],
            TextureFormat::Rgba16Float,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
        let image = images.add(image);
        let cam = commands
            .spawn((
                PortraitCamera,
                Camera3d::default(),
                Camera {
                    order: -2,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    // the cleared alpha kept: the portrait's backdrop shows through
                    output_mode: bevy::camera::CameraOutputMode::Write {
                        blend_state: None,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                    },
                    ..default()
                },
                // raw HDR, as the main view draws it before Kenshi's exposure (see EXPOSURE)
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                bevy::core_pipeline::tonemapping::DebandDither::Disabled,
                RenderTarget::Image(image.clone().into()),
                Msaa::Off,
                Projection::Perspective(PerspectiveProjection { fov: 25f32.to_radians(), near: 0.5, far: 60.0, ..default() }),
                RenderLayers::layer(PORTRAIT_LAYER),
                Transform::default(),
            ))
            .id();
        portraits.by.insert(id, (cam, image));
    }
    // each camera before its face: 9 units in front of the head bone, 1 above, looking at it;
    // drawing only while a picture is being taken
    let Portraits { by, shot, backdrops, .. } = &mut *portraits;
    shot.retain(|id, _| by.contains_key(id));
    for (&id, (cam, _)) in by.iter() {
        let Some(&root) = game.bodies.get(id) else { continue };
        let limbs = game.sim.folk[id].limbs.iter().enumerate().fold(0u64, |m, (k, l)| m | ((l.is_some() as u64) << k))
            | (0..4).fold(0u64, |m, k| m | ((game.sim.limb_lost(id, k) as u64) << (4 + k)));
        let sig = root.to_bits() ^ (limbs << 56) ^ (game.body_scale.get(id).copied().unwrap_or(1.0).to_bits() as u64) << 20;
        let state = shot.entry(id).or_insert((!sig, 0));
        if state.0 != sig {
            *state = (sig, 30);
        }
        let taking = state.1 > 0;
        state.1 = state.1.saturating_sub(1);
        if let Ok((_, mut c)) = cams.get_mut(*cam)
            && c.is_active != taking
        {
            c.is_active = taking;
        }
        if !taking {
            continue;
        }
        let Ok(anim) = bodies.get(root) else { continue };
        let k = game.body_outfit.get(id).copied().unwrap_or(0);
        let head = game.outfits.get(k).and_then(|o| o.rig.names.iter().position(|n| n == "Bip01 Head"));
        let (Some(j), Ok(body)) = (head.and_then(|h| anim.joints.get(h)), roots.get(root)) else { continue };
        let Ok(h) = joints.get(*j) else { continue };
        let at = h.translation() + Vec3::Y * 0.6;
        let forward = (body.rotation() * Vec3::Z).with_y(0.0).normalize_or(Vec3::Z);
        let eye = at + forward * 9.0 + Vec3::Y * 1.0;
        if let Ok((mut t, _)) = cams.get_mut(*cam) {
            *t = Transform::from_translation(eye).looking_at(at, Vec3::Y);
        }
        // the backdrop 5 behind the head, filling the view (25 degrees at 14 away: 6.2)
        if let Some(d) = backdrops.get(&id)
            && let Ok(mut t) = drops.get_mut(*d)
        {
            let behind = at - (eye - at).normalize_or(Vec3::Z) * 5.0;
            // (the quad faces +z: turned away from the eye so its face is toward it)
            *t = Transform::from_translation(behind).looking_at(behind * 2.0 - eye, Vec3::Y).with_scale(Vec3::splat(6.6));
        }
    }
}
