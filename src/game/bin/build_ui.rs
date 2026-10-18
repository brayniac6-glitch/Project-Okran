//! Build mode (BuildModeWindow, Kenshi_BuildWindow.layout; "toggle_build": B, controls.cfg):
//! the categories and buildings the player's research allows, the chosen one's figures (F036238)
//! and description, and the commands text (F036214). The chosen building follows the cursor as a
//! ghost: left click places it, right click clears a placed one, "build_rotate_left" /
//! "build_rotate_right" (, and .) turn it, "build_undo" (Backspace) takes back the last, Confirm
//! ("build_apply": Space) makes the placed ones construction sites. A site is built by ordering
//! characters to it (a right click on it with them selected, `Sim::order_build`).
//! The player's buildings are drawn here: sites see-through, built ones with their parts'
//! object materials.
//! A placed building is drawn with Kenshi's construction material (the scaffold, see
//! kenshi_render::object_material) at its progress.
//! (stand-in) The ghost is drawn green (red where it can't go) (Kenshi's PreviewBuilding look is
//! not ported); "Constructing n%" is shown over a site in place of the game's progress bar.
//! Interior furniture ("is interior furniture") goes only inside a built building shell of the
//! player's (one with an "interior"), on its floor.
//! (assumption) A place is refused where the ground under the footprint slopes more than the
//! building's "max slope" (degrees), it overlaps another building, or it is under water; inside
//! a shell, where it overlaps other furniture; "inside" is within 60% of the shell's footprint
//! radius (the interior's walls are not tested); the turn while a rotate key is held is 90
//! degrees a second.
use super::{Game, MainCam, ground_at, hud_ui};
use bevy::camera::primitives::MeshAabb;
use bevy::prelude::*;
use kenshi_game::base::{self, PlayerBuilding};
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption, SetVisible};
use kenshi_render::object_material::{KenshiObjectMaterial, ObjectLook};
use kenshi_render::terrain_material::TerrainLook;
use std::collections::HashMap;

/// The list rows' text colours (Kenshi_ListBoxItemSkin: normal, checked).
const ROW_TEXT: Color = Color::srgb(0.078, 0.031, 0.024);
const ROW_CHECKED: Color = Color::srgb(0.626, 0.605, 0.578);
/// The invalid placement message (F036214's caller).
const CANT_PLACE: &str = "Oi, You can't place that there.";

#[derive(Resource, Default)]
pub struct BuildMode {
    open: Option<Open>,
    /// Each building record's meshes: (mesh, look, has vertex colours) and its footprint
    /// (half extents in x and z, building frame).
    meshes: HashMap<String, (Vec<PartMesh>, Vec2, f32)>,
    /// The player's buildings drawn: per building, its root, the state it was drawn in and its
    /// construction materials (updated with its progress).
    shown: Vec<(Entity, DrawnAs, Vec<Handle<KenshiObjectMaterial>>)>,
    /// The ghost's and sites' materials.
    looks: Option<Looks>,
    /// The "Constructing" labels over sites, by building.
    labels: HashMap<usize, Entity>,
}

struct Looks {
    ok: Handle<StandardMaterial>,
    bad: Handle<StandardMaterial>,
}

/// The "zone" the player's buildings' parts are marked with (BuildingFloor: their building is
/// the player's building's number).
pub const PLAYER_ZONE: (i32, i32) = (i32::MIN, i32::MIN);

/// A door of one of the player's buildings (its gate): the door state it shows, moved as it
/// opens (kenshi_game::doors), in its building's frame.
#[derive(Component)]
pub struct PlayerDoorPiece(pub usize);

#[derive(Clone, PartialEq)]
struct DrawnAs {
    record: String,
    at: [f32; 3],
    yaw: f32,
    done: bool,
    /// How many door states it has (its doors drawn once they exist).
    doors: usize,
}

struct Open {
    ui: Built,
    categories: Vec<String>,
    cat: usize,
    /// The chosen category's buildings (record ids).
    list: Vec<String>,
    chosen: Option<usize>,
    /// The list rows spawned (despawned on a refill).
    rows: Vec<Entity>,
    /// The ghost: its root, record and whether it may go where it is.
    ghost: Option<(Entity, String, bool)>,
    yaw: f32,
    /// The message shown and its time left.
    message: Option<(Entity, f32)>,
    fonts: ((Handle<Font>, f32), (Handle<Font>, f32)),
    /// A wall being laid (PreviewBuilding_Wall): where the line starts (scene units).
    wall_start: Option<Vec3>,
}

impl BuildMode {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// A list row: which list (0 categories, 1 buildings) and its index.
#[derive(Component)]
pub struct ListRow(pub u8, pub usize);

/// One mesh of a building: its handle, look, whether it has vertex colours, its "building
/// floor", whether one of its "interior" parts, and its bounds in the building's frame.
#[derive(Clone)]
pub struct PartMesh {
    pub mesh: Handle<Mesh>,
    pub look: ObjectLook,
    pub colours: bool,
    pub floor: i32,
    pub interior: bool,
    pub lo: Vec3,
    pub hi: Vec3,
    /// A part that moves (a turret's head, a wind generator's blades, a machine's spinner): its
    /// mesh left in its own frame, and its node chain and spins (kenshi_render::zone::Swivel).
    pub moves: Option<(Vec<(Vec3, Option<i32>)>, Vec<kenshi_render::zone::SpinDef>)>,
}

/// Where a part stands at rest in its building's frame: a moving part (its mesh in its own
/// frame) by its node chain, the rest where their mesh already is.
fn rest_of(moves: &Option<(Vec<(Vec3, Option<i32>)>, Vec<kenshi_render::zone::SpinDef>)>) -> Transform {
    moves.as_ref().map_or(Transform::default(), |(chain, _)| chain.iter().fold(Transform::IDENTITY, |t, (o, _)| t * Transform::from_translation(*o)))
}

/// Load a building's part meshes once (its parts, and its interior's).
/// A NEST_ITEM's mesh (its "ground mesh", or with `mesh` above 0 that file's numbered
/// variant, F048851: "name" + n + ".mesh") in its "material", cached by record and number.
fn debris_meshes<'a>(cache: &'a mut HashMap<String, (Vec<PartMesh>, Vec2, f32)>, game: &Game, record: &str, mesh: i32, meshes: &mut Assets<Mesh>) -> &'a Vec<PartMesh> {
    &cache
        .entry(format!("{record}#{mesh}"))
        .or_insert_with(|| {
            let mut out = Vec::new();
            if let Some(r) = game.sim.data.get(record)
                && let Some(file) = r.files.get("ground mesh").filter(|f| !f.is_empty())
            {
                let name = std::path::Path::new(file).file_name().map_or(file.clone(), |n| n.to_string_lossy().into_owned());
                let name = if mesh > 0 { format!("{}{mesh}.mesh", name.trim_end_matches(".mesh")) } else { name };
                let material = r.refs("material").first().and_then(|(m, _)| game.sim.data.get(m));
                if let Some(path) = game.res.find(&name)
                    && let Ok(m) = kenshi_io::ogre_mesh::read(&game.root.join(&path))
                {
                    let look = ObjectLook::from_record(material, |f| game.res.find(f)).for_building();
                    for (_, bm) in kenshi_render::to_bevy(&m) {
                        let colours = bm.attribute(Mesh::ATTRIBUTE_COLOR).is_some();
                        out.push(PartMesh { mesh: meshes.add(bm), look: look.clone(), colours, floor: 0, interior: false, lo: Vec3::ZERO, hi: Vec3::ZERO, moves: None });
                    }
                }
            }
            (out, Vec2::ZERO, 0.0)
        })
        .0
}

fn meshes_of<'a>(
    cache: &'a mut HashMap<String, (Vec<PartMesh>, Vec2, f32)>,
    game: &Game,
    record: &str,
    meshes: &mut Assets<Mesh>,
) -> &'a (Vec<PartMesh>, Vec2, f32) {
    cache.entry(record.to_string()).or_insert_with(|| {
        let mut out = Vec::new();
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        if let Some(b) = game.sim.data.get(record) {
            let floors = kenshi_render::zone::mesh_floors(&game.sim.data, b);
            // each part baked where its offsets put it (a turret's head at rest), then the
            // interior's parts (shown once someone is inside)
            type Moves = Option<(Vec<(Vec3, Option<i32>)>, Vec<kenshi_render::zone::SpinDef>)>;
            let mut parts: Vec<(&str, Option<&kenshi_data::GameData>, Transform, bool, Moves)> = kenshi_render::zone::part_meshes_placed(&game.sim.data, b, false)
                .into_iter()
                .map(|p| {
                    let moves = p.swivels().then(|| (p.chain.clone(), p.spins.clone()));
                    (p.mesh, p.material, p.local, false, moves)
                })
                .collect();
            for (sid, _) in b.refs("interior") {
                if let Some(r) = game.sim.data.get(sid) {
                    parts.extend(kenshi_render::zone::record_meshes(&game.sim.data, r).into_iter().map(|(m, mat)| (m, mat, Transform::IDENTITY, true, None)));
                }
            }
            for (name, material, local, interior, moves) in parts {
                let Some(path) = game.res.find(name) else { continue };
                let Ok(mesh) = kenshi_io::ogre_mesh::read(&game.root.join(&path)) else { continue };
                let look = ObjectLook::from_record(material, |f| game.res.find(f)).for_building();
                for (_, m) in kenshi_render::to_bevy(&mesh) {
                    // (a moving part's mesh stays in its own frame: its pose places it)
                    let m = if local == Transform::IDENTITY || moves.is_some() { m } else { m.transformed_by(local) };
                    let (mut plo, mut phi) = (Vec3::ZERO, Vec3::ZERO);
                    if let Some(a) = m.compute_aabb() {
                        (plo, phi) = (Vec3::from(a.min()), Vec3::from(a.max()));
                        if !interior {
                            lo = lo.min(plo);
                            hi = hi.max(phi);
                        }
                    }
                    let colours = m.attribute(Mesh::ATTRIBUTE_COLOR).is_some();
                    out.push(PartMesh { mesh: meshes.add(m), look: look.clone(), colours, floor: floors.get(name).copied().unwrap_or(0), interior, lo: plo, hi: phi, moves: moves.clone() });
                }
            }
        }
        let half = if lo.x <= hi.x { Vec2::new((hi.x - lo.x) * 0.5, (hi.z - lo.z) * 0.5).max(Vec2::splat(3.0)) } else { Vec2::splat(15.0) };
        // the top of its parts over its base: construction's upperPos
        let top = if lo.y <= hi.y { hi.y.max(1.0) } else { 30.0 };
        (out, half, top)
    })
}

/// Spawn the list rows of `names` into list widget `list`.
fn fill_list(commands: &mut Commands, o: &mut Open, which: u8, list: Entity, names: &[String], chosen: Option<usize>) {
    let rows: Vec<(String, bool)> = names.iter().map(|n| (n.clone(), true)).collect();
    let column = list_rows(commands, list, which, &rows, chosen, &o.fonts.0, (10.0, 17.0, 74.0, 65.0));
    o.rows.push(column);
}

/// A list's rows (Kenshi_ListBoxItem: dark text, the chosen one light on dark; a row not
/// enabled faded) in a column over `list` at `area` (left, top, width, height in percent; the
/// ListBoxEmpty template's client area is 10, 17, 74, 65), each row tagged ListRow(which, k).
pub(super) fn list_rows(commands: &mut Commands, list: Entity, which: u8, rows: &[(String, bool)], chosen: Option<usize>, font: &(Handle<Font>, f32), area: (f32, f32, f32, f32)) -> Entity {
    let scale = kenshi_gui::res::ui_scale();
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(area.0),
                top: Val::Percent(area.1),
                width: Val::Percent(area.2),
                height: Val::Percent(area.3),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            ZIndex(2),
        ))
        .id();
    commands.entity(list).add_child(column);
    for (k, (name, enabled)) in rows.iter().enumerate() {
        let on = chosen == Some(k);
        let row = commands
            .spawn((
                Button,
                ListRow(which, k),
                Node { width: Val::Percent(100.0), height: Val::Px(18.0 * scale), flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::left(Val::Px(3.0 * scale)), ..default() },
                BackgroundColor(if on { Color::srgba(0.12, 0.07, 0.05, 0.85) } else { Color::NONE }),
            ))
            .id();
        let colour = if on {
            ROW_CHECKED
        } else if *enabled {
            ROW_TEXT
        } else {
            ROW_TEXT.with_alpha(0.45)
        };
        let t = commands
            .spawn((
                Text::new(name.clone()),
                TextFont { font: bevy::text::FontSource::Handle(font.0.clone()), font_size: bevy::text::FontSize::Px(font.1), ..default() },
                TextColor(colour),
                // one line a row (a long name cut at the list's edge, as MyGUI's list does)
                TextLayout::new(Justify::Left, bevy::text::LineBreak::NoWrap),
            ))
            .id();
        commands.entity(row).add_child(t);
        commands.entity(column).add_child(row);
    }
    column
}

/// Refill both lists (and the chosen building's panels).
fn refill(commands: &mut Commands, o: &mut Open, game: &Game) {
    for r in o.rows.drain(..) {
        commands.entity(r).despawn();
    }
    let all = base::unlocked(&game.sim.data, &game.sim.research_done);
    let cat = o.categories.get(o.cat).cloned().unwrap_or_default();
    o.list = all.iter().filter(|b| b.strings.get("building category").is_some_and(|c| *c == cat)).map(|b| b.string_id.clone()).collect();
    let names: Vec<String> = o.list.iter().filter_map(|s| game.sim.data.get(s)).map(|b| b.name.clone()).collect();
    let cats: Vec<String> = o.categories.iter().map(|c| title_case(c)).collect();
    let (cl, bl) = (o.ui.get("CategoriesList"), o.ui.get("BuildingsList"));
    if let Some(l) = cl {
        let chosen = Some(o.cat);
        fill_list(commands, o, 0, l, &cats, chosen);
    }
    if let Some(l) = bl {
        let chosen = o.chosen;
        fill_list(commands, o, 1, l, &names, chosen);
    }
    describe(commands, o, game);
}

/// "BUILDINGS" -> "Buildings".
fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c.flat_map(|c| c.to_lowercase())).collect())
}

/// @fn F036238
///
/// The chosen building's name, description and figures: materials cost, estimated build time,
/// power, people, what it produces, its type.
fn describe(commands: &mut Commands, o: &mut Open, game: &Game) {
    let data = &game.sim.data;
    let b = o.chosen.and_then(|k| o.list.get(k)).and_then(|s| data.get(s));
    if let Some(e) = o.ui.get("BuildingName") {
        commands.queue(SetCaption(e, b.map_or(String::new(), |b| b.name.clone())));
    }
    if let Some(e) = o.ui.get("DescriptionText") {
        commands.queue(SetCaption(e, b.and_then(|b| b.strings.get("Description")).cloned().unwrap_or_default()));
    }
    let Some(panel) = o.ui.get("StatsPanel") else { return };
    let mut rows = Vec::new();
    if let Some(b) = b {
        let pb = PlayerBuilding::new(b, [0.0; 3], 0.0);
        for (k, (m, n)) in pb.materials.iter().enumerate() {
            let name = data.get(m).map_or(m.clone(), |r| r.name.clone());
            rows.push(hud_ui::row(if k == 0 { "Materials cost:" } else { "" }, format!("{n} {name}")));
        }
        let h = base::estimated_hours(b);
        let hrs = if (h - 1.0).abs() < 0.05 { "hr" } else { "hrs" };
        rows.push(hud_ui::row("Estimated build time:", format!("{h:.1} {hrs}")));
        let num = |k: &str| b.floats.get(k).copied().or_else(|| b.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
        for (k, label) in [("power output", "Power Output:"), ("power consumption", "Power Consumption:"), ("power capacity", "Power Capacity:")] {
            if num(k) > 0.0 {
                rows.push(hud_ui::row(label, format!("{}", num(k))));
            }
        }
        if num("max operators") > 0.0 {
            rows.push(hud_ui::row("Num People:", format!("{}", num("max operators"))));
        }
        let produces: Vec<String> = b
            .refs("functionality")
            .iter()
            .filter_map(|(s, _)| data.get(s))
            .flat_map(|f| f.refs("produces").iter().filter_map(|(s, _)| data.get(s)).map(|r| r.name.clone()).collect::<Vec<_>>())
            .collect();
        if !produces.is_empty() {
            rows.push(hud_ui::row("Produces:", produces.join(", ")));
        }
        let bool_of = |k: &str| b.bools.get(k).copied().unwrap_or(false);
        let kind = if !b.refs("interior").is_empty() {
            "Building shell"
        } else if bool_of("is interior furniture") {
            "Interior"
        } else {
            "Exterior"
        };
        rows.push(hud_ui::row("Type:", kind));
    }
    commands.queue(hud_ui::SetRows { panel, rows, font: o.fonts.0.clone(), label_colour: hud_ui::LIGHT, centre: false });
}

/// B or a Build button: open or close build mode.
#[allow(clippy::too_many_arguments)]
pub fn toggle(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    act: super::bindings::Actions,
    mut clicks: MessageReader<GuiClick>,
    mut mode: ResMut<BuildMode>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    hud: Option<Res<hud_ui::Hud>>,
    shown: Option<Res<super::OpenBuildings>>,
) {
    let mut toggled = act.just("toggle_build");
    for c in clicks.read() {
        if c.name == "BuildButton" {
            toggled = true;
        }
    }
    let escape = keys.just_pressed(KeyCode::Escape) && mode.open.is_some();
    if !(toggled || escape) {
        return;
    }
    if let Some(o) = mode.open.take() {
        close(&mut commands, o, &mut game);
        if let Some(h) = &hud {
            for r in h.roots() {
                commands.queue(SetVisible(*r, true));
            }
        }
        return;
    }
    if escape {
        return;
    }
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let defs = gui.0.layout("Kenshi_BuildWindow.layout");
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(40));
    }
    if let Some(h) = &hud {
        for r in h.roots() {
            commands.queue(SetVisible(*r, false));
        }
    }
    // F036214: the commands
    if let Some(e) = ui.get("CommandsTextBox") {
        commands.queue(SetCaption(
            e,
            "Place Building: Left-Click\nClear Building: Right-Click\nRotate: , or . keys\nUndo: Backspace\nCancel All\nPress Confirm when finished".to_string(),
        ));
    }
    // the image box's layout caption ("picture") is the editor's placeholder
    if let Some(e) = ui.get("BuildingImage") {
        commands.queue(SetCaption(e, String::new()));
    }
    if let Some(e) = ui.get("FloorText") {
        // F036258: "Floor {1,num}", the floor shown
        commands.queue(SetCaption(e, format!("Floor {}", shown.as_ref().map_or(0, |o| o.floor))));
    }
    // F036258: the floor arrows' tips (the game's tooltip)
    for (name, key) in [("FloorArrowDown", "Page Down"), ("FloorArrowUp", "Page Up")] {
        if let Some(e) = ui.get(name) {
            commands.entity(e).insert(super::inventory_ui::ItemTip(format!(
                "Change the selected floor, use to move to a certain floor of a building, 0 is ground floor (also: {key} key)"
            )));
        }
    }
    let font = |name: &str| gui.0.fonts.get(name).map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
    let mut categories: Vec<String> = Vec::new();
    for b in base::unlocked(&game.sim.data, &game.sim.research_done) {
        if let Some(c) = b.strings.get("building category")
            && !categories.contains(c)
        {
            categories.push(c.clone());
        }
    }
    let mut o = Open {
        ui,
        categories,
        cat: 0,
        list: Vec::new(),
        chosen: None,
        rows: Vec::new(),
        ghost: None,
        yaw: 0.0,
        message: None,
        fonts: (font("Kenshi_StandardFont_Medium"), font("Kenshi_PaintedTextFont_Small")),
        wall_start: None,
    };
    refill(&mut commands, &mut o, &game);
    mode.open = Some(o);
}

/// Close build mode: the window, the ghost and the buildings placed but not confirmed go.
fn close(commands: &mut Commands, o: Open, game: &mut Game) {
    for r in o.ui.roots.iter().chain(o.rows.iter()) {
        commands.entity(*r).despawn();
    }
    if let Some((g, _, _)) = o.ghost {
        commands.entity(g).despawn();
    }
    game.sim.buildings.retain(|b| b.confirmed);
}

/// The built shell of the player's (a building with an "interior") whose floor scene point `at`
/// is over, if any.
fn host_shell(game: &Game, at: Vec3) -> Option<usize> {
    let w = at + game.zone.origin;
    (0..game.sim.buildings.len()).find(|&k| {
        let o = &game.sim.buildings[k];
        o.done && game.sim.data.get(&o.record).is_some_and(|r| !r.refs("interior").is_empty()) && (o.at[0] - w.x).hypot(o.at[2] - w.z) < o.radius * 0.6
    })
}

/// Whether building record `b` links together (a wall section or gate, "links together").
fn links(b: &kenshi_data::GameData) -> bool {
    b.bools.get("links together").copied().unwrap_or(false)
}

/// A wall record's "link type" (WallSection: 0 NORMAL, 1 CONNECTOR, 2 LOWER_WEDGE, 3 SINGLE,
/// 4 SHORT) and its length between its link points ("link length" x its "scale", F033634).
fn link_of(b: &kenshi_data::GameData) -> (i32, f32) {
    let scale = b.floats.get("scale").copied().unwrap_or(1.0);
    (b.ints.get("link type").copied().unwrap_or(0), b.floats.get("link length").copied().unwrap_or(0.0) * scale)
}

/// A turn about the vertical that points a building's x axis (its link axis) along `d`.
fn yaw_along(d: Vec3) -> f32 {
    (-d.z).atan2(d.x)
}

/// The ends of the player's walls (scene units): each end, the way out of it along its wall,
/// and its wall's turn.
fn wall_ends(game: &Game) -> Vec<(Vec3, Vec3, f32)> {
    let mut out = Vec::new();
    for b in &game.sim.buildings {
        let Some(r) = game.sim.data.get(&b.record) else { continue };
        if !links(r) {
            continue;
        }
        let half = link_of(r).1 * 0.5;
        if half <= 0.0 {
            continue;
        }
        let x = Quat::from_rotation_y(b.yaw) * Vec3::X;
        let at = Vec3::from(b.at) - game.zone.origin;
        out.push((at + x * half, x, b.yaw));
        out.push((at - x * half, -x, b.yaw));
    }
    out
}

/// The wall end within `reach` of `p`, nearest first.
fn snap_end(game: &Game, p: Vec3, reach: f32) -> Option<(Vec3, Vec3, f32)> {
    wall_ends(game).into_iter().filter(|(e, _, _)| (e.x - p.x).hypot(e.z - p.z) < reach).min_by(|a, b| (a.0 - p).length().total_cmp(&(b.0 - p).length()))
}

/// @fn F033634
///
/// The sections of wall `b` laid from `from` to `to` (scene units): the wall and its "wall
/// subsections" of link type NORMAL or SHORT, longest first, each where it fits, until what is
/// left is under half the shortest. (assumption) Kenshi also sets CONNECTOR pieces at turns and
/// LOWER_WEDGE ones on slopes; here the line is laid with the straight pieces only.
fn wall_run(data: &kenshi_data::GameDataContainer, b: &kenshi_data::GameData, from: Vec3, to: Vec3) -> Vec<(String, Vec3, f32)> {
    let mut pieces: Vec<(String, f32)> = std::iter::once(b)
        .chain(b.refs("wall subsections").iter().filter_map(|(s, _)| data.get(s)))
        .filter_map(|r| {
            let (kind, len) = link_of(r);
            (matches!(kind, 0 | 4) && len > 1.0).then(|| (r.string_id.clone(), len))
        })
        .collect();
    pieces.sort_by(|a, c| c.1.total_cmp(&a.1));
    let Some(shortest) = pieces.last().map(|p| p.1) else { return Vec::new() };
    let d = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let length = d.length();
    if length < 1.0 {
        return Vec::new();
    }
    let dir = d / length;
    let yaw = yaw_along(dir);
    let mut out = Vec::new();
    let mut done = 0.0;
    while length - done > shortest * 0.5 && out.len() < 64 {
        let left = length - done;
        let (rec, len) = pieces.iter().find(|p| p.1 <= left + shortest * 0.25).or(pieces.last()).cloned().unwrap();
        let mid = from + dir * (done + len * 0.5);
        out.push((rec, mid, yaw));
        done += len;
    }
    out
}

/// Whether building record `b` is interior furniture.
fn interior_furniture(b: &kenshi_data::GameData) -> bool {
    b.bools.get("is interior furniture").copied().unwrap_or(false)
}

/// Where building `b` would stand for the ground point `at`: interior furniture on the floor of
/// the shell it is in.
fn placed_at(game: &Game, b: &kenshi_data::GameData, at: Vec3) -> Vec3 {
    match host_shell(game, at).filter(|_| interior_furniture(b)) {
        Some(k) => Vec3::new(at.x, game.sim.buildings[k].at[1] - game.zone.origin.y, at.z),
        None => at,
    }
}

/// Whether building `b` with footprint `half` may stand at scene point `at` turned `yaw`.
fn may_place(game: &Game, b: &kenshi_data::GameData, at: Vec3, yaw: f32, half: Vec2) -> bool {
    if interior_furniture(b) {
        // inside a shell, clear of the other furniture in it
        let Some(host) = host_shell(game, at) else { return false };
        let r = half.length();
        let w = at + game.zone.origin;
        return !game.sim.buildings.iter().enumerate().any(|(k, o)| k != host && (o.at[0] - w.x).hypot(o.at[2] - w.z) < (o.radius + r) * 0.7);
    }
    let rot = Quat::from_rotation_y(yaw);
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
        let p = at + rot * Vec3::new(x * half.x, 0.0, z * half.y);
        let y = game.ground(p.x, p.z);
        lo = lo.min(y);
        hi = hi.max(y);
    }
    let max_slope = b.floats.get("max slope").copied().or_else(|| b.ints.get("max slope").map(|&v| v as f32)).unwrap_or(10.0);
    let across = 2.0 * half.x.max(half.y);
    if ((hi - lo) / across).atan().to_degrees() > max_slope {
        return false;
    }
    if lo + game.zone.origin.y < kenshi_render::water_material::WATER_HEIGHT {
        return false;
    }
    let r = half.length();
    let w = at + game.zone.origin;
    let wall = links(b);
    if game.sim.buildings.iter().any(|o| {
        let both_walls = wall && game.sim.data.get(&o.record).is_some_and(links);
        !both_walls && (o.at[0] - w.x).hypot(o.at[2] - w.z) < (o.radius + r) * 0.7
    }) {
        return false;
    }
    let near = std::iter::once(&game.zone).chain(game.zones.values()).flat_map(|z| z.occupied.iter());
    !near.into_iter().any(|(c, cr)| (c.x - at.x).hypot(c.z - at.z) < cr + r * 0.7)
}

/// The window's lists, buttons and keys, and placing with the mouse.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut mode: ResMut<BuildMode>,
    mut game: ResMut<Game>,
    mut clicks: MessageReader<GuiClick>,
    rows: Query<(&Interaction, &ListRow), Changed<Interaction>>,
    ui: Query<&Interaction>,
    buttons: Res<ButtonInput<MouseButton>>,
    act: super::bindings::Actions,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
    time: Res<Time>,
) {
    if mode.looks.is_none() {
        let mat = |c: Color| StandardMaterial { base_color: c, alpha_mode: AlphaMode::Blend, unlit: true, cull_mode: None, ..default() };
        mode.looks = Some(Looks { ok: materials.add(mat(Color::srgba(0.3, 0.9, 0.35, 0.45))), bad: materials.add(mat(Color::srgba(0.95, 0.2, 0.15, 0.45))) });
    }
    let BuildMode { open, meshes: cache, looks, .. } = &mut *mode;
    let Some(o) = open.as_mut() else {
        clicks.clear();
        return;
    };
    let looks = looks.as_ref().unwrap();
    // the lists
    let mut changed = false;
    for (i, row) in &rows {
        if *i != Interaction::Pressed {
            continue;
        }
        match row.0 {
            0 if o.cat != row.1 => {
                o.cat = row.1;
                o.chosen = None;
                changed = true;
            }
            1 => {
                o.chosen = Some(row.1);
                changed = true;
            }
            _ => {}
        }
    }
    // the buttons and keys
    let mut confirm = act.just("build_apply");
    let mut undo = act.just("build_undo");
    for c in clicks.read() {
        match c.name.as_str() {
            "ConfirmButton" => confirm = true,
            "UndoButton" => undo = true,
            "BuldingTypePrevButton" | "BuldingTypeNextButton" if !o.list.is_empty() => {
                let n = o.list.len();
                let k = o.chosen.unwrap_or(0);
                o.chosen = Some(if c.name.ends_with("PrevButton") { (k + n - 1) % n } else { (k + 1) % n });
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        refill(&mut commands, o, &game);
    }
    if confirm {
        for b in game.sim.buildings.iter_mut() {
            b.confirmed = true;
        }
    }
    if undo && game.sim.buildings.last().is_some_and(|b| !b.confirmed) {
        game.sim.buildings.pop();
    }
    if act.held("build_rotate_left") {
        o.yaw += std::f32::consts::FRAC_PI_2 * time.delta_secs();
    }
    if act.held("build_rotate_right") {
        o.yaw -= std::f32::consts::FRAC_PI_2 * time.delta_secs();
    }
    // the message's time
    if let Some((e, t)) = o.message.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            commands.entity(*e).despawn();
            o.message = None;
        }
    }
    // the ghost
    let chosen = o.chosen.and_then(|k| o.list.get(k)).cloned();
    if o.ghost.as_ref().map(|g| Some(&g.1)) != Some(chosen.as_ref()) {
        if let Some((g, _, _)) = o.ghost.take() {
            commands.entity(g).despawn();
        }
        if let Some(rec) = &chosen {
            let parts = meshes_of(cache, &game, rec, &mut meshes).0.clone();
            let root = commands.spawn((Transform::default(), Visibility::Hidden)).id();
            for pm in parts.into_iter().filter(|p| !p.interior) {
                let e = commands.spawn((Mesh3d(pm.mesh), MeshMaterial3d(looks.ok.clone()), rest_of(&pm.moves), bevy::light::NotShadowCaster)).id();
                commands.entity(root).add_child(e);
            }
            o.ghost = Some((root, rec.clone(), true));
        }
    }
    let (Ok(win), Ok((cam, at))) = (windows.single(), cams.single()) else { return };
    let cursor = win.cursor_position();
    let over_ui = ui.iter().any(|i| *i != Interaction::None);
    let point = cursor.filter(|_| !over_ui).and_then(|c| ground_at(cam, at, &game, c));
    if let Some((g, rec, ok)) = o.ghost.as_mut() {
        let half = meshes_of(cache, &game, rec, &mut meshes).1;
        match (point, game.sim.data.get(rec)) {
            (Some(p), Some(b)) => {
                let mut p = placed_at(&game, b, p);
                // a wall or gate: its end on a wall end near the cursor, lined up with it
                let (kind, len) = link_of(b);
                if links(b) && len > 0.0 {
                    if let Some((end, out, _)) = snap_end(&game, p, 60.0) {
                        if o.wall_start.is_none() && kind == 3 {
                            p = end + out * (len * 0.5);
                            p.y = game.ground(p.x, p.z);
                            o.yaw = yaw_along(out);
                        } else {
                            p = Vec3::new(end.x, game.ground(end.x, end.z), end.z);
                        }
                    }
                    if let Some(s) = o.wall_start
                        && (p - s).length() > 1.0
                    {
                        o.yaw = yaw_along(p - s);
                    }
                }
                let valid = may_place(&game, b, p, o.yaw, half);
                if valid != *ok {
                    *ok = valid;
                    commands.queue(Recolour(*g, if valid { looks.ok.clone() } else { looks.bad.clone() }));
                }
                commands.entity(*g).insert(Visibility::Inherited);
                if let Ok(mut t) = transforms.get_mut(*g) {
                    *t = Transform::from_translation(p).with_rotation(Quat::from_rotation_y(o.yaw));
                }
                // a wall laid in a line: the first click starts it, the second lays its sections
                let line = links(b) && kind != 3 && len > 0.0;
                if line && buttons.just_pressed(MouseButton::Left) {
                    match o.wall_start.take() {
                        None => o.wall_start = Some(p),
                        Some(s) => {
                            let run = wall_run(&game.sim.data, b, s, p);
                            let mut placed = 0;
                            for (rec, mid, yaw) in run {
                                let Some(r) = game.sim.data.get(&rec).cloned() else { continue };
                                let mid = Vec3::new(mid.x, game.ground(mid.x, mid.z), mid.z);
                                let half = meshes_of(cache, &game, &rec, &mut meshes).1;
                                if !may_place(&game, &r, mid, yaw, half) {
                                    continue;
                                }
                                let w = mid + game.zone.origin;
                                let mut pb = PlayerBuilding::new(&r, [w.x, w.y, w.z], yaw);
                                pb.radius = half.length();
                                game.sim.buildings.push(pb);
                                placed += 1;
                            }
                            if placed == 0 {
                                message(&mut commands, o, CANT_PLACE);
                            }
                            // the next line goes on from this one's end
                            o.wall_start = Some(p);
                        }
                    }
                } else if buttons.just_pressed(MouseButton::Left) {
                    if valid {
                        let w = p + game.zone.origin;
                        let mut pb = PlayerBuilding::new(b, [w.x, w.y, w.z], o.yaw);
                        pb.radius = half.length();
                        if std::env::var("KENSHI_BUILD_DEBUG").is_ok() {
                            eprintln!("placed {} at {w}", b.name);
                        }
                        game.sim.buildings.push(pb);
                    } else {
                        message(&mut commands, o, CANT_PLACE);
                    }
                }
            }
            _ => {
                commands.entity(*g).insert(Visibility::Hidden);
            }
        }
    }
    // right click: a wall line given up, else clear the placed (unconfirmed) building there
    if buttons.just_pressed(MouseButton::Right) && o.wall_start.take().is_some() {
        return;
    }
    if buttons.just_pressed(MouseButton::Right)
        && let Some(p) = point
    {
        let w = p + game.zone.origin;
        if let Some(k) = game.sim.buildings.iter().rposition(|b| !b.confirmed && (b.at[0] - w.x).hypot(b.at[2] - w.z) < b.radius) {
            game.sim.buildings.remove(k);
        }
    }
}

/// Show `text` in the window's message panel for a few seconds.
fn message(commands: &mut Commands, o: &mut Open, text: &str) {
    let Some(panel) = o.ui.get("MessagePanel") else { return };
    if let Some((e, _)) = o.message.take() {
        commands.entity(e).despawn();
    }
    commands.queue(SetVisible(panel, true));
    let t = commands
        .spawn((
            Text::new(text),
            TextFont { font: bevy::text::FontSource::Handle(o.fonts.1.0.clone()), font_size: bevy::text::FontSize::Px(o.fonts.1.1 * 1.3), ..default() },
            TextColor(hud_ui::LIGHT),
            TextShadow::default(),
            Node { position_type: PositionType::Absolute, left: Val::Px(8.0), top: Val::Px(4.0), ..default() },
        ))
        .id();
    commands.entity(panel).add_child(t);
    o.message = Some((t, 3.0));
}

/// Give a ghost's meshes another material.
struct Recolour(Entity, Handle<StandardMaterial>);

impl Command for Recolour {
    type Out = ();
    fn apply(self, world: &mut World) {
        let kids: Vec<Entity> = world.get::<Children>(self.0).map(|c| c.iter().collect()).unwrap_or_default();
        for k in kids {
            world.entity_mut(k).insert(MeshMaterial3d(self.1.clone()));
        }
    }
}

/// Draw the player's buildings: sites see-through (filling in with their progress), built ones
/// with their parts' object materials; redrawn when one changes.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    mut mode: ResMut<BuildMode>,
    game: Res<Game>,
    assets: Res<AssetServer>,
    mut look: ResMut<TerrainLook>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut objects: ResMut<Assets<KenshiObjectMaterial>>,
) {
    let origin = game.zone.origin;
    let want: Vec<DrawnAs> = game
        .sim
        .buildings
        .iter()
        .enumerate()
        .map(|(k, b)| DrawnAs {
            record: b.record.clone(),
            at: b.at,
            yaw: b.yaw,
            done: b.done,
            doors: game.sim.door_states.iter().filter(|d| d.player_building == Some(k)).count(),
        })
        .collect();
    let BuildMode { meshes: cache, shown, .. } = &mut *mode;
    // the ones gone
    while shown.len() > want.len() {
        let (e, _, _) = shown.pop().unwrap();
        commands.entity(e).despawn();
    }
    for (k, w) in want.iter().enumerate() {
        // a site's construction state (F037798 "construction progress"), each frame
        let pb = &game.sim.buildings[k];
        let state = if pb.done { 1.0 } else { pb.progress / pb.total().max(1) as f32 };
        if let Some(s) = shown.get(k)
            && s.1 == *w
        {
            for h in &s.2 {
                if let Some(mut m) = objects.get_mut(h)
                    && m.params.construction.x != state
                {
                    m.params.construction.x = state;
                }
            }
            continue;
        }
        let (parts, _, top) = meshes_of(cache, &game, &w.record, &mut meshes).clone();
        let at = Vec3::from(w.at) - origin;
        let root = commands.spawn((Transform::from_translation(at).with_rotation(Quat::from_rotation_y(w.yaw)), Visibility::default())).id();
        let mut sites = Vec::new();
        let rot = Quat::from_rotation_y(w.yaw);
        for pm in parts {
            // a site shows no interior
            if pm.interior && !w.done {
                continue;
            }
            let (m, o, colours) = (pm.mesh.clone(), pm.look.clone(), pm.colours);
            let ground = look.ground_colour(w.at[0], w.at[2]);
            let mut mat = look.object(&assets, &o, origin, colours, ground);
            if !w.done {
                // construction.material: scaffoldTiling 10, upperPos the building's top
                mat.params.construction = Vec4::new(state, top, 10.0, at.y);
            }
            let h = objects.add(mat);
            if !w.done {
                sites.push(h.clone());
            }
            let e = commands.spawn((Mesh3d(m), MeshMaterial3d(h), Transform::default())).id();
            // a part that moves, posed in its building's frame (the root's child) once built
            if let Some((chain, spins)) = pm.moves.clone() {
                let local = chain.iter().fold(Transform::IDENTITY, |t, (o, _)| t * Transform::from_translation(*o));
                commands.entity(e).insert(local);
                if w.done {
                    commands.entity(e).insert(kenshi_render::zone::Swivel::new(Transform::IDENTITY, Vec3::from(w.at), chain, spins));
                }
            }
            // its floor and bounds, for the roofs hidden while someone is inside (hide_roofs)
            if w.done {
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for c in 0..8 {
                    let p = Vec3::new(if c & 1 == 0 { pm.lo.x } else { pm.hi.x }, if c & 2 == 0 { pm.lo.y } else { pm.hi.y }, if c & 4 == 0 { pm.lo.z } else { pm.hi.z });
                    let q = rot * p + at;
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
                commands.entity(e).insert((
                    kenshi_render::zone::BuildingPiece { lo, hi },
                    kenshi_render::zone::BuildingFloor { zone: PLAYER_ZONE, building: k, floor: pm.floor, interior: pm.interior },
                ));
            }
            commands.entity(root).add_child(e);
        }
        // its lamps, torches and fires (kenshi_render::local_lights), once built
        if w.done
            && let Some(rec) = game.sim.data.get(&w.record)
        {
            for (light, lat, lrot) in kenshi_render::zone::placed_lights(&game.sim.data, rec, None) {
                if let Some(e) = kenshi_render::local_lights::spawn_light(&mut commands, &game.sim.data, &light, lat, lrot) {
                    commands.entity(root).add_child(e);
                }
            }
        }
        // a gate's doors, each moved with its door
        if w.done
            && let Some(rec) = game.sim.data.get(&w.record)
        {
            let doors: Vec<usize> = (0..game.sim.door_states.len()).filter(|&d| game.sim.door_states[d].player_building == Some(k)).collect();
            for (n, (sid, _)) in rec.refs("doors").iter().enumerate() {
                let (Some(d), Some(dr)) = (doors.get(n).copied(), game.sim.data.get(sid)) else { continue };
                for (mesh, mat) in kenshi_render::zone::record_meshes(&game.sim.data, dr) {
                    let Some(path) = game.res.find(mesh) else { continue };
                    let Ok(om) = kenshi_io::ogre_mesh::read(&game.root.join(&path)) else { continue };
                    let o = ObjectLook::from_record(mat, |f| game.res.find(f)).for_building();
                    for (_, m) in kenshi_render::to_bevy(&om) {
                        let colours = m.attribute(Mesh::ATTRIBUTE_COLOR).is_some();
                        let ground = look.ground_colour(w.at[0], w.at[2]);
                        let h = objects.add(look.object(&assets, &o, origin, colours, ground));
                        let e = commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(h), Transform::default(), PlayerDoorPiece(d))).id();
                        commands.entity(root).add_child(e);
                    }
                }
            }
        }
        if k < shown.len() {
            commands.entity(shown[k].0).despawn();
            shown[k] = (root, w.clone(), sites);
        } else {
            shown.push((root, w.clone(), sites));
        }
    }
}

/// "Constructing n%" over each confirmed site, the crop's state over each farm.
pub fn labels(
    mut commands: Commands,
    mut mode: ResMut<BuildMode>,
    game: Res<Game>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    mut nodes: Query<(&mut Node, &mut Text)>,
) {
    let Ok((cam, at)) = cams.single() else { return };
    let origin = game.zone.origin;
    // sites: their construction; farms: their crop
    let sites: Vec<(usize, Vec3, String)> = game
        .sim
        .buildings
        .iter()
        .enumerate()
        .filter_map(|(k, b)| {
            let at = Vec3::from(b.at) - origin + Vec3::Y * 30.0;
            if b.confirmed && !b.done {
                return Some((k, at, format!("Constructing {}%", (b.progress / b.total().max(1) as f32 * 100.0) as u32)));
            }
            if b.done && !game.sim.has_power(k) {
                return Some((k, at, "No power".to_string()));
            }
            if let Some(item) = b.craft_queue.first().filter(|_| b.done) {
                return Some((k, at, format!("Crafting {}", game.sim.data.get(item).map_or(item.clone(), |r| r.name.clone()))));
            }
            if b.full {
                return Some((k, at, "Full".to_string()));
            }
            let farm = b.done && game.sim.data.get(&b.record).is_some_and(base::is_farm);
            farm.then(|| {
                // (stand-in for the game's crop look) the crop's stage in words
                let state = if b.death >= 1.0 {
                    "Dead".to_string()
                } else if b.stage >= 2.0 {
                    "Rotting".to_string()
                } else if b.stage >= 1.0 {
                    "Ripe".to_string()
                } else {
                    format!("Growing {}%", (b.stage * 100.0) as u32)
                };
                let dry = if b.water <= 0.0 { ", dry" } else { "" };
                (k, at, format!("{state}{dry}"))
            })
        })
        .collect();
    mode.labels.retain(|k, e| {
        let keep = sites.iter().any(|s| s.0 == *k);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    for (k, p, label) in sites {
        let e = *mode.labels.entry(k).or_insert_with(|| {
            commands
                .spawn((
                    Text::new(""),
                    TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
                    TextColor(hud_ui::LIGHT),
                    TextShadow::default(),
                    Node { position_type: PositionType::Absolute, ..default() },
                ))
                .id()
        });
        let Ok((mut n, mut t)) = nodes.get_mut(e) else { continue };
        match cam.world_to_viewport(at, p) {
            Ok(v) => {
                n.display = Display::Flex;
                n.left = Val::Px(v.x - 50.0);
                n.top = Val::Px(v.y);
                if t.0 != label {
                    t.0 = label;
                }
            }
            Err(_) => n.display = Display::None,
        }
    }
}

/// A right click on a building of the player's with characters selected: a site, they build
/// it; a production building, they operate it; one with storage, the first walks to it and
/// opens it.
pub fn order_at(game: &mut Game, inv: &mut super::inventory_ui::InventoryWindow, w: Vec3) -> bool {
    let Some(b) = game.sim.building_at([w.x, w.y, w.z]) else { return false };
    let sel = game.selected.clone();
    if !game.sim.buildings[b].done {
        game.sim.order_build(&sel, b);
    } else if game.sim.is_operated(b) {
        game.sim.order_operate(&sel, b);
    } else if game.sim.buildings[b].storage.is_some() {
        let Some(&me) = sel.first() else { return false };
        let pb = &game.sim.buildings[b];
        game.sim.order_move(&[me], pb.at);
        inv.pending_loot = Some(base::BUILDING_OWNER + b);
    } else {
        return false;
    }
    true
}

/// (testing) KENSHI_BUILD="frame@name[;name...]@dx,dz": at that frame, build mode opens with
/// the (first) building chosen; each named one is placed by the first selected character, the
/// first offset (dx, dz), the next ones further along x; 30 frames on they are confirmed and the
/// mode closed, then the squad is ordered to build the first. KENSHI_BUILD_DONE: they are built
/// at once and the squad operates the first it can work. KENSHI_STORAGE_AT=frame: the squad
/// opens the first built storage then; KENSHI_LABOUR=level sets the squad's labouring;
/// KENSHI_UNLOCK=all: all research done first (crafting benches then make their first item).
#[allow(clippy::too_many_arguments)]
pub fn script(
    mut frames: Local<u32>,
    mut mode: ResMut<BuildMode>,
    mut game: ResMut<Game>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut inv: ResMut<super::inventory_ui::InventoryWindow>,
) {
    *frames += 1;
    if let Ok(v) = std::env::var("KENSHI_STORAGE_AT")
        && v.parse::<u32>().ok() == Some(*frames)
        && let Some(b) = game.sim.buildings.iter().position(|b| b.done && b.storage.is_some() && !game.sim.is_production_record(&b.record))
    {
        let at = Vec3::from(game.sim.buildings[b].at);
        order_at(&mut game, &mut inv, at);
    }
    // (testing) KENSHI_MACHINE_AT=frame: the first built production building's window
    if let Ok(v) = std::env::var("KENSHI_MACHINE_AT")
        && v.parse::<u32>().ok() == Some(*frames)
        && let Some(b) = (0..game.sim.buildings.len()).find(|&b| game.sim.buildings[b].done && game.sim.buildings[b].storage.is_some())
    {
        if let Some(&me) = game.selected.first() {
            let at = game.sim.buildings[b].at;
            game.sim.folk[me].mover.position = [at[0] / 10.0 + 3.0, at[1] / 10.0, at[2] / 10.0];
        }
        inv.pending_loot = Some(base::BUILDING_OWNER + b);
    }
    // (testing) KENSHI_BOX_AT=frame: the first selected is put by the nearest container with
    // something in it and opens it
    if std::env::var("KENSHI_BOX_AT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames)
        && let Some(&me) = game.selected.first()
    {
        let here = game.sim.folk[me].mover.position_units();
        let counters = std::env::var("KENSHI_BOX_COUNTER").is_ok();
        let best = (0..game.sim.boxes.len())
            .filter(|&k| if counters { game.sim.counter_trader(k).is_some() } else { game.sim.boxes[k].inventory.sections.iter().any(|s| !s.items.is_empty()) })
            .min_by(|&a, &b| {
                let d = |k: usize| {
                    let p = game.sim.boxes[k].at.position;
                    (p[0] - here[0]).hypot(p[2] - here[2])
                };
                d(a).total_cmp(&d(b))
            });
        if let Some(k) = best {
            let p = game.sim.boxes[k].at.position;
            game.sim.folk[me].mover.position = [p[0] / 10.0 + 1.0, p[1] / 10.0, p[2] / 10.0];
            inv.pending_loot = Some(kenshi_game::loot::BOX_OWNER + k);
            let shown = game.sim.inv(kenshi_game::loot::BOX_OWNER + k).map_or(0, |i| i.sections[0].items.len());
            eprintln!("box test: {} with {shown} items, faction {:?}", game.sim.boxes[k].at.name, game.sim.box_faction(k));
        }
    }
    let Ok(v) = std::env::var("KENSHI_BUILD") else { return };
    let mut it = v.split('@');
    let at = it.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let names: Vec<String> = it.next().unwrap_or("Small Shack").split(';').map(str::to_string).collect();
    let off: Vec<f32> = it.next().unwrap_or("60,0").split(',').filter_map(|s| s.parse().ok()).collect();
    let done = std::env::var("KENSHI_BUILD_DONE").is_ok();
    let Some(&me) = game.selected.first() else { return };
    if *frames == at || *frames == at + 35 {
        keys.press(KeyCode::KeyB);
    }
    if *frames == at + 1 || *frames == at + 36 {
        keys.release(KeyCode::KeyB);
    }
    // KENSHI_UNLOCK=all: every research done first
    if *frames == at && std::env::var("KENSHI_UNLOCK").is_ok_and(|v| v == "all") {
        game.sim.research_done = game.sim.data.of_type(kenshi_data::ItemType::Research).map(|r| r.string_id.clone()).collect();
    }
    if *frames == at + 5
        && mode.open.is_some()
    {
        let BuildMode { open, meshes: cache, .. } = &mut *mode;
        let o = open.as_mut().unwrap();
        let unlocked: Vec<kenshi_data::GameData> = base::unlocked(&game.sim.data, &game.sim.research_done).into_iter().cloned().collect();
        let mut x = off.first().copied().unwrap_or(60.0);
        for (k, name) in names.iter().enumerate() {
            let Some(b) = unlocked.iter().find(|b| b.name == *name) else {
                eprintln!("build script: {name} not unlocked ({:?})", unlocked.iter().map(|b| &b.name).collect::<Vec<_>>());
                continue;
            };
            if k == 0 {
                let cat = b.strings.get("building category").cloned().unwrap_or_default();
                o.cat = o.categories.iter().position(|c| *c == cat).unwrap_or(0);
                refill(&mut commands, o, &game);
                o.chosen = o.list.iter().position(|s| *s == b.string_id);
                refill(&mut commands, o, &game);
            }
            let half = meshes_of(cache, &game, &b.string_id, &mut meshes).1;
            x += half.x;
            let here = Vec3::from(game.sim.folk[me].mover.position_units());
            let mut w = here + Vec3::new(x, 0.0, off.get(1).copied().unwrap_or(0.0));
            x += half.x + 20.0;
            w.y = game.ground(w.x - game.zone.origin.x, w.z - game.zone.origin.z) + game.zone.origin.y;
            let mut pb = PlayerBuilding::new(b, [w.x, w.y, w.z], 0.0);
            pb.radius = half.length();
            if done {
                pb.done = true;
                pb.progress = pb.total() as f32;
            }
            eprintln!("build script: placed {} at {w} ({} materials, est {:.1} h)", b.name, pb.total(), base::estimated_hours(b));
            game.sim.buildings.push(pb);
        }
    }
    if *frames == at + 35 {
        for b in game.sim.buildings.iter_mut() {
            b.confirmed = true;
        }
    }
    if *frames == at + 40 {
        let sel = game.selected.clone();
        if done {
            // KENSHI_SKILL=stat:level (StatsEnumerated's number): that skill of the squad's set first
            if let Some((st, l)) = std::env::var("KENSHI_SKILL").ok().and_then(|v| v.split_once(':').and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<f32>().ok()?))))
                && let Some(stat) = kenshi_char::StatsEnumerated::from_i32(st)
            {
                for &id in &sel {
                    game.sim.arena.f[id].skills.levels.insert(stat, l);
                }
            }
            // KENSHI_LABOUR=level: the squad's labouring set first
            if let Some(l) = std::env::var("KENSHI_LABOUR").ok().and_then(|v| v.parse::<f32>().ok()) {
                for &id in &sel {
                    game.sim.arena.f[id].skills.levels.insert(kenshi_char::StatsEnumerated::StatLabouring, l);
                }
            }
            // KENSHI_FILL=n: the first producer's storage given n of its product and marked full
            if let Some(n) = std::env::var("KENSHI_FILL").ok().and_then(|v| v.parse::<u32>().ok())
                && let Some(k) = (0..game.sim.buildings.len()).find(|&k| game.sim.is_production(k))
            {
                let rec = game.sim.data.get(&game.sim.buildings[k].record).cloned();
                let product = rec.as_ref().and_then(|r| kenshi_game::production::function_of(&game.sim.data, r)).and_then(|(f, _)| f.refs("produces").first().map(|(s, _)| s.clone()));
                if let Some(item) = product.and_then(|p| game.sim.data.get(&p).cloned()) {
                    let pb = &mut game.sim.buildings[k];
                    for _ in 0..n {
                        if let Some(st) = pb.storage.as_mut() {
                            st.add(kenshi_char::inventory::Item::from_record(&item, 1), false);
                        }
                    }
                    pb.full = true;
                }
            }
            // a crafting bench makes its first item
            for k in 0..game.sim.buildings.len() {
                if let Some(item) = game.sim.crafts(k).first().cloned() {
                    game.sim.buildings[k].craft_queue = vec![item];
                    game.sim.buildings[k].craft_repeat = true;
                }
            }
            if let Some(k) = (0..game.sim.buildings.len()).find(|&k| game.sim.is_operated(k)) {
                game.sim.order_operate(&sel, k);
                eprintln!("build script: {} ordered to operate {}", sel.len(), game.sim.buildings[k].record);
                let at = game.sim.buildings[k].at;
                eprintln!(
                    "build script: resource {:.3}, stone {:.3}, farming {:.3}, water {:.3}",
                    game.sim.building_resource(k),
                    game.sim.resource_level(2, at),
                    game.sim.resource_level(6, at),
                    game.sim.resource_level(5, at)
                );
            }
        } else {
            let k = game.sim.buildings.len().saturating_sub(names.len());
            game.sim.order_build(&sel, k);
            eprintln!("build script: {} ordered to build", sel.len());
        }
    }
}

/// Draw the wilds' camps and dens (kenshi_game::nests): each built nest's buildings, with their
/// parts' object materials, while the nest stands.
#[allow(clippy::too_many_arguments)]
pub fn draw_nests(
    mut commands: Commands,
    mut mode: ResMut<BuildMode>,
    game: Res<Game>,
    assets: Res<AssetServer>,
    mut look: ResMut<TerrainLook>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut objects: ResMut<Assets<KenshiObjectMaterial>>,
    mut shown: Local<Vec<(usize, Entity)>>,
) {
    let origin = game.zone.origin;
    // a game loaded in the program: its nests drawn afresh
    if game.is_added() {
        shown.clear();
    }
    // the nests drawn that are gone (wiped out and left behind)
    shown.retain(|(k, e)| {
        let keep = game.sim.nests.get(*k).is_some_and(|n| n.built && !n.pieces.is_empty());
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    for (k, n) in game.sim.nests.iter().enumerate() {
        if !n.built || n.pieces.is_empty() || shown.iter().any(|(s, _)| *s == k) {
            continue;
        }
        let root = commands.spawn((Transform::default(), Visibility::default())).id();
        for (record, at, yaw) in &n.pieces {
            let (parts, _, _) = meshes_of(&mut mode.meshes, &game, record, &mut meshes).clone();
            let place = commands
                .spawn((Transform::from_translation(Vec3::from(*at) - origin).with_rotation(Quat::from_rotation_y(*yaw)), Visibility::default()))
                .id();
            for pm in parts.into_iter().filter(|p| !p.interior) {
                let ground = look.ground_colour(at[0], at[2]);
                let h = objects.add(look.object(&assets, &pm.look, origin, pm.colours, ground));
                let e = commands.spawn((Mesh3d(pm.mesh), MeshMaterial3d(h), rest_of(&pm.moves))).id();
                commands.entity(place).add_child(e);
            }
            commands.entity(root).add_child(place);
        }
        // its loose debris
        for (record, mesh, at, yaw, scale) in &n.loose {
            let parts = debris_meshes(&mut mode.meshes, &game, record, *mesh, &mut meshes).clone();
            if parts.is_empty() {
                continue;
            }
            let place = commands
                .spawn((Transform::from_translation(Vec3::from(*at) - origin).with_rotation(Quat::from_rotation_y(*yaw)).with_scale(Vec3::splat(scale.max(0.05))), Visibility::default()))
                .id();
            for pm in parts {
                let ground = look.ground_colour(at[0], at[2]);
                let h = objects.add(look.object(&assets, &pm.look, origin, pm.colours, ground));
                let e = commands.spawn((Mesh3d(pm.mesh), MeshMaterial3d(h), rest_of(&pm.moves))).id();
                commands.entity(place).add_child(e);
            }
            commands.entity(root).add_child(place);
        }
        shown.push((k, root));
    }
}
