//! terrain <Kenshi install dir> [--at fx,fz] [--size units] [--res n] [--screenshot out.png]
//!
//! Renders a square patch of Kenshi's terrain from the install's heightmap: centred at fraction
//! (fx, fz) of the world (default the middle), `size` units across (default the whole world), with
//! `n` vertices per side (default 512). Coloured from the painted world colour tiles
//! (`overlaymaps/colour.X.Y.png`); `--slope` colours by slope and height instead.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use kenshi_render::{terrain_mesh, terrain_tiles};
use kenshi_world::{Terrain, WORLD_SIZE, colour_tile};
use std::path::PathBuf;

#[derive(Resource)]
struct Scene {
    /// (colour tile, mesh); tile `None` = slope-coloured
    meshes: Vec<(Option<(u32, u32)>, Mesh)>,
    size: f32,
    screenshot: Option<PathBuf>,
}

fn arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(root) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!(
            "usage: terrain <Kenshi install dir> [--at fx,fz] [--size units] [--res n] [--screenshot out.png]"
        );
        return AppExit::error();
    };
    let mut terrain = Terrain::open(&PathBuf::from(root)).expect("heightmap");
    let (fx, fz) = arg(&args, "--at").map_or((0.5, 0.5), |s| {
        let v: Vec<f32> = s
            .split(',')
            .map(|x| x.parse().expect("--at fx,fz"))
            .collect();
        (v[0], v[1])
    });
    let size: f32 = arg(&args, "--size").map_or(WORLD_SIZE, |s| s.parse().expect("--size units"));
    let res: u32 = arg(&args, "--res").map_or(512, |s| s.parse().expect("--res n"));

    // sample window in heightmap pixels
    let n = terrain.samples();
    let span_px = ((size / terrain.spacing()) as u32).clamp(1, n - 1);
    let step = (span_px / (res - 1)).max(1);
    let verts = (span_px / step + 1).min(res);
    let span_used = (verts - 1) * step;
    let cx = ((fx * (n - 1) as f32) as u32).clamp(span_used / 2, n - 1 - span_used / 2);
    let cz = ((fz * (n - 1) as f32) as u32).clamp(span_used / 2, n - 1 - span_used / 2);
    let patch = terrain
        .patch(cx - span_used / 2, cz - span_used / 2, verts, verts, step)
        .expect("heightmap window");
    let (lo, hi) = patch
        .heights
        .iter()
        .fold((f32::MAX, f32::MIN), |a, &h| (a.0.min(h), a.1.max(h)));
    println!(
        "patch at sample ({cx}, {cz}), {verts}x{verts} vertices every {} units ({:.0} units across), heights {lo:.0}..{hi:.0}",
        patch.spacing,
        patch.spacing * (verts - 1) as f32
    );

    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: root.clone(),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.72, 0.68, 0.6)))
        .insert_resource(Scene {
            meshes: if args.iter().any(|a| a == "--slope") {
                vec![(None, terrain_mesh(&patch, None))]
            } else {
                terrain_tiles(&patch, None)
                    .into_iter()
                    .map(|(t, m)| (Some(t), m))
                    .collect()
            },
            size: patch.spacing * (verts - 1) as f32,
            screenshot: arg(&args, "--screenshot").map(PathBuf::from),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, take_screenshot)
        .run()
}

fn setup(
    mut commands: Commands,
    mut scene: ResMut<Scene>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (tile, mesh) in std::mem::take(&mut scene.meshes) {
        let material = StandardMaterial {
            base_color_texture: tile.map(|(x, y)| assets.load(colour_tile(x, y))),
            perceptual_roughness: 0.95,
            ..default()
        };
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
        ));
    }
    let s = scene.size;
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: s * 4.0,
            ..default()
        }),
        Transform::from_xyz(0.0, s * 0.45, s * 0.75)
            .looking_at(Vec3::new(0.0, 0.0, s * 0.05), Vec3::Y),
        DistanceFog {
            color: Color::srgb(0.72, 0.68, 0.6),
            falloff: FogFalloff::Linear {
                start: s * 0.9,
                end: s * 3.0,
            },
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(1.0, 0.8, 0.4).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 120.0,
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
    if *frames == 180 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
    }
    if *frames == 210 {
        exit.write(AppExit::Success);
    }
}
