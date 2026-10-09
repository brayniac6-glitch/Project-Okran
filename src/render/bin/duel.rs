//! duel <Kenshi install dir> <recording> [--frames <dir>] [--gif <out.gif>]
//!
//! Replays a fight recorded by the combat crate's `duel` example (its stdout): two Greenlanders with
//! katanas, each placed where the fight put it, facing the other, posed by the clips the ported
//! animation layers blended at that moment. With `--frames` every recorded moment is captured to a
//! PNG; with `--gif` those frames become an animated GIF (20 frames a second, as recorded).
//! A knocked-out fighter stays standing: knockdowns and ragdolls are not ported.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{mods::Install, ogre_mesh, resources::Resources};
use kenshi_render::character::{
    Blend, CharacterAsset, CharacterSpec, animate_characters, load_character,
    spawn_character_with_joints, upload_character,
};
use kenshi_render::{appearance_material, to_bevy};
use std::path::PathBuf;

/// One fighter at one recorded moment.
#[derive(Clone)]
struct Pose {
    x: f32,
    z: f32,
    /// The root bone's in-clip travel (side, forward) to take back out.
    off: [f32; 2],
    /// The direction the fighter faces (toward its target; zero when it has none). Older
    /// two-fighter recordings lack it: they face each other.
    face: Option<[f32; 2]>,
    /// (clip name, time, weight)
    clips: Vec<(String, f32, f32)>,
}

#[derive(Resource)]
struct Fight {
    character: CharacterAsset,
    weapon: Option<(Vec<Mesh>, Option<String>, Option<String>)>,
    /// (time, every fighter) per recorded moment.
    frames: Vec<(f32, Vec<Pose>)>,
    events: Vec<(f32, String)>,
    out: Option<PathBuf>,
}

#[derive(Resource, Default)]
struct Scene {
    roots: Vec<Entity>,
    camera: Option<Entity>,
    caption: Option<Entity>,
}

fn arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

/// Recorded moments and events.
type Recording = (Vec<(f32, Vec<Pose>)>, Vec<(f32, String)>);

fn parse(text: &str) -> Recording {
    let mut frames: Vec<(f32, Vec<Option<Pose>>)> = Vec::new();
    let mut events = Vec::new();
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("f,") {
            let mut c: Vec<&str> = rest.splitn(9, ',').collect();
            // the facing columns, when the recording has them
            let face = if c.len() == 9 && c[6].parse::<f32>().is_ok() {
                Some([c[6].parse().unwrap(), c[7].parse().unwrap()])
            } else {
                c = rest.splitn(7, ',').collect();
                None
            };
            let t: f32 = c[0].parse().unwrap();
            let k: usize = c[1].parse().unwrap();
            let clips = c[c.len() - 1]
                .split('|')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let p: Vec<&str> = s.rsplitn(3, ':').collect();
                    (
                        p[2].to_string(),
                        p[1].parse().unwrap(),
                        p[0].parse().unwrap(),
                    )
                })
                .collect();
            let pose = Pose {
                x: c[2].parse().unwrap(),
                z: c[3].parse().unwrap(),
                off: [c[4].parse().unwrap(), c[5].parse().unwrap()],
                face,
                clips,
            };
            if frames.last().is_none_or(|f| f.0 != t) {
                frames.push((t, Vec::new()));
            }
            let row = &mut frames.last_mut().unwrap().1;
            if row.len() <= k {
                row.resize(k + 1, None);
            }
            row[k] = Some(pose);
        } else if let Some((t, s)) = l.trim_start().split_once("s ")
            && let Ok(t) = t.parse::<f32>()
        {
            // keep the event short: drop the numbers after the move
            let s = s
                .split(" frame ")
                .next()
                .unwrap()
                .split(" (")
                .next()
                .unwrap()
                .to_string();
            events.push((t, s));
        }
    }
    let frames = frames
        .into_iter()
        .filter_map(|(t, p)| Some((t, p.into_iter().collect::<Option<Vec<_>>>()?)))
        .collect();
    (frames, events)
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: duel <Kenshi install dir> <recording> [--frames dir] [--gif out.gif]");
        return AppExit::error();
    }
    let root = PathBuf::from(&args[0]);
    let text = std::fs::read_to_string(&args[1]).expect("recording");
    let (frames, events) = parse(&text);
    eprintln!("{} moments, {} events", frames.len(), events.len());
    let mut order = Install::new(&root).load_order().expect("load order");
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).expect("game data");
    let res = Resources::index(&root);
    let mut spec = CharacterSpec::new("Greenlander");
    spec.hairs = vec!["Haircut09-merc".into()];
    spec.wear = vec![
        "Leather Shirt".into(),
        "Cargopants".into(),
        "Drifter's Boots".into(),
    ];
    let mut character = load_character(&root, &data, &res, &spec).expect("character");
    kenshi_render::character::strip_tracks(&mut character, &data);
    let weapon = data
        .of_type(ItemType::Weapon)
        .find(|r| r.name == "Katana")
        .and_then(|w| {
            let path = w
                .files
                .get("bare sword")
                .filter(|p| !p.is_empty())
                .or_else(|| w.files.get("mesh"))
                .and_then(|p| res.path(p))?;
            let mesh = ogre_mesh::read(&path).ok()?;
            let mat = appearance_material(&data, w);
            let tex = |k: &str| {
                mat.and_then(|m| m.files.get(k))
                    .filter(|p| !p.is_empty())
                    .and_then(|p| res.find(p))
            };
            Some((
                to_bevy(&mesh).into_iter().map(|(_, m)| m).collect(),
                tex("texture map"),
                tex("normal map"),
            ))
        });
    let out = arg(&args, "--frames").map(PathBuf::from);
    if let Some(d) = &out {
        std::fs::create_dir_all(d).expect("frames dir");
    }
    let n = frames.len();
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
                        title: "Kenshi rewrite - duel".into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .insert_resource(ClearColor(Color::srgb(0.78, 0.74, 0.66)))
        .insert_resource(Fight {
            character,
            weapon,
            frames,
            events,
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
        write_gif(&dir, n, std::path::Path::new(gif));
    }
    exit
}

#[allow(clippy::too_many_arguments)] // a Bevy startup system taking its resources
fn setup(
    mut commands: Commands,
    fight: Res<Fight>,
    mut scene: ResMut<Scene>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let handles = upload_character(
        &fight.character,
        &assets,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut bindposes,
    );
    let prop = fight
        .character
        .rig
        .names
        .iter()
        .position(|n| n == "Bip01 Prop2");
    let weapon = fight.weapon.as_ref().map(|(ms, t, n)| {
        let mat = materials.add(StandardMaterial {
            base_color_texture: t.clone().map(|p| assets.load(p)),
            normal_map_texture: n.clone().map(|p| assets.load(p)),
            perceptual_roughness: 0.5,
            metallic: 0.3,
            ..default()
        });
        let hs: Vec<Handle<Mesh>> = ms.iter().map(|m| meshes.add(m.clone())).collect();
        (hs, mat)
    });
    for _ in 0..fight.frames.first().map_or(0, |f| f.1.len()) {
        let (root, joints) = spawn_character_with_joints(
            &mut commands,
            &fight.character,
            &handles,
            Transform::default(),
            "combatstance",
            0.0,
            None,
        );
        commands.entity(root).insert(Blend(Vec::new()));
        if let (Some((hs, mat)), Some(p)) = (&weapon, prop) {
            for h in hs {
                let e = commands
                    .spawn((
                        Mesh3d(h.clone()),
                        MeshMaterial3d(mat.clone()),
                        Transform::default(),
                    ))
                    .id();
                commands.entity(joints[p]).add_child(e);
            }
        }
        scene.roots.push(root);
    }
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(3000.0, 3000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.55, 0.45),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(150.0, 0.0, 0.0),
    ));
    scene.camera = Some(
        commands
            .spawn((
                Camera3d::default(),
                Projection::Perspective(PerspectiveProjection {
                    fov: std::f32::consts::FRAC_PI_4,
                    ..default()
                }),
                Transform::from_xyz(150.0, 12.0, 300.0)
                    .looking_at(Vec3::new(150.0, 9.0, 0.0), Vec3::Y),
            ))
            .id(),
    );
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.6, 1.0, 0.8).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 350.0,
        ..default()
    });
    commands.spawn((
        Text::new("Kenshi rewrite: a fight on the ported combat rules (A left, B right)"),
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
    fight: Res<Fight>,
    scene: Res<Scene>,
    mut tick: Local<usize>,
    mut cam_dist: Local<f32>,
    mut q: Query<(&mut Transform, &mut Blend), Without<Camera3d>>,
    mut cams: Query<&mut Transform, With<Camera3d>>,
    mut texts: Query<&mut Text>,
    mut exit: MessageWriter<AppExit>,
) {
    *tick += 1;
    let k = tick.saturating_sub(WARMUP).min(fight.frames.len() - 1);
    let (t, poses) = &fight.frames[k];
    let lib = &fight.character.animations;
    for (i, p) in poses.iter().enumerate() {
        let (dx, dz) = match p.face {
            Some(f) => (f[0], f[1]),
            None => (poses[1 - i].x - p.x, poses[1 - i].z - p.z),
        };
        let Some(&root) = scene.roots.get(i) else {
            continue;
        };
        // no target: keep the last facing
        let rot = if dx == 0.0 && dz == 0.0 {
            q.get(root).map_or(Quat::IDENTITY, |(tf, _)| tf.rotation)
        } else {
            Quat::from_rotation_y(dx.atan2(dz))
        };
        let off = rot * Vec3::new(p.off[0], 0.0, p.off[1]);
        if let Ok((mut tf, mut blend)) = q.get_mut(root) {
            *tf = Transform::from_xyz(p.x - off.x, 0.0, p.z - off.z).with_rotation(rot);
            blend.0 = p
                .clips
                .iter()
                .filter_map(|(n, time, w)| {
                    let idx = lib.iter().position(|a| a.name.eq_ignore_ascii_case(n))?;
                    Some((idx, *time, *w))
                })
                .collect();
        }
    }
    // follow the fight from the side, far enough to keep everyone in view
    let (lo, hi) = poses
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
    let depth = poses.iter().map(|p| p.z.abs()).fold(0.0f32, f32::max);
    let mid = (lo + hi) * 0.5;
    let sep = hi - lo;
    let fov = std::f32::consts::FRAC_PI_4;
    let want = ((sep * 0.5 + 30.0) / ((fov * 0.5).tan() * (16.0 / 9.0))).max(70.0);
    *cam_dist = if *cam_dist == 0.0 {
        want
    } else {
        *cam_dist + (want - *cam_dist) * 0.15
    };
    if let Ok(mut c) = cams.single_mut() {
        // a crowd is watched from above at three quarters, a pair from eye height
        *c = if depth > 0.0 {
            // the crowd's centre and spread, the view held between 60 and 220 units across
            let n = poses.len() as f32;
            let (cx, cz) = poses
                .iter()
                .fold((0.0, 0.0), |(x, z), p| (x + p.x / n, z + p.z / n));
            let r = (sep * 0.5).max(depth).clamp(60.0, 220.0);
            let d = r / (fov * 0.5).tan();
            let dir = Vec3::new(0.35, 0.6, 0.72).normalize();
            Transform::from_translation(Vec3::new(cx, 0.0, cz) + dir * d)
                .looking_at(Vec3::new(cx, 0.0, cz), Vec3::Y)
        } else {
            Transform::from_xyz(mid, 14.0, *cam_dist).looking_at(Vec3::new(mid, 9.0, 0.0), Vec3::Y)
        };
    }
    if let Some(e) = scene.caption
        && let Ok(mut text) = texts.get_mut(e)
    {
        let recent: Vec<&str> = fight
            .events
            .iter()
            .filter(|(et, _)| *et <= *t && *t - *et < 1.5)
            .map(|(_, s)| s.as_str())
            .collect();
        let shown = recent
            .iter()
            .rev()
            .take(2)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        text.0 = format!("{t:5.2} s\n{shown}");
    }
    if *tick > WARMUP
        && *tick <= WARMUP + fight.frames.len()
        && let Some(dir) = &fight.out
    {
        {
            let path = dir.join(format!("frame{:04}.png", *tick - WARMUP - 1));
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        }
    }
    if fight.out.is_some() && *tick > WARMUP + fight.frames.len() + DRAIN {
        exit.write(AppExit::Success);
    }
}

/// Encode the captured frames as an animated GIF (halved in size, 50 ms a frame).
fn write_gif(dir: &std::path::Path, n: usize, out: &std::path::Path) {
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
            img.width() * 2 / 3,
            img.height() * 2 / 3,
            imageops::FilterType::Triangle,
        );
        let delay = if i + 1 == n { 3000 } else { 50 };
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
