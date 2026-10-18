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

/// The item lying under the cursor on screen (within 22 pixels of it, the nearest): items on a
/// floor or a platform over the ground included, which the ground under the cursor misses.
pub fn under_cursor(cam: &Camera, at: &GlobalTransform, game: &Game, cursor: Vec2) -> Option<u64> {
    let eye = at.translation();
    game.sim
        .ground
        .iter()
        .filter_map(|g| {
            let p = Vec3::from(g.at) - game.zone.origin + Vec3::Y * 1.0;
            if p.distance(eye) > 3000.0 {
                return None;
            }
            let d = cam.world_to_viewport(at, p).ok()?.distance(cursor);
            (d < 22.0).then_some((g.id, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// An item drop label (Kenshi_ItemDropLabel): the ground item it names.
#[derive(Component)]
pub struct DropLabel(pub u64);

/// A drop label's name text and its colour (the item's, `item_tip::item_colour`).
#[derive(Component)]
pub struct LabelLook {
    text: Option<Entity>,
    colour: Color,
}

/// kenshi_colours.xml's "DropItemLabelTextSelected" and "DropItemLabelBackSelected" (#66ff66):
/// a label under the mouse (F024538).
const LABEL_SELECTED: Color = Color::srgb(0.4, 1.0, 0.4);

/// @fn F024557
///
/// Kenshi's "highlight" (controls.cfg: Left Alt) held: an item drop label
/// (Kenshi_ItemDropLabel.layout) over each item on the ground in sight, its name and, for more
/// than one, the count in brackets ("{1,num}"), the label as wide as its text; the items glow.
/// Clicking a label (F024551) sends the first selected character to pick the item up.
/// The label's text and its skin in the item's colour (F024557, F024540: `item_tip::item_colour`),
/// under the mouse both #66ff66 (F024538). (not ported) An item someone owns (its owner not the
/// player's faction nor "not real", F024557) is stolen if taken: its label's back #ff6666, its
/// text #ff6666 under the mouse ("DropItemLabelBackSelectedStealing", "...TextSelectedStealing");
/// the rewrite's ground items belong to no one. The glow is the object shader's emissive,
/// Kenshi's own highlight look not being ported.
#[allow(clippy::too_many_arguments)]
pub fn highlight(
    mut commands: Commands,
    act: super::bindings::Actions,
    mut game: ResMut<Game>,
    items: Res<GroundItems>,
    mut objects: ResMut<Assets<KenshiObjectMaterial>>,
    cams: Query<(&Camera, &GlobalTransform), super::MainCam>,
    mut labels: Local<HashMap<u64, Entity>>,
    mut glowing: Local<bool>,
    gui: Res<kenshi_gui::GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    mut nodes: Query<&mut Node>,
    pressed: Query<(&Interaction, &DropLabel), Changed<Interaction>>,
    looks: Query<(Entity, &Interaction, &LabelLook), Changed<Interaction>>,
) {
    // F024538 / F024540: a label under the mouse, its text and back in #66ff66, else in its
    // item's colour
    for (e, i, l) in &looks {
        let c = if *i == Interaction::None { l.colour } else { LABEL_SELECTED };
        if let Some(t) = l.text {
            commands.queue(kenshi_gui::SetTextColour(t, c));
        }
        commands.queue(kenshi_gui::SetColour(e, c));
    }
    // a game loaded in the program: its own labels
    if game.is_added() {
        labels.clear();
        *glowing = false;
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
    // a label clicked: picked up (F024551)
    if on
        && let Some(id) = pressed.iter().find(|(i, _)| **i == Interaction::Pressed).map(|(_, l)| l.0)
    {
        let selected = game.selected.clone();
        game.sim.order_pick_item(&selected, id);
    }
    let Ok((cam, at)) = cams.single() else { return };
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let origin = game.zone.origin;
    let mut seen = std::collections::HashSet::new();
    if on {
        for g in &game.sim.ground {
            let p = Vec3::from(g.at) - origin + Vec3::Y * 4.0;
            if p.distance(at.translation()) > 2500.0 {
                continue;
            }
            let Ok(v) = cam.world_to_viewport(at, p) else { continue };
            seen.insert(g.id);
            let e = *labels.entry(g.id).or_insert_with(|| {
                let text = if g.item.quantity > 1 { format!("{} ({})", g.item.name, g.item.quantity) } else { g.item.name.clone() };
                let mut defs = gui.0.layout("Kenshi_ItemDropLabel.layout");
                // as wide as its text (the layout's height kept)
                let px = gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or(14.0, |f| f.screen_px());
                let w = (text.chars().count() as f32 * px * 0.52 + px) / size[0];
                if let Some(d) = defs.first_mut()
                    && let kenshi_gui::res::Pos::Real(r) = d.pos
                {
                    d.pos = kenshi_gui::res::Pos::Real([0.0, 0.0, w.max(r[2] * 0.5), r[3]]);
                }
                let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
                let colour = super::hud_ui::tag(super::item_tip::item_colour(&game.sim.data, &g.item));
                let name = ui.get("ItemNameText");
                if let Some(t) = name {
                    commands.queue(kenshi_gui::SetCaption(t, text));
                    commands.queue(kenshi_gui::SetTextColour(t, colour));
                }
                let root = ui.roots[0];
                commands.queue(kenshi_gui::SetColour(root, colour));
                commands.entity(root).insert((DropLabel(g.id), LabelLook { text: name, colour }, Interaction::default(), Button, GlobalZIndex(4)));
                root
            });
            if let Ok(mut n) = nodes.get_mut(e) {
                let w = match n.width {
                    Val::Px(w) => w,
                    _ => 60.0,
                };
                n.left = Val::Px(v.x - w * 0.5);
                n.top = Val::Px(v.y - 12.0);
            }
        }
    }
    labels.retain(|id, e| {
        let keep = seen.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
}
