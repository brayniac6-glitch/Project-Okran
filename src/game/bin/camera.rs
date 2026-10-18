//! Kenshi's camera (CameraClass: F041481 builds it, F041486 runs it each frame through F041475
//! zoom, F041467 pan, F041482 turn and F041476 height; F041474 keeps it near the selection;
//! settings.cfg through F032944).
//!
//! Its centre ("camera_center", `Game::focus`) stands on the ground, or 10 over the character it
//! follows; the camera ("camera_node") sits `Game::dist` behind it along the centre's turn
//! (`Game::yaw`) and tilt (`Game::pitch`, down positive), looking at it.
//! - Pan: the keys (camera_left / right / forward / back) and, with "Edge scrolling", the
//!   cursor within 1% of the window's edge move the centre over the ground at "camera speed" x
//!   distance / 600 a second (F041467). Panning stops following.
//! - Turn: the mouse with mouse_rotate held (its movement x "mouse speed horizontal" / "vertical",
//!   F046179; the cursor held in place), camera_rotate_left / right (9) and camera_tilt+ / - (2),
//!   each smoothed over its last 20 frames (weights 0.5^k for the turn, 0.7^k for the tilt,
//!   F029587) and applied as "mouse speed pan" / "mouse speed tilt" degrees x the frame time, by
//!   "invert X" / "invert Y" (none over a frame longer than 0.2 s). The tilt stops once the view
//!   points 0.92 down or 0.2 up (F041482).
//! - Zoom: a wheel notch (OIS's 120 / 100) takes "camera zoom" x min(distance / 600, 0.75) off the
//!   distance, kept within 10 - 2000; camera_zoom_in / out add 5 notches a second (3x with shift),
//!   not while following or placing a building; the camera closes 70 x the frame time of the gap
//!   (F041475).
//! - Follow: a squad member's select key pressed twice within 0.5 s (F044306 -> F045539) or
//!   their portrait double-clicked (F033288, `hud_ui`) has the camera follow them: its centre 10 over them, the screen's edges turning it (3 / 1) instead of
//!   panning (F041486).
//! - Height (F041476): the centre stands on the ground or the building floor shown under it (and
//!   not under the water); it rises so the camera stays min(distance x 0.2, 20) + 10 over the
//!   ground under it, and moves to its height at 10 x the gap a second.
//! - Leash (F041474): the centre is kept within 2250 of the nearest selected character.
//! - It starts 150 back, 30 degrees down, the centre unturned (looking along +z), the near clip
//!   at 5 (F041481).
//!
//! - Free camera ("Free camera mode", toggle_fps_camera; F041485): flown with the pan keys
//!   (camera_zoom_in / out up and down) along its own facing, gathering speed ("camera speed" x
//!   0.1 x a speed the wheel scales by 1 + 0.1 a notch, 0.0001 - 0.1, first 0.01) and slowing by
//!   0.1 of it each 1/100 s; turned by the mouse (x 0.01 x "mouse speed tilt" and the inverts) and
//!   rolled by the rotate keys (x 0.1), likewise gathered and slowed; kept 2 over the ground; the
//!   cursor held. (assumption) It flies from where the camera is and sees from its centre.
//! (not ported) The camera grid; the
//! follow target's body scale on its 10 over (F038840); the centre's physics rays (groups 9 - 12
//! by the floor shown, and 14) stand in as the navmesh's floors (assumption).

use super::bindings;
use super::{BuildingView, Game, MainCam, OpenBuildings, building_at};
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use std::collections::VecDeque;
use std::path::Path;

/// F041481: the camera node 150 behind the centre, the centre tilted 30 degrees down.
pub const ZOOM_START: f32 = 150.0;
pub const PITCH_START: f32 = std::f32::consts::FRAC_PI_6;
/// F041481: the centre unturned, the camera looking along +z (yaw here counts from -z).
pub const YAW_START: f32 = std::f32::consts::PI;
/// F041475: the distance kept within 10 - 2000.
pub const ZOOM_MIN: f32 = 10.0;
pub const ZOOM_MAX: f32 = 2000.0;
/// F041481: the camera's near clip distance.
pub const NEAR_CLIP: f32 = 5.0;

/// The camera's options (settings.cfg; F032944 reads them into 0x142133490, F045909 the mouse
/// speeds), read again when the rewrite's copy of the file changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraSettings {
    /// "camera speed" (500): the pan's speed at 600 back, a second.
    pub speed: f32,
    /// "mouse speed pan" / "mouse speed tilt" (6): degrees a second per unit of turn.
    pub pan: f32,
    pub tilt: f32,
    /// "camera zoom" (125): distance a notch.
    pub zoom: f32,
    /// "invert X" (off) / "invert Y" (on): -1 when on.
    pub invert_x: f32,
    pub invert_y: f32,
    /// "Edge scrolling" (on).
    pub edge: bool,
    /// "mouse speed horizontal" / "vertical" (1.5): the mouse's movement scale (F046179).
    pub mouse_x: f32,
    pub mouse_y: f32,
}

impl CameraSettings {
    pub fn read(root: &Path) -> CameraSettings {
        let cfg = kenshi_io::cfg::load(root, "settings.cfg");
        let on = |k: &str, d: bool| cfg.get(k).map_or(d, |v| v.trim() != "0" && !v.trim().eq_ignore_ascii_case("false"));
        let sign = |on: bool| if on { -1.0 } else { 1.0 };
        CameraSettings {
            speed: cfg.f32("camera speed", 500.0),
            pan: cfg.f32("mouse speed pan", 6.0),
            tilt: cfg.f32("mouse speed tilt", 6.0),
            zoom: cfg.f32("camera zoom", 125.0),
            invert_x: sign(on("invert X", false)),
            invert_y: sign(on("invert Y", true)),
            edge: on("Edge scrolling", true),
            mouse_x: cfg.f32("mouse speed horizontal", 1.5),
            mouse_y: cfg.f32("mouse speed vertical", 1.5),
        }
    }
}

/// The camera's own state beside `Game`'s focus, yaw, pitch and distance.
#[derive(Resource)]
pub struct CameraRig {
    /// The character the camera follows (the camera's target, +0x28; F045539 sets it, F045486 /
    /// F041466 clear it).
    pub follow: Option<usize>,
    /// The turn and tilt smoothing filters' last 20 inputs, newest first (F040279 / F029587).
    yaw_in: VecDeque<f32>,
    pitch_in: VecDeque<f32>,
    /// The centre's height it moves to and its rate (+0xa4, +0xa8; F041476).
    rise_to: f32,
    rise: f32,
    /// The next height is taken at once (F041479's F041476 without smoothing).
    pub snap: bool,
    /// Where the cursor is held while the mouse turns the camera (F041486).
    held_cursor: Option<Vec2>,
    /// The last squad select key pressed (its number, the character and when; F044306).
    tap: Option<(usize, usize, f32)>,
    pub settings: CameraSettings,
    /// The settings file's time when read, and when it was last looked at.
    read_at: Option<std::time::SystemTime>,
    looked: f32,
    /// The free camera (F041485): on, where, its turn, its speed and turning speed (0x1421306b0,
    /// 0x1421306a0), its speed scale (0x141efacf4).
    pub free: bool,
    free_at: Vec3,
    free_rot: Quat,
    free_vel: Vec3,
    free_turn: Vec3,
    free_scale: f32,
}

impl CameraRig {
    pub fn new(root: &Path) -> CameraRig {
        CameraRig {
            follow: None,
            yaw_in: VecDeque::from(vec![0.0; 20]),
            pitch_in: VecDeque::from(vec![0.0; 20]),
            rise_to: 0.0,
            rise: 0.0,
            snap: true,
            held_cursor: None,
            tap: None,
            settings: CameraSettings::read(root),
            read_at: settings_time(),
            looked: 0.0,
            free: false,
            free_at: Vec3::ZERO,
            free_rot: Quat::IDENTITY,
            free_vel: Vec3::ZERO,
            free_turn: Vec3::ZERO,
            free_scale: 0.01,
        }
    }

    /// As at a game's start (a new game or a load).
    pub fn reset(&mut self) {
        self.follow = None;
        self.yaw_in = VecDeque::from(vec![0.0; 20]);
        self.pitch_in = VecDeque::from(vec![0.0; 20]);
        self.rise = 0.0;
        self.snap = true;
        self.held_cursor = None;
        self.tap = None;
        self.free = false;
    }

    /// Follow a character (F045539: the camera's target, from 10 over them).
    pub fn follow(&mut self, id: usize) {
        self.follow = Some(id);
    }

    /// F044306: squad member `id`'s select key `n` pressed at `now`; pressed twice within 0.5 s
    /// the camera follows them.
    pub fn select_key(&mut self, n: usize, id: usize, now: f32) -> bool {
        let twice = self.tap.is_some_and(|(k, who, t)| k == n && who == id && now - t < 0.5);
        self.tap = Some((n, id, now));
        if twice {
            self.follow(id);
        }
        twice
    }

    /// The settings read again once a second if the file changed (the options window saves it).
    fn reread(&mut self, root: &Path, dt: f32) {
        self.looked += dt;
        if self.looked < 1.0 {
            return;
        }
        self.looked = 0.0;
        let t = settings_time();
        if t != self.read_at {
            self.read_at = t;
            self.settings = CameraSettings::read(root);
        }
    }
}

fn settings_time() -> Option<std::time::SystemTime> {
    std::fs::metadata(kenshi_io::cfg::user_dir().join("settings.cfg")).and_then(|m| m.modified()).ok()
}

/// F029587: the inputs weighted w^k, newest first, over their weights.
fn smoothed(q: &VecDeque<f32>, w: f32) -> f32 {
    let (mut sum, mut weights, mut k) = (0.0, 0.0, 1.0);
    for x in q {
        sum += k * x;
        weights += k;
        k *= w;
    }
    if weights > 0.0 { sum / weights } else { 0.0 }
}

fn push(q: &mut VecDeque<f32>, x: f32) {
    q.push_front(x);
    q.truncate(20);
}

/// Where the camera is: `dist` behind the centre along its turn and tilt.
pub fn eye_of(game: &Game) -> Vec3 {
    let (sy, cy) = game.yaw.sin_cos();
    let (sp, cp) = game.pitch.sin_cos();
    game.focus + Vec3::new(sy * cp, sp, cy * cp) * game.dist
}

/// The highest navmesh floor under `p` (scene) that is shown: not a storey of an opened building
/// above the floor it shows, nor over its interior (its roof) when its floors are hidden.
/// (stand-in for F041476's ray from 3000 over the centre against the building floors' physics
/// groups 9 - 12 up to the floor shown)
fn floor_below(game: &Game, views: &Query<&BuildingView>, open: &OpenBuildings, p: Vec3, up: f32) -> Option<f32> {
    let m = kenshi_world::navmesh::METRES;
    let origin = game.zone.origin;
    let opened: Vec<&BuildingView> = views.iter().filter(|v| open.open.contains(&(v.zone, v.building))).collect();
    let shown = |q: [f32; 3]| {
        let w = Vec3::from(q) / m;
        if building_at(views, w.to_array()).is_some_and(|(b, f)| open.open.contains(&b) && f > open.floor) {
            return false;
        }
        !opened.iter().any(|v| {
            let (lo, hi) = (v.interior.lo, v.interior.hi);
            let over = lo[0] <= w.x && w.x <= hi[0] && lo[2] <= w.z && w.z <= hi[2] && w.y > hi[1] - 2.0;
            let top = v.levels.iter().map(|l| l.0).max().unwrap_or(0);
            over && top + 1 > open.floor
        })
    };
    let from = (p + Vec3::Y * up + origin) * m;
    game.sim
        .nav
        .raycast(from.to_array(), [0.0, -1.0, 0.0], (up + 200.0) * m, &shown)
        .map(|(q, _)| q[1] / m - origin.y)
}

/// F041476: the centre's height. `smooth`: moved there at 10 x the gap a second (the frame's
/// update), else at once (F041479's move).
fn height(game: &mut Game, rig: &mut CameraRig, views: &Query<&BuildingView>, open: &OpenBuildings, dt: f32, smooth: bool) {
    let c = game.focus;
    let eye = eye_of(game);
    let water = kenshi_render::water_material::WATER_HEIGHT - game.zone.origin.y;
    // the ground under the centre (or a floor shown) and under the camera, not under the water
    let mut under = if rig.follow.is_some() {
        c.y
    } else {
        let g = game.ground(c.x, c.z);
        floor_below(game, views, open, c, 3000.0).map_or(g, |f| f.max(g))
    };
    let mut below_eye = game.ground(eye.x, eye.z);
    under = under.max(water);
    below_eye = below_eye.max(water);
    // free, the centre comes down to the camera's ground (no lower than its own); the camera kept
    // min(distance x 0.2, 20) + 10 over the ground under it
    let down = if rig.follow.is_none() { (below_eye - c.y).min(0.0) } else { 0.0 };
    let mut y = c.y + down;
    let clear = below_eye + (game.dist * 0.2).min(20.0) + 10.0;
    if eye.y + down <= clear {
        y = c.y + clear - eye.y;
    }
    y = y.max(under);
    if rig.follow.is_none() && smooth {
        if (y - c.y).abs() > 0.1 {
            if y != rig.rise_to {
                rig.rise = (y - c.y) * 10.0;
                rig.rise_to = y;
            }
            y = c.y + dt * rig.rise;
        }
    } else {
        rig.rise_to = y;
        rig.rise = 0.0;
    }
    if rig.rise != 0.0 {
        let short = if rig.rise <= 0.0 { rig.rise_to < y } else { y < rig.rise_to };
        if !short {
            y = rig.rise_to;
            rig.rise = 0.0;
        }
    }
    game.focus.y = y;
}

/// @fn F041485
///
/// The free camera's frame (see the module notes); `moves` the pan keys (x left, y up, z ahead),
/// `notches` the wheel, (`mdx`, `mdy`) the mouse, `roll` the rotate keys (right - left).
#[allow(clippy::too_many_arguments)]
fn free_camera(game: &Game, rig: &mut CameraRig, dt: f32, moves: Vec3, notches: f32, mdx: f32, mdy: f32, roll: f32) -> Transform {
    let s = rig.settings;
    let k = dt * 100.0;
    // the wheel scales the speed
    rig.free_scale = (rig.free_scale * (notches * 0.1 + 1.0)).clamp(0.0001, 0.1);
    // its own facing: x left (Ogre's camera looks along +z of the centre: here -z, so left is -x)
    let dir = moves.normalize_or_zero();
    let world = rig.free_rot * Vec3::new(-dir.x, dir.y, -dir.z);
    rig.free_vel += world * k * 0.1 * s.speed * rig.free_scale;
    rig.free_at += rig.free_vel * k;
    rig.free_vel -= rig.free_vel * (0.1 * k).min(1.0);
    // 2 over the ground
    let g = game.ground(rig.free_at.x, rig.free_at.z) + 2.0;
    if rig.free_at.y < g {
        rig.free_at.y = g;
        rig.free_vel.y = rig.free_vel.y.max(0.0);
    }
    // turning: yaw about the world's up (its own while rolled over), pitch and roll its own
    let v = Vec3::new(-mdx * 0.01 * s.tilt * s.invert_x, -mdy * 0.01 * s.tilt * s.invert_y, roll * s.tilt * 0.1);
    rig.free_turn += v * k * 0.1;
    let up = rig.free_rot * Vec3::Y;
    let side = (rig.free_rot * Vec3::NEG_Z).cross(Vec3::Y);
    let own_yaw = up.dot(side).abs() > 0.01 || up.y < 0.0;
    let yaw = Quat::from_rotation_y((rig.free_turn.x * k).to_radians());
    rig.free_rot = if own_yaw { rig.free_rot * yaw } else { yaw * rig.free_rot };
    // (Kenshi's camera looks along its node's +z, Bevy's along -z: its pitch and roll turn the
    // other way here)
    rig.free_rot = rig.free_rot * Quat::from_rotation_x(-(rig.free_turn.y * k).to_radians()) * Quat::from_rotation_z(-(rig.free_turn.z * k).to_radians());
    rig.free_rot = rig.free_rot.normalize();
    rig.free_turn -= rig.free_turn * (0.1 * k).min(1.0);
    Transform::from_translation(rig.free_at).with_rotation(rig.free_rot)
}

/// The camera, each frame (F041486 and F041474).
#[allow(clippy::too_many_arguments)]
pub fn camera(
    act: bindings::Actions,
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut rig: ResMut<CameraRig>,
    mut cams: Query<&mut Transform, MainCam>,
    mut centre: ResMut<kenshi_render::zone::FoliageCentre>,
    mut windows: Query<&mut Window>,
    (overview, editor, build, trade, tutorials): (
        Res<super::research_ui::ResearchWindow>,
        Option<Res<super::editor::Editor>>,
        Option<Res<super::build_ui::BuildMode>>,
        Res<super::slave_ui::SlaveTrade>,
        Option<Res<super::tutorial_ui::Tutorials>>,
    ),
    (views, open): (Query<&BuildingView>, Res<OpenBuildings>),
) {
    let dt = time.delta_secs();
    let root = game.root.clone();
    rig.reread(&root, dt);
    let s = rig.settings;
    let Ok(mut win) = windows.single_mut() else { return };
    let cursor = win.cursor_position();
    // the overview window (map, factions), the character trade windows and the tutorials
    // window have the wheel while open; the character editor's rows have it over its right
    // panel
    let over_editor = editor.is_some() && cursor.is_some_and(|c| c.x > win.width() * 0.7);
    let mut notches: f32 = wheel
        .read()
        .map(|w| match w.unit {
            // OIS's 120 a notch, over 100 (F046179)
            MouseScrollUnit::Line => w.y * 1.2,
            MouseScrollUnit::Pixel => w.y / 100.0,
        })
        .sum();
    // (testing) KENSHI_NO_MOUSE: the mouse's wheel, movement and place left out (screenshots
    // taken while someone uses the mouse elsewhere)
    let no_mouse = std::env::var("KENSHI_NO_MOUSE").is_ok();
    if overview.open.is_some() || over_editor || trade.is_open() || tutorials.is_some_and(|t| t.window_open()) || no_mouse {
        notches = 0.0;
    }
    let cursor = if no_mouse { None } else { cursor };
    if no_mouse {
        motion.clear();
    }
    // the mouse's movement (F046179: OIS's relative movement x "mouse speed horizontal" /
    // "vertical")
    let (mut mdx, mut mdy) = (0.0, 0.0);
    for m in motion.read() {
        mdx += m.delta.x * s.mouse_x;
        mdy += m.delta.y * s.mouse_y;
    }
    let typing = act.is_typing();
    let placing = build.is_some_and(|b| b.is_open());
    // the free camera ("Free camera mode", F041485): from where the camera is
    if !typing && act.just("toggle_fps_camera") {
        rig.free = !rig.free;
        if rig.free {
            let eye = eye_of(&game);
            rig.free_at = eye;
            rig.free_rot = Transform::from_translation(eye).looking_at(game.focus, Vec3::Y).rotation;
            rig.free_vel = Vec3::ZERO;
            rig.free_turn = Vec3::ZERO;
            rig.free_scale = 0.01;
            rig.held_cursor = cursor;
        }
    }
    if rig.free {
        let mut moves = Vec3::ZERO;
        if !typing {
            for (a, d) in [("camera_left", Vec3::X), ("camera_right", -Vec3::X), ("camera_forward", Vec3::Z), ("camera_back", -Vec3::Z), ("camera_zoom_in", Vec3::Y), ("camera_zoom_out", -Vec3::Y)] {
                if act.held(a) {
                    moves += d;
                }
            }
        }
        let roll = if typing { 0.0 } else { act.held("camera_rotate_right") as i32 as f32 - act.held("camera_rotate_left") as i32 as f32 };
        let t = free_camera(&game, &mut rig, dt, moves, notches, mdx, mdy, roll);
        if let Some(c) = rig.held_cursor {
            win.set_cursor_position(Some(c));
        }
        game.mouse_turning = false;
        if let Ok(mut tr) = cams.single_mut() {
            *tr = t;
        }
        centre.0 = Some(t.translation);
        return;
    }
    // the keys' pan: x to the left, z ahead (F041486)
    let mut pan = Vec2::ZERO;
    if !typing {
        for (a, d) in [("camera_left", Vec2::X), ("camera_right", -Vec2::X), ("camera_forward", Vec2::Y), ("camera_back", -Vec2::Y)] {
            if act.held(a) {
                pan += d;
            }
        }
    }
    // zoom (F041475): the zoom keys add 5 notches a second (3x with shift), not while following
    // or placing a building; the target distance taken from the current one, the camera closing
    // 70 x the frame time of the gap
    if rig.follow.is_none() && !placing && !typing {
        let keys = act.held("camera_zoom_in") as i32 - act.held("camera_zoom_out") as i32;
        let shift = act.keys().pressed(KeyCode::ShiftLeft) || act.keys().pressed(KeyCode::ShiftRight);
        notches += dt * keys as f32 * if shift { 3.0 } else { 1.0 } * 5.0;
    }
    let to = (game.dist - s.zoom * notches * (game.dist / 600.0).min(0.75)).clamp(ZOOM_MIN, ZOOM_MAX);
    game.dist += (to - game.dist) * (70.0 * dt).min(1.0);
    if notches != 0.0 && std::env::var("KENSHI_CAMERA_DEBUG").is_ok() {
        eprintln!("camera: {notches} notches, distance {} -> {to}", game.dist);
    }
    // turning: the mouse with mouse_rotate held, the rotate (9) and tilt (2) keys
    let turning = !typing && act.held("mouse_rotate");
    let (mut yaw_in, mut pitch_in) = (0.0, 0.0);
    if turning {
        yaw_in -= mdx;
        pitch_in -= mdy;
    }
    if !typing {
        if act.held("camera_rotate_left") {
            yaw_in -= 9.0;
        }
        if act.held("camera_rotate_right") {
            yaw_in += 9.0;
        }
        // (assumption) placing a building, the tilt keys tilt it instead
        if !placing {
            if act.held("camera_tilt+") {
                pitch_in -= 2.0;
            }
            if act.held("camera_tilt-") {
                pitch_in += 2.0;
            }
        }
    }
    // the screen's edges ("Edge scrolling"): pan, or following, turn; neither while the
    // overview (Kenshi's management screen, F035172 / F035115: the update's second argument) is
    // open
    if overview.open.is_none() {
        if s.edge
            && let Some(c) = cursor
        {
            let (u, v) = (c.x / win.width(), c.y / win.height());
            if rig.follow.is_none() {
                pan.x += (u < 0.01) as i32 as f32 - (u > 0.99) as i32 as f32;
                pan.y += (v < 0.01) as i32 as f32 - (v > 0.99) as i32 as f32;
            } else {
                yaw_in += 3.0 * ((u > 0.99) as i32 - (u < 0.01) as i32) as f32;
                pitch_in += (v > 0.99) as i32 as f32 - (v < 0.01) as i32 as f32;
            }
        }
        // F041467: the pan's direction the centre's turn and tilt give, flattened, at "camera
        // speed" x distance / 600; it stops following (F045486)
        if pan != Vec2::ZERO {
            rig.follow = None;
            let (sy, cy) = game.yaw.sin_cos();
            let cp = game.pitch.cos();
            let left = Vec3::new(-cy, 0.0, sy);
            let ahead = Vec3::new(-sy * cp, 0.0, -cy * cp);
            let dir = (left * pan.x + ahead * pan.y).normalize_or_zero();
            let step = dir * dt * s.speed * game.dist / 600.0;
            game.focus += step;
        }
    }
    // following: the centre 10 over the character (F041486)
    if let Some(id) = rig.follow {
        match game.sim.folk.get(id) {
            Some(p) if !p.dead => game.focus = Vec3::from(p.mover.position_units()) - game.zone.origin + Vec3::Y * 10.0,
            _ => rig.follow = None,
        }
    }
    // F041482: the turn and tilt smoothed over their last 20 frames, none over a long frame;
    // the tilt only while the view points less than 0.92 down (or, tilting up, 0.2 up)
    push(&mut rig.yaw_in, yaw_in);
    push(&mut rig.pitch_in, pitch_in);
    let (mut turn, mut tilt) = (smoothed(&rig.yaw_in, 0.5), smoothed(&rig.pitch_in, 0.7));
    if dt > 0.2 {
        turn = 0.0;
        tilt = 0.0;
    }
    if turn != 0.0 {
        game.yaw += (s.pan * turn * dt * s.invert_x).to_radians();
    }
    if tilt != 0.0 {
        let d = s.tilt * tilt * dt * s.invert_y;
        let view_y = -game.pitch.sin();
        if if d < 0.0 { view_y < 0.2 } else { view_y > -0.92 } {
            // (stand-in) never past straight up or down
            game.pitch = (game.pitch + d.to_radians()).clamp(-1.5, 1.5);
        }
    }
    // the rotation marker shows while turning; the cursor is held where the turn began
    // (testing) KENSHI_MOUSE_TURN: the mouse-rotate key held (the marker shown)
    game.mouse_turning = turning || std::env::var("KENSHI_MOUSE_TURN").is_ok();
    if turning {
        match rig.held_cursor {
            Some(c) => win.set_cursor_position(Some(c)),
            None => rig.held_cursor = cursor,
        }
    } else {
        rig.held_cursor = None;
    }
    // the character editor's stage, under the ground: its own height, the ground not in the way
    if let Some((_, at)) = game.stage {
        game.focus.y = at.y + 3.5;
    } else {
        // F041476: the centre's height
        let smooth = !std::mem::take(&mut rig.snap);
        height(&mut game, &mut rig, &views, &open, dt, smooth);
        // F041474: within 2250 of the nearest selected character
        if rig.follow.is_none() {
            let origin = game.zone.origin;
            let c = game.focus;
            let near = game
                .selected
                .iter()
                .map(|&i| Vec3::from(game.sim.folk[i].mover.position_units()) - origin)
                .min_by(|a, b| a.distance_squared(c).total_cmp(&b.distance_squared(c)));
            if let Some(p) = near
                && p.distance_squared(c) > 2250.0 * 2250.0
            {
                game.focus = p + (c - p).normalize() * 2250.0;
            }
        }
    }
    let eye = eye_of(&game);
    if let Ok(mut tr) = cams.single_mut() {
        *tr = Transform::from_translation(eye).looking_at(game.focus, Vec3::Y);
    }
    if std::env::var("KENSHI_CAMERA_DEBUG").is_ok() && (time.elapsed_secs() * 2.0).fract() < dt * 2.0 {
        eprintln!(
            "camera: eye {eye:.0} centre {:.0} distance {:.0} pitch {:.2} yaw {:.2} follow {:?}",
            game.focus, game.dist, game.pitch, game.yaw, rig.follow
        );
    }
    centre.0 = Some(game.focus);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_weights_the_newest_most() {
        let mut q = VecDeque::from(vec![0.0; 20]);
        push(&mut q, 9.0);
        // 9 x 1 / (1 + 0.5 + 0.25 + ...)
        let s = smoothed(&q, 0.5);
        assert!((s - 9.0 / ((1.0 - 0.5f32.powi(20)) / 0.5)).abs() < 1e-4, "{s}");
        assert_eq!(q.len(), 20);
        for _ in 0..40 {
            push(&mut q, 9.0);
        }
        assert!((smoothed(&q, 0.7) - 9.0).abs() < 1e-4);
    }
}
