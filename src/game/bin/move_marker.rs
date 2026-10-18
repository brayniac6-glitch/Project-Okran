//! The movement marker (F045571, F045570, F045591): the green pin on the ground where the move
//! order points, shown while the right button is held (following the cursor) and fading out
//! over a second once let go. Kenshi's option "Show movement marker" (on unless set off).
//!
//! Spec:
//! - F045571: the marker is "position.mesh" scaled 8, drawn with a copy of the green debug
//!   material ("movemarker"), alpha blended, after the world (render queue 88), hidden;
//! - F045591 / F045482: an order on the ground puts it there, shows it and sets its fade to 1;
//! - F045570, each frame while the fade is above 0: the material's "colour" is (0.612, 0.694,
//!   0.529, fade) in its first pass and (0.306, 0.347, 0.2645, fade x 0.6) in its second; the
//!   fade goes down by the frame's time and it shows while the fade is above 0.
//!
//! (stand-in) One unlit pass in the first pass's colour: the debug material's second pass (its
//! depth test, its blending) is not read.
use super::Game;
use bevy::prelude::*;

const MESH: &str = "data/meshes/position.mesh";
/// The rotation marker (F041487, F041486): "rotate.mesh" over the camera's centre.
const ROTATE_MESH: &str = "data/meshes/rotate.mesh";
const SCALE: f32 = 8.0;
const COLOUR: [f32; 3] = [0.612, 0.694, 0.529];

/// settings.cfg "Movement marker" and "Rotation marker" ("Show movement marker", "Show rotation
/// marker", F032944: settings +0x80 for the rotation one; on unless set to 0).
#[derive(Resource, Clone, Copy)]
pub struct MarkerOptions {
    pub movement: bool,
    pub rotation: bool,
}

impl MarkerOptions {
    pub fn read(root: &std::path::Path) -> Self {
        let cfg = kenshi_io::cfg::text(root, "settings.cfg");
        let on = |k: &str| cfg.lines().find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix('=')).map(|v| v.trim() != "0")).unwrap_or(true);
        MarkerOptions { movement: on("Movement marker"), rotation: on("Rotation marker") }
    }
}

/// The rotation marker's entity (F041487: on its own node under the camera's centre, hidden).
#[derive(Resource, Default)]
pub struct RotationMarker(Option<Entity>);

/// @fn F041486
///
/// The camera's rotation marker: shown while the camera is turned with the mouse (the camera's
/// +0 flag, set by the mouse-rotate key) and "Show rotation marker" is on, 1 over the camera's
/// centre, not turning with the camera, scaled by the camera's
/// distance x 0.06, at least 3. (assumption) In the movement marker's green: its material,
/// by an id (0x9cb187), is not read.
#[allow(clippy::too_many_arguments)]
pub fn rotation(
    mut commands: Commands,
    mut marker: ResMut<RotationMarker>,
    options: Option<Res<MarkerOptions>>,
    game: Res<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(options) = options else {
        commands.insert_resource(MarkerOptions::read(&game.root));
        return;
    };
    let show = options.rotation && game.mouse_turning;
    let e = match marker.0 {
        Some(e) => e,
        None => {
            if !show {
                return;
            }
            let Ok(mesh) = kenshi_io::ogre_mesh::read(&game.root.join(ROTATE_MESH)) else { return };
            let mat = materials.add(StandardMaterial { base_color: Color::srgb(COLOUR[0], COLOUR[1], COLOUR[2]), unlit: true, ..default() });
            let root = commands.spawn((Transform::default(), Visibility::Hidden)).id();
            for (_, m) in kenshi_render::to_bevy(&mesh) {
                let e = commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(mat.clone()), Transform::default(), bevy::light::NotShadowCaster)).id();
                commands.entity(root).add_child(e);
            }
            marker.0 = Some(root);
            return;
        }
    };
    let Ok((mut tr, mut vis)) = q.get_mut(e) else { return };
    let want = if show { Visibility::Visible } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    if show {
        *tr = Transform::from_translation(game.focus + Vec3::Y).with_scale(Vec3::splat((game.dist * 0.06).max(3.0)));
    }
}

/// Where the marker stands and how faded in it is (1 shown .. 0 gone).
#[derive(Resource, Default)]
pub struct MoveMarker {
    pub at: Option<Vec3>,
    pub fade: f32,
    entity: Option<(Entity, Handle<StandardMaterial>)>,
}

impl MoveMarker {
    /// An order on the ground at `at` (scene coordinates): the marker there, fully shown.
    pub fn show(&mut self, at: Vec3) {
        self.at = Some(at);
        self.fade = 1.0;
    }
}

pub fn update(
    mut commands: Commands,
    mut marker: ResMut<MoveMarker>,
    game: Res<Game>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Transform, &mut Visibility)>,
    mut frames: Local<u32>,
    options: Option<Res<MarkerOptions>>,
) {
    // "Show movement marker" off: none put down
    if options.is_some_and(|o| !o.movement) {
        marker.fade = 0.0;
    }
    // (testing) KENSHI_MARKER=<frame>: from that frame, the marker held shown 25 beside the
    // first selected character
    *frames += 1;
    if let Some(f) = std::env::var("KENSHI_MARKER").ok().and_then(|f| f.parse::<u32>().ok())
        && *frames >= f
        && let Some(&id) = game.selected.first()
    {
        let p = Vec3::from(game.sim.folk[id].mover.position_units()) - game.zone.origin + Vec3::new(std::env::var("KENSHI_MARKER_DX").ok().and_then(|v| v.parse().ok()).unwrap_or(25.0), 0.0, 0.0);
        marker.show(p);
    }
    if marker.entity.is_none() {
        if marker.at.is_none() {
            return;
        }
        let Ok(mesh) = kenshi_io::ogre_mesh::read(&game.root.join(MESH)) else { return };
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgba(COLOUR[0], COLOUR[1], COLOUR[2], 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let root = commands.spawn((Transform::from_scale(Vec3::splat(SCALE)), Visibility::Hidden)).id();
        for (_, m) in kenshi_render::to_bevy(&mesh) {
            let e = commands
                .spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(mat.clone()), Transform::default(), bevy::light::NotShadowCaster))
                .id();
            commands.entity(root).add_child(e);
        }
        marker.entity = Some((root, mat));
        return;
    }
    let Some((e, mat)) = marker.entity.clone() else { return };
    let Ok((mut tr, mut vis)) = q.get_mut(e) else { return };
    let shown = marker.fade > 0.0;
    *vis = if shown { Visibility::Visible } else { Visibility::Hidden };
    if !shown {
        return;
    }
    if let Some(at) = marker.at {
        tr.translation = at;
    }
    if let Some(mut m) = materials.get_mut(&mat) {
        m.base_color = Color::srgba(COLOUR[0], COLOUR[1], COLOUR[2], marker.fade);
    }
    marker.fade -= time.delta_secs();
}
