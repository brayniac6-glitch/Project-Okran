//! town <Kenshi install dir> <zone X.Y> [--screenshot out.png] [--margin units]
//!      [--view near|far|street] [--people N] [--sim town.csv --at seconds [--focus id]]
//!
//! `--sim` places the people of a `kenshi-game --example town` recording where the simulation had
//! them at `--at` seconds: walking when moving, sitting when operating a chair (task 87), idle
//! otherwise, facing the way they last moved; `--focus` aims the street camera at one of them.
//!
//! Renders one 4608-unit world zone (`render::zone`): its terrain and buildings, optionally with
//! `N` outfitted townsfolk (`render::character`) standing on open ground. Townsfolk placement and
//! poses are a display aid, not Kenshi's population logic (not ported yet).

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use kenshi_data::GameDataContainer;
use kenshi_io::{mods::Install, resources::Resources};
use kenshi_render::character::{
    CharacterAsset, CharacterSpec, animate_characters, load_character, spawn_character,
    upload_character,
};
use kenshi_render::zone::{ZoneScene, load_zone};
use kenshi_world::{Terrain, ZONE_SIZE};
use std::path::PathBuf;

#[derive(Resource)]
struct Scene {
    zone: ZoneScene,
    far_view: bool,
    screenshot: Option<PathBuf>,
    /// Character variants and the people to spawn: (variant, where, animation, clock offset).
    characters: Vec<CharacterAsset>,
    people: Vec<(usize, Transform, &'static str, f32)>,
    /// `--view street`: (look-at point, camera position) in scene coordinates.
    street: Option<(Vec3, Vec3)>,
    freeze: bool,
}

/// Outfits for the townsfolk (race, hair, worn items): a deterministic mix.
const VARIANTS: [(&str, &[&str], &[&str]); 3] = [
    (
        "Greenlander",
        &["Haircut09-merc", "Beard02"],
        &["Dustcoat", "Drifter's Leather Pants", "Drifter's Boots"],
    ),
    (
        "Greenlander",
        &["Haircut02-flat"],
        &["Leather Shirt", "Cargopants", "Drifter's Boots"],
    ),
    (
        "Greenlander",
        &["Haircut10-oldman", "Beard05"],
        &["Rag Shirt", "Dyed Trousers"],
    ),
];
const ANIMATIONS: [&str; 5] = [
    "idle_stand_relax",
    "idle_stand_confident",
    "combatwalk",
    "idle_turnhead",
    "idle_stand_guard",
];

/// Small deterministic generator for placing people.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
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
            "usage: town <Kenshi install dir> <zone X.Y> [--screenshot out.png] [--margin units] [--view near|far|street] [--people N]"
        );
        return AppExit::error();
    }
    let root = PathBuf::from(&args[0]);
    let zone_xy: Vec<i32> = args[1]
        .split('.')
        .map(|x| x.parse().expect("zone X.Y"))
        .collect();

    let mut order = Install::new(&root).load_order().expect("load order");
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).expect("game data");
    let res = Resources::index(&root);
    let mut terrain = Terrain::open(&root).expect("heightmap");
    let margin: f32 = arg(&args, "--margin").map_or(1500.0, |m| m.parse().expect("--margin units"));
    let zone = load_zone(
        &root,
        &data,
        &res,
        &mut terrain,
        (zone_xy[0], zone_xy[1]),
        margin,
    )
    .expect("zone");
    println!(
        "zone {}: {} building pieces from {} meshes; ground {:.0}",
        args[1],
        zone.pieces.len(),
        zone.meshes.len(),
        zone.origin.y
    );

    // townsfolk: --people N stand on open ground near the town centre
    let people_n: usize = arg(&args, "--people").map_or(0, |n| n.parse().expect("--people N"));
    let sim = arg(&args, "--sim").map(PathBuf::from);
    let mut characters = Vec::new();
    let mut people = Vec::new();
    let mut street = None;
    if people_n > 0 || sim.is_some() {
        for (race, hairs, wear) in VARIANTS {
            let mut spec = CharacterSpec::new(race);
            spec.hairs = hairs.iter().map(|s| s.to_string()).collect();
            spec.wear = wear.iter().map(|s| s.to_string()).collect();
            match load_character(&root, &data, &res, &spec) {
                Ok(c) => characters.push(c),
                Err(e) => println!("character variant skipped: {e}"),
            }
        }
        let mut rng = Lcg(0x6b656e736869);
        if let Some(path) = &sim {
            let at: f32 = arg(&args, "--at").map_or(60.0, |t| t.parse().expect("--at seconds"));
            // per id: the row at or before `at` and the one before it (for the heading)
            let mut rows: std::collections::BTreeMap<u32, ([f32; 6], [f32; 6])> =
                Default::default();
            for line in std::fs::read_to_string(path).expect("sim csv").lines() {
                let f: Vec<&str> = line.split(',').collect();
                if f.len() < 8 {
                    continue;
                }
                let t: f32 = f[0].parse().unwrap_or(f32::MAX);
                if t > at {
                    continue;
                }
                let id: u32 = f[1].parse().unwrap_or(0);
                let r = [
                    t,
                    f[2].parse().unwrap_or(0.0),
                    f[3].parse().unwrap_or(0.0),
                    f[4].parse().unwrap_or(0.0),
                    f[5].parse().unwrap_or(0.0),
                    f[6].parse().unwrap_or(0.0),
                ];
                let e = rows.entry(id).or_insert((r, r));
                *e = (e.1, r);
            }
            let focus: Option<u32> = arg(&args, "--focus").map(|f| f.parse().expect("--focus id"));
            for (&id, (prev, now)) in &rows {
                let pos = Vec3::new(now[1], now[2], now[3]) - zone.origin;
                let (dx, dz) = (now[1] - prev[1], now[3] - prev[3]);
                let yaw = if dx * dx + dz * dz > 1e-4 {
                    dx.atan2(dz)
                } else {
                    rng.next() * std::f32::consts::TAU
                };
                let anim = if now[5] as u32 == 87 {
                    "sitting chair"
                } else if now[4] > 1.0 {
                    "combatwalk"
                } else {
                    ANIMATIONS[id as usize % 2]
                };
                if focus == Some(id) {
                    let eye = (0..16)
                        .map(|k| k as f32 / 16.0 * std::f32::consts::TAU)
                        .map(|a| pos + Vec3::new(420.0 * a.cos(), 260.0, 420.0 * a.sin()))
                        .find(|e| zone.is_clear(e.x, e.z, 20.0))
                        .unwrap_or(pos + Vec3::new(0.0, 120.0, 300.0));
                    street = Some((pos, eye));
                }
                people.push((
                    id as usize % characters.len().max(1),
                    Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
                    anim,
                    rng.next() * 5.0,
                ));
            }
            println!("{} people from the simulation at t = {at}", people.len());
        }
        let mut tries = 0;
        while people.len() < people_n && tries < people_n * 200 && !characters.is_empty() {
            tries += 1;
            let r = 150.0 + rng.next() * 900.0;
            let a = rng.next() * std::f32::consts::TAU;
            let (dx, dz) = (r * a.cos(), r * a.sin());
            if !zone.is_clear(dx, dz, 40.0) {
                continue;
            }
            let pos = Vec3::new(dx, zone.ground(dx, dz), dz);
            let facing = Quat::from_rotation_y(rng.next() * std::f32::consts::TAU);
            let k = people.len();
            people.push((
                k % characters.len(),
                Transform::from_translation(pos).with_rotation(facing),
                ANIMATIONS[k % ANIMATIONS.len()],
                rng.next() * 5.0,
            ));
        }
        // street camera: a clear spot ~3 m from the first person, eyes ~4.5 m up
        if street.is_none()
            && let Some(first) = people.first()
        {
            let p = first.1.translation;
            let eye = (0..16)
                .map(|k| k as f32 / 16.0 * std::f32::consts::TAU)
                .map(|a| p + Vec3::new(300.0 * a.cos(), 45.0, 300.0 * a.sin()))
                .find(|e| zone.is_clear(e.x, e.z, 20.0));
            street = eye.map(|e| (p, e));
        }
        println!("{} people from {} outfits", people.len(), characters.len());
    }

    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: root.to_string_lossy().into_owned(),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.72, 0.68, 0.6)))
        .insert_resource(Scene {
            zone,
            far_view: arg(&args, "--view") == Some("far"),
            screenshot: arg(&args, "--screenshot").map(PathBuf::from),
            characters,
            people,
            street: street.filter(|_| arg(&args, "--view") == Some("street")),
            freeze: arg(&args, "--screenshot").is_some(),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (take_screenshot, animate_characters))
        .run()
}

#[allow(clippy::too_many_arguments)] // a Bevy startup system taking its resources
fn setup(
    mut commands: Commands,
    mut scene: ResMut<Scene>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    scene
        .zone
        .spawn(&mut commands, &assets, &mut meshes, &mut materials);
    let uploads: Vec<_> = scene
        .characters
        .iter()
        .map(|c| {
            upload_character(
                c,
                &assets,
                &mut meshes,
                &mut materials,
                &mut images,
                &mut bindposes,
            )
        })
        .collect();
    for (k, &(variant, at, anim, offset)) in scene.people.iter().enumerate() {
        let freeze = scene.freeze.then_some(0.4 + k as f32 * 0.37);
        spawn_character(
            &mut commands,
            &scene.characters[variant],
            &uploads[variant],
            at,
            anim,
            offset,
            freeze,
        );
    }

    let s = ZONE_SIZE;
    let (eye, target) = match scene.street {
        Some((p, eye)) => (eye, p + Vec3::new(0.0, 12.0, 0.0)),
        None if scene.far_view => (Vec3::new(0.0, s * 0.3, s * 0.45), Vec3::ZERO),
        None => (Vec3::new(s * 0.12, s * 0.12, s * 0.22), Vec3::ZERO),
    };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: s * 8.0,
            ..default()
        }),
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
        DistanceFog {
            color: Color::srgb(0.72, 0.68, 0.6),
            falloff: FogFalloff::Linear {
                start: s * 0.8,
                end: s * 3.0,
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
        brightness: 250.0,
        ..default()
    });
}

fn take_screenshot(
    mut commands: Commands,
    scene: Res<Scene>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &scene.screenshot else {
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
