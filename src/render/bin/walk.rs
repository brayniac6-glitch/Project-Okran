//! walk <Kenshi install dir> <recording> [--frames <dir>] [--gif <out.gif>] [--every N]
//!
//! Replays a run recorded by the char crate's `walk` example (`t,x,y,z,speed` rows in world units,
//! 60 a second): a Greenlander following a found path over the shipped navmesh, in the zone the
//! run crosses (terrain and buildings, `render::zone`). The base animation layers run as the
//! animation class chooses them (F037184 via `anim::select`: the movement blend by speed, the best
//! idle when standing); the facing is the direction of travel. With `--frames` every Nth tick
//! (default 3) is captured, with `--gif` those frames become an animated GIF.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use kenshi_anim::clip::SingleAnimation;
use kenshi_anim::layer::{AnimationLayer, LAYER_BLEND, PlayParams};
use kenshi_anim::select::{AnimationData, Library, Situation, idle, movement};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{mods::Install, resources::Resources};
use kenshi_render::character::{
    Blend, CharacterAsset, CharacterSpec, animate_characters, load_character,
    spawn_character_with_joints, strip_tracks, upload_character,
};
use kenshi_render::zone::{ZoneScene, load_zone};
use kenshi_world::{Terrain, zone_of};
use std::collections::HashMap;
use std::path::PathBuf;

/// The walker at one captured moment, in scene coordinates.
struct Moment {
    t: f32,
    at: Vec3,
    yaw: f32,
    speed: f32,
    /// (clip index, time, weight)
    clips: Vec<(usize, f32, f32)>,
}

#[derive(Resource)]
struct Walk {
    zone: ZoneScene,
    character: CharacterAsset,
    moments: Vec<Moment>,
    /// The run's track in scene coordinates, drawn as markers.
    track: Vec<Vec3>,
    out: Option<PathBuf>,
}

#[derive(Resource, Default)]
struct Scene {
    root: Option<Entity>,
    caption: Option<Entity>,
}

fn arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

/// Run the base animation layers over the recording (as the combat `duel` example does for its
/// fighters, without an action layer) and keep every `every`th tick.
fn animate(
    rows: &[[f32; 5]],
    lib: &Library,
    lengths: &HashMap<String, f32>,
    character: &CharacterAsset,
    blend_rate: f32,
    origin: Vec3,
    every: usize,
) -> Vec<Moment> {
    let mut layers = [
        AnimationLayer::<()>::with_blend(blend_rate, LAYER_BLEND[0]),
        AnimationLayer::with_blend(blend_rate, LAYER_BLEND[1]),
        AnimationLayer::with_blend(blend_rate, LAYER_BLEND[2]),
    ];
    let mut out = Vec::new();
    let mut yaw = 0.0f32;
    let mut last_t = 0.0;
    for (i, r) in rows.iter().enumerate() {
        let dt = (r[0] - last_t).max(1e-4);
        last_t = r[0];
        if let Some(n) = rows.get(i + 1) {
            let (dx, dz) = (n[1] - r[1], n[3] - r[3]);
            if dx * dx + dz * dz > 1e-6 {
                yaw = dx.atan2(dz);
            }
        }
        let sit = Situation {
            category: 0,
            speed: r[4],
            weapon_l: false,
            weapon_r: false,
            combat_mode: 0,
            stealth_mode: 0,
            r_leg: 100.0,
            l_leg: 100.0,
            ..Default::default()
        };
        let wanted: Vec<(&AnimationData, f32, f32)> = if sit.speed > 0.5 {
            movement(lib, &sit, 1.0, 1.0)
        } else {
            idle(lib, &sit, 1.0)
                .map(|a| vec![(a, a.play_speed, 1.0)])
                .unwrap_or_default()
        };
        for (li, layer) in layers.iter_mut().enumerate() {
            let here: Vec<&(&AnimationData, f32, f32)> = wanted
                .iter()
                .filter(|(a, _, _)| (a.layer.clamp(0, 2) as usize) == li)
                .collect();
            let names: Vec<String> = layer.active.iter().map(|c| c.name.clone()).collect();
            for n in names {
                if !here.iter().any(|(a, _, _)| a.anim_name == n) {
                    layer.stop(&n);
                }
            }
            for (a, rate, w) in here {
                let Some(&len) = lengths.get(&a.anim_name.to_lowercase()) else {
                    continue;
                };
                if let Some(c) = layer.active.iter_mut().find(|c| c.name == a.anim_name) {
                    c.speed = *rate;
                    c.target_weight = *w;
                } else {
                    let p = PlayParams {
                        speed: *rate,
                        looping: true,
                        target_weight: *w,
                        end_time: 1.0,
                        root_motion: false,
                        keep_weight: false,
                    };
                    let name = a.anim_name.clone();
                    layer.play(&name, p, || {
                        SingleAnimation::new(&name, len, *rate, 1.0, true, vec![])
                    });
                }
            }
        }
        for (li, layer) in layers.iter_mut().enumerate() {
            layer.update(0.0, dt, i == 0, li != 2, 1.0);
        }
        if i % every == 0 {
            let clips = layers
                .iter()
                .flat_map(|l| l.active.iter().chain(&l.fading))
                .filter_map(|c| {
                    let idx = character
                        .animations
                        .iter()
                        .position(|a| a.name.eq_ignore_ascii_case(&c.name))?;
                    Some((idx, c.time, c.weight))
                })
                .collect();
            out.push(Moment {
                t: r[0],
                at: Vec3::new(r[1], r[2], r[3]) - origin,
                yaw,
                speed: r[4],
                clips,
            });
        }
    }
    out
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!(
            "usage: walk <Kenshi install dir> <recording> [--frames dir] [--gif out.gif] [--every N]"
        );
        return AppExit::error();
    }
    let root = PathBuf::from(&args[0]);
    let rows: Vec<[f32; 5]> = std::fs::read_to_string(&args[1])
        .expect("recording")
        .lines()
        .filter_map(|l| {
            let v: Vec<f32> = l.split(',').filter_map(|x| x.parse().ok()).collect();
            v.try_into().ok()
        })
        .collect();
    let every: usize = arg(&args, "--every").map_or(3, |n| n.parse().expect("--every N"));
    let mut order = Install::new(&root).load_order().expect("load order");
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).expect("game data");
    let res = Resources::index(&root);
    let mut spec = CharacterSpec::new("Greenlander");
    spec.hairs = vec!["Haircut09-merc".into(), "Beard02".into()];
    spec.wear = vec![
        "Dustcoat".into(),
        "Drifter's Leather Pants".into(),
        "Drifter's Boots".into(),
    ];
    let mut character = load_character(&root, &data, &res, &spec).expect("character");
    strip_tracks(&mut character, &data);
    let lib = Library::sort(data.of_type(ItemType::Animation));
    let lengths: HashMap<String, f32> = character
        .animations
        .iter()
        .map(|a| (a.name.to_lowercase(), a.length))
        .collect();
    let blend_rate = data
        .get("110-gamedata.quack")
        .and_then(|g| g.floats.get("animation blend rate").copied())
        .unwrap_or(4.0);

    // the zone the run spends most time in
    let mut count: HashMap<(i32, i32), usize> = HashMap::new();
    for r in &rows {
        *count.entry(zone_of(r[1], r[3])).or_default() += 1;
    }
    let zone_xy = count.into_iter().max_by_key(|e| e.1).expect("rows").0;
    let mut terrain = Terrain::open(&root).expect("heightmap");
    let zone = load_zone(&root, &data, &res, &mut terrain, zone_xy, 2500.0).expect("zone");
    eprintln!(
        "zone {}.{}: {} building pieces; {} ticks",
        zone_xy.0,
        zone_xy.1,
        zone.pieces.len(),
        rows.len()
    );
    let origin = zone.origin;
    let moments = animate(&rows, &lib, &lengths, &character, blend_rate, origin, every);
    let track = rows
        .iter()
        .step_by(30)
        .map(|r| Vec3::new(r[1], r[2], r[3]) - origin)
        .collect();
    let out = arg(&args, "--frames").map(PathBuf::from);
    if let Some(d) = &out {
        std::fs::create_dir_all(d).expect("frames dir");
    }
    let n = moments.len();
    let fps = 60.0 / every as f32;
    let exit = App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: root.to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: (960, 540).into(),
                        title: "Kenshi rewrite - walk".into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .insert_resource(ClearColor(Color::srgb(0.72, 0.68, 0.6)))
        .insert_resource(Walk {
            zone,
            character,
            moments,
            track,
            out: out.clone(),
        })
        .init_resource::<Scene>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (play.before(animate_characters), animate_characters),
        )
        .run();
    if let (Some(dir), Some(gif)) = (out, arg(&args, "--gif")) {
        write_gif(&dir, n, std::path::Path::new(gif), (1000.0 / fps) as u32);
    }
    exit
}

#[allow(clippy::too_many_arguments)] // a Bevy startup system taking its resources
fn setup(
    mut commands: Commands,
    mut walk: ResMut<Walk>,
    mut scene: ResMut<Scene>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    walk.zone
        .spawn(&mut commands, &assets, &mut meshes, &mut materials);
    let handles = upload_character(
        &walk.character,
        &assets,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut bindposes,
    );
    let (root, _) = spawn_character_with_joints(
        &mut commands,
        &walk.character,
        &handles,
        Transform::default(),
        "idle_stand_relax",
        0.0,
        None,
    );
    commands.entity(root).insert(Blend(Vec::new()));
    scene.root = Some(root);
    // the path ahead, as small posts
    let post = meshes.add(Cylinder::new(1.2, 6.0));
    let post_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.85, 0.3),
        emissive: LinearRgba::rgb(0.5, 0.45, 0.1),
        ..default()
    });
    for p in &walk.track {
        commands.spawn((
            Mesh3d(post.clone()),
            MeshMaterial3d(post_mat.clone()),
            Transform::from_translation(*p + Vec3::Y * 3.0),
        ));
    }
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 0.42,
            far: 40000.0,
            ..default()
        }),
        Transform::default(),
        DistanceFog {
            color: Color::srgb(0.72, 0.68, 0.6),
            falloff: FogFalloff::Linear {
                start: 4000.0,
                end: 14000.0,
            },
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 1.2, 0.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 300.0,
        ..default()
    });
    commands.spawn((
        Text::new("Kenshi rewrite: a path over Kenshi's own navmesh, run by the ported mover"),
        TextFont {
            font_size: FontSize::Px(20.0),
            ..default()
        },
        TextColor(Color::srgb(0.12, 0.11, 0.1)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(16.0),
            ..default()
        },
    ));
    scene.caption = Some(
        commands
            .spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(Color::srgb(0.12, 0.11, 0.1)),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(14.0),
                    left: Val::Px(16.0),
                    ..default()
                },
            ))
            .id(),
    );
}

/// Frames to wait for shaders to compile before the first capture, and after the last.
const WARMUP: usize = 240;
const DRAIN: usize = 90;

#[allow(clippy::too_many_arguments)] // a Bevy system taking its queries and resources
fn play(
    mut commands: Commands,
    walk: Res<Walk>,
    scene: Res<Scene>,
    mut tick: Local<usize>,
    mut cam: Local<Option<(Vec3, Vec3)>>,
    mut q: Query<(&mut Transform, &mut Blend), Without<Camera3d>>,
    mut cams: Query<&mut Transform, With<Camera3d>>,
    mut texts: Query<&mut Text>,
    mut exit: MessageWriter<AppExit>,
) {
    *tick += 1;
    let k = tick.saturating_sub(WARMUP).min(walk.moments.len() - 1);
    let m = &walk.moments[k];
    if let Some(root) = scene.root
        && let Ok((mut tf, mut blend)) = q.get_mut(root)
    {
        *tf = Transform::from_translation(m.at).with_rotation(Quat::from_rotation_y(m.yaw));
        blend.0 = m.clips.clone();
    }
    // a high camera on a fixed bearing (clear of the buildings), easing after the walker
    let want_look = m.at + Vec3::Y * 10.0;
    let want_eye = want_look + Vec3::new(-70.0, 420.0, 110.0);
    let (eye, look) = match *cam {
        Some((e, l)) => (e + (want_eye - e) * 0.15, l + (want_look - l) * 0.15),
        None => (want_eye, want_look),
    };
    *cam = Some((eye, look));
    if let Ok(mut c) = cams.single_mut() {
        *c = Transform::from_translation(eye).looking_at(look, Vec3::Y);
    }
    if let Some(e) = scene.caption
        && let Ok(mut text) = texts.get_mut(e)
    {
        text.0 = format!("{:5.1} s   {:4.0} units/s", m.t, m.speed);
    }
    if *tick > WARMUP
        && *tick <= WARMUP + walk.moments.len()
        && let Some(dir) = &walk.out
    {
        let path = dir.join(format!("frame{:04}.png", *tick - WARMUP - 1));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if walk.out.is_some() && *tick > WARMUP + walk.moments.len() + DRAIN {
        exit.write(AppExit::Success);
    }
}

/// Encode the captured frames as an animated GIF (half size).
fn write_gif(dir: &std::path::Path, n: usize, out: &std::path::Path, delay_ms: u32) {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, imageops};
    let file = std::fs::File::create(out).expect("gif");
    let mut enc = GifEncoder::new_with_speed(file, 20);
    enc.set_repeat(Repeat::Infinite).unwrap();
    let mut written = 0;
    for i in 0..n {
        let Ok(img) = image::open(dir.join(format!("frame{i:04}.png"))) else {
            continue;
        };
        let small = imageops::resize(
            &img.to_rgba8(),
            img.width() / 2,
            img.height() / 2,
            imageops::FilterType::Triangle,
        );
        let delay = if i + 1 == n { 2000 } else { delay_ms };
        enc.encode_frame(Frame::from_parts(
            small,
            0,
            0,
            Delay::from_numer_denom_ms(delay, 1),
        ))
        .unwrap();
        written += 1;
    }
    eprintln!("{written} frames -> {}", out.display());
}
