//! viewer <Kenshi install dir> <record name or string_id> [options]
//!   --screenshot out.png   save one frame and exit
//!   --untextured           skip textures
//!   --anim <text>          play the first skeleton animation whose name contains <text>
//!   --time <seconds>       freeze the animation at this time (default: play)
//!   --head <name>          races: HEAD record (default the first `heads male`)
//!   --hair <name>          races: hair/beard attachment, repeatable (default the first hairstyle)
//!   --hair-colour r,g,b    tint for hair "Mod" textures (default dark brown)
//!   --focus <bone>         frame the camera on one bone (e.g. "Bip01 Head")
//!   --frame <fraction>     with --focus: framed radius as a fraction of the mesh size (1/12)
//!   --wear <name>          races: put on an ARMOUR record (clothing), repeatable
//!   --body <file.bod2>     races: a character's appearance file (CHARACTER_APPEARANCE): its head and
//!                          face morph weights (mesh poses); body sliders are not applied yet
//!
//! Shows the mesh of one game-data record from the player's install: items and weapons textured
//! through `appearance_material`, races as their skinned male body on the linked skeleton.

use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use kenshi_anim::Rig;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{dds, mods::Install, ogre_mesh, ogre_skeleton, resources::Resources};
use kenshi_render::{
    appearance_material, bounds, hair_image, split_head_tile, to_bevy, to_bevy_skinned,
};
use std::path::PathBuf;

#[derive(Resource)]
struct Target {
    meshes: Vec<(String, Mesh)>,
    texture: Option<String>,
    normal: Option<String>,
    center: Vec3,
    radius: f32,
    screenshot: Option<PathBuf>,
    rig: Option<Rig>,
    /// Races: (texture, normal map) of the HEAD record, drawn on the body mesh's head UV tile.
    head: Option<(Option<String>, Option<String>)>,
    /// Extra skinned meshes on the same skeleton (hair, beards, clothing) with their surface.
    extras: Vec<(Mesh, Surface)>,
}

/// How an extra mesh is drawn.
#[derive(Clone)]
enum Surface {
    /// Hair/beard cards: a CPU-tinted image, alpha-masked and double-sided.
    Hair(Option<Image>),
    /// Clothing/armour: texture and normal map files from its material spec.
    Cloth(Option<String>, Option<String>),
}

/// The playing animation and the joint entity of each rig bone.
#[derive(Resource)]
struct Playing {
    rig: Rig,
    anim: ogre_skeleton::Animation,
    joints: Vec<Entity>,
    freeze_at: Option<f32>,
}

#[derive(Component)]
struct Spin;

fn arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!(
            "usage: viewer <Kenshi install dir> <record name or string_id> [--screenshot out.png] [--untextured] [--anim text] [--time s]"
        );
        return AppExit::error();
    }
    let root = PathBuf::from(&args[0]);
    let untextured = args.iter().any(|a| a == "--untextured");
    let screenshot = arg(&args, "--screenshot").map(PathBuf::from);

    let mut order = Install::new(&root).load_order().expect("load order");
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).expect("game data");
    let res = Resources::index(&root);
    let has_mesh = |r: &kenshi_data::GameData| {
        r.files.contains_key("mesh") || r.files.contains_key("male mesh")
    };
    let rec = data
        .get(&args[1])
        .or_else(|| {
            data.records
                .values()
                .find(|r| r.name == args[1] && has_mesh(r))
        })
        .unwrap_or_else(|| panic!("no record '{}' with a mesh", args[1]));

    // races: the male body with its own skin textures; everything else: `mesh` + its material
    let race = rec.item_type() == Some(ItemType::Race);
    let own = |key: &str| {
        rec.files
            .get(key)
            .filter(|p| !p.is_empty())
            .and_then(|p| res.find(p))
    };
    let material = appearance_material(&data, rec);
    let file = |key: &str| {
        material
            .and_then(|m| m.files.get(key))
            .filter(|p| !p.is_empty())
            .and_then(|p| res.find(p))
    };
    let (mesh_path, texture, normal) = if race {
        (own("male mesh"), own("body texture male"), own("nm male"))
    } else {
        (own("mesh"), file("texture map"), file("normal map"))
    };
    let mesh_path = mesh_path.expect("record has no mesh");
    // races: a character appearance file (--body): head string id and morph weights by pose name
    let body = arg(&args, "--body").map(|b| {
        let f = kenshi_io::fcs::read(std::path::Path::new(b)).expect("body file");
        f.records.into_iter().next().expect("body record")
    });
    let body_head = body.as_ref().and_then(|b| {
        b.strings
            .iter()
            .find(|(k, _)| k == "head")
            .map(|(_, v)| v.clone())
    });
    // races: a HEAD record (--head <name>, default the first `heads male` reference)
    let head = race
        .then(|| {
            let want = arg(&args, "--head");
            let heads: Vec<_> = rec
                .refs("heads male")
                .iter()
                .filter_map(|(sid, _)| data.get(sid))
                .collect();
            let h = heads
                .iter()
                .find(|h| {
                    want.is_some_and(|w| h.name == w)
                        || body_head.as_deref() == Some(h.string_id.as_str())
                })
                .or(heads.first())?;
            println!("head '{}'", h.name);
            let f = |k: &str| {
                h.files
                    .get(k)
                    .filter(|p| !p.is_empty())
                    .and_then(|p| res.find(p))
            };
            Some((
                f("texture map").filter(|_| !untextured),
                f("normal map").filter(|_| !untextured),
            ))
        })
        .flatten();
    let mut mesh = ogre_mesh::read(&root.join(&mesh_path)).expect("mesh");
    if let Some(b) = &body {
        for (name, w) in &b.floats {
            if mesh.apply_pose(name, *w) {
                println!("morph {name} = {w}");
            }
        }
    }

    let skeleton = mesh.skeleton.as_ref().and_then(|name| {
        let path = res.path(name)?;
        Some(ogre_skeleton::read(&path).expect("skeleton"))
    });
    let rig = skeleton.as_ref().map(Rig::new);
    let meshes = match &rig {
        Some(rig) => to_bevy_skinned(&mesh, Some(&|h| rig.index_of_handle(h).map(|i| i as u16))),
        None => to_bevy(&mesh),
    };

    // races: hair/beard ATTACHMENTs (--hair <name>, repeatable; default the first hairstyle with a
    // mesh). They are modelled in the body's space and skinned to the same skeleton; bones the body
    // lacks (extra hair bones) follow the head.
    let mut extras = Vec::new();
    let hair_colour: [f32; 3] = arg(&args, "--hair-colour").map_or([0.24, 0.16, 0.1], |s| {
        let v: Vec<f32> = s
            .split(',')
            .map(|x| x.parse().expect("--hair-colour r,g,b"))
            .collect();
        [v[0], v[1], v[2]]
    });
    if let (true, Some(rig)) = (race, &rig) {
        let wanted: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "--hair")
            .map(|w| w[1].as_str())
            .collect();
        let hairs: Vec<_> = rec
            .refs("hairs")
            .iter()
            .filter_map(|(sid, _)| data.get(sid))
            .filter(|h| h.files.get("mesh").is_some_and(|m| !m.is_empty()))
            .collect();
        let chosen: Vec<_> = if wanted.is_empty() {
            hairs
                .iter()
                .find(|h| !h.name.starts_with("Beard"))
                .into_iter()
                .collect()
        } else {
            hairs
                .iter()
                .filter(|h| wanted.contains(&h.name.as_str()))
                .collect()
        };
        let head = rig
            .names
            .iter()
            .position(|n| n == "Bip01 Head")
            .unwrap_or(0) as u16;
        for h in chosen {
            let Some(path) = h.files.get("mesh").and_then(|p| res.path(p)) else {
                continue;
            };
            let Ok(hm) = ogre_mesh::read(&path) else {
                continue;
            };
            let tex = h
                .files
                .get("texture map")
                .and_then(|p| res.path(p))
                .filter(|_| !untextured);
            println!("hair '{}' texture {tex:?}", h.name);
            // the "Mod" map is decoded and tinted with the hair colour on the CPU
            let image = tex
                .and_then(|p| dds::read(&p).ok())
                .map(|t| hair_image(&t, hair_colour));
            let joint = |b: u16| Some(rig.index_of_handle(b).map_or(head, |i| i as u16));
            for (_, m) in to_bevy_skinned(&hm, Some(&joint)) {
                extras.push((m, Surface::Hair(image.clone())));
            }
        }

        // clothing/armour (--wear <name>, repeatable): ARMOUR records, `mesh` modelled in the male
        // body's space on the same skeleton, textured from their `material` reference
        let root_joint = 0u16;
        for name in args
            .windows(2)
            .filter(|w| w[0] == "--wear")
            .map(|w| w[1].as_str())
        {
            let Some(item) = data
                .records
                .values()
                .find(|r| r.name == name && r.item_type() == Some(ItemType::Armour))
            else {
                println!("no armour '{name}'");
                continue;
            };
            let Some(path) = item.files.get("mesh").and_then(|p| res.path(p)) else {
                println!("armour '{name}' has no mesh");
                continue;
            };
            let Ok(cm) = ogre_mesh::read(&path) else {
                continue;
            };
            let mat = appearance_material(&data, item);
            let f = |k: &str| {
                mat.and_then(|m| m.files.get(k))
                    .and_then(|p| res.find(p))
                    .filter(|_| !untextured)
            };
            let (tex, nm) = (f("texture map"), f("normal map"));
            println!(
                "wear '{}' mesh {:?} texture {tex:?}",
                item.name,
                path.file_name()
            );
            let joint = |b: u16| Some(rig.index_of_handle(b).map_or(root_joint, |i| i as u16));
            for (_, m) in to_bevy_skinned(&cm, Some(&joint)) {
                extras.push((m, Surface::Cloth(tex.clone(), nm.clone())));
            }
        }
    }

    let (lo, hi) = bounds(&mesh).expect("empty mesh");
    let (lo, hi) = (Vec3::from(lo), Vec3::from(hi));
    println!(
        "{} '{}': {mesh_path}, {} submeshes, skeleton {:?} ({} animations), texture {texture:?}",
        rec.string_id,
        rec.name,
        mesh.submeshes.len(),
        mesh.skeleton,
        skeleton.as_ref().map_or(0, |s| s.animations.len())
    );

    // --focus <bone>: frame a small sphere around that bone's bind position instead of the mesh
    let focus = arg(&args, "--focus").and_then(|bone| {
        let rig = rig.as_ref()?;
        let i = rig.names.iter().position(|n| n == bone)?;
        Some(rig.model_space(&rig.bind)[i].transform_point3(Vec3::ZERO))
    });
    let (center, radius) = match focus {
        Some(p) => {
            let frac: f32 =
                arg(&args, "--frame").map_or(1.0 / 12.0, |f| f.parse().expect("--frame fraction"));
            (p, ((hi - lo).length() * frac).max(0.01))
        }
        None => ((lo + hi) / 2.0, ((hi - lo).length() / 2.0).max(0.01)),
    };
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(AssetPlugin {
        file_path: root.to_string_lossy().into_owned(),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.55, 0.5, 0.42)))
    .insert_resource(Target {
        meshes,
        texture: texture.filter(|_| !untextured),
        normal: normal.filter(|_| !untextured),
        center,
        radius,
        screenshot,
        rig,
        head,
        extras,
    })
    .add_systems(Startup, setup)
    .add_systems(Update, (spin, animate, take_screenshot));

    if let Some(s) = skeleton {
        let want = arg(&args, "--anim").unwrap_or("idle").to_lowercase();
        let anim = s
            .animations
            .iter()
            .find(|a| a.name.to_lowercase().contains(&want))
            .or(s.animations.first())
            .cloned();
        if let Some(anim) = anim {
            println!("animation '{}' ({:.2}s)", anim.name, anim.length);
            app.insert_resource(Playing {
                rig: Rig::new(&s),
                anim,
                joints: Vec::new(),
                freeze_at: arg(&args, "--time").map(|t| t.parse().expect("--time seconds")),
            });
        }
    }
    app.run()
}

#[allow(clippy::too_many_arguments)] // a Bevy startup system taking its resources
fn setup(
    mut commands: Commands,
    mut target: ResMut<Target>,
    playing: Option<ResMut<Playing>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let material = materials.add(StandardMaterial {
        base_color_texture: target.texture.clone().map(|p| assets.load(p)),
        normal_map_texture: target.normal.clone().map(|p| assets.load(p)),
        perceptual_roughness: 0.8,
        ..default()
    });
    let pivot = commands
        .spawn((Transform::default(), Visibility::default(), Spin))
        .id();
    // model space, centred in the view; joints and meshes live under it
    let model = commands
        .spawn((
            Transform::from_translation(-target.center),
            Visibility::default(),
        ))
        .id();
    commands.entity(pivot).add_child(model);

    // joints: one entity per bone in its bind pose, parented like the skeleton
    let mut joints = Vec::new();
    let mut skin = None;
    if let Some(rig) = &target.rig {
        for b in &rig.bind {
            let t = Transform {
                translation: b.translation,
                rotation: b.rotation,
                scale: b.scale,
            };
            joints.push(commands.spawn((t, Visibility::default())).id());
        }
        for (i, parent) in rig.parents.iter().enumerate() {
            let p = parent.map_or(model, |p| joints[p]);
            commands.entity(p).add_child(joints[i]);
        }
        skin = Some(SkinnedMesh {
            inverse_bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(rig.inverse_bind())),
            joints: joints.clone(),
        });
    }
    if let Some(mut p) = playing {
        p.joints = joints;
    }

    let head_material = target.head.clone().map(|(tex, nm)| {
        materials.add(StandardMaterial {
            base_color_texture: tex.map(|p| assets.load(p)),
            normal_map_texture: nm.map(|p| assets.load(p)),
            perceptual_roughness: 0.8,
            ..default()
        })
    });
    // with a head, each body mesh splits into its body and head-tile parts
    let mut parts = Vec::new();
    for (_, m) in std::mem::take(&mut target.meshes) {
        match (&head_material, split_head_tile(&m)) {
            (Some(hm), Some((body, head))) => {
                parts.push((body, material.clone()));
                parts.push((head, hm.clone()));
            }
            _ => parts.push((m, material.clone())),
        }
    }
    for (m, surface) in std::mem::take(&mut target.extras) {
        let mat = match surface {
            Surface::Hair(img) => StandardMaterial {
                base_color_texture: img.map(|img| images.add(img)),
                perceptual_roughness: 0.9,
                alpha_mode: AlphaMode::Mask(0.4),
                // hair cards are thin double-sided geometry
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            Surface::Cloth(tex, nm) => StandardMaterial {
                base_color_texture: tex.map(|p| assets.load(p)),
                normal_map_texture: nm.map(|p| assets.load(p)),
                perceptual_roughness: 0.85,
                ..default()
            },
        };
        parts.push((m, materials.add(mat)));
    }
    for (m, mat) in parts {
        let mut e = commands.spawn((
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(mat),
            Transform::default(),
        ));
        if let Some(s) = &skin {
            e.insert(s.clone());
        }
        let id = e.id();
        commands.entity(model).add_child(id);
    }
    let r = target.radius;
    commands.spawn((
        Camera3d::default(),
        // the bounding sphere fits a 45 degree vertical field of view at distance r / sin(22.5)
        Transform::from_translation(Vec3::new(1.0, 0.7, 1.0).normalize() * r * 2.7)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 2.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    });
}

/// Pose the joints from the sampled animation every frame.
fn animate(time: Res<Time>, playing: Option<Res<Playing>>, mut q: Query<&mut Transform>) {
    let Some(p) = playing else { return };
    let t = p.freeze_at.unwrap_or(time.elapsed_secs());
    for (local, &e) in p.rig.sample(&p.anim, t).iter().zip(&p.joints) {
        if let Ok(mut tr) = q.get_mut(e) {
            *tr = Transform {
                translation: local.translation,
                rotation: local.rotation,
                scale: local.scale,
            };
        }
    }
}

fn spin(time: Res<Time>, target: Res<Target>, mut q: Query<&mut Transform, With<Spin>>) {
    if target.screenshot.is_some() {
        return; // keep the shot deterministic
    }
    for mut t in &mut q {
        t.rotate_y(time.delta_secs() * 0.5);
    }
}

/// Wait for textures to stream in, save one frame, then exit.
fn take_screenshot(
    mut commands: Commands,
    target: Res<Target>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &target.screenshot else {
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
