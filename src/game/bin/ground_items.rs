//! Items on the ground (`kenshi_game::loot`) drawn by their "ground mesh" (else their "mesh"),
//! its appearance material's texture and normal map, where they lie; right-clicked, the
//! selection goes and picks one up.
//! (stand-in) The item's own material settings (shininess, colour tints) are not read.

use super::Game;
use bevy::prelude::*;
use kenshi_io::ogre_mesh;
use kenshi_render::object_material::KenshiObjectMaterial;
use kenshi_render::terrain_material::TerrainLook;
use kenshi_render::{appearance_material, to_bevy};
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct GroundItems {
    /// Ground item -> its entity.
    shown: HashMap<u64, Entity>,
    /// Item record -> its meshes and material (None: no mesh found).
    looks: HashMap<String, Option<(Vec<Handle<Mesh>>, Handle<KenshiObjectMaterial>)>>,
}

/// An item lying on the ground (its `GroundItem` number).
#[derive(Component)]
#[allow(dead_code)]
pub struct GroundMark(pub u64);

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut items: ResMut<GroundItems>,
    game: Res<Game>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut look: ResMut<TerrainLook>,
    mut objects: ResMut<Assets<KenshiObjectMaterial>>,
) {
    let origin = game.zone.origin;
    // gone from the ground: gone from the world
    let present: std::collections::HashSet<u64> = game.sim.ground.iter().map(|g| g.id).collect();
    items.shown.retain(|id, e| {
        let keep = present.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    for g in &game.sim.ground {
        if items.shown.contains_key(&g.id) {
            continue;
        }
        let record = g.item.record.clone();
        let look = items
            .looks
            .entry(record.clone())
            .or_insert_with(|| {
                let data = &game.sim.data;
                let r = data.get(&record)?;
                let path = ["ground mesh", "mesh"]
                    .iter()
                    .filter_map(|k| r.files.get(*k))
                    .find(|p| !p.is_empty())
                    .and_then(|p| game.res.path(p))?;
                let mesh = ogre_mesh::read(&path).ok()?;
                let mat = appearance_material(data, r);
                // (assumption) Kenshi's object material, as the scene's other objects (the
                // item shader is not ported)
                let o = kenshi_render::object_material::ObjectLook::from_record(mat, |f| game.res.find(f));
                let material = objects.add(look.object(&assets, &o, game.zone.origin, false, [0.0; 3]));
                let mut hs: Vec<Mesh> = to_bevy(&mesh).into_iter().map(|(_, m)| m).collect();
                for m in &mut hs {
                    if m.attribute(Mesh::ATTRIBUTE_TANGENT).is_none() {
                        let _ = m.generate_tangents();
                    }
                    // (assumption) a weapon has no "ground mesh": its held mesh, modelled upright
                    // along y, lies flat on its side
                    if r.item_type() == Some(kenshi_data::ItemType::Weapon) || r.item_type() == Some(kenshi_data::ItemType::Crossbow) {
                        m.rotate_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
                    }
                }
                Some((hs.into_iter().map(|m| meshes.add(m)).collect(), material))
            })
            .clone();
        let at = Vec3::from(g.at) - origin;
        let root = commands.spawn((GroundMark(g.id), Transform::from_translation(at).with_rotation(Quat::from_rotation_y(g.yaw)), Visibility::default())).id();
        match look {
            Some((hs, mat)) => {
                for h in hs {
                    let e = commands.spawn((Mesh3d(h), MeshMaterial3d(mat.clone()), Transform::default())).id();
                    commands.entity(root).add_child(e);
                }
            }
            None => {
                // (stand-in) an item with no mesh: a small box where it lies
                let e = commands.spawn((Mesh3d(meshes.add(Cuboid::new(2.0, 1.0, 2.0))), MeshMaterial3d(materials.add(Color::srgb(0.5, 0.45, 0.35))), Transform::from_xyz(0.0, 0.5, 0.0))).id();
                commands.entity(root).add_child(e);
            }
        }
        items.shown.insert(g.id, root);
    }
}

/// The ground item under the cursor (scene point `at`, from `ground_at`): the nearest within 12
/// units.
pub fn under(game: &Game, at: Vec3) -> Option<u64> {
    let w = at + game.zone.origin;
    game.sim.ground_item_near(w.to_array(), 12.0)
}

/// Kenshi's "highlight" (controls.cfg: Left Alt) held: the items on the ground within sight
/// glow and show their names. (stand-in) Kenshi's own highlight look is not ported: a glow on
/// their material and a label over each.
#[allow(clippy::too_many_arguments)]
pub fn highlight(
    mut commands: Commands,
    act: super::bindings::Actions,
    game: Res<Game>,
    items: Res<GroundItems>,
    mut objects: ResMut<Assets<KenshiObjectMaterial>>,
    cams: Query<(&Camera, &GlobalTransform), super::MainCam>,
    mut labels: Local<Vec<Entity>>,
    mut glowing: Local<bool>,
    gui: Res<kenshi_gui::GuiRes>,
    assets: Res<AssetServer>,
) {
    for e in labels.drain(..) {
        commands.entity(e).despawn();
    }
    // (testing) KENSHI_HIGHLIGHT: held on
    let on = act.held("highlight") || std::env::var("KENSHI_HIGHLIGHT").is_ok();
    if on != *glowing {
        *glowing = on;
        for (h, _) in items.looks.values().flatten().map(|(m, h)| (h, m)) {
            // the object shader's emissive brightness (its texture's own glow, by the normal
            // map's alpha) as the glow
            if let Some(mut m) = objects.get_mut(h) {
                m.params.flags.x = if on { 1.0 } else { 0.0 };
            }
        }
    }
    if !on {
        return;
    }
    let Ok((cam, at)) = cams.single() else { return };
    let font = gui.0.fonts.get("Kenshi_StandardFont_Medium").map(|f| (assets.load(f.file.clone()), f.screen_px()));
    let origin = game.zone.origin;
    for g in &game.sim.ground {
        let p = Vec3::from(g.at) - origin + Vec3::Y * 4.0;
        if p.distance(at.translation()) > 2500.0 {
            continue;
        }
        let Ok(v) = cam.world_to_viewport(at, p) else { continue };
        let mut text = TextFont { font_size: bevy::text::FontSize::Px(font.as_ref().map_or(14.0, |f| f.1)), ..default() };
        if let Some((h, _)) = &font {
            text.font = bevy::text::FontSource::Handle(h.clone());
        }
        let e = commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(v.x - 40.0), top: Val::Px(v.y - 10.0), ..default() },
                Text::new(g.item.name.clone()),
                text,
                TextColor(Color::srgb(0.95, 0.88, 0.55)),
                TextShadow::default(),
                GlobalZIndex(4),
            ))
            .id();
        labels.push(e);
    }
}
