//! technique <Kenshi install dir> <COMBAT_TECHNIQUE name> [--screenshot out.png] [--weapon <WEAPON>]
//!           [--race <RACE>] [--wear <ARMOUR>]...
//!
//! Shows one combat move as a strip of poses: the same character repeated left to right, each frozen
//! at a moment of the move - its start, every hit's impact, the end, and a moment after it stops.
//! The move comes from its COMBAT_TECHNIQUE record (`kenshi_combat::technique`) and plays on the
//! action layer over a combat stance on the base layer, both run by the ported animation layers
//! (`kenshi_anim::layer`, fade rates from GLOBAL CONSTANTS); poses blend the skeleton's own
//! keyframes by the layers' weights. `--base <clip>` picks the stance. The weapon sits on the right-hand prop bone ("Bip01 Prop2") with no
//! offset: Kenshi's attachment offsets are not decoded yet.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use kenshi_anim::clip::SingleAnimation;
use kenshi_anim::layer::{AnimationLayer, LAYER_BLEND, PlayParams};
use kenshi_combat::technique::{CombatTechnique, technique_play};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{mods::Install, ogre_mesh, resources::Resources};
use kenshi_render::character::{
    Blend, CharacterAsset, CharacterSpec, animate_characters, load_character,
    spawn_character_with_joints, upload_character,
};
use kenshi_render::{appearance_material, to_bevy};
use std::path::PathBuf;

/// A clip in a layer snapshot: library index, clip time, weight.
type LayeredClip = (usize, f32, f32);

#[derive(Resource)]
struct Strip {
    character: CharacterAsset,
    clip: String,
    /// (label, layered clips: library index, clip time, weight) per pose.
    poses: Vec<(String, Vec<LayeredClip>)>,
    weapon: Option<(Vec<Mesh>, Option<String>, Option<String>)>,
    title: String,
    screenshot: Option<PathBuf>,
}

fn arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 || args[0].starts_with("--") {
        eprintln!(
            "usage: technique <Kenshi install dir> <COMBAT_TECHNIQUE name> [--screenshot out.png] [--weapon name] [--race name] [--wear name]..."
        );
        return AppExit::error();
    }
    let root = PathBuf::from(&args[0]);
    let mut order = Install::new(&root).load_order().expect("load order");
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).expect("game data");
    let res = Resources::index(&root);

    let rec = data
        .of_type(ItemType::CombatTechnique)
        .find(|r| r.name == args[1])
        .unwrap_or_else(|| panic!("no COMBAT_TECHNIQUE '{}'", args[1]));
    let t = CombatTechnique::from_record(rec);

    let mut spec = CharacterSpec::new(arg(&args, "--race").unwrap_or("Greenlander"));
    spec.hairs = vec!["Haircut09-merc".into()];
    spec.wear = args
        .windows(2)
        .filter(|w| w[0] == "--wear")
        .map(|w| w[1].clone())
        .collect();
    if spec.wear.is_empty() {
        spec.wear = vec![
            "Leather Shirt".into(),
            "Cargopants".into(),
            "Drifter's Boots".into(),
        ];
    }
    let character = load_character(&root, &data, &res, &spec).expect("character");
    let clip = character
        .animations
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(&t.anim_name))
        .unwrap_or_else(|| panic!("no clip '{}' on the skeleton", t.anim_name));

    // the character's animation layers as the original runs them (F038910): a combat stance on the
    // base layer, the technique on the action layer; the base layer gets what the action's
    // weight-keeping clips leave
    let lib = &character.animations;
    let index = |name: &str| lib.iter().position(|a| a.name.eq_ignore_ascii_case(name));
    let gc = data.get("110-gamedata.quack"); // GLOBAL CONSTANTS
    let blend_rate = gc
        .and_then(|r| r.floats.get("animation blend rate").copied())
        .unwrap_or(4.0);
    let stance = arg(&args, "--base").unwrap_or("combatstance");
    let stance_clip = &lib[index(stance).unwrap_or_else(|| panic!("no clip '{stance}'"))];
    let mut base = AnimationLayer::<()>::with_blend(blend_rate, LAYER_BLEND[0]);
    let mut action = AnimationLayer::<()>::with_blend(blend_rate, LAYER_BLEND[1]);
    let looped = PlayParams {
        speed: 1.0,
        looping: true,
        target_weight: 1.0,
        end_time: 1.0,
        root_motion: false,
        keep_weight: false,
    };
    base.play(&stance_clip.name, looped, || {
        SingleAnimation::new(
            &stance_clip.name,
            stance_clip.length,
            1.0,
            1.0,
            true,
            vec![],
        )
    });
    base.update(0.0, 0.0, true, true, 1.0);
    let (params, reverse) = technique_play(&t, t.speed);
    action.play(&clip.name, params, || {
        let mut c = SingleAnimation::new(
            &clip.name,
            clip.length,
            params.speed,
            params.end_time,
            false,
            vec![],
        );
        c.reverse_fading = reverse;
        c
    });
    let dt = 1.0 / 240.0;
    let step = |base: &mut AnimationLayer<()>, action: &mut AnimationLayer<()>| {
        action.update(0.0, dt, false, true, 1.0);
        let keep: f32 = action
            .active
            .iter()
            .chain(&action.fading)
            .filter(|c| c.keep_weight)
            .map(|c| c.weight)
            .sum();
        base.update(0.0, dt, false, true, 1.0 - keep);
    };
    let snapshot =
        |base: &AnimationLayer<()>, action: &AnimationLayer<()>| -> Vec<(usize, f32, f32)> {
            [base, action]
                .iter()
                .flat_map(|l| l.active.iter().chain(&l.fading))
                .filter_map(|c| Some((index(&c.name)?, c.time, c.weight)))
                .collect()
        };
    let move_weight = |action: &AnimationLayer<()>| {
        action
            .active
            .iter()
            .chain(&action.fading)
            .map(|c| c.weight)
            .sum::<f32>()
    };
    // moments to show: the start, each hit's impact, the end of the move, then after it stops
    let mut marks: Vec<(String, f32)> = vec![("start".into(), 0.05)];
    for (i, f) in t.frames.iter().enumerate() {
        marks.push((
            format!("hit {} ({:.0}%)", i + 1, f.impact * 100.0),
            f.impact,
        ));
    }
    let end = t
        .frames
        .iter()
        .map(|f| f.stop)
        .fold(0.0, f32::max)
        .min(0.99);
    marks.push((format!("end ({:.0}%)", end * 100.0), end));
    let mut poses = Vec::new();
    let mut secs = 0.0;
    for (label, at) in marks {
        while action
            .find_active(&clip.name)
            .is_some_and(|c| c.progress < at && c.playing)
        {
            step(&mut base, &mut action);
            secs += dt;
        }
        let w = move_weight(&action);
        println!("{label}: {secs:.3} s, move weight {w:.2}");
        poses.push((
            format!("{label}\nmove {:.0}%", w * 100.0),
            snapshot(&base, &action),
        ));
    }
    action.stop(&clip.name);
    let mut since = 0.0;
    for after in [0.1, 0.25] {
        while since < after {
            step(&mut base, &mut action);
            since += dt;
        }
        let w = move_weight(&action).max(0.0);
        println!("stopped +{after} s: move weight {w:.2}");
        poses.push((
            format!(
                "stopped +{after} s
move {:.0}%",
                w * 100.0
            ),
            snapshot(&base, &action),
        ));
    }

    let weapon = arg(&args, "--weapon").map(|name| {
        let w = data
            .of_type(ItemType::Weapon)
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no WEAPON '{name}'"));
        let path = w
            .files
            .get("bare sword")
            .filter(|p| !p.is_empty())
            .or_else(|| w.files.get("mesh"))
            .and_then(|p| res.path(p))
            .expect("weapon mesh");
        let mesh = ogre_mesh::read(&path).expect("weapon mesh");
        let mat = appearance_material(&data, w);
        let tex = |k: &str| {
            mat.and_then(|m| m.files.get(k))
                .filter(|p| !p.is_empty())
                .and_then(|p| res.find(p))
        };
        (
            to_bevy(&mesh).into_iter().map(|(_, m)| m).collect(),
            tex("texture map"),
            tex("normal map"),
        )
    });

    let title = format!(
        "{}  -  clip '{}' {:.2} s at speed {:.2}",
        rec.name, clip.name, clip.length, t.speed
    );
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: root.to_string_lossy().into_owned(),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.78, 0.74, 0.66)))
        .insert_resource(Strip {
            clip: clip.name.clone(),
            character,
            poses,
            weapon,
            title,
            screenshot: arg(&args, "--screenshot").map(PathBuf::from),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (take_screenshot, animate_characters))
        .run()
}

#[allow(clippy::too_many_arguments)] // a Bevy startup system taking its resources
fn setup(
    mut commands: Commands,
    strip: Res<Strip>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let handles = upload_character(
        &strip.character,
        &assets,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut bindposes,
    );
    let prop = strip
        .character
        .rig
        .names
        .iter()
        .position(|n| n == "Bip01 Prop2");
    let weapon = strip.weapon.as_ref().map(|(ms, t, n)| {
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

    let n = strip.poses.len() as f32;
    // a person is ~18 units (1 unit ~ 10 cm) tall
    let spacing = 34.0;
    for (i, (_, clips)) in strip.poses.iter().enumerate() {
        let x = (i as f32 - (n - 1.0) / 2.0) * spacing;
        // facing the viewer's left, so the swing reads side-on
        let at = Transform::from_xyz(x, 0.0, 0.0)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));
        let (root, joints) = spawn_character_with_joints(
            &mut commands,
            &strip.character,
            &handles,
            at,
            &strip.clip,
            0.0,
            None,
        );
        commands.entity(root).insert(Blend(clips.clone()));
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
    }
    // ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(spacing * n + 600.0, 1200.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.55, 0.45),
            perceptual_roughness: 1.0,
            ..default()
        })),
    ));
    // frame the row: half the row width plus a margin fills half the horizontal field of view
    let fov = std::f32::consts::FRAC_PI_4;
    let aspect = 16.0 / 9.0;
    let half_w = spacing * n * 0.5 + 2.0;
    let dist = half_w / ((fov * 0.5).tan() * aspect);
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov, ..default() }),
        Transform::from_xyz(0.0, 10.0, dist).looking_at(Vec3::new(0.0, 9.0, 0.0), Vec3::Y),
    ));
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
    // labels: the title, and each pose's moment underneath it
    commands.spawn((
        Text::new(strip.title.clone()),
        TextFont {
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextColor(Color::srgb(0.12, 0.11, 0.1)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            left: Val::Px(20.0),
            ..default()
        },
    ));
    for (i, (label, _)) in strip.poses.iter().enumerate() {
        commands.spawn((
            Text::new(label.clone()),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(Color::srgb(0.12, 0.11, 0.1)),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(24.0),
                left: Val::Percent(
                    50.0 + (i as f32 - (n - 1.0) / 2.0) * spacing / half_w * 50.0 - 4.0,
                ),
                ..default()
            },
        ));
    }
}

fn take_screenshot(
    mut commands: Commands,
    strip: Res<Strip>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &strip.screenshot else {
        return;
    };
    *frames += 1;
    // shaders compile asynchronously on a cold start; give them time before capturing
    if *frames == 240 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
    }
    if *frames == 270 {
        exit.write(AppExit::Success);
    }
}
