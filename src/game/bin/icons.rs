//! Item icons. An item with "auto icon image" (or no icon file) has its icon drawn from its mesh
//! (rttIcons, F046427 / F046428): a camera with a 45 degree field of view (near 0.1, far 300,
//! F046422) on the z axis at "icon zoom" x 5 looking at the item, the item moved by "icon offset
//! H" and "icon offset V" and turned by "icon offset pitch", "yaw" - 90 and "roll" (a weapon:
//! -90, -90 and 90), into an image of the item's footprint at twice the cell size. The others use
//! their icon file.
//! (stand-in) The icon scene's light: one white sun from over the camera's shoulder and some
//! ambient (the game's icon scene manager lighting is not read).
use super::Game;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use kenshi_char::inventory::Item;
use kenshi_data::ItemType;
use kenshi_io::ogre_mesh;
use std::collections::{HashMap, HashSet};

/// The layer only icon cameras (and the items they draw) are on.
pub const ICON_LAYER: usize = 5;
/// Pixels per inventory cell in a drawn icon (the game: twice its cell size).
const CELL_PX: u32 = 64;
/// Frames an icon's camera is kept before its image is taken as drawn, once its textures are
/// in: enough for the renderer to build the pipelines a new mesh and material need (Bevy
/// compiles them in the background and draws nothing until they are; with the textures already
/// loaded, as a pack bull's bags worn in the world have them, three frames left the icon empty).
const FRAMES: u32 = 30;

#[derive(Resource, Default)]
pub struct ItemIcons {
    /// Drawn icons (and icon files) by item record.
    done: HashMap<String, Handle<Image>>,
    /// Items wanting an icon drawn.
    queue: Vec<String>,
    queued: HashSet<String>,
    /// Icons being drawn: (record, the camera and item entities, frames left, the textures to
    /// wait for).
    drawing: Vec<(String, Vec<Entity>, u32, Vec<UntypedHandle>)>,
    /// An icon finished since the inventory last asked.
    fresh: bool,
    light: bool,
    slot: u32,
}

impl ItemIcons {
    /// The item's icon: its file, or its drawn image (asked for the first time: None, drawn
    /// shortly).
    pub fn icon(&mut self, game: &Game, assets: &AssetServer, item: &Item) -> Option<Handle<Image>> {
        if let Some(h) = self.done.get(&item.record) {
            return Some(h.clone());
        }
        let rec = game.sim.data.get(&item.record)?;
        let auto = rec.bools.get("auto icon image").copied().unwrap_or(false);
        let file = item.icon.as_ref().and_then(|f| game.res.find(f));
        if !auto && let Some(f) = file {
            let h: Handle<Image> = assets.load(f);
            self.done.insert(item.record.clone(), h.clone());
            return Some(h);
        }
        if self.queued.insert(item.record.clone()) {
            self.queue.push(item.record.clone());
        }
        None
    }

    /// Whether an icon was finished since the last call (the inventory draws again).
    pub fn pending_redraw(&mut self) -> bool {
        std::mem::take(&mut self.fresh)
    }
}

/// Draw the icons asked for: a few started a frame, each kept for some frames, then taken.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    mut icons: ResMut<ItemIcons>,
    game: Res<Game>,
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !icons.light {
        icons.light = true;
        commands.spawn((
            DirectionalLight { illuminance: 3500.0, shadow_maps_enabled: false, ..default() },
            Transform::default().looking_to(Vec3::new(-0.4, -0.6, -1.0), Vec3::Y),
            RenderLayers::layer(ICON_LAYER),
        ));
    }
    // the ones drawn
    let mut finished = Vec::new();
    for (k, job) in icons.drawing.iter_mut().enumerate() {
        // the frames count once its textures are in (or failed)
        let ready = job.3.iter().all(|h| {
            assets.is_loaded_with_dependencies(h.id()) || matches!(assets.get_load_state(h.id()), Some(bevy::asset::LoadState::Failed(_)))
        });
        if !ready {
            continue;
        }
        job.2 = job.2.saturating_sub(1);
        if job.2 == 0 {
            finished.push(k);
        }
    }
    for k in finished.into_iter().rev() {
        let (_, entities, _, _) = icons.drawing.remove(k);
        for e in entities {
            commands.entity(e).despawn();
        }
        icons.fresh = true;
    }
    // new ones, a few a frame
    for _ in 0..4 {
        let Some(record) = icons.queue.pop() else { break };
        let data = game.sim.data.clone();
        let Some(rec) = data.get(&record) else { continue };
        let weapon = rec.item_type() == Some(ItemType::Weapon);
        let f = |k: &str| rec.floats.get(k).copied().or_else(|| rec.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
        let i = |k: &str| rec.ints.get(k).copied().unwrap_or(1).max(1) as u32;
        let (w, h) = (i("inventory footprint width"), i("inventory footprint height"));
        let Some(mesh) = rec.files.get("mesh").filter(|p| !p.is_empty()).and_then(|p| game.res.path(p)).and_then(|p| ogre_mesh::read(&p).ok()) else {
            if std::env::var("KENSHI_ICON_DEBUG").is_ok() {
                eprintln!("icon for {}: no mesh", rec.name);
            }
            continue;
        };
        let mat_rec = kenshi_render::appearance_material(&data, rec);
        let tex = |k: &str| mat_rec.and_then(|m| m.files.get(k)).filter(|p| !p.is_empty()).and_then(|p| game.res.find(p));
        let base_tex: Option<Handle<Image>> = tex("texture map").map(|p| assets.load(p));
        let normal_tex: Option<Handle<Image>> = tex("normal map").map(|p| assets.load(p));
        let wait: Vec<UntypedHandle> = base_tex.iter().chain(normal_tex.iter()).map(|h| h.clone().untyped()).collect();
        let material = materials.add(StandardMaterial {
            base_color_texture: base_tex,
            normal_map_texture: normal_tex,
            perceptual_roughness: 0.6,
            metallic: if weapon { 0.4 } else { 0.0 },
            ..default()
        });
        let mut image = Image::new_fill(
            Extent3d { width: w * CELL_PX, height: h * CELL_PX, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
        let image = images.add(image);
        // each icon's scene somewhere of its own, far from the world and the others
        icons.slot += 1;
        let base = Vec3::new(icons.slot as f32 % 64.0 * 400.0, -50_000.0, (icons.slot / 64) as f32 * 400.0);
        let (pitch, yaw, roll) = if weapon {
            (-90.0f32, -90.0f32, 90.0f32)
        } else {
            (f("icon offset pitch"), f("icon offset yaw"), f("icon offset roll"))
        };
        let rot = Quat::from_rotation_x(pitch.to_radians()) * Quat::from_rotation_y((yaw - 90.0).to_radians()) * Quat::from_rotation_z(roll.to_radians());
        let zoom = f("icon zoom").max(0.01);
        let cam = commands
            .spawn((
                Camera3d::default(),
                Camera { order: -3, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
                RenderTarget::Image(image.clone().into()),
                Msaa::Off,
                Projection::Perspective(PerspectiveProjection { fov: 45f32.to_radians(), near: 0.1, far: 300.0, ..default() }),
                AmbientLight { brightness: 250.0, ..default() },
                RenderLayers::layer(ICON_LAYER),
                Transform::from_translation(base + Vec3::Z * zoom * 5.0),
                kenshi_render::reflection::PortraitCamera,
            ))
            .id();
        let mut entities = vec![cam];
        let node = commands
            .spawn((Transform::from_translation(base + Vec3::new(f("icon offset H"), f("icon offset V"), 0.0)).with_rotation(rot), Visibility::default()))
            .id();
        for (_, m) in kenshi_render::to_bevy(&mesh) {
            let e = commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(material.clone()), RenderLayers::layer(ICON_LAYER), Transform::default())).id();
            commands.entity(node).add_child(e);
        }
        entities.push(node);
        if std::env::var("KENSHI_ICON_DEBUG").is_ok() {
            eprintln!("icon for {} ({}): {}x{} cells, zoom {zoom}, at {base}", rec.name, record, w, h);
        }
        icons.drawing.push((record.clone(), entities, FRAMES, wait));
        icons.done.insert(record, image);
    }
}
