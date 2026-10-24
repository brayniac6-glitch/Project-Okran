//! kenshi <Kenshi install dir> [X.Y (zone)] [--player N] [squads...]
//!
//! The playable shell: one zone (default 34.46, Mourn) simulated by `kenshi_game::sim` and drawn
//! with `kenshi_render`, with the player's squad (`--player`, default 3) under the mouse.
//!
//! Controls: WASD / arrows pan, Q / E or middle-drag turn, wheel zooms; left click or drag
//! selects your characters (shift adds); right click moves them (in formation), or on someone
//! attacks them;
//! Space pauses, 1 / 2 / 3 run the game at 1x / 2x / 4x; F focuses the camera on the selection;
//! R switches running / walking for orders.
//!
//! Testing: `--shot out.png` saves a screenshot after `--shot-at` frames (default 600) and exits
//! (`--shots N`: N consecutive frames, out_000.png ...);
//! `--cam dist,pitch,yaw` and `--pan dx,dz` place the camera;
//! `--auto-attack` orders the squad onto the nearest resident at the start;
//! `KENSHI_WATER_REFLECTION=n` overrides the "water reflection" option, `KENSHI_REFLECTION_SHOT=out.png`
//! saves the reflection's target (frame 300), `KENSHI_WATER_MODE=5` shows its mask on the water.
//!
//! Harness (not Kenshi's): the outfits are a fixed mix, everyone carries a katana, the ground
//! under the mouse is the terrain (buildings are not picked). (testing) `TOWN_HOUR_SECONDS`
//! overrides the game hour's length.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use kenshi_anim::select::{idle as idle_anim, movement};
#[path = "birds.rs"]
mod birds;
#[path = "build_ui.rs"]
mod build_ui;
#[path = "building_ui.rs"]
mod building_ui;
#[path = "camera.rs"]
mod camera;
use camera::{PITCH_START, YAW_START, ZOOM_START, camera};
#[path = "tooltip_ui.rs"]
mod tooltip_ui;
#[path = "context_ui.rs"]
mod context_ui;
#[path = "dialogue_ui.rs"]
mod dialogue_ui;
#[path = "editor.rs"]
mod editor;
#[path = "floaters.rs"]
mod floaters;
#[path = "hud_ui.rs"]
mod hud_ui;
#[path = "icons.rs"]
mod icons;
#[path = "inventory_ui.rs"]
mod inventory_ui;
#[path = "menu.rs"]
mod menu;
#[path = "nametags.rs"]
mod nametags;
#[path = "ground_items.rs"]
mod ground_items;
#[path = "move_marker.rs"]
mod move_marker;
#[path = "options_ui.rs"]
mod options_ui;
#[path = "bindings.rs"]
mod bindings;
#[path = "looks.rs"]
mod looks;
#[path = "blood.rs"]
mod blood;
#[path = "slave_ui.rs"]
mod slave_ui;
#[path = "pickbar.rs"]
mod pickbar;
#[path = "ranged_view.rs"]
mod ranged_view;
#[path = "sound.rs"]
mod sound;
#[path = "speech_ui.rs"]
mod speech_ui;
#[path = "portraits.rs"]
mod portraits;
#[path = "research_ui.rs"]
mod research_ui;
#[path = "prospect_ui.rs"]
mod prospect_ui;
#[path = "tutorial_table.rs"]
mod tutorial_table;
#[path = "tutorial_ui.rs"]
mod tutorial_ui;
#[path = "item_tip.rs"]
mod item_tip;
mod span_prof;
#[path = "order_test.rs"]
mod order_test;
#[path = "stats_ui.rs"]
mod stats_ui;
#[path = "saves_ui.rs"]
mod saves_ui;
#[path = "popup_ui.rs"]
mod popup_ui;
#[path = "message_box.rs"]
mod message_box;
#[path = "appearance.rs"]
mod appearance;

use bevy::math::Affine3A;
use kenshi_physics::ragdoll::Ragdoll;
use kenshi_render::ragdoll::{PoseOverride, RagdollRig};
use kenshi_combat::arena::{opponent, root_at};
use kenshi_data::ItemType;
use kenshi_game::sim::{GET_UP_BLEND, Sim};
use kenshi_io::{ogre_mesh, resources::Resources};

/// The window as Kenshi's kenshi.cfg sets the Direct3D11 renderer up: "Video Mode" (width x
/// height) and "Full Screen" (borderless full screen here). Without the file 1600 x 900 windowed.
/// (testing) Screenshots (`--shot`) and KENSHI_WINDOWED keep a window, at the video mode's size.
fn video_window(root: &std::path::Path, testing: bool) -> Window {
    // Ogre's kenshi.cfg, the rewrite's copy (the options' resolution and screen mode) over it
    let cfg = kenshi_io::cfg::text(root, "kenshi.cfg");
    let get = |k: &str| {
        cfg.lines()
            .find_map(|l| l.split_once('=').filter(|(n, _)| n.trim() == k).map(|(_, v)| v.trim().to_string()))
    };
    let (w, h) = get("Video Mode")
        .and_then(|v| {
            let mut it = v.split(|c: char| !c.is_ascii_digit()).filter(|t| !t.is_empty());
            Some((it.next()?.parse::<u32>().ok()?, it.next()?.parse::<u32>().ok()?))
        })
        .unwrap_or((1600, 900));
    // full screen or borderless: (stand-in) both a borderless window over the screen
    let full = (get("Full Screen").is_some_and(|v| v.eq_ignore_ascii_case("yes")) || get("Border").is_some_and(|v| v.eq_ignore_ascii_case("none")))
        && !testing
        && std::env::var("KENSHI_WINDOWED").is_err();
    Window {
        title: "Kenshi rewrite".into(),
        resolution: bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0),
        // kenshi.cfg "VSync" (Yes / No)
        present_mode: if get("VSync").is_some_and(|v| v.eq_ignore_ascii_case("yes")) {
            bevy::window::PresentMode::AutoVsync
        } else {
            bevy::window::PresentMode::AutoNoVsync
        },
        mode: if full {
            bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Primary)
        } else {
            bevy::window::WindowMode::Windowed
        },
        ..default()
    }
}

/// The main camera (not the water reflection's).
type MainCam = (With<Camera3d>, Without<kenshi_render::reflection::ReflectionCamera>, Without<kenshi_render::reflection::PortraitCamera>, Without<kenshi_render::interior_mask::MaskCamera>);
use kenshi_render::character::{
    Animated, Blend, CharacterAsset, CharacterHandles, CharacterSpec, animate_characters,
    load_character, spawn_character_with_joints, strip_tracks, upload_character_with,
};
use kenshi_render::object_material::KenshiObjectMaterial;
use kenshi_render::character_material::KenshiCharacterMaterial;
use kenshi_render::moons::{KenshiMoonMaterial, Planet, place_planets, relight_moons, spawn_moons};
use kenshi_render::sky::SkySettings;
use kenshi_render::water_material::{KenshiWaterMaterial, WATER_HEIGHT, ZONE_WATER_SIZE, DISTANT_WATER_SIZE};
use kenshi_render::sky_dome::{KenshiSkyMaterial, KenshiSkyPlugin, spawn_sky_dome};
use kenshi_render::terrain_material::{KenshiTerrainMaterial, KenshiTerrainPlugin, TerrainLook};
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use kenshi_render::zone::{BuildingFloor, BuildingPiece, BuildingView, ZoneScene, ZoneTag, ground_on, load_zone, load_zone_shared};
use kenshi_render::{appearance_material, to_bevy};
use kenshi_world::{Terrain, WORLD_SIZE, ZONE_SIZE};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// A zone loaded by a loader thread.
type Loaded = ((i32, i32), Result<ZoneScene, String>);

/// Outfits: the player's, then a mix for everyone else (race, hair, worn items).
const OUTFITS: [(&str, &[&str], &[&str]); 4] = [
    (
        "Greenlander",
        &["Haircut02-flat"],
        &["Leather Shirt", "Cargopants", "Drifter's Boots"],
    ),
    (
        "Greenlander",
        &["Haircut09-merc", "Beard02"],
        &["Dustcoat", "Drifter's Leather Pants", "Drifter's Boots"],
    ),
    (
        "Greenlander",
        &["Haircut10-oldman", "Beard05"],
        &["Rag Shirt", "Dyed Trousers"],
    ),
    (
        "Scorchlander",
        &["Haircut09-merc"],
        &["Rag Shirt", "Dyed Trousers", "Drifter's Boots"],
    ),
];

/// Game speed steps (1 / 2 / 3).
const SPEEDS: [f32; 3] = [1.0, 2.0, 4.0];
/// Sim ticks per second.
const TICK: f32 = 1.0 / 60.0;
/// Pixels within which a click picks a character.
const PICK_PX: f32 = 28.0;

/// An animal kind's animation library and its skeleton's clip lengths.
type AnimalAnims = (kenshi_anim::select::Library, HashMap<String, f32>);

#[derive(Resource)]
struct Game {
    /// Animal races' bodies loaded (race name -> outfit index).
    race_outfits: HashMap<String, usize>,
    /// Each animal kind's ANIMAL_ANIMATION library and its skeleton's clip lengths (by its
    /// ANIMAL_CHARACTER).
    animal_anims: HashMap<String, Arc<AnimalAnims>>,
    sim: Sim,
    /// What the zones' foliage reads.
    foliage: Option<Arc<kenshi_render::zone::FoliageWorld>>,
    zone: ZoneScene,
    /// The zones streamed in round the camera (the first zone is `zone`), and the ones asked
    /// for; zone loads come back over `loaded`.
    zones: HashMap<(i32, i32), ZoneScene>,
    pub(crate) asked: std::collections::HashSet<(i32, i32)>,
    /// The first zone's (X, Y).
    home: (i32, i32),
    loaded: Mutex<Receiver<Loaded>>,
    loader: Sender<Loaded>,
    root: PathBuf,
    res: Arc<Resources>,
    outfits: Vec<CharacterAsset>,
    /// The loaded zones' building collision, for ragdolls.
    statics: std::sync::Arc<kenshi_physics::world::StaticWorld>,
    /// Severed limbs lying as physics bodies (F040471 / F043917), and each one's turn (ground
    /// items are drawn turned about the vertical only; these as their bodies lie).
    limbs_lying: Vec<LyingLimb>,
    pub(crate) limb_turns: HashMap<u64, Quat>,
    /// Each "physics file"'s body (None: none it can make).
    limb_bodies: HashMap<String, Option<Arc<kenshi_physics::ragdoll::Template>>>,
    /// Per outfit: its race's ragdoll ("male ragdoll" / "female ragdoll", F039259) bound to its
    /// skeleton: knocked out, and carried (F039247's 0x800 request takes the same file; the
    /// race's "male carry ragdoll" is never read by the game).
    ragdoll_rigs: Vec<Option<Arc<RagdollRig>>>,
    weapon: Option<(Vec<Mesh>, Option<String>, Option<String>)>,
    /// Per person: the root entity and its outfit.
    bodies: Vec<Entity>,
    body_outfit: Vec<usize>,
    /// Per person: the body's size (the character editor's Height).
    body_scale: Vec<f32>,
    /// People to give a new look.
    rebody: Vec<(usize, CharacterSpec)>,
    /// People still to be given their own look (`looks`), and the bodies made so far: by look,
    /// and the first of each race and sex (whose skeleton the others share).
    dress_queue: std::collections::VecDeque<usize>,
    looks_made: HashMap<String, usize>,
    look_base: HashMap<(String, bool), usize>,
    /// What each person wore when last dressed (their equipped clothing's records).
    worn: HashMap<usize, Vec<String>>,
    /// The looks given (race, hair, clothes), by person.
    looks: HashMap<usize, CharacterSpec>,
    /// The character editor's Body and Face values (raw, every range) by person, with its face
    /// poses' weights by pose name (`appearance`): the editor's, a body file's, else rolled.
    body_values: HashMap<usize, HashMap<String, f32>>,
    /// DA_CHARACTER_EDITOR: the character the editor is to open on (`editor::open_surgery`).
    pub surgery: Option<usize>,
    /// (testing) the person KENSHI_KO knocked out.
    ko_test: Option<usize>,
    /// The outfits' uploaded handles and the sword's, for bodies spawned later.
    uploads: Vec<CharacterHandles<KenshiCharacterMaterial>>,
    sword: Option<(Vec<Handle<Mesh>>, Handle<StandardMaterial>)>,
    selected: Vec<usize>,
    /// The building picked (building_ui): the main panel shows it instead of the selected
    /// characters until a character is picked or nothing is.
    selected_building: Option<building_ui::Selected>,
    /// A crafting bench asked for in the overview's crafting tab (its menu's "Crafting").
    crafting_request: Option<usize>,
    /// A question for Kenshi's message box (message_box), asked next frame.
    ask: Option<message_box::Ask>,
    /// The message log (the overview window's DIALOGUE tab): what was said (speech, the
    /// dialogue window's lines) and the game's messages, oldest first.
    log: Vec<String>,
    /// The selection to dismantle a building (asked first, message_box::ask_dismantle).
    dismantle_request: Option<(Vec<usize>, usize)>,
    paused: bool,
    speed: f32,
    carry: f32,
    /// Building effects of zones just spawned (record, world position, orientation), for the
    /// effects to start.
    /// Effects to attach: EFFECT, world position, orientation and the building it is on (its
    /// zone and placement), whose view it follows.
    new_effects: Vec<(String, Vec3, Quat, Option<((i32, i32), usize)>)>,
    /// The camera (`camera`): its centre (scene units: on the ground, or 10 over the character
    /// it follows), turn (from -z), tilt (down positive) and distance behind the centre; turned
    /// with the mouse now (the rotation marker, move_marker::rotation).
    focus: Vec3,
    mouse_turning: bool,
    yaw: f32,
    pitch: f32,
    dist: f32,
    /// A drag selection's start (pixels).
    drag: Option<Vec2>,
    hud: Option<Entity>,
    marquee: Option<Entity>,
    /// Per person: the clip it plays when not posed by a blend.
    playing: Vec<String>,
    /// Per person: what drew its body last frame (walking, fighting, an activity...), the
    /// clips it drew, and the clips being faded out after a change of kind with their weight
    /// left (place_people's crossfade).
    anim_kind: Vec<u8>,
    last_blend: Vec<Vec<(usize, f32, f32)>>,
    fading_blend: Vec<(Vec<(usize, f32, f32)>, f32)>,
    /// The character editor's stage (F039470): the person being made, drawn standing there
    /// (scene coordinates) instead of where they are.
    stage: Option<(usize, Vec3)>,
    /// Per person, out of combat: the base animation layers (movement / idles), as the
    /// animation class keeps them (kenshi_combat::arena::set_base).
    walk_layers: Vec<[kenshi_anim::layer::AnimationLayer<()>; 3]>,
    /// Per person: what its one-shot task clips watch for (place_people).
    transitions: HashMap<usize, Transition>,
    shot: Option<(PathBuf, u32, u32)>,
    auto_attack: bool,
    /// (testing) Someone the camera keeps in view with the selection.
    watch: Option<usize>,
    /// The testing camera's offset from the squad.
    pan: Vec3,
}

fn arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

/// Tell the player Kenshi is needed (a message box, as the program may have no console).
fn need_kenshi(why: &str) {
    let text = format!(
        "{why}\n\nProject Okran needs your own copy of Kenshi (Steam or GOG) installed. It ships none of the game's files and plays from your install.\n\nInstall Kenshi, or start Project Okran with the Kenshi folder: kenshi.exe \"<Kenshi folder>\""
    );
    eprintln!("{text}");
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (t, c) = (wide(&text), wide("Project Okran: Kenshi needed"));
        // SAFETY: both strings are NUL-terminated UTF-16 that live through the call; 0x10 is MB_ICONERROR
        unsafe {
            MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x10);
        }
    }
}

/// A line onto the message log (`Game::log`), its oldest dropped past 500.
fn log(log: &mut Vec<String>, line: String) {
    log.push(line);
    if log.len() > 500 {
        let n = log.len() - 500;
        log.drain(..n);
    }
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // the install: the first argument, else found where Steam (or this folder) has it
    let Some(root) = args.first().map(PathBuf::from).or_else(menu::find_install) else {
        eprintln!("usage: kenshi <Kenshi install dir> [X.Y (zone)] [--player N] [squad:<template>@1 ...]");
        need_kenshi("No Kenshi install was found.");
        return AppExit::error();
    };
    // Project Okran plays only from a copy of Kenshi the player owns
    if let Err(e) = kenshi_io::mods::check_install(&root) {
        need_kenshi(&e);
        return AppExit::error();
    }
    // the heightmap into the system's file cache while the game loads (kenshi_world::warm_heightmap)
    kenshi_world::warm_heightmap(&root);
    // with no zone or squads asked for, Kenshi's main menu starts the game
    let menu = args.len() <= 1;
    let zone_xy: (i32, i32) = args
        .get(1)
        .filter(|s| !s.starts_with("--") && s.contains('.') && !s.contains(':'))
        .map_or((34, 46), |s| {
            let v: Vec<i32> = s.split('.').map(|x| x.parse().expect("zone X.Y")).collect();
            (v[0], v[1])
        });
    let players: usize = arg(&args, "--player").map_or(3, |n| n.parse().expect("--player N"));
    let mut extras = vec![format!("player:{players}")];
    extras.extend(
        args.iter()
            .filter(|a| a.starts_with("squad:") || a.starts_with("demo:") || a.starts_with("spawn:") || a.starts_with("kit:"))
            .cloned(),
    );
    let mut app = App::new();
    app
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin {
                    // Kenshi's terrain detail sampler: anisotropic, wrapping
                    default_sampler: bevy::image::ImageSamplerDescriptor {
                        address_mode_u: bevy::image::ImageAddressMode::Repeat,
                        address_mode_v: bevy::image::ImageAddressMode::Repeat,
                        address_mode_w: bevy::image::ImageAddressMode::Repeat,
                        mag_filter: bevy::image::ImageFilterMode::Linear,
                        min_filter: bevy::image::ImageFilterMode::Linear,
                        mipmap_filter: bevy::image::ImageFilterMode::Linear,
                        anisotropy_clamp: 16,
                        ..default()
                    },
                })
                .set(AssetPlugin {
                    file_path: root.to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(video_window(&root, args.iter().any(|a| a == "--shot"))),
                    // (the close is ours: `close_now`)
                    close_when_requested: false,
                    ..default()
                })
                // (testing) KENSHI_SPAN_PROF: Bevy's spans timed (a `--features bevy/trace` build)
                .set(bevy::log::LogPlugin { custom_layer: span_prof::layer, ..default() }),
        )
        .insert_resource(ClearColor(Color::srgb(0.72, 0.68, 0.6)))
        // the texture quality option (kenshi_render::texture_quality; 1 when missing)
        .insert_resource(kenshi_render::texture_quality::TextureGimping(
            kenshi_io::cfg::load(&root, "settings.cfg").get("texture resolution gimping").and_then(|v| v.trim().parse().ok()).unwrap_or(1),
        ))
        .add_plugins(KenshiTerrainPlugin)
        .add_plugins(KenshiSkyPlugin)
        .init_resource::<OpenBuildings>()
        .insert_resource(camera::CameraRig::new(&root))
        .init_resource::<move_marker::MoveMarker>()
        .init_resource::<move_marker::RotationMarker>()
        .init_resource::<ranged_view::RangedMeshes>()
        .init_resource::<options_ui::Menus>()
        // the options as the game holds them (F032944), and the fonts sized by "font size2"
        .insert_resource({
            let st = options_ui::Settings::load(&root);
            kenshi_gui::res::set_font_size2(st.f32("font size2"));
            options_ui::set_censorship(st.on("censorship"));
            st
        })
        .init_resource::<blood::Blood>()
        .init_resource::<slave_ui::SlaveTrade>()
        .init_resource::<pickbar::PickBars>()
        .insert_resource(bindings::Bindings::load(&root))
        .init_resource::<bindings::UiPresses>()
        .add_systems(PreUpdate, bindings::turn_presses)
        .init_resource::<kenshi_render::zone::TurretAims>()
        .init_resource::<kenshi_render::zone::PartMotion>()
        .insert_resource(sound::Sound::new(&root))
        .init_resource::<kenshi_render::zone::FoliageCentre>()
        .add_plugins(kenshi_gui::GuiPlugin)
        .init_resource::<inventory_ui::InventoryWindow>()
        .init_resource::<stats_ui::StatsWindow>()
        .init_resource::<dialogue_ui::DialogueWindow>()
        .init_resource::<context_ui::ContextMenu>()
        .init_resource::<build_ui::BuildMode>()
        .init_resource::<research_ui::ResearchWindow>()
        .init_resource::<prospect_ui::ProspectWindow>()
        .add_systems(Update, prospect_ui::update.run_if(resource_exists::<WorldReady>))
        .init_resource::<tutorial_ui::Tutorials>()
        .add_systems(Update, order_test::script.run_if(resource_exists::<WorldReady>))
        .add_systems(Update, building_ui::script.run_if(resource_exists::<WorldReady>))
        .add_systems(Update, tutorial_ui::update.run_if(resource_exists::<WorldReady>.and_then(resource_exists::<hud_ui::Hud>)))
        .add_systems(Update, tutorial_ui::pointer.after(tutorial_ui::update).after(hud_ui::jobs_panel).run_if(resource_exists::<WorldReady>.and_then(resource_exists::<hud_ui::Hud>)))
        .add_systems(Update, (research_ui::script, research_ui::update).chain().run_if(resource_exists::<WorldReady>))
        .add_systems(
            Update,
            (build_ui::script.before(build_ui::toggle), build_ui::toggle, build_ui::update.after(build_ui::toggle), build_ui::draw,
                build_ui::draw_nests, build_ui::labels)
                .distributive_run_if(resource_exists::<WorldReady>),
        )
        .init_resource::<portraits::Portraits>()
        .init_resource::<icons::ItemIcons>()
        .add_systems(Update, icons::draw.run_if(resource_exists::<WorldReady>))
        .add_systems(Update, (portraits::layer_characters, portraits::update).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, (context_ui::script, context_ui::law_script, context_ui::update).chain().run_if(resource_exists::<WorldReady>))
        .add_systems(
            Update,
            (dialogue_ui::script, dialogue_ui::update).chain().before(inventory_ui::toggle).distributive_run_if(resource_exists::<WorldReady>),
        )
        .add_systems(Update, (key_script, save_keys).chain().run_if(resource_exists::<WorldReady>))
        .add_systems(Update, light_visibility.after(hide_roofs).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, (door_show.before(move_doors), move_doors, move_player_doors, sound::doors, sound::machines, sound::door_locks, sound::dismantled, sound::ground_pickups, cut_player_buildings, player_masks).run_if(resource_exists::<WorldReady>))
        .init_resource::<kenshi_render::local_lights::LightClock>()
        .add_systems(Update, kenshi_render::local_lights::animate.run_if(resource_exists::<WorldReady>))
        .init_resource::<saves_ui::SavesPanel>()
        .init_resource::<popup_ui::Popup>()
        .init_resource::<message_box::MessageBoxes>()
        .add_systems(Update, message_box::update)
        .add_systems(Update, tooltip_ui::update.after(inventory_ui::draw))
        .init_resource::<Autosave>()
        .add_message::<saves_ui::SavesDone>()
        .add_systems(Update, saves_ui::update.after(options_ui::update))
        .add_systems(Update, (saves_done.after(saves_ui::update), autosave).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, floaters::update.run_if(resource_exists::<WorldReady>))
        .add_systems(Update, hide_away.run_if(resource_exists::<WorldReady>))
        .init_resource::<nametags::NameTags>()
        .add_systems(Update, nametags::update.run_if(resource_exists::<WorldReady>))
        .add_systems(Startup, gizmo_setup)
        .add_systems(Update, ray_test.run_if(resource_exists::<WorldReady>))
        .add_systems(PostUpdate, kenshi_render::body_shape::place_sized_bones.after(bevy::transform::TransformSystems::Propagate))
        .add_systems(Update, nav_debug.run_if(resource_exists::<WorldReady>))
        .init_resource::<speech_ui::Speech>()
        .init_resource::<ground_items::GroundItems>()
        .add_systems(Update, (ground_items::update, ground_items::follow_bodies.after(ragdolls), ground_items::highlight).chain().run_if(resource_exists::<WorldReady>))
        .init_resource::<birds::Birds>()
        .add_systems(Update, birds::update.after(camera).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, move_marker::update.after(command).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, menu::ui_script.before(saves_ui::update).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, move_marker::rotation.after(camera).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, context_ui::rclick_script.before(command).run_if(resource_exists::<WorldReady>.and_then(resource_exists::<Game>)))
        // (over the title screen too: its OPTIONS)
        .add_systems(Update, options_ui::update.run_if(resource_exists::<WorldReady>.or_else(resource_exists::<menu::Menu>)))
        .add_systems(Update, options_ui::sync_game.run_if(resource_exists::<Game>))
        .add_systems(PreUpdate, options_ui::swap_mouse.after(bevy::input::InputSystems).before(bevy::ui::UiSystems::Focus))
        .add_systems(Update, (slave_ui::test_hook, slave_ui::update).chain().run_if(resource_exists::<WorldReady>))
        .add_systems(Update, pickbar::update.run_if(resource_exists::<WorldReady>))
        .add_systems(Update, (ranged_view::bolts, ranged_view::bows, ranged_view::test_hook, ranged_view::turret_test, ranged_view::nest_test, ranged_view::weapon_row, ranged_view::wound_test, ranged_view::ko_me_test, ranged_view::arm_test, ranged_view::turret_aims, part_motion, kenshi_render::zone::pose_swivels.after(ranged_view::turret_aims).after(part_motion)).run_if(resource_exists::<WorldReady>))
        .add_systems(Update, (sound::listener, sound::menu_music, sound::interface, sound::play, sound::music).chain())
        .add_systems(
            Update,
            (
                sound::regions,
                sound::weather,
                blood::paint.before(sound::fights),
                sound::fights,
                sound::items,
                sound::indoors,
                sound::emitters,
                sound::work,
                sound::creatures,
            )
                .before(sound::play)
                .run_if(resource_exists::<WorldReady>),
        )
        .add_systems(Update, speech_ui::update.run_if(resource_exists::<WorldReady>))
        .add_systems(
            Update,
            (stats_ui::script.before(stats_ui::toggle), stats_ui::toggle, stats_ui::update.after(stats_ui::toggle))
                .distributive_run_if(resource_exists::<WorldReady>),
        )
        .add_systems(
            Update,
            (
                hud_ui::open.run_if(
                    resource_exists::<WorldReady>
                        .and_then(not(resource_exists::<hud_ui::Hud>))
                        .and_then(not(resource_exists::<editor::Editor>))
                        .and_then(not(resource_exists::<editor::Pending>)),
                ),
                hud_ui::update.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                hud_ui::cycle_squads.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                hud_ui::banner.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                hud_ui::extending_panel.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                popup_ui::update.run_if(resource_exists::<WorldReady>),
                hud_ui::squad_panel.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                hud_ui::jobs_panel.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                hud_ui::hotkeys.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                hud_ui::portrait_clicks.run_if(resource_exists::<hud_ui::Hud>.and_then(resource_exists::<WorldReady>)),
                debug_text.run_if(resource_exists::<WorldReady>),
            ),
        )
        .add_systems(PostUpdate, no_ui_shots)
        .add_systems(
            Update,
            (
                inventory_ui::script.before(inventory_ui::toggle),
                inventory_ui::trade_script.before(inventory_ui::toggle),
                inventory_ui::scroll.before(inventory_ui::draw),
                inventory_ui::trade_buy_script.after(inventory_ui::toggle).before(inventory_ui::draw),
                inventory_ui::toggle,
                inventory_ui::limbs_window.after(inventory_ui::toggle),
                inventory_ui::bag_window.after(inventory_ui::toggle),
                inventory_ui::arrange_button.after(inventory_ui::toggle).before(inventory_ui::draw),
                inventory_ui::machine_window.after(inventory_ui::toggle),
                inventory_ui::mark_open.after(inventory_ui::toggle),
                inventory_ui::drag.after(inventory_ui::toggle),
                inventory_ui::draw.after(inventory_ui::drag),
            )
                .distributive_run_if(resource_exists::<WorldReady>),
        )
        .insert_resource(kenshi_gui::GuiRes(Arc::new(kenshi_gui::Gui::load(&root))))
        .add_systems(PreUpdate, setup.run_if(resource_added::<Game>))
        .add_systems(
            Update,
            kenshi_render::reflection::follow_main_camera.after(camera).run_if(resource_exists::<WorldReady>),
        )
        .add_systems(
            Update,
            (
                (controls, camera, select, command, run_jobs, run_sim, place_people).chain(),
                ragdolls.after(place_people),
                body_scales.after(place_people).before(animate_characters),
                postures.after(place_people).before(animate_characters),
                rebody,
                dress,
                animate_characters.after(ragdolls),
                roll_looks.before(body_shapes),
                face_morphs.after(roll_looks),
                body_shapes.after(animate_characters),
                (
                    hud,
                    testing,
                    hide_roofs,
                    inside_hook.before(hide_roofs),
                    people_ranges.after(hide_roofs),
                    stream_zones,
                    distant_terrain,
                    lod_terrain,
                    kenshi_render::zone::foliage_ranges,
                    water_clock,
                    weather_tick.before(relight),
                    stars,
                    planets,
                    new_bodies,
                    relight,
                    sun_follow,
                    tick_particles.after(weather_tick),
                    fog_volumes.after(tick_particles),
                    fog_features,
                    footsteps.after(animate_characters).before(tick_particles),
                ),
            )
                .distributive_run_if(resource_exists::<WorldReady>),
        );
    // the character editor: at a new game's start (the menu's) and the plastic surgeon's
    app.add_systems(
        Update,
        (
            editor::open_surgery.run_if(not(resource_exists::<editor::Editor>).and_then(resource_exists::<WorldReady>)),
            editor::update.run_if(resource_exists::<editor::Editor>),
            editor::type_script,
            hud_ui::hide_under_editor,
        ),
    );
    // the menu: the main menu's, or over a game its new game panel and the loading screen of a
    // game loaded from the game (`unload_world` first)
    app.add_systems(First, unload_world.run_if(resource_exists::<Unload>))
        .add_systems(
            Update,
            editor::open.run_if(resource_exists::<editor::Pending>.and_then(resource_exists::<WorldReady>)),
        )
        .add_systems(
            Update,
            (menu::buttons, menu::advanced.after(options_ui::update), menu::arrive).distributive_run_if(resource_exists::<menu::Menu>),
        );
    if menu {
        app.insert_resource(menu::Menu::new(root.clone()))
            .add_systems(Startup, menu::open)
            .add_systems(Update, menu::script.before(menu::buttons));
    } else {
        let load = build_world(root.clone(), args.clone(), zone_xy, extras, Loading::New);
        load.install(app.world_mut());
    }
    // (testing) KENSHI_PROFILE: frame times and the GPU's time per render pass, logged each second
    if std::env::var("KENSHI_PROFILE").is_ok() {
        app.add_plugins((
            bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
            bevy::diagnostic::LogDiagnosticsPlugin::default(),
        ));
    }
    app.add_systems(First, close_now);
    app.add_systems(Last, (quit_now, vram_debug));
    // (testing) KENSHI_FIXED_DT=<seconds>: every frame moves the clocks on by that much, however
    // long it takes to draw (captures at an even game pace)
    if let Some(dt) = std::env::var("KENSHI_FIXED_DT").ok().and_then(|v| v.parse::<f32>().ok()) {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(dt)));
    }
    let exit = app.run();
    // loads still running when the app closes finish first: the process ending under them
    // crashed it
    join_workers();
    exit
}

/// (testing) KENSHI_KEYS="F5@100,F9@200,...": those keys pressed at those frames (F1..F12,
/// letters); "-E@300" lets go of one held since.
fn key_script(mut frames: Local<u32>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_KEYS") else { return };
    for step in v.split(',') {
        let Some((k, at)) = step.rsplit_once('@') else { continue };
        if at.parse::<u32>().ok() != Some(*frames) {
            continue;
        }
        let (k, up) = match k.strip_prefix('-') {
            Some(k) => (k, true),
            None => (k, false),
        };
        let Some(bindings::Combo { button: bindings::Button::Key(code), .. }) = bindings::parse(k) else { continue };
        if up {
            keys.release(code);
        } else {
            keys.press(code);
        }
    }
}

/// A game to unload (next frame, `unload_world`), and what to load after it.
#[derive(Resource)]
pub struct Unload(pub menu::After);

/// Unload the game in play, in the program (as Kenshi does before a load or a new game): every
/// drawn thing (each entity with a place or a UI node) despawned, the world's resources
/// dropped, the windows' states and the sounds made afresh; then the menu (`menu::Menu`)
/// loads what follows behind its loading screen.
fn unload_world(world: &mut World) {
    let Some(Unload(after)) = world.remove_resource::<Unload>() else { return };
    let Some(root) = world.get_resource::<Game>().map(|g| g.root.clone()) else { return };
    eprintln!("unloading the game");
    // the sounds playing in the world
    let voices = world.resource_mut::<sound::Sound>().leave_world();
    for e in voices {
        world.try_despawn(e).ok();
    }
    // everything drawn: the world's meshes, lights and cameras, and the windows
    let mut q = world.query_filtered::<Entity, (Or<(With<Transform>, With<Node>)>, Without<Window>)>();
    let drawn: Vec<Entity> = q.iter(world).collect();
    for e in drawn {
        world.try_despawn(e).ok();
    }
    // the world and what was made for it
    world.remove_resource::<WorldReady>();
    world.remove_resource::<Game>();
    world.remove_resource::<TerrainLook>();
    world.remove_resource::<Sky>();
    world.remove_resource::<WeatherState>();
    world.remove_resource::<kenshi_render::particles::render::Particles>();
    world.remove_resource::<kenshi_render::particles::effects::Effects>();
    world.remove_resource::<kenshi_render::terrain_lod::LodTerrain>();
    world.remove_resource::<Distant>();
    world.remove_resource::<kenshi_render::interior_mask::InteriorMask>();
    world.remove_resource::<kenshi_render::reflection::WaterReflection>();
    world.remove_resource::<ranged_view::AttachPoints>();
    world.remove_resource::<hud_ui::Hud>();
    world.remove_resource::<editor::Editor>();
    world.remove_resource::<editor::Pending>();
    // the windows and the world's caches as at the start
    fn fresh<R: Resource + Default>(world: &mut World) {
        world.insert_resource(R::default());
    }
    fresh::<OpenBuildings>(world);
    if let Some(mut rig) = world.get_resource_mut::<camera::CameraRig>() {
        rig.reset();
    }
    fresh::<move_marker::MoveMarker>(world);
    fresh::<move_marker::RotationMarker>(world);
    fresh::<ranged_view::RangedMeshes>(world);
    fresh::<options_ui::Menus>(world);
    fresh::<blood::Blood>(world);
    fresh::<slave_ui::SlaveTrade>(world);
    fresh::<pickbar::PickBars>(world);
    fresh::<kenshi_render::zone::TurretAims>(world);
    fresh::<kenshi_render::zone::PartMotion>(world);
    fresh::<kenshi_render::zone::FoliageCentre>(world);
    fresh::<inventory_ui::InventoryWindow>(world);
    fresh::<stats_ui::StatsWindow>(world);
    fresh::<dialogue_ui::DialogueWindow>(world);
    fresh::<context_ui::ContextMenu>(world);
    fresh::<build_ui::BuildMode>(world);
    fresh::<research_ui::ResearchWindow>(world);
    fresh::<prospect_ui::ProspectWindow>(world);
    fresh::<tutorial_ui::Tutorials>(world);
    fresh::<portraits::Portraits>(world);
    fresh::<icons::ItemIcons>(world);
    fresh::<popup_ui::Popup>(world);
    fresh::<message_box::MessageBoxes>(world);
    fresh::<Autosave>(world);
    fresh::<nametags::NameTags>(world);
    fresh::<speech_ui::Speech>(world);
    fresh::<ground_items::GroundItems>(world);
    fresh::<birds::Birds>(world);
    // the save panels (the save the next game comes from is kept)
    let current = world.resource::<saves_ui::SavesPanel>().current.clone();
    world.insert_resource(saves_ui::SavesPanel::with_current(current));
    // the menu: the new game panel's (its start and options kept), else a loading screen's
    match world.get_resource_mut::<menu::Menu>() {
        Some(mut m) => m.unloaded(after),
        None => {
            world.insert_resource(menu::Menu::loading(root, after));
        }
    }
}

/// Load a save from inside a game: the game unloaded first (`unload_world`), the save loaded
/// behind the loading screen (as the main menu's Continue does).
fn load_from_game(commands: &mut Commands, saves: &mut saves_ui::SavesPanel, path: &std::path::Path, reset: bool, import: Option<kenshi_game::kenshi_save::ImportOptions>) {
    if import.is_none() {
        saves.current = Some(kenshi_game::save::slot_name(path));
    }
    commands.insert_resource(Unload(menu::After::Load(path.to_path_buf(), reset, import)));
}

/// KENSHI_IMPORT's flags (reset, buildings, research, dead NPCs, relations as 0 / 1) as import
/// options (testing: the menu's Continue imports KENSHI_LOAD with them).
pub fn import_of_flags(s: &str) -> kenshi_game::kenshi_save::ImportOptions {
    let b: Vec<bool> = s.chars().map(|c| c == '1').collect();
    let at = |k: usize, d: bool| b.get(k).copied().unwrap_or(d);
    kenshi_game::kenshi_save::ImportOptions { reset_positions: at(0, false), buildings: at(1, true), research: at(2, true), dead_npcs: at(3, true), relations: at(4, true) }
}

/// Save the game as `name` (F034700): its whole state and the same in Kenshi's layout; the save
/// Continue loads from now on (settings.cfg "continue").
pub(crate) fn save_game(game: &Game, name: &str) -> std::io::Result<PathBuf> {
    let looks = &game.looks;
    let save = game.sim.snapshot(game.home, |id| looks.get(&id).cloned());
    let data = &game.sim.data;
    let k = game.sim.kenshi_snapshot(|id| looks.get(&id).map(|s| looks::body_of_spec(data, s, game.body_values.get(&id))));
    let path = kenshi_game::save::write_slot(name, &save, &k)?;
    set_continue(&game.root, name);
    Ok(path)
}

/// settings.cfg "continue": the save Continue loads.
pub fn set_continue(root: &std::path::Path, name: &str) {
    let mut cfg = kenshi_io::cfg::load(root, "settings.cfg");
    cfg.set("continue", name);
    let _ = kenshi_io::cfg::save("settings.cfg", &cfg);
}

/// Save under `name`, the result told as Kenshi does (F034700: "Save successful", "Autosaved"
/// for an autosave, else a "Save Error").
fn save_and_tell(game: &mut Game, name: &str) {
    match save_game(game, name) {
        Ok(p) => {
            eprintln!("saved to {}", p.display());
            let note = if name.starts_with(kenshi_game::save::AUTOSAVE) { "Autosaved" } else { "Save successful" };
            game.sim.messages.push(note.into());
        }
        Err(e) => {
            eprintln!("save failed: {e}");
            // F034848: an error box
            game.ask = Some(message_box::Ask { title: "Error".into(), text: format!("Failed to save game\n{e}"), question: message_box::Question::Note });
        }
    }
}

/// F5 quicksaves, F9 quickloads (controls.cfg "quicksave" / "quickload").
fn save_keys(mut commands: Commands, act: bindings::Actions, mut game: ResMut<Game>, mut saves: ResMut<saves_ui::SavesPanel>) {
    if act.just("quicksave") {
        save_and_tell(&mut game, kenshi_game::save::QUICKSAVE);
    }
    if act.just("quickload") {
        match kenshi_game::save::slots().into_iter().find(|s| s.name == kenshi_game::save::QUICKSAVE) {
            Some(s) => load_from_game(&mut commands, &mut saves, &s.path, false, None),
            None => eprintln!("no quicksave to load"),
        }
    }
}

/// The save panels' requests in a game: save, or load (the game unloaded, then the save loaded).
fn saves_done(mut commands: Commands, mut done: MessageReader<saves_ui::SavesDone>, mut game: ResMut<Game>, mut saves: ResMut<saves_ui::SavesPanel>) {
    for d in done.read() {
        match d {
            saves_ui::SavesDone::Save(name) => save_and_tell(&mut game, name),
            saves_ui::SavesDone::Load(path, reset) => load_from_game(&mut commands, &mut saves, path, *reset, None),
            saves_ui::SavesDone::Import(path, opt) => load_from_game(&mut commands, &mut saves, path, false, Some(*opt)),
        }
    }
}

/// The autosave's clock (F034847): real seconds since the last, or the one about to be made.
#[derive(Resource, Default)]
struct Autosave {
    timer: f32,
    due: bool,
}

/// @fn F034847
///
/// With "Auto save" on, every "Auto save time (minutes)" of play: "Autosaving..." shown, then
/// next frame the game saved as "autosave" + the next of 0, 1, 2 ("autosaveindex").
/// (assumption) Its other conditions (two of the game's states, not decompiled) are not
/// checked.
fn autosave(time: Res<Time<Real>>, mut a: ResMut<Autosave>, mut game: ResMut<Game>, settings: Option<Res<options_ui::Settings>>) {
    if a.due {
        a.due = false;
        a.timer = 0.0;
        let mut cfg = kenshi_io::cfg::load(&game.root, "settings.cfg");
        let index = cfg.get("autosaveindex").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(-1);
        let (name, next) = kenshi_game::save::next_autosave(index);
        cfg.set("autosaveindex", &next.to_string());
        let _ = kenshi_io::cfg::save("settings.cfg", &cfg);
        save_and_tell(&mut game, &name);
        return;
    }
    a.timer += time.delta_secs();
    // the settings looked at once a second
    if a.timer.fract() > time.delta_secs() {
        return;
    }
    let st = settings.map(|s| s.clone()).unwrap_or_else(|| options_ui::Settings::load(&game.root));
    if !st.on("Auto save") {
        a.timer = 0.0;
        return;
    }
    let minutes = std::env::var("KENSHI_AUTOSAVE_SECONDS").ok().and_then(|v| v.parse::<f32>().ok()).map_or_else(|| st.f32("Auto save time (minutes)"), |s| s / 60.0);
    if a.timer > minutes * 60.0 {
        a.due = true;
        game.sim.messages.push("Autosaving...".into());
    }
}

/// The background loads (world, zones) running: joined before the process ends.
static WORKERS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(Vec::new());

/// Run `f` on a background thread the exit waits for.
pub(crate) fn spawn_worker(f: impl FnOnce() + Send + 'static) {
    let h = std::thread::spawn(f);
    if let Ok(mut w) = WORKERS.lock() {
        w.retain(|h| !h.is_finished());
        w.push(h);
    }
}

/// (testing) KENSHI_VRAM_DEBUG=<frame>: at that frame, the images and meshes held (what the GPU
/// is asked to keep): images by format and size, meshes by vertex bytes.
fn vram_debug(mut frames: Local<u32>, images: Res<Assets<Image>>, meshes: Res<Assets<Mesh>>, assets: Res<AssetServer>) {
    *frames += 1;
    if std::env::var("KENSHI_VRAM_DEBUG").ok().and_then(|v| v.parse::<u32>().ok()) != Some(*frames) {
        return;
    }
    let mut by: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let mut total = 0usize;
    let mut named: Vec<(usize, String)> = Vec::new();
    for (id, img) in images.iter() {
        let d = &img.texture_descriptor;
        let bytes = img.data.as_ref().map_or(0, |v| v.len());
        let est = if bytes > 0 { bytes } else { (d.size.width * d.size.height * 4) as usize };
        total += est;
        let e = by.entry(format!("{:?} {}x{} mips {}", d.format, d.size.width, d.size.height, d.mip_level_count)).or_default();
        e.0 += 1;
        e.1 += est;
        let path = assets.get_path(id).map(|p| p.to_string()).unwrap_or_else(|| "(made)".into());
        if d.format == bevy::render::render_resource::TextureFormat::Rgba8Unorm && d.size.width >= 1024 {
            eprintln!("vram:     uncompressed {}x{} {path}", d.size.width, d.size.height);
        }
        named.push((est, path));
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by(|a, b| b.1.1.cmp(&a.1.1));
    eprintln!("vram: {} images, ~{} MB", images.len(), total / (1 << 20));
    for (k, (n, b)) in v.iter().take(15) {
        eprintln!("vram:   {n:5} x {k}: {} MB", b / (1 << 20));
    }
    named.sort_by(|a, b| b.0.cmp(&a.0));
    let mut per_file: std::collections::HashMap<String, (usize, usize)> = Default::default();
    for (b, p) in named.iter().filter(|(_, p)| p != "(made)") {
        let base = p.split('#').next().unwrap_or(p).to_lowercase();
        let e = per_file.entry(base).or_default();
        e.0 += 1;
        e.1 += b;
    }
    let dup: usize = per_file.values().filter(|(n, _)| *n > 1).map(|(n, b)| b - b / n).sum();
    eprintln!("vram: {} files, loaded more than once: {} ({} MB extra)", per_file.len(), per_file.values().filter(|(n, _)| *n > 1).count(), dup / (1 << 20));
    let made: usize = named.iter().filter(|(_, p)| p == "(made)").map(|x| x.0).sum();
    eprintln!("vram: made (not from files) {} MB in {}", made / (1 << 20), named.iter().filter(|(_, p)| p == "(made)").count());
    let mut mesh_bytes = 0usize;
    let mut sizes: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut gpu_only = 0;
    for (_, m) in meshes.iter() {
        // (one kept only on the GPU cannot be counted here)
        if !m.asset_usage.contains(bevy::asset::RenderAssetUsages::MAIN_WORLD) {
            gpu_only += 1;
            continue;
        }
        let b = m.count_vertices() * m.get_vertex_size() as usize + m.indices().map_or(0, |i| i.len() * 4);
        mesh_bytes += b;
        *sizes.entry(m.count_vertices()).or_default() += 1;
    }
    let mut common: Vec<_> = sizes.into_iter().collect();
    common.sort_by(|a, b| b.1.cmp(&a.1));
    eprintln!("vram: meshes by vertex count (count x vertices): {:?}", common.iter().take(10).map(|(v, n)| format!("{n}x{v}")).collect::<Vec<_>>());
    eprintln!("vram: {} meshes, ~{} MB ({gpu_only} more kept only on the GPU)", meshes.len(), mesh_bytes / (1 << 20));
}

/// The app asked to close (the window closed, Quit, a test's end): the process ends here, once
/// the loads still running are done. Bevy's own teardown after its loop crashed (a segmentation
/// fault in the renderer's or the window's shutdown) and nothing of the game's runs after an
/// exit.
fn quit_now(mut exits: MessageReader<AppExit>) {
    let Some(e) = exits.read().last().cloned() else { return };
    join_workers();
    std::process::exit(match e {
        AppExit::Success => 0,
        AppExit::Error(n) => n.get() as i32,
    });
}

/// The window's close button: the process ends at once (the loads still running done first),
/// as `quit_now` does. Left to Bevy, the window went first and the renderer crashed on it (a
/// segmentation fault) before the exit was seen.
fn close_now(mut closes: MessageReader<bevy::window::WindowCloseRequested>) {
    if closes.read().next().is_some() {
        join_workers();
        std::process::exit(0);
    }
}

fn join_workers() {
    let hs: Vec<_> = WORKERS.lock().map(|mut w| w.drain(..).collect()).unwrap_or_default();
    for h in hs {
        let _ = h.join();
    }
}

/// The world is set up (`setup` has run): the game's systems may run.
#[derive(Resource)]
struct WorldReady;

/// Everything a world needs once loaded: the game and the resources its systems read.
struct WorldLoad {
    game: Game,
    look: TerrainLook,
    sky: Sky,
    weather: WeatherState,
    particles: kenshi_render::particles::render::Particles,
    effects: kenshi_render::particles::effects::Effects,
    lod: kenshi_render::terrain_lod::LodTerrain,
    distant: Distant,
}

impl WorldLoad {
    fn install(self, world: &mut World) {
        world.insert_resource(self.look);
        world.insert_resource(self.sky);
        world.insert_resource(self.weather);
        world.insert_resource(self.particles);
        world.insert_resource(self.effects);
        world.insert_resource(self.lod);
        world.insert_resource(self.distant);
        world.insert_resource(self.game);
    }
}

/// What a world is built from.
pub enum Loading {
    New,
    /// A save of ours, the squad gathered or not.
    Save(Box<kenshi_game::save::SaveGame>, bool),
    /// A Kenshi save imported.
    Kenshi(Box<kenshi_game::kenshi_save::KenshiSave>, kenshi_game::kenshi_save::ImportOptions),
}

/// Load a world: zone `zone_xy` with the squads `extras` (the player's among them); `args` are
/// the command line's testing options.
fn build_world(root: PathBuf, args: Vec<String>, zone_xy: (i32, i32), extras: Vec<String>, load: Loading) -> WorldLoad {
    let mut sim = Sim::load(&root, zone_xy.0, zone_xy.1, 0, &extras);
    // a loaded game: the save put on the world before anyone is drawn
    let restored = match &load {
        Loading::Save(s, reset) => {
            let r = sim.restore(s);
            if *reset {
                sim.gather_squad();
            }
            eprintln!("loaded: {} people, {} characters, c.{}", s.people.len(), s.players.len(), s.money);
            Some(r)
        }
        _ => None,
    };
    let imported = match &load {
        Loading::Kenshi(k, opt) => {
            let r = sim.apply_kenshi(k, opt);
            eprintln!("imported: {} characters, {} buildings, c.{}", r.len(), k.buildings.len(), k.money);
            Some(r)
        }
        _ => None,
    };
    let res = Arc::new(Resources::index(&root));
    let mut terrain = Terrain::open(&root).expect("heightmap");
    let mut zone = load_zone(&root, &sim.data, &res, &mut terrain, zone_xy, 1500.0).expect("zone");
    // the zones' foliage (settings.cfg "foliage range", "grass range", "grass density")
    let cfg = kenshi_io::cfg::text(&root, "settings.cfg");
    let setting = |k: &str, d: f32| {
        cfg.lines()
            .find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix('=')).and_then(|v| v.trim().parse::<f32>().ok()))
            .unwrap_or(d)
    };
    let settings = (setting("foliage range", 1.0), setting("grass range", 1.0), setting("grass density", 1.0));
    let foliage = (std::env::var("KENSHI_NO_FOLIAGE").is_err())
        .then(|| kenshi_render::zone::FoliageWorld::new(&root, &sim.data, &res, WATER_HEIGHT, settings))
        .flatten()
        .map(Arc::new);
    if let Some(f) = &foliage {
        zone.add_foliage(&root, f, zone_xy);
    }
    let mut outfits = vec![];
    for (race, hairs, wear) in OUTFITS {
        let mut spec = CharacterSpec::new(race);
        spec.hairs = hairs.iter().map(|s| s.to_string()).collect();
        spec.wear = wear.iter().map(|s| s.to_string()).collect();
        match load_character(&root, &sim.data, &res, &spec) {
            Ok(mut c) => {
                strip_tracks(&mut c, &sim.data);
                outfits.push(c)
            }
            Err(e) => eprintln!("outfit skipped: {e}"),
        }
    }
    assert!(!outfits.is_empty(), "no outfit loaded");
    let data = &sim.data;
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
            let mat = appearance_material(data, w);
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
    // start looking at the player's squad
    let first = sim.folk.iter().find(|p| p.player).map(|p| p.id);
    // `--pan dx,dz`: the camera that far from the squad (testing)
    let pan = arg(&args, "--pan").map_or(Vec3::ZERO, |p| {
        let d: Vec<f32> = p.split(',').map(|v| v.parse().expect("--pan dx,dz")).collect();
        Vec3::new(d[0], 0.0, d[1])
    });
    let focus = first.map_or(Vec3::ZERO, |i| {
        Vec3::from(sim.folk[i].mover.position_units()) - zone.origin
    }) + pan;
    let selected: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let n = sim.folk.len();
    let (loader, loaded) = channel();
    // the sky's main light (Kenshi's SkyXController and SkyX's scattering)
    let sky = SkySettings::from_data(&sim.data).with_install(&root);
    let mut look = TerrainLook::new(&root, &sim.data, sky.terrain_light(sim.hour() as f32))
        .expect("terrain blend info");
    look.resources = Some(res.clone());
    // `--cam dist,pitch,yaw` (testing)
    let cam: Vec<f32> = arg(&args, "--cam").map_or(vec![ZOOM_START, PITCH_START, YAW_START], |c| {
        c.split(',')
            .map(|v| v.parse().expect("--cam d,p,y"))
            .collect()
    });
    // the particle systems' templates and materials (data/particles), and the EFFECT records
    let library = Arc::new(kenshi_render::particles::library::ParticleLibrary::load(&root));
    let effects = kenshi_render::particles::effects::Effects::new(&sim.data, &library.templates, &root);

    let mut lod = kenshi_render::terrain_lod::LodTerrain::new(
            Terrain::open(&root).expect("terrain heightmap"),
        );
    // the terrain options (F045916): "terrain detail" (1), "terrain distant" (0), "terrain
    // threshold" (750)
    {
        let cfg = kenshi_io::cfg::load(&root, "settings.cfg");
        lod.detail = kenshi_render::terrain_lod::Detail::from_options(cfg.f32("terrain detail", 1.0), cfg.f32("terrain distant", 0.0), cfg.f32("terrain threshold", 750.0));
    }
    let distant = Distant {
            terrain: Terrain::open(&root).expect("heightmap"),
            cells: HashMap::new(),
        };
    let data_for_weather = sim.data.clone();
    let mut game = Game {
            race_outfits: HashMap::new(),
            animal_anims: HashMap::new(),
            foliage: foliage.clone(),
            new_effects: Vec::new(),
            sim,
            zone,
            zones: HashMap::new(),
            asked: [zone_xy].into_iter().collect(),
            home: zone_xy,
            loaded: Mutex::new(loaded),
            loader,
            root: root.clone(),
            res: res.clone(),
            statics: Default::default(),
            limbs_lying: Vec::new(),
            limb_turns: HashMap::new(),
            limb_bodies: HashMap::new(),
            ragdoll_rigs: outfits
                .iter()
                .map(|o| o.ragdoll.as_ref().map(|r| Arc::new(RagdollRig::new(r, &o.rig))))
                .collect(),
            outfits,
            weapon,
            bodies: vec![],
            body_outfit: vec![],
            body_scale: vec![],
            rebody: vec![],
            dress_queue: Default::default(),
            looks_made: HashMap::new(),
            look_base: HashMap::new(),
            worn: HashMap::new(),
            looks: HashMap::new(),
            body_values: HashMap::new(),
            surgery: None,
            ko_test: None,
            uploads: vec![],
            sword: None,
            selected,
            crafting_request: None,
            ask: None,
            log: Vec::new(),
            dismantle_request: None,
            selected_building: None,
            paused: false,
            speed: std::env::var("KENSHI_SPEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            carry: 0.0,
            focus,
            mouse_turning: false,
            yaw: cam[2],
            pitch: cam[1],
            dist: cam[0],
            drag: None,
            hud: None,
            marquee: None,
            playing: vec![String::new(); n],
            anim_kind: vec![0; n],
            last_blend: vec![Vec::new(); n],
            fading_blend: vec![(Vec::new(), 0.0); n],
            stage: None,
            walk_layers: Vec::new(),
            transitions: HashMap::new(),
            shot: arg(&args, "--shot").map(|p| {
                (
                    PathBuf::from(p),
                    arg(&args, "--shot-at").map_or(600, |f| f.parse().expect("--shot-at frames")),
                    arg(&args, "--shots").map_or(1, |f| f.parse().expect("--shots count")),
                )
            }),
            auto_attack: args.iter().any(|a| a == "--auto-attack"),
            watch: None,
            pan,
        };
    for (id, look) in restored.into_iter().flatten() {
        if let Some(look) = look {
            game.looks.insert(id, look.clone());
            game.sim.folk[id].female = Some(look.female);
            let race_id = game.sim.data.of_type(kenshi_data::ItemType::Race).find(|r| r.name == look.race).map(|r| r.string_id.clone());
                game.sim.folk[id].look_race = race_id;
            game.rebody.push((id, look));
        }
    }
    // an imported save's characters dressed as they were
    for (id, body) in imported.into_iter().flatten() {
        let data = game.sim.data.clone();
        let p = &game.sim.folk[id];
        let race = body.race.as_deref().and_then(|r| data.get(r)).or_else(|| data.of_type(kenshi_data::ItemType::Race).find(|r| r.name == "Greenlander"));
        let Some(race) = race else { continue };
        let mut spec = looks::spec_of_body(&data, race, body.female, &body, p.shaved);
        spec.wear = looks::worn(p);
        game.body_values.insert(id, body.values.iter().cloned().collect());
        game.looks.insert(id, spec.clone());
        game.rebody.push((id, spec));
    }
    WorldLoad {
        game,
        look,
        sky: Sky(sky),
        weather: WeatherState::new(&data_for_weather),
        particles: kenshi_render::particles::render::Particles::new(library, Some(res.clone())),
        effects,
        lod,
        distant,
    }
}

/// The Kenshi materials and the sky, for `setup`.
type Looks<'a> = (
    ResMut<'a, Assets<KenshiTerrainMaterial>>,
    ResMut<'a, Assets<KenshiObjectMaterial>>,
    ResMut<'a, TerrainLook>,
    ResMut<'a, Assets<KenshiSkyMaterial>>,
    Res<'a, Sky>,
    ResMut<'a, Assets<KenshiWaterMaterial>>,
    ResMut<'a, Assets<KenshiCharacterMaterial>>,
    ResMut<'a, Assets<KenshiMoonMaterial>>,
);

#[allow(clippy::too_many_arguments)] // a Bevy startup system taking its resources
fn setup(
    mut commands: Commands,
    mut game: ResMut<Game>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    (mut terrain_materials, mut object_materials, mut look, mut sky_materials, sky, mut waters, mut characters, mut moons): Looks,
    (windows, mut mask_materials): (Query<&Window>, ResMut<Assets<kenshi_render::interior_mask::InteriorMaskMaterial>>),
) {
    commands.insert_resource(WorldReady);
    let game = &mut *game;
    // the interior mask (kenshi_render::interior_mask), before the materials that read it
    if std::env::var("KENSHI_NO_INTERIOR_MASK").is_err() {
        let size = windows.iter().next().map_or(UVec2::new(1280, 720), |w| UVec2::new(w.physical_width(), w.physical_height()));
        let mask = kenshi_render::interior_mask::spawn_interior_mask(&mut commands, &mut images, &mut mask_materials, size, game.zone.origin);
        look.interior_mask = Some(mask.image.clone());
        commands.insert_resource(mask);
    }
    if !old_terrain() {
        game.zone.terrain.clear();
    }
    game.zone.spawn_kenshi(
        &mut commands,
        &assets,
        &mut meshes,
        &mut object_materials,
        &mut terrain_materials,
        &mut look,
    );
    rebuild_statics(game);
    // the home zone's lamps, torches and fires
    let (lights, inside, tag) = (game.zone.lights.clone(), game.zone.lights_inside.clone(), game.zone.zone);
    spawn_zone_lights(&mut commands, &game.sim.data, &lights, &inside, tag);
    // the home zone's building effects
    let o = game.zone.origin;
    let z = game.zone.zone;
    let fx: Vec<_> = game.zone.effects.iter().zip(&game.zone.effects_of).map(|((e, at, r), &b)| (e.clone(), *at + o, *r, Some((z, b)))).collect();
    game.new_effects.extend(fx);
    // the characters with Kenshi's character shaders
    let origin = game.zone.origin;
    let uploads: Vec<CharacterHandles<KenshiCharacterMaterial>> = game
        .outfits
        .iter()
        .map(|c| {
            upload_character_with(c, &mut meshes, &mut bindposes, |surface| {
                characters.add(look.character(&assets, &mut images, surface, origin))
            })
        })
        .collect();
    let sword = game.weapon.as_ref().map(|(ms, t, n)| {
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
    game.uploads = uploads;
    game.sword = sword;
    spawn_bodies(&mut commands, game);
    // the water reflection (kenshi_render::reflection), before the water that reads it
    let origin = game.zone.origin;
    let reflection = kenshi_render::reflection::spawn_reflection(
        &mut commands,
        &mut images,
        // (testing) KENSHI_WATER_REFLECTION overrides the option
        std::env::var("KENSHI_WATER_REFLECTION").ok().and_then(|v| v.parse().ok()).unwrap_or(sky.0.water_reflection),
        sky.0.view_distance,
        sky.0.reflection_range,
        WATER_HEIGHT - origin.y,
    );
    look.reflection = (reflection.level > 0).then(|| reflection.image.clone());
    commands.insert_resource(reflection);
    // the water: the distant plane, and the first zone's
    let m = look.distant_water(&assets, origin);
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(DISTANT_WATER_SIZE, DISTANT_WATER_SIZE))),
        MeshMaterial3d(waters.add(m)),
        Transform::from_xyz(-origin.x, WATER_HEIGHT - origin.y, -origin.z),
        bevy::light::NotShadowCaster,
    ));
    spawn_zone_water(&mut commands, &assets, &mut meshes, &mut waters, &mut look, &game.zone, game.zone.zone);
    let s = ZONE_SIZE;
    // the far clip: the View Distance option x 10 (F041473)
    let far = sky.0.fog_params().x;
    let root = game.root.clone();
    spawn_moons(&mut commands, &assets, &mut meshes, &mut moons, &root);
    // the heat haze's textures and option
    let raw = |p: &str| -> Handle<Image> {
        assets
            .load_builder()
            .with_settings(|s: &mut bevy::image::ImageLoaderSettings| s.is_srgb = false)
            .load(p.to_string())
    };
    commands.insert_resource(kenshi_render::hdr::KenshiHdr {
        haze_enabled: sky.0.heat_haze,
        fxaa: sky.0.fxaa,
        flow: raw("data/materials/FlowHAZE.dds"),
        perturber: raw("data/materials/Perturber.dds"),
        ..Default::default()
    });
    spawn_sky_dome(
        &mut commands,
        &assets,
        &mut meshes,
        &mut sky_materials,
        &look.light,
        far,
    );
    commands.spawn((
        Camera3d::default(),
        // the scene in raw HDR for Kenshi's HDR chain (kenshi_render::hdr)
        bevy::camera::Hdr,
        // kenshi.cfg FSAA=1 (no multisampling); Kenshi's own FXAA runs in kenshi_render::hdr;
        // no tone curve or dither after it
        Msaa::Off,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        // the water reads the scene's depth behind it
        bevy::core_pipeline::prepass::DepthPrepass,
        // F041481: the near clip 5
        Projection::Perspective(PerspectiveProjection {
            far,
            near: camera::NEAR_CLIP,
            ..default()
        }),
        Transform::default(),
        DistanceFog {
            color: Color::srgb(0.72, 0.68, 0.6),
            falloff: FogFalloff::Linear {
                start: s * 0.8,
                end: s * 3.0,
            },
            ..default()
        },
    ));
    // the sun: Kenshi's CSM shadows (kenshi_render::shadows, kenshi::lighting::sun_shadow), a
    // cascade a layer of the map's quarter. (stand-in) The RTW mode (2) draws the CSM ones.
    commands.insert_resource(bevy::light::DirectionalLightShadowMap {
        size: kenshi_render::shadows::cascade_size(sky.0.shadow_quality) as usize,
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0,
            shadow_maps_enabled: sky.0.shadow_mode != 0,
            // the casters' bias is Kenshi's, in sun_shadow
            shadow_depth_bias: 0.0,
            shadow_normal_bias: 0.0,
            ..default()
        },
        kenshi_render::shadows::cascade_config(sky.0.shadow_range),
        Transform::default().looking_to(-look.light.sun_direction, Vec3::Y),
    ));
    commands.insert_resource(ranged_view::AttachPoints::load(&game.root, &game.sim.data, &game.res));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 250.0,
        ..default()
    });
    let hud = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(Color::srgb(0.1, 0.09, 0.08)),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(10.0),
                left: Val::Px(12.0),
                ..default()
            },
        ))
        .id();
    let marquee = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                border: UiRect::all(Val::Px(1.0)),
                display: Display::None,
                ..default()
            },
            BorderColor::all(Color::srgb(0.2, 0.9, 0.3)),
        ))
        .id();
    game.hud = Some(hud);
    game.marquee = Some(marquee);
}

/// Pause and game speed.
fn controls(
    act: bindings::Actions,
    mut game: ResMut<Game>,
    build: Res<build_ui::BuildMode>,
    mut open: ResMut<OpenBuildings>,
    mut rig: ResMut<camera::CameraRig>,
    time: Res<Time>,
) {
    if act.is_typing() {
        return;
    }
    // (assumption) "build_apply" and "pause" share Space: placing a building has it
    if act.just("pause") && !build.is_open() {
        game.paused = !game.paused;
    }
    for (k, action) in ["speed_1", "speed_2", "speed_3"].into_iter().enumerate() {
        if act.just(action) {
            game.speed = SPEEDS[k];
            game.paused = false;
        }
    }
    if act.just("cycle_run_speed") {
        game.sim.player_run = !game.sim.player_run;
    }
    if act.just("stop_movement") {
        let sel = game.selected.clone();
        game.sim.order_stop(&sel);
    }
    if act.just("toggle_sneak") {
        let sel = game.selected.clone();
        game.sim.toggle_sneak(&sel);
    }
    // select_0 .. select_9: the squad's first .. tenth member, shift adding them; select_all
    // the squad (the squad panel's order: the player's characters as listed)
    let shift = act.keys().pressed(KeyCode::ShiftLeft) || act.keys().pressed(KeyCode::ShiftRight);
    let squad: Vec<usize> = game.sim.folk.iter().filter(|p| p.player && !p.dead).map(|p| p.id).collect();
    for n in 0..10 {
        if act.just(&format!("select_{n}"))
            && let Some(&id) = squad.get(n)
        {
            if !shift {
                game.selected.clear();
            }
            if !game.selected.contains(&id) {
                game.selected.push(id);
            }
            // F044306: pressed twice within 0.5 s, the camera follows them (F045539; their
            // floor shown, `hide_roofs`)
            rig.select_key(n, id, time.elapsed_secs());
        }
    }
    if act.just("select_all") {
        game.selected = squad;
    }
    // (Kenshi puts down whoever the selection carries from the context menu only: Put Down,
    // context_ui; no key does it)
    // F045512: the focus key moves the camera to the selected character (F041479: at once, no
    // longer following) and shows their floor; (assumption) the selection's first is the one
    if act.just("focus_char")
        && let Some(&i) = game.selected.first()
    {
        game.focus = Vec3::from(game.sim.folk[i].mover.position_units()) - game.zone.origin;
        rig.follow = None;
        rig.snap = true;
        open.focused = true;
    }
}


/// A person's chest on screen (pixels), if in view.
fn on_screen(cam: &Camera, at: &GlobalTransform, game: &Game, i: usize) -> Option<Vec2> {
    let p = Vec3::from(game.sim.folk[i].mover.position_units()) - game.zone.origin + Vec3::Y * 12.0;
    cam.world_to_viewport(at, p).ok()
}

/// How far the cursor is outside person `i`'s body on screen (pixels; 0 or less: on it): a
/// standing body the stroke from the feet to the head (19 units up) a fifth of its height wide;
/// one lying down a disc over the ground where it lies, 9 units round. None when out of view.
/// (The game picks by the body's collision; this is its outline on screen.)
fn body_distance(cam: &Camera, at: &GlobalTransform, game: &Game, i: usize, cursor: Vec2) -> Option<f32> {
    let p = &game.sim.folk[i];
    let base = Vec3::from(p.mover.position_units()) - game.zone.origin;
    if p.down {
        let c = cam.world_to_viewport(at, base + Vec3::Y * 2.0).ok()?;
        // the body's reach on screen: 9 units across the view
        let right = at.right().as_vec3();
        let edge = cam.world_to_viewport(at, base + Vec3::Y * 2.0 + right * 9.0).ok()?;
        return Some(c.distance(cursor) - c.distance(edge).max(10.0));
    }
    let feet = cam.world_to_viewport(at, base + Vec3::Y * 1.0).ok()?;
    let head = cam.world_to_viewport(at, base + Vec3::Y * 19.0).ok()?;
    let seg = head - feet;
    let t = if seg.length_squared() > 0.0 { ((cursor - feet).dot(seg) / seg.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    let d = cursor.distance(feet + seg * t);
    Some(d - (seg.length() * 0.2).max(8.0))
}

/// The person under the cursor: on their body on screen (`body_distance`, a few pixels' slack),
/// else the nearest whose middle is within `PICK_PX` (a small, far one); the nearest of them.
fn pick(
    cam: &Camera,
    at: &GlobalTransform,
    game: &Game,
    cursor: Vec2,
    players: bool,
) -> Option<usize> {
    game.sim
        .folk
        .iter()
        .filter(|p| p.player == players && p.carried_by.is_none())
        .filter_map(|p| {
            let body = body_distance(cam, at, game, p.id, cursor)?;
            let middle = on_screen(cam, at, game, p.id)?.distance(cursor);
            (body < 4.0 || middle < PICK_PX).then_some((p.id, body.min(middle - PICK_PX)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Left click / drag: select the player's characters.
fn select(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    mut game: ResMut<Game>,
    mut nodes: Query<&mut Node>,
    ui: Query<&Interaction>,
    build: Res<build_ui::BuildMode>,
    (mut ray, pieces, mut frame): (MeshRayCast, Query<PickPiece>, Local<u32>),
) {
    if build.is_open() {
        return;
    }
    // (testing) KENSHI_CLICK_AT=frame@x,y: the building at window point (x, y) picked then, as
    // a left click there on no character
    *frame += 1;
    if let Ok(v) = std::env::var("KENSHI_CLICK_AT")
        && let Some((f, xy)) = v.split_once('@')
        && f.parse::<u32>().ok() == Some(*frame)
        && let Some((x, y)) = xy.split_once(',')
        && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
        && let Ok((cam, at)) = cams.single()
    {
        let place = pick_building(cam, at, &game, Vec2::new(x, y), &mut ray, &pieces);
        game.selected_building = place.and_then(|p| building_ui::Selected::new(&game.sim, p));
        let name = place.and_then(|p| match p {
            building_ui::Place::Town(k) => game.sim.placed.get(k).map(|q| q.building.clone()),
            building_ui::Place::Furniture(k) => game.sim.furn.get(k).map(|q| q.record.clone()),
            building_ui::Place::Container(k) => game.sim.boxes.get(k).map(|q| q.at.record.clone()),
            building_ui::Place::Door(k) => game.sim.door_states.get(k).map(|q| q.door.record.clone()),
            building_ui::Place::Player(k) => game.sim.buildings.get(k).map(|q| q.record.clone()),
        });
        eprintln!("click at {x},{y}: {place:?} {:?}", name.and_then(|r| game.sim.data.get(&r)).map(|r| r.name.clone()));
    }
    if buttons.just_pressed(MouseButton::Left) && on_ui(&ui) {
        return;
    }
    let (Ok(win), Ok((cam, at))) = (windows.single(), cams.single()) else {
        return;
    };
    let Some(cursor) = win.cursor_position() else {
        return;
    };
    if buttons.just_pressed(MouseButton::Left) {
        game.drag = Some(cursor);
    }
    let marquee = game.marquee;
    if let (Some(start), Some(m)) = (game.drag, marquee)
        && let Ok(mut node) = nodes.get_mut(m)
    {
        let (lo, hi) = (start.min(cursor), start.max(cursor));
        let big = (hi - lo).length() > 6.0;
        node.display = if big { Display::Flex } else { Display::None };
        node.left = Val::Px(lo.x);
        node.top = Val::Px(lo.y);
        node.width = Val::Px(hi.x - lo.x);
        node.height = Val::Px(hi.y - lo.y);
    }
    if buttons.just_released(MouseButton::Left)
        && let Some(start) = game.drag.take()
    {
        if let Some(m) = marquee
            && let Ok(mut node) = nodes.get_mut(m)
        {
            node.display = Display::None;
        }
        let add = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let dragged = (cursor - start).length() > 6.0;
        let mut chosen: Vec<usize> = if dragged {
            let (lo, hi) = (start.min(cursor), start.max(cursor));
            game.sim
                .folk
                .iter()
                .filter(|p| p.player)
                .filter(|p| {
                    on_screen(cam, at, &game, p.id)
                        .is_some_and(|s| s.cmpge(lo).all() && s.cmple(hi).all())
                })
                .map(|p| p.id)
                .collect()
        } else {
            pick(cam, at, &game, cursor, true).into_iter().collect()
        };
        // a click on no character but on a building picks the building (the characters
        // selected stay so, F045559); a character picked, or nothing, lets it go
        game.selected_building = None;
        if !dragged
            && chosen.is_empty()
            && let Some(place) = pick_building(cam, at, &game, cursor, &mut ray, &pieces)
        {
            game.selected_building = building_ui::Selected::new(&game.sim, place);
            return;
        }
        if add {
            for &i in &game.selected {
                if !chosen.contains(&i) {
                    chosen.push(i);
                }
            }
        }
        game.selected = chosen;
    }
}

/// What a building part drawn tells of its building: its building and floor, a town door's or
/// one of the player's doors, and where it stands.
type PickPiece = (Option<&'static BuildingFloor>, Option<&'static kenshi_render::zone::DoorPiece>, Option<&'static build_ui::PlayerDoorPiece>, &'static GlobalTransform);

/// The building under the cursor, as GameWorld's pick (F045559) takes the object its ray meets
/// first: the nearest visible building part along the ray short of the ground. A door's part
/// is its door; a part standing where one of a town building's containers or a piece of its
/// furniture stands is that; any other its building (a town's, or one of the player's).
/// (stand-in for the physics ray: the parts' meshes) Meeting none, the ground point under the
/// cursor within the footprint of one of the player's picks that (a site has no parts up yet).
fn pick_building(cam: &Camera, at: &GlobalTransform, game: &Game, cursor: Vec2, ray: &mut MeshRayCast, pieces: &Query<PickPiece>) -> Option<building_ui::Place> {
    use bevy::picking::mesh_picking::ray_cast::{MeshRayCastSettings, RayCastVisibility};
    use building_ui::Place;
    let r = cam.viewport_to_world(at, cursor).ok()?;
    let ground = terrain_at(game, r.origin, *r.direction).map(|p| p.distance(r.origin));
    let filter = |e: Entity| pieces.get(e).is_ok_and(|(f, d, pd, _)| f.is_some() || d.is_some() || pd.is_some());
    let settings = MeshRayCastSettings::default().with_filter(&filter).with_visibility(RayCastVisibility::Visible);
    let hit = ray.cast_ray(r, &settings).first().map(|(e, h)| (*e, h.distance));
    if let Some((e, distance)) = hit
        && ground.is_none_or(|g| distance <= g + 1.0)
        && let Ok((floor, door, player_door, t)) = pieces.get(e)
    {
        if let Some(d) = player_door {
            return Some(Place::Door(d.0));
        }
        if let Some(d) = door
            && let Some(off) = game.sim.zone_offsets.get(&d.zone)
            && let Some(k) = game.sim.door_states.iter().position(|s| !s.gone && s.door.placement == off + d.building && s.door.index == d.index)
        {
            return Some(Place::Door(k));
        }
        if let Some(f) = floor {
            if f.zone == build_ui::PLAYER_ZONE {
                return Some(Place::Player(f.building));
            }
            let p = game.sim.zone_offsets.get(&f.zone)? + f.building;
            let here = t.translation() + game.zone.origin;
            let near = |q: [f32; 3]| Vec3::from(q).distance(here) < 2.0;
            if let Some(k) = game.sim.boxes.iter().position(|x| x.at.placement == p && near(x.at.position)) {
                return Some(Place::Container(k));
            }
            if let Some(k) = game.sim.furn.iter().position(|x| x.placement == p && near(x.position)) {
                return Some(Place::Furniture(k));
            }
            return Some(Place::Town(p));
        }
    }
    let q = ground_at(cam, at, game, cursor)?;
    let w = q + game.zone.origin;
    game.sim.building_at([w.x, w.y, w.z]).map(Place::Player)
}

/// The ground under the cursor (scene units): the ray marched down to the terrain.
fn ground_at(cam: &Camera, at: &GlobalTransform, zone: &Game, cursor: Vec2) -> Option<Vec3> {
    ground_at_shown(cam, at, zone, cursor, &|_| true)
}

/// `ground_at`, the floors at the points `shown` refuses (scene: the storeys of an opened
/// building above the floor it shows) passed through.
fn ground_at_shown(cam: &Camera, at: &GlobalTransform, zone: &Game, cursor: Vec2, shown: &dyn Fn(Vec3) -> bool) -> Option<Vec3> {
    let ray = cam.viewport_to_world(at, cursor).ok()?;
    let dir = *ray.direction;
    // the terrain, or nearer along the ray the navmesh's floors (a building's storeys, its
    // stairs and decks): where a click on an upper floor means
    let terrain = terrain_at(zone, ray.origin, dir);
    let world = ray.origin + zone.zone.origin;
    let reach = terrain.map_or(20000.0, |p| p.distance(ray.origin) - 1.0);
    let floor = zone
        .sim
        .nav
        .raycast((world * kenshi_world::navmesh::METRES).to_array(), dir.to_array(), reach * kenshi_world::navmesh::METRES, &|p| {
            shown(Vec3::from(p) / kenshi_world::navmesh::METRES - zone.zone.origin)
        })
        .map(|(p, _)| Vec3::from(p) / kenshi_world::navmesh::METRES - zone.zone.origin);
    floor.or(terrain)
}

/// Where a ray from `origin` along `dir` (scene) meets the terrain.
fn terrain_at(zone: &Game, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let ray = Ray3d { origin, direction: Dir3::new(dir).ok()? };
    let mut t = 0.0;
    let mut prev = ray.origin;
    while t < 20000.0 {
        t += 5.0;
        let p = ray.origin + dir * t;
        if p.y <= zone.ground(p.x, p.z) {
            // refine between the last two samples
            let (mut a, mut b) = (prev, p);
            for _ in 0..12 {
                let m = (a + b) * 0.5;
                if m.y <= zone.ground(m.x, m.z) {
                    b = m;
                } else {
                    a = m;
                }
            }
            return Some(Vec3::new(b.x, zone.ground(b.x, b.z), b.z));
        }
        prev = p;
    }
    None
}

/// Right click: on someone, their default order (a quick click) or, held a quarter second, the
/// menu of orders (`context_ui`); on the ground, move there, and held after a move the squad
/// keeps following the cursor ("mouse_command" held, Mouse2).
#[allow(clippy::too_many_arguments)]
fn command(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    mut game: ResMut<Game>,
    ui: Query<&Interaction>,
    mut inv: ResMut<inventory_ui::InventoryWindow>,
    mut talk: ResMut<dialogue_ui::DialogueWindow>,
    mut menu: ResMut<context_ui::ContextMenu>,
    time: Res<Time>,
    mut held: Local<Option<f32>>,
    mut press: Local<Option<(usize, f32)>>,
    build: Res<build_ui::BuildMode>,
    (views, open, mut marker, keys): (Query<&BuildingView>, Res<OpenBuildings>, ResMut<move_marker::MoveMarker>, Res<ButtonInput<KeyCode>>),
) {
    // a click goes to the floor shown: the storeys of an opened building above it are hidden
    let shown = |p: Vec3| {
        let w = (p + game.zone.origin).to_array();
        !building_at(&views, w).is_some_and(|(b, f)| open.open.contains(&b) && f > open.floor)
    };
    if build.is_open() {
        return;
    }
    let (Ok(win), Ok((cam, at))) = (windows.single(), cams.single()) else {
        return;
    };
    // (the testing click's place, else the mouse's)
    let Some(cursor) = menu.test_cursor.or_else(|| win.cursor_position()) else {
        return;
    };
    // held on the ground: the squad's move order follows the cursor, re-aimed every 0.2 s
    if let Some(t) = held.as_mut() {
        if !buttons.pressed(MouseButton::Right) || game.selected.is_empty() {
            *held = None;
        } else {
            // the movement marker on the cursor's ground while held (F045591)
            if let Some(q) = ground_at_shown(cam, at, &game, cursor, &shown) {
                marker.show(q);
            }
            *t += time.delta_secs();
            if *t >= 0.2 {
                *t = 0.0;
                if let Some(q) = ground_at_shown(cam, at, &game, cursor, &shown) {
                    let w = q + game.zone.origin;
                    let selected = game.selected.clone();
                    game.sim.steer_move_squad(&selected, [w.x, w.y, w.z]);
                }
            }
            return;
        }
    }
    // held on someone: the menu after a quarter second, else the default order on release
    if let Some((target, t)) = press.as_mut() {
        let target = *target;
        if buttons.pressed(MouseButton::Right) {
            *t += time.delta_secs();
            if *t >= context_ui::HOLD {
                *press = None;
                menu.request = Some(target);
            }
            return;
        }
        *press = None;
        default_order(&mut game, &mut inv, &mut talk, target);
        return;
    }
    if !buttons.just_pressed(MouseButton::Right) || game.selected.is_empty() || (on_ui(&ui) && menu.test_cursor.is_none()) {
        return;
    }
    // someone standing, else someone down (anyone's)
    let down = |players| pick(cam, at, &game, cursor, players).filter(|&e| game.sim.folk[e].down && game.sim.folk[e].carried_by.is_none());
    // one of the player's own in a cage (its escape orders), else whoever is in a cage the
    // cursor's ground falls in
    let caged = |e: usize| game.sim.folk[e].prison.is_some();
    let in_cage = || {
        let q = ground_at_shown(cam, at, &game, cursor, &shown)? + game.zone.origin;
        let cage = game.sim.furn.iter().enumerate().find(|(k, f)| {
            f.function == kenshi_world::furniture::BF_CAGE
                && (f.position[0] - q.x).hypot(f.position[2] - q.z) < game.sim.cage_reach.get(k).copied().unwrap_or(15.0)
        })?;
        game.sim.folk.iter().find(|p| p.prison.as_ref().is_some_and(|pr| pr.cage == cage.0 && !pr.out)).map(|p| p.id)
    };
    let someone = pick(cam, at, &game, cursor, false)
        .filter(|&e| !game.sim.folk[e].down)
        .or_else(|| down(false))
        .or_else(|| down(true))
        .or_else(|| pick(cam, at, &game, cursor, true).filter(|&e| caged(e)))
        .or_else(in_cage);
    if menu.test_cursor.is_some() {
        eprintln!("rclick: cursor {cursor}: person {someone:?}, training {:?}", pick_training(cam, at, &game, cursor));
    }
    // shift held: the order a job instead, when it can be one (kenshi_game::orders::Job)
    let shift = menu.test_cursor.is_none() && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    if let Some(e) = someone {
        if !(shift && add_job(&mut game, e)) {
            *press = Some((e, 0.0));
        }
        return;
    }
    // on an item lying on the ground or a floor (picked on screen): the selection goes and
    // picks it up
    // (Collecting item, task 3, is no job: with shift too it is just the order, F036821)
    if let Some(item) = ground_items::under_cursor(cam, at, &game, cursor) {
        let selected = game.selected.clone();
        game.sim.order_pick_item(&selected, item);
        return;
    }
    // on town training equipment (picked on screen: it may stand on a floor over the ground):
    // train on it on release, its menu held
    if let Some(f) = pick_training(cam, at, &game, cursor) {
        if !(shift && add_job(&mut game, kenshi_game::production::FURN_TARGET + f)) {
            *press = Some((kenshi_game::production::FURN_TARGET + f, 0.0));
        }
        return;
    }
    let Some(q) = ground_at_shown(cam, at, &game, cursor, &shown) else {
        return;
    };
    let selected = game.selected.clone();
    let w = q + game.zone.origin;
    // on an item lying on the ground: the selection goes and picks it up
    if let Some(item) = ground_items::under(&game, q) {
        game.sim.order_pick_item(&selected, item);
        return;
    }
    // on a container (a chest, a crate, a storage box): the first selected goes and opens it
    if let Some(owner) = game.sim.box_near([w.x, w.y, w.z], 15.0)
        && let Some(&me) = selected.first()
    {
        let at = game.sim.storage_info(owner).map_or([w.x, w.y, w.z], |i| i.0);
        game.sim.order_move(&[me], at);
        inv.pending_loot = Some(owner);
        return;
    }
    // on a door or gate: open or close it on release, its menu held
    if let Some(d) = game.sim.door_near([w.x, w.y, w.z], 60.0) {
        *press = Some((kenshi_game::doors::DOOR_TARGET + d, 0.0));
        return;
    }
    // on a building of the player's: its default order on release, its menu held
    if let Some(b) = game.sim.building_at([w.x, w.y, w.z]) {
        if !(shift && add_job(&mut game, kenshi_game::base::BUILDING_OWNER + b)) {
            *press = Some((kenshi_game::base::BUILDING_OWNER + b, 0.0));
        }
        return;
    }
    game.sim.order_move_squad(&selected, [w.x, w.y, w.z]);
    marker.show(q);
    *held = Some(0.0);
}

/// The town furniture with orders under the cursor (training equipment, beds, cages): its middle
/// (8 units up) within 30 pixels of it on screen, the nearest to the camera of those.
fn pick_training(cam: &Camera, at: &GlobalTransform, game: &Game, cursor: Vec2) -> Option<usize> {
    let eye = at.translation();
    (0..game.sim.furn.len())
        .filter(|&k| matches!(game.sim.furn[k].function, kenshi_game::production::FUNCTION_TRAINING | kenshi_game::sim::BF_BED | kenshi_world::furniture::BF_CAGE))
        .filter_map(|k| {
            let p = Vec3::from(game.sim.furn[k].position) - game.zone.origin + Vec3::Y * 8.0;
            let d = p.distance(eye);
            if d > 3000.0 {
                return None;
            }
            let s = cam.world_to_viewport(at, p).ok()?;
            (s.distance(cursor) < 30.0).then_some((k, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// Shift with an order on `target`: a job for the selection when the order can be one.
fn add_job(game: &mut Game, target: usize) -> bool {
    let sel = game.selected.clone();
    game.sim.add_job(&sel, target)
}

/// (testing) KENSHI_JOB_AT=<frame>: the selection given a job on the nearest standing person not
/// of the player's, and its JOBS turned off and on again. (The jobs themselves run on the AI's
/// planner: kenshi_game::jobs.)
fn run_jobs(mut game: ResMut<Game>, mut frames: Local<u32>) {
    *frames += 1;
    if std::env::var("KENSHI_JOB_AT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames)
        && let Some(&me) = game.selected.first()
    {
        let at = game.sim.folk[me].mover.position_units();
        let near = game.sim.folk.iter().filter(|p| !p.player && !p.down && !p.dead).min_by(|a, b| {
            let d = |p: &kenshi_game::sim::Person| { let q = p.mover.position_units(); (q[0] - at[0]).hypot(q[2] - at[2]) };
            d(a).total_cmp(&d(b))
        }).map(|p| p.id);
        if let Some(e) = near {
            add_job(&mut game, e);
            let sel = game.selected.clone();
            game.sim.toggle_jobs(&sel);
            game.sim.toggle_jobs(&sel);
            eprintln!("job test: {:?} jobs {:?} on {}", sel, game.sim.folk[me].jobs, game.sim.folk[me].jobs_on);
        }
    }
}

/// A quick right click on someone: someone down is picked up; someone not fighting with
/// something to say is walked to and talked to (a trader with nothing to say traded with);
/// anyone else attacked.
fn default_order(game: &mut Game, inv: &mut inventory_ui::InventoryWindow, talk: &mut dialogue_ui::DialogueWindow, e: usize) {
    use context_ui::Action;
    if let Some(f) = e.checked_sub(kenshi_game::production::FURN_TARGET) {
        // a quick click: the furniture's own order (training; a bed: sleep, or put the one
        // carried in it; a cage: put the one carried in it)
        let carrying = game.selected.iter().any(|&i| game.sim.folk[i].carrying.is_some());
        let a = match game.sim.furn.get(f).map(|k| k.function) {
            Some(kenshi_game::production::FUNCTION_TRAINING) => Some(Action::Train),
            Some(kenshi_game::sim::BF_BED) => Some(if carrying { Action::PutInBed } else { Action::Sleep }),
            Some(kenshi_world::furniture::BF_CAGE) if carrying => Some(Action::PutInCage),
            _ => None,
        };
        if let Some(a) = a {
            context_ui::act(game, inv, talk, e, a);
        }
        return;
    }
    if e >= kenshi_game::doors::DOOR_TARGET {
        context_ui::act(game, inv, talk, e, Action::DoorToggle);
        return;
    }
    if let Some(b) = e.checked_sub(kenshi_game::base::BUILDING_OWNER) {
        if let Some(pb) = game.sim.buildings.get(b) {
            let at = Vec3::from(pb.at);
            build_ui::order_at(game, inv, at);
        }
        return;
    }
    let p = &game.sim.folk[e];
    let a = if p.down {
        Action::PickUp
    } else if !game.sim.arena.f[e].active && p.dialogue.is_some() {
        Action::TalkTo
    } else if !game.sim.arena.f[e].active && p.shop.is_some() {
        Action::Trade
    } else if p.player {
        return;
    } else {
        Action::AttackTarget
    };
    context_ui::act(game, inv, talk, e, a);
}

/// (testing) the hit constants' bleed rate.
fn mc_bleed(sim: &kenshi_game::sim::Sim) -> f32 {
    sim.arena.sh.hc.bleed_rate
}

/// Advance the simulation by the frame's time at the game speed.
fn run_sim(time: Res<Time>, mut game: ResMut<Game>) {
    kenshi_render::prof::frame();
    let mut prof = kenshi_render::prof::Marks::start();
    if game.paused {
        return;
    }
    game.carry += time.delta_secs() * game.speed;
    let mut steps = 0;
    while game.carry >= TICK && steps < 16 {
        game.sim.step();
        game.carry -= TICK;
        steps += 1;
    }
    if steps == 16 {
        game.carry = 0.0;
    }
    prof.mark("run_sim");
    kenshi_render::prof::add("sim steps (count x 1s)", steps as f64);
}

/// What turns the buildings' moving parts (kenshi_render::zone::PartMotion): the wind where the
/// camera is, each machine's work (the player's while they produce or burn fuel; a town's
/// furniture while someone operates it) and the game's frame time.
fn part_motion(game: Res<Game>, time: Res<Time>, weather: Res<WeatherState>, mut motion: ResMut<kenshi_render::zone::PartMotion>) {
    let m = &mut *motion;
    m.dt = if game.paused { 0.0 } else { time.delta_secs() * game.speed };
    m.wind_speed = if weather.current.is_some() { weather.wind.length() } else { 0.0 };
    m.wind_dir = weather.wind.normalize_or_zero();
    m.rates.clear();
    for b in game.sim.buildings.iter().filter(|b| b.done && b.running > 0.0) {
        m.rates.insert(kenshi_render::zone::TurretAims::key(Vec3::from(b.at)), b.running);
    }
    for (f, op) in game.sim.operators.iter().enumerate() {
        if op.is_some()
            && let Some(fu) = game.sim.furn.get(f)
        {
            m.rates.insert(kenshi_render::zone::TurretAims::key(Vec3::from(fu.position)), 1.0);
        }
    }
}

/// The building collision of the zones loaded (scene frame).
fn rebuild_statics(game: &mut Game) {
    let parts: Vec<_> = std::iter::once(&game.zone)
        .chain(game.zones.values())
        .flat_map(|z| z.collision.iter().zip(z.collision_group.iter().copied()))
        .map(|((t, s), g)| (t.compute_affine(), s.clone(), g))
        .collect();
    // and the foliage objects' (F041802)
    let foliage: Vec<_> = std::iter::once(&game.zone)
        .chain(game.zones.values())
        .flat_map(|z| z.foliage_collision.iter())
        .map(|(t, s, g)| (t.compute_affine(), s.clone(), *g))
        .collect();
    game.statics = std::sync::Arc::new(kenshi_physics::world::StaticWorld::grouped(parts.iter().chain(foliage.iter()).map(|(a, s, g)| (*a, &**s, *g))));
    let passed = parts.iter().filter(|p| kenshi_physics::world::ragdoll_ignores(false) & 1 << (p.2 & 31) != 0).count();
    let walkable = foliage.iter().filter(|p| p.2 == kenshi_render::zone::FEATURE_GROUP).count();
    // (testing) KENSHI_FOLIAGE_DEBUG: the buildings the foliage made (the sim's) and the foliage
    // collision standing at each
    if std::env::var("KENSHI_FOLIAGE_DEBUG").is_ok() {
        let origin = game.zone.origin;
        let me = game.sim.folk.iter().find(|p| p.player).map(|p| Vec3::from(p.mover.position_units())).unwrap_or(origin);
        for f in game.sim.furn.iter().filter(|f| game.sim.data.get(&f.record).is_some_and(|r| r.bools.get("is foliage").copied().unwrap_or(false))) {
            let at = Vec3::from(f.position) - origin;
            let near = foliage.iter().filter(|p| Vec3::from(p.0.translation).with_y(0.0).distance(at.with_y(0.0)) < 1.0).map(|p| p.2).collect::<Vec<_>>();
            eprintln!("foliage building {} ({}) at {:?}, {:.0} from the squad (off {:?}): collision groups there {near:?}", f.name, f.function, f.position.map(|v| v as i32), (Vec3::from(f.position) - me).with_y(0.0).length(), (Vec3::from(f.position) - me).to_array().map(|v| v as i32));
        }
    }
    eprintln!(
        "ragdoll collision: {} solids ({passed} of {} parts passed through outside: doors, passable; {} foliage objects, {walkable} walkable)",
        game.statics.len(),
        parts.len(),
        foliage.len()
    );
    // the same solids stop bolts and block lines of fire in the fights, each trace taking its own
    // groups (`arena::LINE_OF_FIRE_GROUPS`, `arena::BOLT_GROUPS`) (scene frame: from the zone's
    // origin)
    game.sim.arena.obstacles = Some((game.statics.clone(), game.zone.origin.to_array()));
    // the navmesh cut round what each zone stands on the ground (`navcut`), once a zone
    let origin = game.zone.origin;
    // (keyed by zone; the first zone loaded by a key no zone has)
    let zones: Vec<((i64, i64), &kenshi_render::zone::ZoneScene)> =
        std::iter::once(((i64::MIN, 0), &game.zone)).chain(game.zones.iter().map(|(k, z)| ((k.0 as i64, k.1 as i64), z))).collect();
    let mut cuts = Vec::new();
    for (key, z) in zones {
        if game.sim.nav.has_cutters(key) {
            continue;
        }
        let mut cutters = Vec::new();
        // a building's parts cut together (or not at all); a part of none on its own
        let mut groups: HashMap<usize, u32> = HashMap::new();
        let mut next = 0u32;
        for (k, (t, solids)) in z.collision.iter().enumerate() {
            if z.collision_door.get(k).copied().unwrap_or(false) {
                continue;
            }
            if std::env::var("KENSHI_NAV_GROUPS").is_ok() && let Some(b) = z.collision_building.get(k).copied().flatten() && !groups.contains_key(&b) {
                let name = game.sim.placed.get(b).and_then(|p| game.sim.data.get(&p.building)).map_or("?", |r| r.name.as_str());
                eprintln!("navgroup {key:?} {} = {name}", next);
            }
            let group = match z.collision_building.get(k).copied().flatten() {
                Some(b) => *groups.entry(b).or_insert_with(|| {
                    next += 1;
                    next - 1
                }),
                None => {
                    next += 1;
                    next - 1
                }
            };
            let a = t.compute_affine();
            // the building's "path mode" (PathMode): 0 NAVMESH_IGNORE not cut; 1 NAVMESH_PROJECTED
            // and 2 NAVMESH_OBSTACLE cut by their shape; 3 NAVMESH_WALKABLE (a navmesh of its own,
            // baked into the tiles or made by the game) cut only by its walls, so its ramps, floors
            // and gateways are left
            let mode = z
                .collision_building
                .get(k)
                .copied()
                .flatten()
                .and_then(|b| game.sim.placed.get(b))
                .and_then(|p| game.sim.data.get(&p.building))
                .and_then(|r| r.ints.get("path mode").copied())
                .unwrap_or(3);
            if mode == 0 {
                continue;
            }
            for (pts, wall) in solids.footprints() {
                if mode == 3 && !wall {
                    continue;
                }
                let m: Vec<[f32; 3]> = pts.iter().map(|p| ((a.transform_point3(*p) + origin) * kenshi_world::navmesh::METRES).to_array()).collect();
                cutters.extend(kenshi_world::navcut::Cutter::from_points(&m, kenshi_world::navcut::PAD).map(|mut c| {
                    c.group = group;
                    c
                }));
            }
        }
        cuts.push((key, cutters));
    }
    let first = cuts.iter().any(|(k, _)| k.0 == i64::MIN);
    for (key, cutters) in cuts {
        let n = cutters.len();
        let t = std::time::Instant::now();
        game.sim.nav.add_cutters(key, cutters);
        if std::env::var("KENSHI_NAV_DEBUG").is_ok() {
            eprintln!("navcut: zone at {key:?}: {n} cutters, {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    // the zone's furniture (kenshi_game::furniture_cut): beds, tables, counters, storage and
    // machines cut out of the ground's and the buildings' navmesh, each piece by itself.
    // ((testing) KENSHI_NO_FURNITURE_CUT: the cages alone, as before)
    if first {
        let only_cages = std::env::var("KENSHI_NO_FURNITURE_CUT").is_ok();
        let (res, root) = (game.res.clone(), game.root.clone());
        let mut next = 1_000_000;
        let t = std::time::Instant::now();
        let (n, freed) = kenshi_game::furniture_cut::cut_furniture(&mut game.sim, &res, &root, &mut next, only_cages);
        if std::env::var("KENSHI_NAV_DEBUG").is_ok() {
            eprintln!("navcut: furniture {n}, {freed} left uncut, {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

/// The player's buildings cut out of the navmesh (as the zones' are, `rebuild_statics`): each
/// built wall, gate and obstacle (a building that "links together", or "path mode" 1 / 2) by its
/// physics silhouette, its parts' collision ("xml collision") footprints turned with it
/// (`zone::building_footprints`; a part without one by its mesh's bounds); a gate's door is not
/// among its parts, so its doorway is left for the door (whose barrier shuts it,
/// kenshi_game::doors). Cut again whenever the buildings change. A building with a navmesh of
/// its own ("path mode" 3, a house) is not cut: its inside is not walkable without its interior
/// navmesh, which the game makes and this does not.
fn cut_player_buildings(mut game: ResMut<Game>, mut was: Local<(Vec<(String, [i32; 3], i32, bool)>, usize)>, mut prints: Local<HashMap<String, Vec<Vec<Vec3>>>>) {
    // a game loaded in the program: its own buildings
    if game.is_added() {
        *was = Default::default();
    }
    let now: Vec<(String, [i32; 3], i32, bool)> = game
        .sim
        .buildings
        .iter()
        .map(|b| (b.record.clone(), b.at.map(|v| v as i32), (b.yaw * 1000.0) as i32, b.done))
        .collect();
    if now == was.0 && !game.sim.buildings_changed {
        return;
    }
    game.sim.buildings_changed = false;
    for k in 0..was.1 {
        game.sim.nav.remove_cutters((-2, k as i64));
    }
    was.0 = now;
    let m = kenshi_world::navmesh::METRES;
    let mut n = 0;
    for b in 0..game.sim.buildings.len() {
        let pb = game.sim.buildings[b].clone();
        let Some(rec) = game.sim.data.get(&pb.record).cloned() else { continue };
        let links = rec.bools.get("links together").copied().unwrap_or(false);
        let mode = rec.ints.get("path mode").copied().unwrap_or(2);
        if !pb.done || !(links || mode == 1 || mode == 2) {
            continue;
        }
        let footprints = prints
            .entry(pb.record.clone())
            .or_insert_with(|| kenshi_render::zone::building_footprints(&game.sim.data, &game.res, &game.root, &rec))
            .clone();
        let rot = Quat::from_rotation_y(pb.yaw);
        let at = Vec3::from(pb.at);
        let mut cutters: Vec<kenshi_world::navcut::Cutter> = footprints
            .iter()
            .filter_map(|pts| {
                let w: Vec<[f32; 3]> = pts.iter().map(|p| ((rot * *p + at) * m).to_array()).collect();
                kenshi_world::navcut::Cutter::from_points(&w, kenshi_world::navcut::PAD)
            })
            .collect();
        for c in cutters.iter_mut() {
            c.group = 0;
        }
        game.sim.nav.add_cutters((-2, n as i64), cutters);
        n += 1;
    }
    was.1 = n;
}

/// A player building's interior mask entity (kenshi_render::interior_mask).
#[derive(Component, Clone)]
struct PlayerMask;

/// The player's finished buildings with an "interior mask" get theirs (F037793), made again
/// whenever the buildings change.
fn player_masks(
    mut commands: Commands,
    game: Res<Game>,
    mask: Option<Res<kenshi_render::interior_mask::InteriorMask>>,
    old: Query<Entity, With<PlayerMask>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut was: Local<Vec<(String, [i32; 3], i32)>>,
) {
    let Some(mask) = mask else { return };
    if game.is_added() {
        was.clear();
    }
    let now: Vec<(String, [i32; 3], i32)> = game
        .sim
        .buildings
        .iter()
        .filter(|b| b.done)
        .map(|b| (b.record.clone(), b.at.map(|v| v as i32), (b.yaw * 1000.0) as i32))
        .collect();
    if now == *was {
        return;
    }
    *was = now;
    for e in &old {
        commands.entity(e).despawn();
    }
    let places: Vec<kenshi_world::Placement> = game
        .sim
        .buildings
        .iter()
        .filter(|b| b.done)
        .map(|pb| kenshi_world::Placement {
            building: pb.record.clone(),
            position: pb.at,
            rotation: [(pb.yaw * 0.5).cos(), 0.0, (pb.yaw * 0.5).sin(), 0.0],
            world_y: Some(pb.at[1]),
            interior: None,
            exterior: None,
            town: None,
            inside: false,
            squad: None,
            residents: None,
            locks: Vec::new(),
            door_saved: Vec::new(),
            destroyed: false,
            power_on: None,
            handle: None,
        })
        .collect();
    for i in kenshi_world::interior::interiors(&game.sim.data, &game.res, &places) {
        kenshi_render::interior_mask::spawn_mask(&mut commands, &mut meshes, &mask, &i.triangles, PlayerMask);
    }
}

/// A body gone limp: its ragdoll, and the pose it fell from (for the bones without a body).
#[derive(Component)]
struct Limp {
    doll: Ragdoll,
    rig: Arc<RagdollRig>,
    locals: Vec<Transform>,
    /// Seconds since getting up.
    up: Option<f32>,
    /// Getting up, the hand-over (F044963): each bone's local transform from the ragdoll's pose
    /// to the get-up clip's first key, for the bones the clip has tracks for.
    hand_over: Option<HandOver>,
}

/// F044963 / F044960: the bones held from the animation while getting up, lerped from where the
/// ragdoll left them to the clip's first key.
struct HandOver {
    from: Vec<Transform>,
    to: Vec<Transform>,
    bones: Vec<usize>,
}

/// Ogre's Quaternion::nlerp with the shortest path (F044960's): the normalised straight blend.
fn nlerp(a: Quat, b: Quat, t: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    (a + (b - a) * t).normalize()
}

/// World transforms of a skeleton's bones from their locals under `root_parent`.
fn world_bones(locals: &[Transform], parents: &[Option<usize>], root_parent: Affine3A) -> Vec<Affine3A> {
    fn at(k: usize, locals: &[Transform], parents: &[Option<usize>], rp: Affine3A, out: &mut Vec<Option<Affine3A>>) -> Affine3A {
        if let Some(w) = out[k] {
            return w;
        }
        let up = parents[k].map_or(rp, |p| at(p, locals, parents, rp, out));
        let w = up * locals[k].compute_affine();
        out[k] = Some(w);
        w
    }
    let mut out = vec![None; locals.len()];
    (0..locals.len()).map(|k| at(k, locals, parents, root_parent, &mut out)).collect()
}

/// A limb hanging limp while the body moves on (F038923 / F038930): the ragdoll with every
/// other body held to the animation, the limb's bones (and those under them) from it.
#[derive(Component)]
struct LimpLimbs {
    doll: Ragdoll,
    rig: Arc<RagdollRig>,
    /// The flags (RagdollPart: right arm 2, left arm 4, head 8, right leg 0x10, left leg 0x20)
    /// and the bones they free.
    mask: u32,
    bones: Vec<usize>,
}

/// @fn F043917
///
/// A severed limb dropped with physics on (Item +0x1da, F043877): its item's "physics file" made a
/// moving actor (F002750c with static off), in collision group 3, or 4 in a building (+0x1d8 a
/// building): it falls, rolls, meets the buildings and the ragdolls, and comes to rest.
struct LyingLimb {
    /// Its ground item.
    id: u64,
    body: kenshi_physics::ragdoll::Ragdoll,
    template: Arc<kenshi_physics::ragdoll::Template>,
    inside: bool,
}

/// A ragdoll stepped this frame, posed after (`ragdolls`): its body, the root's parent, the
/// pose for the bones without a body, the bones it poses (none: all), and the floor it lies on
/// inside (else the ground).
struct Stepped {
    e: Entity,
    id: usize,
    rp: Affine3A,
    locals: Vec<Transform>,
    only: Vec<usize>,
    floor: Option<f32>,
    /// In a building: the ragdoll in collision group 4, else 3 (F038930 / F044968).
    inside: bool,
    /// The full ragdoll of one knocked out (AnimationClass ragdoll flag 1, its +0x59), not a
    /// carried one or limp limbs.
    limp: bool,
}


/// The carried ragdoll request (F039247: Character::setCarried asks AnimationClass::setRagdoll,
/// F038930, for part flags 0x800; F044931 marks the ragdoll +0x5a) as a `LimpLimbs` mask.
const CARRIED_RAGDOLL: u32 = 0x800;
/// F038923 for a carried human (Character vtable 0x248 answering none): every ragdoll body
/// follows the animation but these bones' (+0x31 clear: physics drives them, F044967).
const CARRIED_FREE: [&str; 3] = ["Bip01 L Forearm", "Bip01 R Forearm", "Bip01 Head"];

/// The bones named in `freed` and every bone under them, less the bones named in `kept` and
/// everything under those.
fn subtrees(names: &[String], parents: &[Option<usize>], freed: &[&str], kept: &[&str]) -> Vec<usize> {
    let grow = |mut set: Vec<usize>| {
        let mut grew = true;
        while grew {
            grew = false;
            for (k, par) in parents.iter().enumerate() {
                if let Some(pk) = par
                    && set.contains(pk)
                    && !set.contains(&k)
                {
                    set.push(k);
                    grew = true;
                }
            }
        }
        set
    };
    let pick = |list: &[&str]| -> Vec<usize> { names.iter().enumerate().filter(|(_, n)| list.contains(&n.as_str())).map(|(k, _)| k).collect() };
    let keep = grow(pick(kept));
    let mut bones = grow(pick(freed));
    bones.retain(|b| !keep.contains(b));
    bones
}

/// @fn F039240
///
/// Where a carried body hangs from its carrier's carry arm bone (Character::updateCarried):
/// at the bone's place plus, turned by the bone, an offset by the carried body's height (its
/// AppearanceBase +0x18c, F037429: ("Height" - 80) x 2.5 x 0.01, doubled): from (-1.4, 0, 1)
/// at 80 through (-1.9, -1, 2.6) at 100 to (-3, -0.4, 5) at 120 (and on beyond, linearly).
fn carry_offset(height: f32) -> Vec3 {
    let f = (height - 80.0) * 2.5 * 0.01 * 2.0;
    let (low, mid, high) = (Vec3::new(-1.4, 0.0, 1.0), Vec3::new(-1.9, -1.0, 2.6), Vec3::new(-3.0, -0.4, 5.0));
    if 1.0 <= f {
        let t = f - 1.0;
        mid * (1.0 - t) + high * t
    } else {
        low * (1.0 - f) + mid * f
    }
}

/// F038904: the carried body's turn from its carrier's carry bone: a quarter turn about x
/// (Ogre Quaternion(0.7071, 0.7071, 0, 0)), then 14 degrees about z ((0.99255, 0, 0, 0.12187)).
fn carry_turn() -> Quat {
    Quat::from_xyzw(0.707_106_77, 0.0, 0.0, 0.707_106_77) * Quat::from_xyzw(0.0, 0.0, 0.121_869_3, 0.992_546_1)
}

/// Knocked-out people go limp (Character::setRagdoll, F039259): their race's ragdoll takes over
/// from the pose last drawn and falls under gravity. Getting up, the body plays "standing up 3"
/// (F037164) with the ragdoll's pose blended out, from where the body lies. A carried body is
/// posed by its carrier.
#[allow(clippy::type_complexity)]
fn ragdolls(
    mut commands: Commands,
    mut game: ResMut<Game>,
    time: Res<Time>,
    mut bodies: Query<(Entity, &Animated, Option<&mut Limp>, Option<&mut LimpLimbs>)>,
    joints: Query<(&Transform, &GlobalTransform), Without<Animated>>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    mut nodes: Query<&mut Transform, With<Animated>>,
) {
    let dt = if game.paused { 0.0 } else { time.delta_secs() * game.speed };
    let origin = game.zone.origin;
    // people getting up where their body came to rest: id, the pelvis (scene units), the way it
    // now faces and the body's node
    let mut rest: Vec<(usize, Vec3, Quat, Affine3A)> = Vec::new();
    // the ragdolls to step this frame, all together (they collide with each other: F045103
    // leaves ragdoll groups 3 and 4 colliding; the people on their feet have no PhysX actor)
    let mut stepped: Vec<Stepped> = Vec::new();
    for p in &game.sim.folk {
        let Some(&e) = game.bodies.get(p.id) else { continue };
        let Ok((_, anim, limp, limbs)) = bodies.get_mut(e) else { continue };
        let root = anim.rig.parents.iter().position(Option::is_none).unwrap_or(0);
        let root_parent = || {
            let up = parents.get(*anim.joints.get(root)?).ok()?.parent();
            Some(globals.get(up).ok()?.affine())
        };
        // carried (F038923 for the 0x800 request, F039247): the race's ragdoll with every body
        // held to the animation ("carry me", posed where `place_people` hangs the body) but the
        // forearms' and the head's, which hang free and swing as the carrier moves (their bones
        // and those under them from the ragdoll)
        if p.carried_by.is_some() {
            if limp.is_some() {
                commands.entity(e).remove::<Limp>();
            }
            let posed: Option<Vec<(Affine3A, Transform)>> = anim.joints.iter().map(|&j| joints.get(j).ok().map(|(t, g)| (g.affine(), *t))).collect();
            let (Some(posed), Some(rp)) = (posed, root_parent()) else { continue };
            let world_bones: Vec<Affine3A> = posed.iter().map(|x| x.0).collect();
            let locals: Vec<Transform> = posed.iter().map(|x| x.1).collect();
            match limbs {
                Some(mut l) if l.mask == CARRIED_RAGDOLL => {
                    let rig = l.rig.clone();
                    for i in 0..rig.body_count() {
                        let freed = l.bones.iter().any(|&k| rig.body_of_bone(k) == Some(i));
                        if !freed && let Some((x, q)) = rig.body_pose(i, &world_bones) {
                            l.doll.pin(i, x, q);
                        }
                    }
                    stepped.push(Stepped { e, id: p.id, rp, locals, only: l.bones.clone(), floor: None, inside: p.inside.is_some(), limp: false });
                }
                _ => {
                    let Some(rig) = game.body_outfit.get(p.id).and_then(|&k| game.ragdoll_rigs.get(k)?.clone()) else { continue };
                    let bones = subtrees(&anim.rig.names, &anim.rig.parents, &CARRIED_FREE, &[]);
                    let doll = rig.start(&world_bones, Vec3::ZERO);
                    commands.entity(e).insert(LimpLimbs { doll, rig, mask: CARRIED_RAGDOLL, bones });
                }
            }
            continue;
        }
        // put down (F039236 / F039247 with 0: the 0x800 request dropped): knocked out, the full
        // ragdoll carries on from the carried one as it hangs (its bodies' places and speeds),
        // the held ones let go
        if let Some(l) = limbs.as_ref().filter(|l| l.mask == CARRIED_RAGDOLL) {
            let rig = l.rig.clone();
            let mut doll = l.doll.clone();
            commands.entity(e).remove::<(LimpLimbs, PoseOverride)>();
            if p.down {
                doll.unpin();
                let locals: Option<Vec<Transform>> = anim.joints.iter().map(|&j| joints.get(j).ok().map(|(t, _)| *t)).collect();
                if let Some(locals) = locals {
                    commands.entity(e).insert(Limp { doll, rig, locals, up: None, hand_over: None });
                }
            }
            continue;
        }
        match (p.down, limp) {
            (true, None) => {
                let Some(rig) = game.body_outfit.get(p.id).and_then(|&k| game.ragdoll_rigs.get(k)?.clone()) else {
                    continue;
                };
                let posed: Option<Vec<(Affine3A, Transform)>> = anim
                    .joints
                    .iter()
                    .map(|&j| joints.get(j).ok().map(|(t, g)| (g.affine(), *t)))
                    .collect();
                let Some(posed) = posed else { continue };
                let world: Vec<Affine3A> = posed.iter().map(|x| x.0).collect();
                // the pose last drawn must be the body's here (not from before a move)
                let here = Vec3::from(p.mover.position_units()) - origin;
                if world.get(root).is_none_or(|w| Vec3::from(w.translation).distance(here) > 40.0) {
                    continue;
                }
                let locals: Vec<Transform> = posed.iter().map(|x| x.1).collect();
                // knocked out by a blow just now: the blow's knockback throws the body.
                // (assumption) the knockback is a force over one frame (1/60 s) on a 75 kg body,
                // in m/s x 10 for units, at most 80 units/s
                let fighter = &game.sim.arena.f[p.id];
                let push = if game.sim.arena.sh.t - fighter.knock_at < 1.0 {
                    (Vec3::from(fighter.knock) * (10.0 / (60.0 * 75.0))).clamp_length_max(80.0)
                } else {
                    Vec3::ZERO
                };
                let doll = rig.start(&world, push);
                commands.entity(e).insert(Limp { doll, rig, locals, up: None, hand_over: None });
            }
            (true, Some(mut l)) => {
                if l.up.take().is_some() {
                    l.doll.wake();
                }
                // the terrain and the buildings' collision; inside a building (ragdoll group 4,
                // F039138) not the terrain (group 2, F045103): its floors hold it
                let floor = p.inside.map(|_| f32::MIN);
                let Some(rp) = root_parent() else { continue };
                stepped.push(Stepped { e, id: p.id, rp, locals: l.locals.clone(), only: Vec::new(), floor, inside: p.inside.is_some(), limp: true });
            }
            (false, Some(mut l)) => {
                // F037164: prone (the legs collapsed), it comes round crawling
                let prone = game.sim.arena.f.get(p.id).is_some_and(|f| f.prone());
                let clip = anim.find(if prone { "crawl idle down" } else { "standing up 3" });
                if l.up.is_none() {
                    // @fn F044963
                    //
                    // Task_GetUp's start (F031044 -> F039096): the ragdoll goes and the body's
                    // node turns to face from its root bone toward "Bip01 Head" across the ground
                    // (the root bone turned back, so the pose stays), standing where the pelvis
                    // lies (F038821); each bone the clip moves is then held, from its pose now to
                    // the clip's first key over `GET_UP_BLEND` (F044960)
                    let Some(rp) = root_parent() else { continue };
                    let Ok(body) = globals.get(e).map(|g| g.affine()) else { continue };
                    let Some((pelvis, _)) = l.doll.poses().next() else { continue };
                    let lying = l.rig.locals(&l.doll, rp, &anim.rig.parents, &l.locals);
                    let world = world_bones(&lying, &anim.rig.parents, rp);
                    let head = anim.rig.names.iter().position(|n| n == "Bip01 Head");
                    let dir = head.map_or(Vec3::ZERO, |h| Vec3::from(world[h].translation - world[root].translation));
                    let (scale, turn, _) = body.to_scale_rotation_translation();
                    let turn = if dir.x * dir.x + dir.z * dir.z > 1e-6 { Quat::from_rotation_y(dir.x.atan2(dir.z)) } else { turn };
                    // the character's place: where the ragdoll's root lay (F044942), put on the
                    // floor under it as it came round (`Sim::wake`)
                    let stand = Vec3::from(p.mover.position_units()) - origin;
                    let new_body = Affine3A::from_scale_rotation_translation(scale, turn, stand);
                    let rp_new = new_body * body.inverse() * rp;
                    let from = l.rig.locals(&l.doll, rp_new, &anim.rig.parents, &l.locals);
                    let (to, bones) = match clip {
                        Some(i) => {
                            let c = &anim.library[i];
                            let to: Vec<Transform> = anim
                                .rig
                                .sample(c, 0.0)
                                .iter()
                                .map(|x| Transform { translation: x.translation, rotation: x.rotation, scale: x.scale })
                                .collect();
                            let bones = (0..anim.rig.handles.len()).filter(|&k| c.tracks.iter().any(|t| t.bone == anim.rig.handles[k])).collect();
                            (to, bones)
                        }
                        None => (from.clone(), Vec::new()),
                    };
                    l.hand_over = Some(HandOver { from, to, bones });
                    rest.push((p.id, pelvis, turn, new_body));
                }
                let up = l.up.unwrap_or(0.0) + dt;
                l.up = Some(up);
                let len = clip.map_or(0.0, |i| anim.library[i].length);
                if up >= len.max(GET_UP_BLEND) || (p.getting_up <= 0.0 && p.speed > 5.0) {
                    commands.entity(e).remove::<(Limp, PoseOverride)>();
                    continue;
                }
                // the clip runs from the start (F038905 updates every playing clip), the held
                // bones hidden from it until the hand-over is done
                if let Some(i) = clip {
                    commands.entity(e).insert(Blend(vec![(i, up, 1.0)]));
                }
                match l.hand_over.as_ref().filter(|_| up < GET_UP_BLEND) {
                    Some(h) => {
                        let f = up / GET_UP_BLEND;
                        let mut locals = h.from.clone();
                        for &k in &h.bones {
                            let (a, b) = (h.from[k], h.to[k]);
                            locals[k] = Transform { translation: a.translation.lerp(b.translation, f), rotation: nlerp(a.rotation, b.rotation, f), scale: a.scale };
                        }
                        commands.entity(e).insert(PoseOverride { locals, weight: 1.0, only: h.bones.clone() });
                    }
                    None => {
                        commands.entity(e).remove::<PoseOverride>();
                    }
                }
            }
            (false, None) => {
                // F038923 / F038930: a disabled limb (its part at no health, still on) hangs: an
                // arm's upper arm and forearm (and the hand under them) from the ragdoll, its
                // clavicle from the animation; a leg's thigh and calf, its foot from the
                // animation; the head's neck and head. (assumption, the callers not found) A part
                // at no health sets its RagdollPart flag (right arm 2, left arm 4, head 8, right
                // leg 0x10, left leg 0x20)
                let f = &game.sim.arena.f[p.id];
                let out = |name: &str, slot: Option<usize>| {
                    !slot.is_some_and(|k| game.sim.limb_lost(p.id, k))
                        && f.medical.parts.iter().any(|q| game.sim.data.get(&q.id).is_some_and(|r| r.name.to_lowercase() == name) && q.health() <= 0.0)
                };
                let mask = if out("left arm", Some(0)) { 4 } else { 0 }
                    | if out("right arm", Some(1)) { 2 } else { 0 }
                    | if out("head", None) { 8 } else { 0 }
                    | if out("right leg", Some(3)) { 0x10 } else { 0 }
                    | if out("left leg", Some(2)) { 0x20 } else { 0 };
                if mask == 0 || p.carried_by.is_some() {
                    if limbs.is_some() {
                        commands.entity(e).remove::<(LimpLimbs, PoseOverride)>();
                    }
                    continue;
                }
                let posed: Option<Vec<(Affine3A, Transform)>> = anim.joints.iter().map(|&j| joints.get(j).ok().map(|(t, g)| (g.affine(), *t))).collect();
                let Some(posed) = posed else { continue };
                let world_bones: Vec<Affine3A> = posed.iter().map(|x| x.0).collect();
                let locals: Vec<Transform> = posed.iter().map(|x| x.1).collect();
                let Some(rp) = root_parent() else { continue };
                match limbs {
                    Some(mut l) if l.mask == mask => {
                        // the rest held where the animation has it
                        let rig = l.rig.clone();
                        for i in 0..rig.body_count() {
                            let freed = l.bones.iter().any(|&k| rig.body_of_bone(k) == Some(i));
                            if !freed && let Some((x, q)) = rig.body_pose(i, &world_bones) {
                                l.doll.pin(i, x, q);
                            }
                        }
                        stepped.push(Stepped { e, id: p.id, rp, locals, only: l.bones.clone(), floor: None, inside: p.inside.is_some(), limp: false });
                    }
                    _ => {
                        let Some(rig) = game.body_outfit.get(p.id).and_then(|&k| game.ragdoll_rigs.get(k)?.clone()) else { continue };
                        let mut freed: Vec<&str> = Vec::new();
                        if mask & 4 != 0 {
                            freed.extend(["Bip01 L UpperArm", "Bip01 L Forearm"]);
                        }
                        if mask & 2 != 0 {
                            freed.extend(["Bip01 R UpperArm", "Bip01 R Forearm"]);
                        }
                        if mask & 8 != 0 {
                            freed.extend(["Bip01 Neck", "Bip01 Head"]);
                        }
                        if mask & 0x10 != 0 {
                            freed.extend(["Bip01 R Thigh", "Bip01 R Calf"]);
                        }
                        if mask & 0x20 != 0 {
                            freed.extend(["Bip01 L Thigh", "Bip01 L Calf"]);
                        }
                        // (kept to the animation: a leg's foot and what is under it)
                        let bones = subtrees(&anim.rig.names, &anim.rig.parents, &freed, &["Bip01 L Foot", "Bip01 R Foot"]);
                        if std::env::var("KENSHI_LIMP_DEBUG").is_ok() {
                            eprintln!("limp limbs: {} mask {mask:#x}, {} bones: {:?}", p.id, bones.len(), bones.iter().map(|&k| anim.rig.names[k].as_str()).collect::<Vec<_>>());
                        }
                        let doll = rig.start(&world_bones, Vec3::ZERO);
                        commands.entity(e).insert(LimpLimbs { doll, rig, mask, bones });
                    }
                }
            }
        }
    }
    // F040471: each limb severed this tick dropped at its part's "bone name 2" bone, as the bone
    // lies (its place and turn)
    for (gid, id, part) in std::mem::take(&mut game.sim.limb_drops) {
        let Some(g) = game.sim.ground.iter().find(|g| g.id == gid) else { continue };
        let record = g.item.record.clone();
        let template = {
            let file = game.sim.data.get(&record).and_then(|r| r.files.get("physics file").cloned()).filter(|f| !f.is_empty());
            let Some(file) = file else { continue };
            let gm: &mut Game = &mut game;
            let res = &gm.res;
            gm.limb_bodies
                .entry(file.clone())
                .or_insert_with(|| {
                    let path = res.path(&file)?;
                    let text = std::fs::read_to_string(path).ok()?;
                    let b = kenshi_io::nxu::parse_bodies(&text).into_iter().next()?;
                    Some(Arc::new(kenshi_physics::ragdoll::Template::from_nxu(&b)))
                })
                .clone()
        };
        if std::env::var("KENSHI_RAGDOLL_DEBUG").is_ok() {
            eprintln!("limb {gid} dropped by {id} (part {part}): body {}", template.is_some());
        }
        let Some(template) = template else { continue };
        let bone_name = game.sim.arena.f[id].medical.parts.get(part).and_then(|q| game.sim.data.get(&q.id)).and_then(|r| r.strings.get("bone name 2").cloned());
        let Some(&e) = game.bodies.get(id) else { continue };
        let Ok((_, anim, ..)) = bodies.get(e) else { continue };
        let Some(k) = bone_name.and_then(|n| anim.rig.names.iter().position(|b| *b == n)) else { continue };
        let Some(bone) = anim.joints.get(k).and_then(|&j| globals.get(j).ok()).map(|g| g.affine()) else { continue };
        let (_, turn, at) = bone.to_scale_rotation_translation();
        let com = Vec3::from(at) + turn * template.bodies[0].bind;
        let body = kenshi_physics::ragdoll::Ragdoll::new(&[(com, turn)], &[]);
        let inside = game.sim.folk[id].inside.is_some();
        game.limbs_lying.push(LyingLimb { id: gid, body, template, inside });
    }
    // picked up: gone
    let present: std::collections::HashSet<u64> = game.sim.ground.iter().map(|g| g.id).collect();
    game.limbs_lying.retain(|l| present.contains(&l.id));
    let mut lying = std::mem::take(&mut game.limbs_lying);
    // every ragdoll stepped at once, then each body posed from its own
    let at: std::collections::HashMap<Entity, usize> = stepped.iter().enumerate().map(|(k, s)| (s.e, k)).collect();
    let mut dolls: Vec<(usize, &mut kenshi_physics::ragdoll::Ragdoll, Arc<RagdollRig>, Vec<Option<usize>>)> = Vec::new();
    for (e, anim, limp, limbs) in bodies.iter_mut() {
        let Some(&k) = at.get(&e) else { continue };
        let parents_of = anim.rig.parents.clone();
        if let Some(l) = limp {
            let l = l.into_inner();
            dolls.push((k, &mut l.doll, l.rig.clone(), parents_of.clone()));
        } else if let Some(l) = limbs {
            let l = l.into_inner();
            dolls.push((k, &mut l.doll, l.rig.clone(), parents_of.clone()));
        }
    }
    dolls.sort_by_key(|d| d.0);
    // @fn F044968
    //
    // in the water: a knocked-out body of a race that swims (race +0x79) whose root body is at or
    // under the water's surface (world 100, F050961) has each actor at or under it pushed up at
    // sqrt(100 - its height + 1) + 98.1 units / s^2 (addForce, NX_ACCELERATION: gravity undone
    // and more the deeper it is), and its settling (F044957) runs at half the time
    let mut wet = vec![false; dolls.len()];
    for (m, d) in dolls.iter_mut().enumerate() {
        let st = &stepped[d.0];
        let race = game.sim.race_of(st.id);
        let swims = race.and_then(|r| game.sim.data.get(&r)).and_then(|r| r.bools.get("swims").copied()).unwrap_or(true);
        let Some((root, _)) = d.1.poses().next() else { continue };
        if !st.limp || !swims || root.y + origin.y > kenshi_game::sim::WATER_LEVEL {
            continue;
        }
        wet[m] = true;
        if std::env::var("KENSHI_RAGDOLL_DEBUG").is_ok() && (game.sim.t * 2.0).fract() < dt * 2.0 {
            eprintln!("ragdoll {} in the water: root at world height {:.1}", st.id, root.y + origin.y);
        }
        for i in 0..d.2.template.bodies.len() {
            if let Some(at) = d.1.actor_position(&d.2.template, i)
                && at.y + origin.y <= kenshi_game::sim::WATER_LEVEL
            {
                d.1.accelerate(i, Vec3::Y * ((kenshi_game::sim::WATER_LEVEL - (at.y + origin.y) + 1.0).sqrt() + 98.1));
            }
        }
    }
    {
        let g = &*game;
        let worlds: Vec<_> = dolls
            .iter()
            .map(|d| {
                let floor = stepped[d.0].floor;
                let ignored = kenshi_physics::world::ragdoll_ignores(stepped[d.0].inside);
                kenshi_physics::world::Scene { ground: move |x: f32, z: f32| floor.unwrap_or_else(|| g.ground(x, z)), statics: &g.statics, ignored }
            })
            .collect();
        let worlds: Vec<&dyn kenshi_physics::world::Surroundings> = worlds.iter().map(|w| w as &dyn kenshi_physics::world::Surroundings).collect();
        // and the limbs lying about, in the ragdolls' groups
        let limb_worlds: Vec<_> = lying
            .iter()
            .map(|l| {
                let inside = l.inside;
                kenshi_physics::world::Scene { ground: move |x: f32, z: f32| if inside { f32::MIN } else { g.ground(x, z) }, statics: &g.statics, ignored: kenshi_physics::world::ragdoll_ignores(inside) }
            })
            .collect();
        let limb_templates: Vec<Arc<kenshi_physics::ragdoll::Template>> = lying.iter().map(|l| l.template.clone()).collect();
        let mut worlds = worlds;
        worlds.extend(limb_worlds.iter().map(|w| w as &dyn kenshi_physics::world::Surroundings));
        let rigs: Vec<Arc<RagdollRig>> = dolls.iter().map(|d| d.2.clone()).collect();
        let mut templates: Vec<&kenshi_physics::ragdoll::Template> = rigs.iter().map(|r| &r.template).collect();
        templates.extend(limb_templates.iter().map(|t| &**t));
        let mut refs: Vec<&mut kenshi_physics::ragdoll::Ragdoll> = dolls.iter_mut().map(|d| &mut *d.1).collect();
        refs.extend(lying.iter_mut().map(|l| &mut l.body));
        kenshi_physics::ragdoll::Ragdoll::step_together(&mut refs, &templates, dt, &worlds);
    }
    // the severed limbs' items where their bodies lie (the item's node is the actor's, F043917)
    for l in &lying {
        let Some(at) = l.body.actor_position(&l.template, 0) else { continue };
        let turn = l.body.poses().next().map_or(Quat::IDENTITY, |p| p.1);
        let w = at + origin;
        if let Some(g) = game.sim.ground.iter_mut().find(|g| g.id == l.id) {
            g.at = w.to_array();
            let f = turn * Vec3::Z;
            g.yaw = f.x.atan2(f.z);
        }
        game.limb_turns.insert(l.id, turn);
        // (testing) KENSHI_RAGDOLL_DEBUG: the limbs lying about
        if std::env::var("KENSHI_RAGDOLL_DEBUG").is_ok() && (game.sim.t * 2.0).fract() < dt * 2.0 {
            eprintln!("limb {} asleep {} at {at:?} speeds {:?}", l.id, l.body.asleep, l.body.speeds());
        }
    }
    game.limbs_lying = lying;
    // F044957: a knocked-out body put to sleep once it has lain a while over a floor
    let mut settled = Vec::new();
    for (m, d) in dolls.iter_mut().enumerate() {
        if !stepped[d.0].limp {
            continue;
        }
        let g = &*game;
        let below = |from: Vec3| -> f32 {
            let mask = kenshi_physics::ragdoll::SETTLE_RAY_GROUPS;
            // the terrain (group 2), else the solids of the groups the ray takes
            let terrain = Some(g.ground(from.x, from.z)).filter(|&y| y <= from.y);
            let solid = g.statics.raycast_groups(from, Vec3::NEG_Y, 1.0e6, mask).map(|t| from.y - t);
            terrain.into_iter().chain(solid).fold(None, |b: Option<f32>, y| Some(b.map_or(y, |b| b.max(y)))).unwrap_or(kenshi_physics::ragdoll::SETTLE_NO_HIT - origin.y)
        };
        if d.1.kenshi_settle(if wet[m] { dt * 0.5 } else { dt }, below) {
            settled.push(m);
        }
    }
    for m in settled {
        let r = game.sim.rng.next();
        dolls[m].1.settle_again(r);
    }
    let mut limp_at: Vec<(usize, Vec3)> = Vec::new();
    for (k, doll, rig, parents_of) in &dolls {
        let st = &stepped[*k];
        if game.sim.folk[st.id].down
            && game.sim.folk[st.id].carried_by.is_none()
            && let Some((x, _)) = doll.poses().next()
        {
            limp_at.push((st.id, x));
        }
        let out = rig.locals(doll, st.rp, parents_of, &st.locals);
        // (testing) KENSHI_RAGDOLL_DEBUG: each ragdoll's state twice a game second
        if std::env::var("KENSHI_RAGDOLL_DEBUG").is_ok() && (game.sim.t * 2.0).fract() < dt * 2.0 {
            eprintln!("ragdoll {} asleep {} speeds {:?} pelvis {:?} ground {:.1}", st.id, doll.asleep, doll.speeds(), doll.poses().next().map(|x| x.0), doll.poses().next().map_or(0.0, |x| game.ground(x.0.x, x.0.z)));
        }
        commands.entity(st.e).insert(PoseOverride { locals: out, weight: 1.0, only: st.only.clone() });
    }
    // @fn F044942
    //
    // while limp the character is where its ragdoll's root body (the pelvis) is (Character's
    // update, F039249)
    for (id, x) in limp_at {
        let at = [(x.x + origin.x) / 10.0, (x.y + origin.y) / 10.0, (x.z + origin.z) / 10.0];
        game.sim.folk[id].mover.position = at;
        game.sim.arena.f[id].mover.position = at;
    }
    // getting up, facing the way the body lies (F044960 sets the character's direction from the
    // node's turn)
    for (id, _, turn, node) in rest {
        game.sim.folk[id].facing = (turn * Vec3::Z).to_array();
        if let Some(&e) = game.bodies.get(id)
            && let Ok(mut tr) = nodes.get_mut(e)
        {
            let (_, r, t) = node.to_scale_rotation_translation();
            tr.rotation = r;
            tr.translation = t;
        }
    }
}

/// Put every body where the simulation has it and choose what it plays.
#[allow(clippy::too_many_arguments)]
fn place_people(
    mut game: ResMut<Game>,
    mut bodies: Query<(&mut Transform, &mut Animated)>,
    globals: Query<&GlobalTransform>,
    mut commands: Commands,
    time: Res<Time>,
    weather: Option<Res<WeatherState>>,
) {
    let wind = weather.as_ref().filter(|w| w.current.is_some()).map_or(Vec2::ZERO, |w| w.wind);
    let origin = game.zone.origin;
    let game = &mut *game;
    // the frame's game time (animations run at the game speed, stop when paused)
    let dt = if game.paused { 0.0 } else { time.delta_secs() * game.speed };
    let blend_rate = game.sim.arena.blend_rate;
    while game.walk_layers.len() < game.sim.folk.len() {
        game.walk_layers.push(kenshi_combat::arena::base_layers(blend_rate));
    }
    // the animals' animation libraries (F038935 over their ANIMAL_CHARACTER's "animations")
    // and clip lengths, once per kind
    for p in &game.sim.folk {
        let (Some(c), Some(r)) = (p.character.as_ref(), p.race.as_ref()) else { continue };
        if game.animal_anims.contains_key(c) {
            continue;
        }
        let data = &game.sim.data;
        let lib = data.get(c).map_or_else(kenshi_anim::select::Library::default, |a| kenshi_anim::select::Library::animal(data, a));
        let lengths = data
            .get(r)
            .and_then(|race| kenshi_combat::arena::skeleton_clips(&game.res, race))
            .map(|(l, _)| l)
            .unwrap_or_default();
        if std::env::var("KENSHI_ANIMAL_DEBUG").is_ok() {
            let refs = data.get(c).map(|a| a.refs("animations").len()).unwrap_or(0);
            eprintln!("animal kind {c}: {refs} animation refs, {} idles, {} moves, {} clips", lib.idles.len(), lib.lower_moves.len(), lengths.len());
        }
        game.animal_anims.insert(c.clone(), Arc::new((lib, lengths)));
    }
    let sim = &game.sim;
    let lib = &sim.arena.sh.library;
    // each carrier's carry bone as last drawn (F039248: the carried body's AnimationClass +0x1b0,
    // "Bip01 L UpperArm" for a carrier carrying on the left, +0x378, which F040018 always sets;
    // "Bip01 R UpperArm" otherwise): its place and turn as F039240 reckons them, the carrier's
    // node place and turn (vtable 0x40 / 0xc0) with the bone's derived place turned by the node
    // (the node's scale left out, F038830) and the node's turn times the bone's derived one
    // (F038829)
    let carry_bones: std::collections::HashMap<usize, (Vec3, Quat)> = sim
        .folk
        .iter()
        .filter(|p| p.carrying.is_some())
        .filter_map(|p| {
            let body = *game.bodies.get(p.id)?;
            let (node, a) = bodies.get(body).ok()?;
            let arm = a.rig.names.iter().position(|n| n == "Bip01 L UpperArm")?;
            let bone = globals.get(*a.joints.get(arm)?).ok()?.affine();
            let own = globals.get(body).ok()?.affine();
            let (derived_s, derived_q, derived_x) = (own.inverse() * bone).to_scale_rotation_translation();
            if std::env::var("KENSHI_CARRY_DEBUG").is_ok() && sim.tick.is_multiple_of(30) {
                eprintln!("carry bone of {} (female {:?}, carrying {:?}): scale {derived_s:?} turn {derived_q:?} at {derived_x:?}; node scale {:?}", p.id, game.looks.get(&p.id).map(|l| l.female), p.carrying, node.scale);
            }
            Some((p.id, (node.translation + node.rotation * derived_x, node.rotation * derived_q)))
        })
        .collect();
    // each body's clips this frame, finished after the loop (crossfade, head turning)
    let mut pending: Vec<(Entity, usize, u8, Vec<(usize, f32, f32)>)> = Vec::new();
    for p in &sim.folk {
        let Some(&e) = game.bodies.get(p.id) else {
            continue;
        };
        let Ok((mut tr, mut anim)) = bodies.get_mut(e) else {
            continue;
        };
        let f = &sim.arena.f[p.id];
        // an animal is drawn at its size (CharacterAnimal vtable 0x390, F039296: lerp(growth,
        // "scale min", "scale max")); people at 1
        let size = if p.race.is_some() { f.body.size } else { 1.0 };
        let mut pos = Vec3::from(p.mover.position_units()) - origin;
        // a swimmer floats at the surface (kenshi_game::sim::SWIM_HEIGHT)
        if let Some(y) = p.float_y {
            pos.y = y - origin.y;
        }
        let pos = game.stage.filter(|(id, _)| *id == p.id).map_or(pos, |(_, at)| at);
        if std::env::var("KENSHI_GROUND_DEBUG").is_ok() && p.player && p.id == 0 {
            let g = game.ground(pos.x, pos.z);
            eprintln!("ground {:.1} body {:.1} diff {:.1} speed {:.0}", g, pos.y, pos.y - g, p.speed);
        }
        // facing: toward the target while fighting, else the way last moved
        let facing = if f.active && !p.down {
            let o = f.focus.or(f.combat.target).or_else(|| {
                (0..sim.arena.f.len()).find(|&o| o != p.id && opponent(f, &sim.arena.f[o]))
            });
            o.map(|o| Vec3::from(sim.arena.f[o].pos()) - Vec3::from(f.pos()))
        } else {
            None
        }
        .or(Some(Vec3::from(p.facing)))
        .filter(|v| v.x * v.x + v.z * v.z > 1e-6);
        // F038847: the body turns toward its facing by at most 2 x dt of the quaternions' half
        // angle a frame (4 radians a second), five times that out of combat (20)
        let target = facing.map_or(tr.rotation, |v| Quat::from_rotation_y(v.x.atan2(v.z)));
        // F038816: an animal turns at its "turn rate" (AnimationClassAnimal +0x300): dt x rate x
        // 0.1 of the quaternions' half angle a frame (0.2 x rate radians a second)
        let animal_turn = p.character.as_ref().filter(|_| p.race.is_some()).and_then(|c| sim.data.get(c)).and_then(|a| a.ints.get("turn rate").copied());
        let step = match animal_turn {
            Some(r) => r as f32 * 0.2 * dt,
            None => (if f.active { 4.0 } else { 20.0 }) * dt,
        };
        let angle = tr.rotation.angle_between(target);
        let rot = if angle <= step || angle.is_nan() {
            target
        } else {
            tr.rotation.slerp(target, step / angle)
        };
        if let Some(c) = p.carried_by {
            // @fn F038904
            //
            // carried (F039240 each frame): playing "carry me" (category 3, being carried), the
            // body's node at the carrier's carry bone plus the height's offset turned by the
            // bone, and turned as the bone then `carry_turn`; until the carrier has been drawn
            // once, at the carrier's place
            if let Some(i) = anim.find("carry me") {
                pending.push((e, p.id, KIND_CARRIED, vec![(i, sim.t, 1.0)]));
            }
            match carry_bones.get(&c) {
                Some(&(at, turn)) => {
                    let height = game.body_values.get(&p.id).and_then(|v| v.get("Height")).copied().unwrap_or(100.0);
                    *tr = Transform { translation: at + turn * carry_offset(height), rotation: turn * carry_turn(), scale: tr.scale };
                }
                None => {
                    *tr = Transform::from_translation(Vec3::from(sim.folk[c].mover.position_units()) - origin).with_scale(tr.scale);
                }
            }
            continue;
        }
        if p.down {
            // (render stand-in until ragdolls are ported) the stealth knockout, held at its end
            let len = sim
                .arena
                .sh
                .lengths
                .get("stealthko")
                .copied()
                .unwrap_or(1.0);
            let t = (sim.t - f.down_at).clamp(0.0, len - 0.01);
            if let Some(i) = anim.find("stealthKO") {
                pending.push((e, p.id, KIND_DOWN, vec![(i, t, 1.0)]));
            }
            *tr = Transform::from_translation(pos).with_rotation(rot).with_scale(Vec3::splat(size));
            continue;
        }
        // an animal: its own clips by state ((stand-in) the game's animal animation records,
        // ANIMATION refs, are not read): asleep when down, attacking while fighting, else by speed
        // an animal's move: its "attack type" technique's clip on its own skeleton, at the
        // arena's clip time
        if p.race.is_some()
            && !p.down
            && let Some(act) = &f.act
            && let Some(i) = anim.find(&act.clip.name)
        {
            // a lunge moves it by its root (the arena's root motion): the clip's own travel so
            // far is taken back out, as the people's moves' is
            let off = f
                .roots
                .as_ref()
                .and_then(|r| r.get(&act.clip.name.to_lowercase()))
                .map_or([0.0; 2], |tr| {
                    let (r0, r1) = (root_at(tr, 0.0), root_at(tr, act.clip.time));
                    [r1[0] - r0[0], r1[1] - r0[1]]
                });
            let o = rot * Vec3::new(off[0], 0.0, off[1]) * size;
            *tr = Transform::from_translation(pos - o).with_rotation(rot).with_scale(Vec3::splat(size));
            pending.push((e, p.id, KIND_ANIMAL, vec![(i, act.clip.time, 1.0)]));
            game.playing[p.id].clear();
            continue;
        }
        if p.race.is_some()
            && let Some(al) = p.character.as_ref().and_then(|c| game.animal_anims.get(c)).cloned()
            && !(al.0.idles.is_empty() && al.0.lower_moves.is_empty())
            && !p.down
        {
            // an animal: its own ANIMAL_ANIMATION records through the ported choice (movement
            // blended by speed at each clip's matched rate, else its idle), on its base layers
            *tr = Transform::from_translation(pos).with_rotation(rot).with_scale(Vec3::splat(size));
            let (alib, lengths) = &*al;
            let mut sit = kenshi_combat::arena::situation(f);
            sit.speed = p.speed;
            sit.combat_mode = 2;
            // (assumption) animal animation records carry no leg damage ranges (all 0), so the
            // leg fit is left out for animals
            sit.ignore_legs = true;
            // F037152 / F037138: turning on the spot (its speed under its radius x the half
            // angle left to turn x 2 pi) it plays its "turning anim" at the half angle x its play
            // speed; F037179: in a fight, a step across its facing plays its strafe at its play
            // speed x the speed across ((assumption) as the people's "strafe lower")
            let half = (angle * 0.5).min(std::f32::consts::FRAC_PI_2);
            let fwd = rot * Vec3::Z;
            let v = Vec3::from(p.mover.velocity) * 10.0;
            let side = fwd.x * v.z - fwd.z * v.x;
            let across = f.active && side.abs() > 0.5 && side.abs() > (fwd.x * v.x + fwd.z * v.z).abs();
            let wanted: Vec<_> = if let Some(t) = alib.turning.as_ref().filter(|_| p.speed < p.mover.radius * 10.0 * half * std::f32::consts::TAU && half > 0.02) {
                vec![(t, half * t.play_speed, 1.0)]
            } else if let Some(s) = alib.strafes.first().filter(|_| across) {
                vec![(s, s.play_speed * side, 1.0)]
            } else if p.speed > 0.0 {
                // (moving at any speed, F037184 as the people's)
                movement(alib, &sit, 1.0, 1.0)
            } else {
                idle_anim(alib, &sit, 1.0).map(|a| vec![(a, a.play_speed, 1.0)]).unwrap_or_default()
            };
            if std::env::var("KENSHI_ANIMAL_DEBUG").is_ok() && game.sim.arena.sh.t as i32 % 3 == 0 {
                let w: Vec<_> = wanted.iter().map(|(a, r, k)| format!("{} x{r:.2} w{k:.2}", a.anim_name)).collect();
                eprintln!("animal {} speed {:.1} wanted {w:?}", p.id, p.speed);
            }
            let layers = &mut game.walk_layers[p.id];
            kenshi_combat::arena::set_base(lengths, layers, &wanted);
            kenshi_combat::arena::update_base(layers, dt);
            let blend: Vec<_> = layers
                .iter()
                .flat_map(|l| l.active.iter().chain(&l.fading))
                .filter(|c| c.weight > 0.001)
                .filter_map(|c| Some((anim.find(&c.name)?, c.time, c.weight)))
                .collect();
            if !blend.is_empty() {
                pending.push((e, p.id, KIND_ANIMAL, blend));
                game.playing[p.id].clear();
                continue;
            }
        }
        if p.race.is_some() {
            commands.entity(e).remove::<Blend>();
            *tr = Transform::from_translation(pos).with_rotation(rot).with_scale(Vec3::splat(size));
            let want = if p.down {
                "sleep"
            } else if p.speed > 40.0 {
                "run"
            } else if p.speed > 3.0 {
                "walk"
            } else {
                "idle1"
            };
            if game.playing[p.id] != want {
                if std::env::var("KENSHI_ANIMAL_DEBUG").is_ok() {
                    let names: Vec<&str> = anim.library.iter().map(|a| a.name.as_str()).collect();
                    eprintln!("animal {} wants {want} (found {:?}) of {names:?}", p.id, anim.find(want));
                }
                anim.play(want);
                game.playing[p.id] = want.to_string();
            }
            continue;
        }
        if f.active {
            // the arena's animation layers: every clip with its time and weight, the root
            // motion's in-clip travel taken back out (as the duel renderer does)
            let (base, action, _, _) = &sim.arena.layers[p.id];
            let clips: Vec<_> = base
                .iter()
                .flat_map(|l| l.active.iter().chain(&l.fading))
                .chain(&action.active)
                .chain(&action.fading)
                .filter(|c| c.weight > 0.001)
                .collect();
            let mut off = [0.0f32; 2];
            for c in clips.iter().filter(|c| c.root_motion) {
                if let Some(tr) = sim.arena.roots.get(&c.name.to_lowercase()) {
                    let (r0, r1) = (root_at(tr, 0.0), root_at(tr, c.time));
                    off[0] += c.weight * (r1[0] - r0[0]);
                    off[1] += c.weight * (r1[1] - r0[1]);
                }
            }
            let blend: Vec<_> = clips
                .iter()
                .filter_map(|c| Some((anim.find(&c.name)?, c.time, c.weight)))
                .collect();
            pending.push((e, p.id, KIND_COMBAT, blend));
            let o = rot * Vec3::new(off[0], 0.0, off[1]);
            *tr = Transform::from_translation(pos - o).with_rotation(rot);
            game.playing[p.id].clear();
            continue;
        }
        // one-shot task clips, played once from when their task starts: lifting someone onto
        // the shoulder (Task_LiftSomething F031290: "shoulder lift left", the body taken up at
        // 0.4 of it, `Person::lifting`), putting them down (Task_PutDownObject F031253:
        // "shoulder lift left" at speed -1, the body let go below 0.2 of it, F031293,
        // `Person::putting_down`; other ways of letting go (into a bed, a cage, a machine) are
        // at once in the simulation, so the clip runs on from 0.2), a stealth knockout's grab
        // (Task_StealthKO F031201 / F031279: "knockout", the KO'd one falling), lying down in
        // a bed ("gotobed") and getting up out of it (Task_GetOutOfBed F031237: "gotobed" at
        // speed -1, done below a tenth), crouching down to loot (Task_Loot F031208: "squat T",
        // then the crouched work)
        {
            let st = game.transitions.entry(p.id).or_default();
            if !st.lifting && p.lifting > 0.0 {
                st.clip = Some(("shoulder lift left", sim.t - p.lifting, false));
            } else if !st.putting_down && p.putting_down > 0.0 {
                st.clip = Some(("shoulder lift left", sim.t - p.putting_down, true));
            } else if st.carrying.is_some() && p.carrying.is_none() && !st.putting_down {
                let len = anim.find("shoulder lift left").map_or(0.0, |i| anim.library[i].length);
                st.clip = Some(("shoulder lift left", sim.t - len * 0.8, true));
            }
            if let Some(t) = st.ko
                && p.ko_target.is_none()
                && sim.folk.get(t).is_some_and(|q| q.down)
            {
                st.clip = Some(("knockout", sim.t, false));
            }
            if st.bed.is_none() && p.bed.is_some() {
                st.clip = Some(("gotobed", sim.t, false));
            } else if st.bed.is_some() && p.bed.is_none() && !p.down {
                st.clip = Some(("gotobed", sim.t, true));
            }
            if !st.looting && p.loot.is_some() {
                st.clip = Some(("squat T", sim.t, false));
            }
            (st.carrying, st.ko, st.bed, st.looting, st.lifting, st.putting_down) = (p.carrying, p.ko_target, p.bed, p.loot.is_some(), p.lifting > 0.0, p.putting_down > 0.0);
            if std::env::var("KENSHI_ANIM_DEBUG").is_ok()
                && let Some((name, start, _)) = st.clip
                && start == sim.t
            {
                eprintln!("one-shot: {} plays '{name}' at tick {}", p.id, sim.tick);
            }
            if let Some((name, start, reverse)) = st.clip {
                let at = sim.t - start;
                match anim.find(name).filter(|&i| at < anim.library[i].length) {
                    Some(i) => {
                        let at = if reverse { anim.library[i].length - at } else { at };
                        *tr = Transform::from_translation(pos).with_rotation(rot);
                        pending.push((e, p.id, KIND_ONESHOT, vec![(i, at, 1.0)]));
                        game.playing[p.id].clear();
                        continue;
                    }
                    None => st.clip = None,
                }
            }
        }
        commands.entity(e).remove::<Blend>();
        *tr = Transform::from_translation(pos).with_rotation(rot);
        // the clip: what the character is doing (its task's animation, `activity_anim`), else
        // the ported movement / idle choice for the speed
        let activity_name = activity_anim(&game, p);
        if std::env::var("KENSHI_ACTIVITY_DEBUG").is_ok() && sim.tick.is_multiple_of(600) && activity_name.is_some() {
            eprintln!("activity: {} {:?} ({})", p.id, activity_name, if activity_name.as_ref().is_some_and(|(n, _)| lib.other.iter().chain(&lib.idles).any(|a| &a.name == n || &a.anim_name == n)) { "found" } else { "missing" });
        }
        let activity = activity_name.and_then(|(name, rate)| lib.other.iter().chain(&lib.idles).find(|a| a.name == name || a.anim_name == name).map(|a| (a, rate)));
        let want: String = {
            // the character as animation choice reads it (kenshi_combat::arena::situation: its
            // wielded weapon in the right hand, its legs' health) out of combat, at its speed
            let mut sit = kenshi_combat::arena::situation(f);
            sit.speed = p.speed;
            sit.combat_mode = 0;
            // the animation category (ANIMATION "category"): 4 swimming, 5 crawling, else 0
            sit.category = movement_category(f, p.water);
            sit.stealth_mode = i32::from(p.sneaking);
            sit.carrying_left = p.carrying.is_some();
            // the weapon in the right hand only while drawn (Character +0x6d8, F039274): a
            // sheathed one walks and idles empty-handed ("walk upper", the idle stances)
            sit.weapon_r = p.weapon_out && f.wielded.category != kenshi_combat::WeaponCategory::SkillUnarmed;
            // the animation class's base layers (F037184): moving (any speed: its test is
            // |+0x180| > 0, 0x141681b38), the movement clips blended by speed, each at its
            // matched rate; standing, the best idle; crossfaded at the blend rate. (Under 5
            // a crawler at its 5 found no clip either way and showed its bind pose, standing.)
            let mut wanted: Vec<_> = if let Some((a, rate)) = activity {
                vec![(a, rate.unwrap_or(a.play_speed.max(0.01)), 1.0)]
            } else if p.speed > 0.0 {
                movement(lib, &sit, 1.0, 1.0)
            } else {
                // (assumption) sneaking, the stealth-only idle ("crouch idle") wins its tie with
                // the any-stealth idles: Kenshi's record order, which breaks ties (F037213), is
                // not known
                let sneak_idle = (sit.stealth_mode == 1)
                    .then(|| {
                        kenshi_anim::select::candidates(&lib.idles, &sit, 1.0, true)
                            .into_iter()
                            .find(|(a, _)| a.stealth_mode == 1 && a.layer != 2)
                            .map(|(a, _)| a)
                    })
                    .flatten();
                // F037172: the character's own idle stance (its body file's "idle stance",
                // F038938) when it suits: its combat and stealth modes (2: either) and the
                // situation's weapons and carrying (the candidates' strict test)
                // F021063: one with no body file of its own (the world's people) has its idle
                // stance by its personality: brave (PersonalityTags 9) "idle_stand_guard",
                // fearful (10) "idle_stand_timid", else "idle_stand_normal" (F021090's default;
                // the random pick F021063 makes first is overwritten)
                let stance = p
                    .character
                    .as_deref()
                    .and_then(|c| kenshi_game::body_file::body_of(&game.root, &sim.data, c))
                    .and_then(|b| b.idle_stance.clone())
                    .or_else(|| {
                        Some(match p.personality {
                            9 => "idle_stand_guard",
                            10 => "idle_stand_timid",
                            _ => "idle_stand_normal",
                        }.to_string())
                    })
                    .and_then(|n| lib.idles.iter().chain(&lib.other).find(|a| a.anim_name == n || a.name == n))
                    .filter(|a| (a.combat_mode == 2 || a.combat_mode == sit.combat_mode) && (a.stealth_mode == 2 || a.stealth_mode == sit.stealth_mode))
                    .filter(|a| !kenshi_anim::select::candidates(std::slice::from_ref(*a), &sit, 1.0, true).is_empty());
                sneak_idle
                    .or(stance)
                    .or_else(|| idle_anim(lib, &sit, 1.0))
                    .map(|a| vec![(a, a.play_speed, 1.0)])
                    .unwrap_or_default()
            };
            // (testing) KENSHI_ANIM_PICK=<id> (or sel, the first selected): that one's base clips each second
            let pick = std::env::var("KENSHI_ANIM_PICK").ok().and_then(|v| if v == "sel" { game.selected.first().copied() } else { v.parse::<usize>().ok() });
            if pick == Some(p.id) && sim.tick.is_multiple_of(60) {
                eprintln!("anim pick {}: speed {:.1} stealth {} weapon {} carrying {} activity {:?} -> {:?}", p.id, p.speed, sit.stealth_mode, sit.weapon_r, sit.carrying_left, activity.map(|(a, _)| a.name.clone()), wanted.iter().map(|(a, r, w)| format!("{} ({}{}) r{:.2} w{:.2}", a.name, a.anim_name, if sim.arena.sh.lengths.contains_key(&a.anim_name.to_lowercase()) { "" } else { ", NO CLIP" }, r, w)).collect::<Vec<_>>());
            }
            // (testing) KENSHI_TPOSE_DEBUG: those with no base clip (their bind pose shows)
            if wanted.is_empty() && std::env::var("KENSHI_TPOSE_DEBUG").is_ok() && sim.tick.is_multiple_of(300) {
                eprintln!("no base clip: {} speed {:.2} category {} weapon {} carrying {} stealth {} race {:?} legs {:.0}/{:.0}", p.id, p.speed, sit.category, sit.weapon_r, sit.carrying_left, sit.stealth_mode, p.race, sit.l_leg, sit.r_leg);
            }
            // a task's overlay (a turret's gunner: "combatstance" over its "stand 1", F031217)
            if let Some(n) = activity_overlay(game, p)
                && let Some(a) = lib.other.iter().chain(&lib.idles).find(|a| a.anim_name == n || a.name == n)
            {
                wanted.push((a, a.play_speed.max(0.01), 1.0));
            }
            // F037181 / F039251: the weather's animation (ANIMATION "weather type" = the
            // weather's affect type, 1 dust: "wind in face") at weight 1, standing or walking
            // (category 0, speed <= 50), when the gear gives no protection and the body faces
            // against the wind (F050491: not dot > 0)
            if sit.category == 0
                && p.speed <= 50.0
                && let Some(h) = p.hazard.as_ref().filter(|h| !h.ground && h.protection <= 0.0)
                && wind.length_squared() > 0.0
            {
                let fwd = rot * Vec3::Z;
                if fwd.x * wind.x + fwd.z * wind.y <= 0.0
                    && let Some(a) = lib.weather.iter().find(|a| a.weather_type == h.affect)
                {
                    wanted.push((a, a.play_speed.max(0.01), 1.0));
                }
            }
            // carrying someone: standing, the plain idle; F039254 marks the carrying arm busy
            // (AnimationClass +0x21e, the left: the body hangs over the left shoulder) and
            // painPosture (F037181) overlays "carrying left arm only" (by that name) on it at any
            // category and speed: the pain postures' test (category 0, speed <= 50) is only on
            // the branch without a busy arm (so a swimming carrier's is that record too, not its
            // "-swim" one)
            if p.carrying.is_some() {
                if p.speed <= 0.0
                    && wanted.is_empty()
                    && let Some(a) = lib.idles.iter().find(|a| a.anim_name == "stand 1")
                {
                    wanted.push((a, a.play_speed, 1.0));
                }
                if let Some(a) = lib.other.iter().chain(&lib.idles).find(|a| a.name == "carrying left arm only")
                {
                    wanted.push((a, a.play_speed.max(0.01), 1.0));
                }
            }
            // F037162: breathing (the overlay "breathing", "breathing noarms" with no arms) at
            // 22 units a second or less, not sneaking, crouching or in a fight; at the game's
            // one random rate (0.35 + 0.1 x a roll) and 0.95 of weight
            if p.speed <= 22.0 && !p.sneaking && !f.active && activity.is_none() {
                let armless = sim.limb_lost(p.id, 0) && sim.limb_lost(p.id, 1);
                let name = if armless { "breathing noarms" } else { "breathing" };
                if let Some(a) = lib.other.iter().find(|a| a.anim_name == name) {
                    wanted.push((a, breath_rate(), 0.95));
                }
            }
            // the most hurt part's pain posture while standing or walking (F040458, F037181)
            if kenshi_anim::select::shows_pain(sit.category, sit.speed.abs()) {
                let pain = &sim.arena.sh.pain;
                let parts = f.medical.parts.iter().filter_map(|q| {
                    let (anim, v) = pain.get(&q.id)?;
                    Some((q.health() / q.max_health(), anim.as_str(), *v))
                });
                if let Some(name) = kenshi_anim::select::pain_postures(parts).first()
                    && let Some(a) = lib.other.iter().chain(&lib.idles).find(|a| &a.anim_name == name)
                {
                    wanted.push((a, a.play_speed, 1.0));
                }
            }
            let layers = &mut game.walk_layers[p.id];
            kenshi_combat::arena::set_base(&sim.arena.sh.lengths, layers, &wanted);
            kenshi_combat::arena::update_base(layers, dt);
            let mut blend: Vec<_> = layers
                .iter()
                .flat_map(|l| l.active.iter().chain(&l.fading))
                .filter(|c| c.weight > 0.001)
                .filter_map(|c| Some((anim.find(&c.name)?, c.time, c.weight)))
                .collect();
            // (render aid) a clip just started has no weight yet: until any has, the newest
            // wanted clip poses the body alone (no bind pose for a frame)
            if blend.is_empty()
                && let Some((i, c)) = layers
                    .iter()
                    .flat_map(|l| l.active.iter())
                    .find_map(|c| Some((anim.find(&c.name)?, c)))
            {
                blend.push((i, c.time, 1.0));
            }
            if std::env::var("KENSHI_ANIM_DEBUG").is_ok() && p.id == game.selected.first().copied().unwrap_or(usize::MAX) {
                let names: Vec<String> = layers
                    .iter()
                    .enumerate()
                    .flat_map(|(li, l)| l.active.iter().chain(&l.fading).map(move |c| format!("L{li} {} t{:.2} w{:.2}", c.name, c.time, c.weight)))
                    .collect();
                let played: Vec<String> = blend.iter().map(|(i, t, w)| format!("{} t{:.2} w{:.2}", anim.library.get(*i).map_or("?", |a| a.name.as_str()), t, w)).collect();
                eprintln!("anim speed {:.1}: {} => plays {}", p.speed, names.join(" | "), played.join(" | "));
            }
            if !blend.is_empty() {
                pending.push((e, p.id, KIND_WALK, blend));
                game.playing[p.id].clear();
                continue;
            }
            // (testing) KENSHI_TPOSE_DEBUG: who is left with no clip (once each)
            if std::env::var("KENSHI_TPOSE_DEBUG").is_ok() {
                static SEEN: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
                if let Ok(mut seen) = SEEN.lock()
                    && !seen.contains(&p.id)
                {
                    seen.push(p.id);
                    let race = p.race.as_ref().or(p.look_race.as_ref()).and_then(|r| sim.data.get(r)).map_or("-", |r| r.name.as_str());
                    eprintln!(
                        "tpose {} ({race}, {}): wanted {:?}; activity {:?}; speed {:.1}; relax clip {}",
                        p.id,
                        sim.display_name(p.id),
                        wanted.iter().map(|(a, _, _)| a.anim_name.clone()).collect::<Vec<_>>(),
                        activity_anim(&game, p),
                        p.speed,
                        anim.find("idle_stand_relax").is_some()
                    );
                }
            }
            String::from("idle_stand_relax")
        };
        if game.playing[p.id] != want {
            anim.play(&want);
            game.playing[p.id] = want;
        }
    }
    finish_bodies(game, &mut commands, &bodies, pending, dt);
}

/// What drew a body (place_people): its kind of animation.
const KIND_WALK: u8 = 1;
const KIND_COMBAT: u8 = 2;
const KIND_ANIMAL: u8 = 3;
const KIND_CARRIED: u8 = 4;
const KIND_DOWN: u8 = 5;
const KIND_ONESHOT: u8 = 6;

/// What a body's one-shot task clips watch for (place_people): whom it carried, whom it
/// stalked to knock out, its bed and whether it looted last frame, and the clip playing with
/// when it began (game seconds) and whether backwards.
#[derive(Default)]
struct Transition {
    carrying: Option<usize>,
    ko: Option<usize>,
    bed: Option<usize>,
    looting: bool,
    lifting: bool,
    putting_down: bool,
    clip: Option<(&'static str, f32, bool)>,
}

/// F037162's breathing rate: the game's one roll (0.35 + 0.1 x a random number), made once.
fn breath_rate() -> f32 {
    static RATE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *RATE.get_or_init(|| {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        0.35 + 0.1 * (n % 1000) as f32 / 1000.0
    })
}

/// The bodies' clips finished:
/// - a change of kind (walking to fighting, to an activity, knocked down...) crossfades: the
///   last frame's clips held and faded out while the new ones fade in. (assumption) Kenshi
///   keeps one animation class whose layers crossfade on their own; here the kinds are drawn
///   from different layer sets, faded over a third of a second (the layers' blend rate 3 a
///   second);
/// - head turning (F038922): the overlay clip "head turning" held at (angle + 90 deg) / 180 deg
///   x 0.07 s for the angle from the body's facing to who it looks at, none past 90 deg either
///   side. (assumption: who looks at whom, the caller not traced) its fight's target, else whom
///   it talks with, else for the world's people the player's nearest character within 250
///   units.
fn finish_bodies(
    game: &mut Game,
    commands: &mut Commands,
    bodies: &Query<(&mut Transform, &mut Animated)>,
    pending: Vec<(Entity, usize, u8, Vec<(usize, f32, f32)>)>,
    dt: f32,
) {
    const FADE: f32 = 3.0;
    let n = game.sim.folk.len();
    if game.anim_kind.len() < n {
        game.anim_kind.resize(n, 0);
        game.last_blend.resize(n, Vec::new());
        game.fading_blend.resize(n, (Vec::new(), 0.0));
    }
    let players: Vec<usize> = game.sim.folk.iter().filter(|p| p.player && !p.dead).map(|p| p.id).collect();
    for (e, id, kind, blend) in pending {
        // the crossfade
        if kind != game.anim_kind[id] && !game.last_blend[id].is_empty() {
            game.fading_blend[id] = (std::mem::take(&mut game.last_blend[id]), 1.0);
        }
        game.anim_kind[id] = kind;
        game.last_blend[id] = blend.clone();
        let mut out = blend;
        let (old, left) = &mut game.fading_blend[id];
        if *left > 0.0 {
            *left = (*left - FADE * dt).max(0.0);
            if *left > 0.0 {
                let keep = 1.0 - *left;
                for c in out.iter_mut() {
                    c.2 *= keep;
                }
                out.extend(old.iter().map(|&(i, t, w)| (i, t, w * *left)));
            } else {
                old.clear();
            }
        }
        // head turning
        let Ok((tr, anim)) = bodies.get(e) else { continue };
        let sim = &game.sim;
        let p = &sim.folk[id];
        if kind != KIND_CARRIED && kind != KIND_DOWN
            && let Some(head) = anim.find("head turning")
        {
            let f = &sim.arena.f[id];
            let me = Vec3::from(p.mover.position_units());
            let look = if f.active { f.focus.or(f.combat.target) } else { None }
                .or_else(|| sim.talking_to(id))
                .or_else(|| sim.dialog_memory.partner.iter().find(|(_, pc)| **pc == id).map(|(npc, _)| *npc))
                .or_else(|| {
                    (!p.player)
                        .then(|| {
                            players
                                .iter()
                                .copied()
                                .filter(|&q| q != id)
                                .map(|q| (q, Vec3::from(sim.folk[q].mover.position_units()).distance(me)))
                                .filter(|(_, d)| *d < 250.0)
                                .min_by(|a, b| a.1.total_cmp(&b.1))
                                .map(|(q, _)| q)
                        })
                        .flatten()
                });
            if let Some(q) = look.filter(|&q| q < n && q != id) {
                let to = Vec3::from(sim.folk[q].mover.position_units()) - me;
                let fwd = tr.rotation * Vec3::Z;
                let (a, b) = (Vec2::new(fwd.x, fwd.z), Vec2::new(to.x, to.z));
                if b.length_squared() > 1e-4 {
                    // signed: positive to the body's left
                    let angle = a.angle_to(b);
                    let half = std::f32::consts::FRAC_PI_2;
                    if angle.abs() <= half {
                        out.push((head, (angle + half) / std::f32::consts::PI * 0.07, 1.0));
                        if std::env::var("KENSHI_HEAD_DEBUG").is_ok() && sim.tick.is_multiple_of(60) {
                            eprintln!("head: {id} looks at {q}, {:.0} deg", angle.to_degrees());
                        }
                    }
                }
            }
        }
        commands.entity(e).insert(Blend(out));
    }
}

/// The animation category a character moves and idles in (ANIMATION "category", F037184): 4
/// swimming (its water state swimming, F039118), 5 crouched / prone: its legs collapsed
/// (MedicalSystem +0x160, F040452: the prone state 2, `Fighter::prone`), crawling ("crawl idle
/// up / down", "crawling move"); else 0.
fn movement_category(f: &kenshi_combat::arena::Fighter, water: i32) -> i32 {
    if water == 3 {
        return 4;
    }
    if f.prone() {
        return 5;
    }
    0
}

/// @fn F037184
///
/// The clip a caged character plays (AnimationClass::updateBase, in a cage: Character +0x2f8 2)
/// and its rate: knocked out (vtable 0x30), the cage's "animation KO" at 1; else by its health
/// (Character +0x51c, MedicalSystem +0xc4: blood over the most blood, lowered to the worst vital
/// part's health fraction, at least 0; F040452): under 0.05 "animation dazed" at 0.025, up to
/// 0.2 "animation dazed" at 0.5, else the cage's "animation" at 0.5 (F029399's keys, F029281).
/// (not ported) Character vtable 0x360's own call for the slowest daze.
fn cage_clip(game: &Game, p: &kenshi_game::sim::Person) -> (&'static str, f32) {
    if p.down {
        return ("animation KO", 1.0);
    }
    let m = &game.sim.arena.f[p.id].medical;
    let mut health = if m.max_blood > 0.0 { m.blood / m.max_blood } else { 1.0 };
    for q in m.parts.iter().filter(|q| q.vital) {
        health = health.min(q.health_frac);
    }
    let health = health.max(0.0);
    if health < 0.05 {
        ("animation dazed", 0.025)
    } else if health <= 0.2 {
        ("animation dazed", 0.5)
    } else {
        ("animation", 0.5)
    }
}

/// The ANIMATION record a character's task plays while it stays put (its task's own clip):
/// - in bed: the bed's BUILDING_FUNCTIONALITY "animation" ("sleepinbed"; a bedroll's
///   "sleeponfloor", Task_SleepOnFloor F031231), else "sleepinbed" (TaskRepertoire, F030716);
/// - operating a machine, sitting on a chair or throne, manning a turret: the furniture's
///   BUILDING_FUNCTIONALITY "animation" (a gun turret's "stand 1"), else "stonemine"
///   (Task_OperateMachine, F031350);
/// - in a cage or on a post (`cage_clip`): the cage's "animation", "animation dazed" when badly
///   hurt, "animation KO" while knocked out;
/// - building, giving first aid, looting (Task_Loot F031208 / F031209: "squat T" down into
///   "medic"): "medic" (Task_Build F031256's crouched work); prone, "crawl use" (F037185);
/// - picking a cage's lock: its functionality's "animation lockpick", else the crouched work;
///   prone, "crawl use" (F037186);
/// - bashing a door or a gate, dismantling: "stonemine" (Task_BashDoor F031240,
///   Task_Dismantle F031298);
/// - picking an item up: "pickup" (Task_Pickup, F031214);
/// - stripping weapons, restraining, processing a new slave: "medic" (`Sim::law_task_clip`).
/// (assumption) First aid's clip is the crouched work pose of the building and looting tasks.
///
/// With it the rate to play it at when Kenshi gives one, else the record's "play speed": the
/// play call's rate is the rate itself (F038917 -> F038900 keep it as the clip's speed; the
/// callers wanting the record's pass its +0x74, "play speed"). updateBase (F037184) plays a
/// bed's (Character +0x2f8 1, Task_UseBed F031297 -> F030710) at 0.1, a cage's (+0x2f8 2) at
/// 0.5 and its "animation KO" at 1; the law's tasks play theirs at 1 (F031285, F031266,
/// F031348); a cage's by the caged one's health (`cage_clip`).
fn activity_anim(game: &Game, p: &kenshi_game::sim::Person) -> Option<(String, Option<f32>)> {
    let still = p.speed < 2.0;
    let data = &game.sim.data;
    // a furniture's functionality animation (or its "animation KO")
    let furniture = |f: usize, ko: bool| -> Option<String> {
        let rec = data.get(&game.sim.furn.get(f)?.record)?;
        let func = data.get(kenshi_game::production::functionality(rec)?)?;
        let key = if ko { "animation KO" } else { "animation" };
        let (sid, _) = func.refs(key).first()?;
        Some(data.get(sid)?.name.clone())
    };
    if let Some(b) = p.bed {
        return Some((furniture(b, false).unwrap_or_else(|| "sleepinbed".into()), Some(0.1)));
    }
    if let Some(cage) = p.prison.as_ref().filter(|c| !c.out).map(|c| c.cage) {
        let (key, rate) = cage_clip(game, p);
        let named = |key: &str| -> Option<String> {
            let rec = data.get(&game.sim.furn.get(cage)?.record)?;
            let func = data.get(kenshi_game::production::functionality(rec)?)?;
            let (sid, _) = func.refs(key).first()?;
            Some(data.get(sid)?.name.clone())
        };
        return named(key).or_else(|| furniture(cage, false)).map(|a| (a, Some(rate)));
    }
    if p.down || !still {
        return None;
    }
    // stripping weapons, restraining, processing a new slave: "medic" (kenshi_game::law)
    if let Some(c) = game.sim.law_task_clip(p.id) {
        return Some((c.into(), Some(1.0)));
    }
    if let Some(f) = p.operating {
        return Some((furniture(f, false).unwrap_or_else(|| "stonemine".into()), None));
    }
    // a building of the player's being worked (a training dummy's "chop down static", a
    // locksmith's box "medic"): its functionality's "animation"
    if let Some(b) = p.operate
        && let Some(pb) = game.sim.buildings.get(b)
        && (p.mover.position_units()[0] - pb.at[0]).hypot(p.mover.position_units()[2] - pb.at[2]) <= pb.radius + 40.0
        && let Some(rec) = data.get(&pb.record)
        && let Some(func) = kenshi_game::production::functionality(rec).and_then(|f| data.get(f))
        && let Some((sid, _)) = func.refs("animation").first()
        && let Some(a) = data.get(sid)
    {
        return Some((a.name.clone(), None));
    }
    let prone = game.sim.arena.f.get(p.id).is_some_and(|f| f.prone());
    if p.build.is_some() || p.first_aid.is_some() || p.loot.is_some() {
        return Some((if prone { "crawl use" } else { "medic" }.into(), None));
    }
    // F037186: a lock's "animation lockpick" (a prisoner post's "stand pole"), else crouched
    if let Some((prisoner, _)) = &p.freeing {
        let cage = game.sim.folk.get(*prisoner).and_then(|q| q.prison.as_ref()).map(|c| c.cage);
        let pick = cage.and_then(|f| {
            let rec = data.get(&game.sim.furn.get(f)?.record)?;
            let func = data.get(kenshi_game::production::functionality(rec)?)?;
            Some(data.get(&func.refs("animation lockpick").first()?.0)?.name.clone())
        });
        return Some((if prone { "crawl use".into() } else { pick.unwrap_or_else(|| "medic".into()) }, None));
    }
    if p.dismantle.is_some() || p.bash_order.is_some() || p.gate_in_way.is_some() {
        return Some(("stonemine".into(), None));
    }
    if p.pickup.is_some() {
        return Some(("pickup".into(), None));
    }
    None
}

/// The overlay a character's task adds on its base clip: a turret's gunner "combatstance"
/// (Task_UseTurret F031217 plays it over the turret's "stand 1").
fn activity_overlay(game: &Game, p: &kenshi_game::sim::Person) -> Option<&'static str> {
    if p.down || p.speed >= 2.0 {
        return None;
    }
    let turret = p.operating.and_then(|f| game.sim.furn.get(f)).is_some_and(|f| f.function == kenshi_game::turrets::BF_TURRET)
        || game.sim.arena.turrets.iter().any(|t| t.operator == Some(p.id));
    turret.then_some("combatstance")
}

/// The overlay: clock, speed and the selection's state.
/// Whether the cursor is on a UI button (clicks there are the UI's, not the world's).
fn on_ui(ui: &Query<&Interaction>) -> bool {
    ui.iter().any(|i| *i != Interaction::None)
}

/// Ctrl+F3: the debug text (time, speed, people, keys) shown or hidden; hidden at first. (F3 is
/// controls.cfg's "speed_2".)
fn debug_text(keys: Res<ButtonInput<KeyCode>>, game: Res<Game>, mut nodes: Query<&mut Node>, mut shown: Local<bool>, mut set: Local<bool>) {
    let Some(e) = game.hud else { return };
    // a game loaded in the program: its own text hidden (or shown) again
    if game.is_added() {
        *set = false;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl && keys.just_pressed(KeyCode::F3) {
        *shown = !*shown;
        *set = false;
    }
    if !*set
        && let Ok(mut n) = nodes.get_mut(e)
    {
        n.display = if *shown || std::env::var("KENSHI_DEBUG_HUD").is_ok() { Display::Flex } else { Display::None };
        *set = true;
    }
}

fn hud(game: Res<Game>, mut texts: Query<&mut Text>, time: Res<Time>, mut fps: Local<f32>) {
    let dt = time.delta_secs().max(1e-4);
    *fps = if *fps == 0.0 {
        1.0 / dt
    } else {
        *fps * 0.95 + 0.05 / dt
    };
    let Some(Ok(mut text)) = game.hud.map(|e| texts.get_mut(e)) else {
        return;
    };
    let h = game.sim.hour();
    let mut s = format!(
        "{:02}:{:02}   {}   {}   {:.0} fps   sim {:.1} s\n",
        h as u32,
        ((h.fract()) * 60.0) as u32,
        if game.paused {
            "PAUSED".to_string()
        } else {
            format!("x{}", game.speed)
        },
        if game.sim.player_run { "run" } else { "walk" },
        *fps,
        game.sim.t
    );
    for &i in &game.selected {
        let p = &game.sim.folk[i];
        let f = &game.sim.arena.f[i];
        let parts = &f.medical.parts;
        let health = if parts.is_empty() {
            1.0
        } else {
            parts
                .iter()
                .map(|q| (q.health() / q.max_health()).clamp(0.0, 1.0))
                .sum::<f32>()
                / parts.len() as f32
        };
        let state = if p.down {
            "knocked out"
        } else if f.active {
            "fighting"
        } else if p.speed > 1.0 {
            "moving"
        } else {
            "idle"
        };
        s += &format!("#{i}  {:3.0}%  {state}\n", health * 100.0);
    }
    s += "\nWASD pan  Q/E turn  wheel zoom  LMB select  RMB move/attack  Space pause  1-3 speed  F focus  R run/walk  I inventory  C stats  Num4 sneak";
    *text = Text::new(s);
}

/// `--shot` and `--auto-attack`.
fn testing(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
    reflection: Option<Res<kenshi_render::reflection::WaterReflection>>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    *frames += 1;
    if *frames == 30 && game.auto_attack {
        let squad = game.selected.clone();
        let me = Vec3::from(game.sim.folk[squad[0]].mover.position_units());
        let victim = game
            .sim
            .folk
            .iter()
            .filter(|p| !p.player && p.inside.is_none())
            .min_by(|a, b| {
                let d = |p: &kenshi_game::sim::Person| {
                    Vec3::from(p.mover.position_units()).distance(me)
                };
                d(a).total_cmp(&d(b))
            })
            .map(|p| p.id);
        if let Some(v) = victim {
            game.sim.order_attack(&squad, v);
            eprintln!("auto attack on {v}");
        }
    }
    // (testing) KENSHI_GIVE="[!]<item name>@<count>[,...]": at frame 30 the first selected
    // character gets those items; a name starting '+' is worn, '>' put in the worn backpack;
    // after the name "~<faction string id>": owned by that faction (Item +0x140); "*<name>":
    // made by that smith; "=<faction string id>": that faction's uniform
    if *frames == 30
        && let Ok(v) = std::env::var("KENSHI_GIVE")
        && let Some(&me) = game.selected.first()
    {
        // a leading '!' empties the main grid first
        let v = match v.strip_prefix('!') {
            Some(rest) => {
                let inv = &mut game.sim.folk[me].inventory;
                if let Some(m) = inv.section("main") {
                    inv.sections[m].items.clear();
                }
                rest.to_string()
            }
            None => v,
        };
        for entry in v.split(',') {
            let (name, n) = entry.rsplit_once('@').map_or((entry, 1), |(a, b)| (a, b.parse().unwrap_or(1)));
            let (name, wear) = match name.strip_prefix('+') {
                Some(n) => (n, true),
                None => (name, false),
            };
            let (name, bagged) = match name.strip_prefix('>') {
                Some(n) => (n, true),
                None => (name, false),
            };
            let (name, uniform) = match name.split_once('=') {
                Some((n, f)) => (n, Some(f.to_string())),
                None => (name, None),
            };
            let (name, crafter) = match name.split_once('*') {
                Some((n, f)) => (n, Some(f.to_string())),
                None => (name, None),
            };
            let (name, owner) = match name.split_once('~') {
                Some((n, f)) => (n, Some(f.to_string())),
                None => (name, None),
            };
            let rec = game.sim.data.records.values().find(|r| r.name == name && r.item_type().is_some()).cloned();
            match rec {
                Some(r) => {
                    let mut item = kenshi_char::inventory::Item::from_record(&r, n);
                    item.owner = owner;
                    item.crafter = crafter;
                    if uniform.is_some() {
                        item.uniform = uniform;
                    }
                    let food = item.food;
                    let inv = &mut game.sim.folk[me].inventory;
                    let ok = if bagged { inv.bag_mut().is_some_and(|b| b.add(item, false)) } else { inv.add(item, wear) };
                    eprintln!("gave {n} {name} ({}, food {food:?}) to {me}: {}", r.string_id, if ok { "ok" } else { "no room" });
                    if !ok {
                        for sec in &game.sim.folk[me].inventory.sections {
                            let names: Vec<String> = sec.items.iter().map(|p| format!("{} {}x{}@{},{}", p.item.name, p.item.w, p.item.h, p.x, p.y)).collect();
                            eprintln!("  {} {}x{}: {}", sec.name, sec.w, sec.h, names.join("; "));
                        }
                    }
                }
                None => eprintln!("no item {name}"),
            }
        }
    }
    // (testing) KENSHI_TALK_AT=<frame>: the first selected character talks to the nearest
    // trader (else anyone with dialogue) and picks the first reply; printed
    if std::env::var("KENSHI_TALK_AT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames)
        && let Some(&me) = game.selected.first()
    {
        let at = game.sim.folk[me].mover.position_units();
        let d = |p: &kenshi_game::sim::Person| {
            let q = p.mover.position_units();
            (q[0] - at[0]).powi(2) + (q[2] - at[2]).powi(2)
        };
        let npc = game
            .sim
            .folk
            .iter()
            .filter(|p| !p.player && p.dialogue.is_some())
            .min_by(|a, b| (a.shop.is_none(), d(a)).partial_cmp(&(b.shop.is_none(), d(b))).unwrap())
            .map(|p| p.id);
        if let Some(npc) = npc {
            eprintln!("talk to {npc} ({:?})", game.sim.folk[npc].dialogue);
            match game.sim.talk(npc, me) {
                Some(c) => {
                    eprintln!("  npc: {:?}", c.npc_text);
                    for (id, t) in &c.choices {
                        eprintln!("  choice {id}: {t:?}");
                    }
                    if let Some((id, _)) = c.choices.first().cloned() {
                        let (effects, next) = game.sim.choose(&c, &id);
                        eprintln!("  -> effects {effects:?}, next {:?}", next.map(|n| n.npc_text));
                    }
                }
                None => eprintln!("  no dialogue"),
            }
        }
    }
    // (testing) KENSHI_EXIT_AT=<frame>: the app closes then (no screenshot)
    if std::env::var("KENSHI_EXIT_AT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames) {
        exit.write(AppExit::Success);
    }
    // (testing) KENSHI_RUN=dx,dz: at frame 30 the first player character alone selected and
    // ordered to run that far (units)
    if *frames == 30
        && let Ok(v) = std::env::var("KENSHI_RUN")
        && let Some(&me) = game.selected.first()
    {
        let d: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        if let [dx, dz] = d.as_slice() {
            let origin = game.zone.origin;
            let [x, _, z] = game.sim.folk[me].mover.position_units();
            let (nx, nz) = (x + dx, z + dz);
            let y = game.ground(nx - origin.x, nz - origin.z) + origin.y;
            game.selected = vec![me];
            game.sim.player_run = true;
            game.sim.order_move(&[me], [nx, y, nz]);
        }
    }
    // (testing) KENSHI_FIGHT=<frame>,<character name>: then (or once someone is selected) the
    // first selected is set down 40
    // units from that character (its CHARACTER record's name, the nearest such) and attacks it;
    // the others selected stay put. KENSHI_FIXED_DT=<seconds> (with captures) keeps the clock
    // even.
    static FIGHT_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static FIGHT_LATER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let later = FIGHT_LATER.load(std::sync::atomic::Ordering::Relaxed);
    if later != 0 && (later >> 32) as u32 == *frames {
        let (me, foe) = (((later >> 16) & 0xffff) as usize, (later & 0xffff) as usize);
        game.sim.order_attack(&[me], foe);
        eprintln!("fight test: {me} attacks {foe}");
    }
    if let Ok(v) = std::env::var("KENSHI_FIGHT")
        && let Some((f, name)) = v.split_once(',')
        && f.parse::<u32>().is_ok_and(|f| *frames >= f)
        && let Some(&me) = game.selected.first()
        && !FIGHT_SET.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        let data = game.sim.data.clone();
        let at = game.sim.folk[me].mover.position_units();
        let foe = game
            .sim
            .folk
            .iter()
            .filter(|p| !p.dead && p.character.as_deref().and_then(|c| data.get(c)).is_some_and(|r| r.name == name))
            .min_by(|a, b| {
                let d = |p: &kenshi_game::sim::Person| {
                    let q = p.mover.position_units();
                    (q[0] - at[0]).hypot(q[2] - at[2])
                };
                d(a).total_cmp(&d(b))
            })
            .map(|p| p.id);
        match foe {
            Some(foe) => {
                // KENSHI_FIGHT_INSIDE: the foe first set in the middle of the nearest building
                // interior's floor
                // (to the point x,z in units when given)
                if let Ok(spot) = std::env::var("KENSHI_FIGHT_INSIDE") {
                    let mut f = game.sim.folk[foe].mover.position_units();
                    if let Some((x, z)) = spot.split_once(',').and_then(|(x, z)| Some((x.parse::<f32>().ok()?, z.parse::<f32>().ok()?))) {
                        f = [x, f[1], z];
                    }
                    let placed_at = game.sim.placed_at.clone();
                    let mid = |v: &kenshi_world::interior::Interior| [(v.lo[0] + v.hi[0]) / 2.0, placed_at.get(v.placement).map_or(v.lo[1], |p| p[1]), (v.lo[2] + v.hi[2]) / 2.0];
                    for r in &game.sim.inner {
                        eprintln!("fight test: room of {:?} at {:?}, {:?} .. {:?}", game.sim.placed.get(r.placement).and_then(|p| game.sim.data.get(&p.building)).map(|b| b.name.clone()), mid(r), r.lo, r.hi);
                    }
                    let room = game.sim.inner.iter().min_by(|a, b| (mid(a)[0] - f[0]).hypot(mid(a)[2] - f[2]).total_cmp(&(mid(b)[0] - f[0]).hypot(mid(b)[2] - f[2])));
                    // (KENSHI_FIGHT_AT=dx,y,dz: that far from the room's middle, at that height)
                    let off = std::env::var("KENSHI_FIGHT_AT").ok().and_then(|v| {
                        let k: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                        (k.len() == 3).then(|| [k[0], k[1], k[2]])
                    });
                    if let Some(r) = room {
                        for y in (0..14).map(|k| r.lo[1] + k as f32 * 50.0) {
                            let m = mid(r);
                            eprintln!("fight test: probe y {y}: {:?}", game.sim.nav.closest_point([m[0], y, m[2] + 120.0].map(|v| v / 10.0), 5.0).map(|(_, p)| p.map(|v| v * 10.0)));
                        }
                    }
                    let near = room.and_then(|r| {
                        let m = mid(r);
                        let at = off.map_or(m, |o| [m[0] + o[0], o[1], m[2] + o[2]]);
                        game.sim.nav.closest_point(at.map(|v| v / 10.0), 30.0)
                    });
                    eprintln!(
                        "fight test: room {:?} of {:?} (from {f:?}), on the navmesh at {:?}",
                        room.map(mid),
                        room.and_then(|r| game.sim.placed.get(r.placement)).and_then(|p| game.sim.data.get(&p.building)).map(|b| b.name.clone()),
                        near.map(|(_, p)| p.map(|v| v * 10.0))
                    );
                    if let Some((_, p)) = near {
                        game.sim.folk[foe].mover.position = p;
                        game.sim.arena.f[foe].mover.position = p;
                        game.sim.folk[foe].pathing.follow = None;
                    }
                }
                // KENSHI_FIGHT_SPOT=x,y,z (units): the foe first set on the navmesh nearest there
                if let Some(k) = std::env::var("KENSHI_FIGHT_SPOT").ok().map(|v| v.split(',').filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<f32>>())
                    && k.len() == 3
                    && let Some((_, p)) = game.sim.nav.closest_point([k[0] / 10.0, k[1] / 10.0, k[2] / 10.0], 60.0)
                {
                    eprintln!("fight test: foe set at {:?}", p.map(|v| v * 10.0));
                    game.sim.folk[foe].mover.position = p;
                    game.sim.arena.f[foe].mover.position = p;
                    game.sim.folk[foe].pathing.follow = None;
                }
                // KENSHI_FIGHT_SKILL=<level>: the attacker's combat skills and attributes at that
                if let Some(l) = std::env::var("KENSHI_FIGHT_SKILL").ok().and_then(|v| v.parse::<f32>().ok()) {
                    use kenshi_char::StatsEnumerated as S;
                    for s in [S::StatStrength, S::StatMeleeAttack, S::StatMeleeDefence, S::StatDexterity, S::StatToughness, S::StatDodge, S::StatKatanas, S::StatSabres, S::StatHackers, S::StatHeavyweapons, S::StatBlunt, S::StatPolearms, S::StatAthletics] {
                        game.sim.arena.f[me].skills.levels.insert(s, l);
                    }
                }
                // (KENSHI_FIGHT_GAP=<metres>: that far off, else 4)
                let gap = std::env::var("KENSHI_FIGHT_GAP").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(4.0);
                let q = game.sim.folk[foe].mover.position;
                let spot = game.sim.nav.closest_point([q[0] + gap, q[1], q[2]], 10.0).map_or([q[0] + gap, q[1], q[2]], |(_, p)| p);
                game.sim.folk[me].mover.position = spot;
                game.sim.arena.f[me].mover.position = spot;
                game.sim.folk[me].pathing.follow = None;
                game.selected = vec![me];
                // (KENSHI_FIGHT_IDLE=<frames>: the attack ordered that much later)
                match std::env::var("KENSHI_FIGHT_IDLE").ok().and_then(|v| v.parse::<u32>().ok()) {
                    Some(n) => FIGHT_LATER.store(((*frames + n) as u64) << 32 | (me as u64) << 16 | foe as u64, std::sync::atomic::Ordering::Relaxed),
                    None => game.sim.order_attack(&[me], foe),
                }
                eprintln!("fight test: {me} set by {foe} ({name}, inside {:?}) and attacking", game.sim.folk[foe].inside);
            }
            None => eprintln!("fight test: no {name} here"),
        }
    }
    // (and where the two are each second after)
    static FIGHT_FRAME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if FIGHT_SET.load(std::sync::atomic::Ordering::Relaxed) && FIGHT_FRAME.load(std::sync::atomic::Ordering::Relaxed) == 0 {
        FIGHT_FRAME.store(*frames, std::sync::atomic::Ordering::Relaxed);
    }
    if let Ok(v) = std::env::var("KENSHI_FIGHT")
        && let Some((f, name)) = v.split_once(',')
        && f.parse::<u32>().is_ok()
        && FIGHT_FRAME.load(std::sync::atomic::Ordering::Relaxed) > 0
        && (*frames - FIGHT_FRAME.load(std::sync::atomic::Ordering::Relaxed)) % 60 == 59
        && let Some(&me) = game.selected.first()
    {
        let data = game.sim.data.clone();
        let foe = game.sim.folk.iter().find(|p| p.character.as_deref().and_then(|c| data.get(c)).is_some_and(|r| r.name == name));
        eprintln!(
            "fight test: t {:.1}: {me} at {:?}, {name} at {:?}",
            game.sim.t,
            game.sim.folk[me].mover.position_units().map(|v| v.round()),
            foe.map(|p| p.mover.position_units().map(|v| v.round()))
        );
    }
    // (testing) KENSHI_TELEPORT=dx,dz: the player characters moved that far (units) at frame 600,
    // on the ground there
    if *frames == 600
        && let Ok(v) = std::env::var("KENSHI_TELEPORT")
    {
        let d: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        if let [dx, dz] = d.as_slice() {
            let origin = game.zone.origin;
            for i in 0..game.sim.folk.len() {
                if !game.sim.folk[i].player {
                    continue;
                }
                let [x, _, z] = game.sim.folk[i].mover.position_units();
                let (nx, nz) = (x + dx, z + dz);
                let y = game.ground(nx - origin.x, nz - origin.z) + origin.y;
                let m = 10.0;
                game.sim.folk[i].mover.position = [nx / m, y / m, nz / m];
            }
            eprintln!("teleported the squad by {dx}, {dz}");
        }
    }
    // (testing) KENSHI_SNEAK: the selection sneaks from frame 30; KENSHI_CARRY: at frame 200 the
    // nearest townsperson is knocked out and the first selected picks them up
    // (KENSHI_CARRY=squad: the nearest of the player's own people instead)
    if *frames == 30 && std::env::var("KENSHI_SNEAK").is_ok() {
        let sel = game.selected.clone();
        game.sim.toggle_sneak(&sel);
    }
    if *frames == 200
        && std::env::var("KENSHI_CARRY").is_ok()
        && let Some(&me) = game.selected.first()
    {
        let at = Vec3::from(game.sim.folk[me].mover.position_units());
        let victim = game
            .sim
            .folk
            .iter()
            .filter(|p| p.id != me && (p.player == std::env::var("KENSHI_CARRY").is_ok_and(|v| v == "squad")) && p.race.is_none() && p.inside.is_none() && !p.down)
            .min_by(|a, b| {
                let d = |p: &kenshi_game::sim::Person| Vec3::from(p.mover.position_units()).distance(at);
                d(a).total_cmp(&d(b))
            })
            .map(|p| p.id);
        if let Some(v) = victim {
            // brought beside the carrier first (the nearest may be far off)
            let mut spot = game.sim.folk[me].mover.position;
            spot[0] += 8.0;
            game.sim.folk[v].mover.position = spot;
            game.sim.arena.f[v].mover.position = spot;
            game.sim.knock_out(v);
            game.sim.order_pickup(&[me], v);
            game.ko_test = Some(v);
            eprintln!("carry test: {me} picks up {v} at {:.0}", Vec3::from(game.sim.folk[v].mover.position_units()).distance(at));
        }
    }
    // (testing) KENSHI_KO: at frame 200 the nearest townsperson, brought beside the first
    // selected, is knocked out (the camera kept on them); KENSHI_KO_UP=<frame>: they come to
    if std::env::var("KENSHI_KO").is_ok()
        && let Some(&me) = game.selected.first()
    {
        if *frames == 200 {
            let at = Vec3::from(game.sim.folk[me].mover.position_units());
            let victim = game
                .sim
                .folk
                .iter()
                .filter(|p| !p.player && p.inside.is_none() && !p.down)
                .min_by(|a, b| {
                    let d = |p: &kenshi_game::sim::Person| Vec3::from(p.mover.position_units()).distance(at);
                    d(a).total_cmp(&d(b))
                })
                .map(|p| p.id);
            if let Some(v) = victim {
                let mut spot = game.sim.folk[me].mover.position;
                spot[0] += 3.0;
                // KENSHI_KO_ROOF: above the nearest building part 20 - 200 units across instead
                let origin = game.zone.origin;
                let here = at - origin;
                // KENSHI_KO_ROOF: above the nearest house-sized solid standing 25+ units over the
                // ground, 60+ units from the squad
                let roof = std::env::var("KENSHI_KO_ROOF").is_ok().then(|| {
                    game.statics
                        .bounds()
                        .filter(|(lo, hi)| {
                            let c = (*lo + *hi) * 0.5;
                            let g = game.ground(c.x, c.z);
                            let d = c.with_y(here.y).distance(here);
                            (15.0..150.0).contains(&(hi.x - lo.x).max(hi.z - lo.z)) && hi.y - g > 15.0 && lo.y < g + 20.0 && d > 25.0 && d < 800.0
                        })
                        .min_by(|a, b| {
                            let d = |(lo, hi): &(Vec3, Vec3)| ((*lo + *hi) * 0.5).with_y(here.y).distance(here);
                            d(a).total_cmp(&d(b))
                        })
                        .map(|(lo, hi)| {
                            eprintln!("ko test: solid {lo:?}..{hi:?}");
                            Vec3::new((lo.x + hi.x) * 0.5, hi.y + 6.0, (lo.z + hi.z) * 0.5)
                        })
                });
                if let Some(Some(r)) = roof {
                    let w = r + origin;
                    spot = [w.x / 10.0, w.y / 10.0, w.z / 10.0];
                    eprintln!("ko test: on the roof at {r}");
                }
                // KENSHI_KO_WATER: at the water's surface over the nearest ground 30+ units under
                // it (within 1500 units of the squad), dropped in
                let wet = std::env::var("KENSHI_KO_WATER").is_ok().then(|| {
                    let level = kenshi_game::sim::WATER_LEVEL - origin.y;
                    let mut best: Option<Vec3> = None;
                    for i in -60..=60 {
                        for k in -60..=60 {
                            let p = Vec3::new(here.x + i as f32 * 25.0, 0.0, here.z + k as f32 * 25.0);
                            if game.ground(p.x, p.z) < level - 30.0 && best.is_none_or(|b| b.distance(here.with_y(0.0)) > p.distance(here.with_y(0.0))) {
                                best = Some(p);
                            }
                        }
                    }
                    best.map(|b| b.with_y(level + 5.0))
                });
                // KENSHI_KO_FOLIAGE: 25 units over the nearest walkable foliage object (its
                // collision, group 14)
                let wet = wet.or_else(|| {
                    std::env::var("KENSHI_KO_FOLIAGE").is_ok().then(|| {
                        std::iter::once(&game.zone)
                            .chain(game.zones.values())
                            .flat_map(|z| z.foliage_collision.iter())
                            .filter(|f| f.2 == kenshi_render::zone::FEATURE_GROUP)
                            .map(|f| f.0.translation)
                            .filter(|t| t.with_y(0.0).distance(here.with_y(0.0)) < 1500.0)
                            .map(|t| {
                                let top = game.statics.raycast_groups(t + Vec3::Y * 300.0, Vec3::NEG_Y, 600.0, 1 << kenshi_render::zone::FEATURE_GROUP).map_or(t.y, |d| t.y + 300.0 - d);
                                (t, top)
                            })
                            // (the tallest over the ground near the squad)
                            .max_by(|a, b| (a.1 - game.ground(a.0.x, a.0.z)).total_cmp(&(b.1 - game.ground(b.0.x, b.0.z))))
                            .map(|(t, top)| {
                                eprintln!("ko test: over walkable foliage at {t}, top {top:.1}, ground {:.1}", game.ground(t.x, t.z));
                                t.with_y(top + 25.0)
                            })
                    })
                });
                let roof = roof.or(wet.clone());
                if let Some(Some(r)) = wet {
                    let w = r + origin;
                    spot = [w.x / 10.0, w.y / 10.0, w.z / 10.0];
                    eprintln!("ko test: in the water at {r}, ground {:.1}", game.ground(r.x, r.z));
                }
                game.sim.folk[v].mover.position = spot;
                game.sim.arena.f[v].mover.position = spot;
                game.ko_test = Some(v);
                if roof.is_some() {
                    // at once, before walking takes them back down
                    game.sim.knock_out(v);
                }
            }
        }
        if *frames == 203
            && let Some(v) = game.ko_test
        {
            game.sim.knock_out(v);
            // KENSHI_KO=pile: the next nearest townsperson dropped onto them from 1.5 m up
            if std::env::var("KENSHI_KO").is_ok_and(|k| k == "pile") {
                let at = Vec3::from(game.sim.folk[v].mover.position_units());
                let other = game
                    .sim
                    .folk
                    .iter()
                    .filter(|p| !p.player && p.inside.is_none() && !p.down && p.id != v)
                    .min_by(|a, b| {
                        let d = |p: &kenshi_game::sim::Person| Vec3::from(p.mover.position_units()).distance(at);
                        d(a).total_cmp(&d(b))
                    })
                    .map(|p| p.id);
                if let Some(o) = other {
                    let mut spot = game.sim.folk[v].mover.position;
                    spot[1] += 1.5;
                    game.sim.folk[o].mover.position = spot;
                    game.sim.arena.f[o].mover.position = spot;
                    game.sim.knock_out(o);
                    eprintln!("ko test: {o} dropped onto {v}");
                }
            }
        }
        if let Some(v) = game.ko_test {
            game.focus = Vec3::from(game.sim.folk[v].mover.position_units()) - game.zone.origin;
            if std::env::var("KENSHI_KO_UP").ok().and_then(|f| f.parse::<u32>().ok()) == Some(*frames) {
                game.sim.wake(v);
            }
        }
    }
    // (testing) KENSHI_SPAWN_NEAR="<squad template>@<frame>[@<distance>]": that squad spawned
    // that far (units, default 300) from the first selected character at that frame
    if let Ok(v) = std::env::var("KENSHI_SPAWN_NEAR") {
        let parts: Vec<&str> = v.split('@').collect();
        if parts.len() >= 2
            && parts[1].parse::<u32>().ok() == Some(*frames)
            && let Some(&me) = game.selected.first()
        {
            let d: f32 = parts.get(2).and_then(|x| x.parse().ok()).unwrap_or(300.0);
            let at = game.sim.folk[me].mover.position_units();
            let sid = game.sim.data.of_type(ItemType::SquadTemplate).find(|t| t.name == parts[0]).map(|t| t.string_id.clone());
            if let Some(sid) = sid {
                let m = game.sim.spawn_template(&sid, [at[0] + d, at[1], at[2]]);
                eprintln!("spawned {} ({sid}) near the squad: {:?}", parts[0], m.as_ref().map(|m| m.iter().map(|&i| (i, game.sim.folk[i].character.clone(), game.sim.folk[i].dialogue.clone())).collect::<Vec<_>>()));
            }
        }
    }
    // (testing) KENSHI_CARRY_WALK: the carrier walks off with it at frame 300
    if *frames == 300 && std::env::var("KENSHI_CARRY_WALK").is_ok() {
        let sel: Vec<usize> = game.selected.first().copied().into_iter().collect();
        if let Some(&me) = sel.first() {
            let at = game.sim.folk[me].mover.position_units();
            game.sim.player_run = false;
            game.sim.order_move(&sel, [at[0] + 120.0, at[1], at[2] + 40.0]);
        }
    }
    // (testing) KENSHI_CARRY: the carried body is put down at frame 450
    if *frames == 450 && std::env::var("KENSHI_CARRY").is_ok() {
        let sel = game.selected.clone();
        game.sim.order_put_down(&sel);
    }
    // (testing) the camera kept on whoever carries someone
    if std::env::var("KENSHI_CARRY").is_ok()
        && let Some(c) = game.sim.folk.iter().find(|p| p.carrying.is_some())
    {
        game.focus = Vec3::from(c.mover.position_units()) - game.zone.origin;
    }
    // (testing) KENSHI_SELECT=<person>: that person selected instead of the squad
    if *frames == 30
        && let Some(id) = std::env::var("KENSHI_SELECT").ok().and_then(|v| v.parse::<usize>().ok())
        && id < game.sim.folk.len()
    {
        game.selected = vec![id];
    }
    // (testing) KENSHI_FOLK_DEBUG: the selection, and everyone's place, team and state every 300 frames
    if (*frames == 2 || (*frames).is_multiple_of(300)) && std::env::var("KENSHI_FOLK_DEBUG").is_ok() {
        eprintln!("f{} selected {:?}", *frames, game.selected.iter().map(|&i| (i, game.sim.display_name(i), game.sim.folk[i].player)).collect::<Vec<_>>());
        for p in &game.sim.folk {
            let f = &game.sim.arena.f[p.id];
            eprintln!(
                "f{} folk {} at {:.0?} inside {:?} player {} team {} active {} foes {:?} neutrals {} faction {:?} tasks {:?}",
                *frames, p.id, p.mover.position_units(), p.inside, p.player, f.team, f.active, f.foes, p.attacks_neutrals, p.faction,
                p.ai.jobs.iter().flatten().map(|t| t.task).collect::<Vec<_>>()
            );
        }
    }
    if *frames == 30 && std::env::var("KENSHI_FOOT_DEBUG").is_ok() {
        let s = game.selected.first().map(|&i| game.sim.folk[i].mover.position_units());
        eprintln!("squad at {s:?}");
    }
    // (testing) KENSHI_HURT=<frame>: the second squad member cut on two parts (a wound each);
    // KENSHI_FIRST_AID=<frame>: the first gives them first aid
    let squad: Vec<usize> = game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    // (testing) KENSHI_UPSTAIRS=<frame>: the selected character sent to the highest building
    // floor within 60 m; where they are printed every second after
    if let (Ok(v), Some(&me)) = (std::env::var("KENSHI_UPSTAIRS"), game.selected.first())
        && let Ok(at) = v.parse::<u32>()
        && *frames >= at
    {
        let here = game.sim.folk[me].mover.position_units().map(|x| x * 0.1);
        if *frames == at
            && let Some(goal) = game.sim.nav.highest_floor(here, 60.0)
        {
            eprintln!("upstairs: from {here:?} to {goal:?}");
            game.sim.order_move(&[me], goal.map(|x| x * 10.0));
        }
        if (*frames - at).is_multiple_of(60) {
            eprintln!("upstairs: f{} at {here:?}", *frames);
        }
    }
    // (testing) KENSHI_DROP=<frame>: the selection's first carried (not worn) item put down
    if std::env::var("KENSHI_DROP").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames)
        && let Some(&me) = game.selected.first()
    {
        let inv = &game.sim.folk[me].inventory;
        let found = inv.sections.iter().enumerate().find(|(_, s)| !s.is_equipment() && !s.items.is_empty()).map(|(k, _)| k);
        if let Some(s) = found
            && let Some(item) = game.sim.folk[me].inventory.take(s, 0)
        {
            eprintln!("drop: {} put down", item.name);
            game.sim.drop_item(me, item);
            eprintln!("drop: {} items on the ground", game.sim.ground.len());
        }
    }
    // (testing) KENSHI_QUALITY_DEBUG: at frame 200, the items people and shops hold, by shown
    // name, quality and value
    if std::env::var("KENSHI_QUALITY_DEBUG").is_ok() && *frames == 200 {
        let data = game.sim.data.clone();
        for p in game.sim.folk.iter().take(40) {
            let mut list: Vec<String> = p.inventory.sections.iter().flat_map(|s| s.items.iter()).filter(|i| matches!(i.item.kind, Some(ItemType::Weapon | ItemType::Armour))).map(|i| format!("{} q{} c.{}", i.item.shown_name(&data), i.item.quality, i.item.value)).collect();
            if let Some(shop) = &p.shop {
                list.extend(shop.stock.sections.iter().flat_map(|s| s.items.iter()).filter(|i| matches!(i.item.kind, Some(ItemType::Weapon | ItemType::Armour))).take(8).map(|i| format!("[shop] {} q{} c.{}", i.item.shown_name(&data), i.item.quality, i.item.value)));
            }
            if !list.is_empty() {
                eprintln!("quality: {} {}: {:?}", p.id, game.sim.display_name(p.id), list);
            }
        }
    }
    // (testing) KENSHI_PICKUP=<frame>: the selection picks up the nearest item on the ground
    if std::env::var("KENSHI_PICKUP").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames)
        && let Some(&me) = game.selected.first()
    {
        let at = game.sim.folk[me].mover.position_units();
        if let Some(id) = game.sim.ground_item_near(at, 3000.0) {
            let sel = game.selected.clone();
            game.sim.order_pick_item(&sel, id);
            eprintln!("pickup: going for item {id}");
        }
    }
    // (KENSHI_HURT_SELECTED: the selected character instead)
    let hurt_who = if std::env::var("KENSHI_HURT_SELECTED").is_ok() { game.selected.first().copied() } else { squad.get(1).copied() };
    if let (Ok(v), Some(who)) = (std::env::var("KENSHI_HURT"), hurt_who)
        && v.parse::<u32>().ok() == Some(*frames)
    {
        let mc = game.sim.arena.sh.mc.clone();
        let bleed = mc_bleed(&game.sim);
        let m = &mut game.sim.arena.f[who].medical;
        for i in [0, 1] {
            let d = kenshi_char::medical::Damage { cut: 45.0, blunt: 0.0, pierce: 0.0, stun: 0.0, bleed: 1.0 };
            m.apply_damage(i, d, 1.0, &mc);
            m.wounds.push(45.0 * bleed);
        }
        eprintln!("hurt {who}: {:?}", game.sim.arena.f[who].medical.parts.iter().map(|p| p.flesh).collect::<Vec<_>>());
    }
    // (testing) KENSHI_CAGED=<frame>[,<blood fraction>]: the selected character put in the
    // nearest cage then (its blood down to that fraction of its most: `cage_clip`)
    if let (Ok(v), Some(&who)) = (std::env::var("KENSHI_CAGED"), game.selected.first()) {
        let mut it = v.split(',');
        if it.next().and_then(|f| f.parse::<u32>().ok()) == Some(*frames) {
            let at = game.sim.folk[who].mover.position_units();
            if let Some(cage) = game.sim.free_cage("", at, true) {
                game.sim.imprison(who, cage, kenshi_game::sim::PLAYER_FACTION, false);
                if let Some(b) = it.next().and_then(|b| b.parse::<f32>().ok()) {
                    let m = &mut game.sim.arena.f[who].medical;
                    m.blood = m.max_blood * b;
                }
                eprintln!("caged {who} in {cage}");
            }
        }
    }
    // (testing) KENSHI_LEGLESS=<frame>: the selected character's legs below the KO point then
    // (they collapse: it crawls, F040452)
    if let (Ok(v), Some(&who)) = (std::env::var("KENSHI_LEGLESS"), game.selected.first())
        && v.parse::<u32>().ok() == Some(*frames)
    {
        let names: Vec<bool> = game.sim.arena.f[who].medical.parts.iter().map(|q| game.sim.data.get(&q.id).is_some_and(|r| r.name.to_lowercase().ends_with("leg"))).collect();
        let ko = game.sim.arena.f[who].ko_point();
        let m = &mut game.sim.arena.f[who].medical;
        for (q, leg) in m.parts.iter_mut().zip(names) {
            if leg {
                q.flesh = ko - 5.0;
                q.update_frac();
            }
        }
        eprintln!("legless {who}");
    }
    let (medic, patient) = if std::env::var("KENSHI_HURT_SELECTED").is_ok() { (squad.get(1).copied(), hurt_who) } else { (squad.first().copied(), squad.get(1).copied()) };
    if let (Ok(v), Some(medic), Some(who)) = (std::env::var("KENSHI_FIRST_AID"), medic, patient)
        && v.parse::<u32>().ok() == Some(*frames)
    {
        game.sim.order_first_aid(&[medic], who);
    }
    if std::env::var("KENSHI_FIRST_AID").is_ok()
        && (*frames).is_multiple_of(120)
        && let Some(&who) = squad.get(1)
    {
        let m = &game.sim.arena.f[who].medical;
        eprintln!(
            "f{} medic {:?} at {:.0} patient wounds {} bleed {:.3} parts {:?}",
            *frames,
            game.sim.folk[squad[0]].first_aid,
            {
                let (a, b) = (game.sim.folk[squad[0]].mover.position_units(), game.sim.folk[who].mover.position_units());
                (a[0] - b[0]).hypot(a[2] - b[2])
            },
            m.wounds.len(),
            m.bleed_rate,
            m.parts.iter().take(2).map(|p| (p.flesh.round(), p.treatment.round())).collect::<Vec<_>>()
        );
    }
    // (testing) KENSHI_STEER: frames 30..600, the held move order re-aimed every 12 frames at a
    // point 300 units ahead of the squad's first member, turning slowly
    if std::env::var("KENSHI_STEER").is_ok()
        && (30..600).contains(&*frames)
        && (*frames).is_multiple_of(12)
        && let Some(&me) = game.selected.first()
    {
        let a = *frames as f32 * 0.004;
        let q = game.sim.folk[me].mover.position_units();
        let to = [q[0] + a.cos() * 300.0, q[1], q[2] + a.sin() * 300.0];
        let squad = game.selected.clone();
        if *frames == 36 {
            game.sim.order_move_squad(&squad, to);
        } else {
            game.sim.steer_move_squad(&squad, to);
        }
    }
    // (testing) KENSHI_RUN_TO=dx,dz: the squad runs that far from where it stands
    // (KENSHI_RUN_AT=<frame>: at that frame instead)
    if *frames == std::env::var("KENSHI_RUN_AT").ok().and_then(|f| f.parse().ok()).unwrap_or(30)
        && let Ok(v) = std::env::var("KENSHI_RUN_TO")
    {
        let d: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        let squad = game.selected.clone();
        if let (Some(&first), [dx, dz]) = (squad.first(), d.as_slice()) {
            let me = game.sim.folk[first].mover.position_units();
            // (testing) KENSHI_WALK: walk there instead
            game.sim.player_run = std::env::var("KENSHI_WALK").is_err();
            game.sim.order_move_squad(&squad, [me[0] + dx, me[1], me[2] + dz]);
        }
    }
    // (testing) KENSHI_REFLECTION_SHOT=out.png: the water reflection's target at frame 300
    if *frames == 300
        && let (Ok(path), Some(r)) = (std::env::var("KENSHI_REFLECTION_SHOT"), &reflection)
    {
        // the bytes as they are (Bevy saves no Rgba8Unorm)
        commands.spawn(Screenshot::image(r.image.clone())).observe(
            move |ev: On<bevy::render::view::screenshot::ScreenshotCaptured>| {
                let mut img = ev.image.clone();
                img.texture_descriptor.format = bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
                if let Ok(d) = img.try_into_dynamic() {
                    let _ = d.to_rgb8().save(&path);
                }
            },
        );
    }
    let Some((path, at, count)) = game.shot.clone() else {
        return;
    };
    // the shot follows the selection's fighters (else the selection)
    let origin = game.zone.origin;
    let pick: Vec<usize> = {
        let fighting: Vec<usize> = game
            .selected
            .iter()
            .copied()
            .filter(|&i| game.sim.arena.f[i].active)
            .collect();
        if fighting.is_empty() {
            game.selected.clone()
        } else {
            fighting
        }
    };
    // (testing) KENSHI_SHOT_STILL=<frame>: from that frame the shots stay where they look
    let still = std::env::var("KENSHI_SHOT_STILL").ok().and_then(|v| v.parse::<u32>().ok()).is_some_and(|f| *frames >= f);
    // (the camera's centre moved over the ground there; its height is the camera's own,
    // `camera::height`)
    let target = if still {
        None
    } else if let Some(v) = game.ko_test {
        Some(Vec3::from(game.sim.folk[v].mover.position_units()) - origin + game.pan)
    } else if !pick.is_empty() {
        let n = pick.len() as f32;
        Some(pick.iter().fold(game.pan, |m, &i| {
            m + (Vec3::from(game.sim.folk[i].mover.position_units()) - origin) / n
        }))
    } else {
        None
    };
    if let Some(t) = target {
        game.focus.x = t.x;
        game.focus.z = t.z;
    }
    // (testing) KENSHI_SHOT_STRIDE=<frames>: the shots that many frames apart
    let stride = std::env::var("KENSHI_SHOT_STRIDE").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).max(1);
    if *frames >= at && (*frames - at).is_multiple_of(stride) && (*frames - at) / stride < count {
        let path = if count == 1 {
            path
        } else {
            let k = (*frames - at) / stride;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("shot");
            path.with_file_name(format!("{stem}_{k:03}.png"))
        };
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if *frames == at + count * stride + 20 {
        exit.write(AppExit::Success);
    }
}

/// (testing) KENSHI_DOOR_SHOW=<k>[,<from frame>]: the k-th door nearest the first selected
/// shown: the camera's centre on it from that frame (default 300), its opening held at 0, 0.5
/// and 1 for 60 frames each, then cycled; its record and data printed.
fn door_show(mut game: ResMut<Game>, mut frames: Local<u32>, mut pick: Local<Option<usize>>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_DOOR_SHOW") else { return };
    let a: Vec<u32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
    let from = a.get(1).copied().unwrap_or(300);
    if *frames < from {
        return;
    }
    if pick.is_none() {
        let Some(&me) = game.selected.first() else { return };
        let at = game.sim.folk[me].mover.position_units();
        let mut ds: Vec<(usize, f32)> = game.sim.door_states.iter().enumerate().map(|(k, d)| (k, (d.door.position[0] - at[0]).hypot(d.door.position[2] - at[2]))).collect();
        ds.sort_by(|x, y| x.1.total_cmp(&y.1));
        let Some(&(d, _)) = ds.get(a.first().copied().unwrap_or(0) as usize) else { return };
        let s = &game.sim.door_states[d];
        let name = game.sim.data.get(&s.door.record).map_or(String::new(), |r| r.name.clone());
        let bname = game.sim.data.get(&s.door.building).map_or(String::new(), |r| r.name.clone());
        eprintln!(
            "door show: {d} {name:?} of {bname:?} (placement {}, destroyed {:?}) at {:?}: axis {} dist {} out {} speed {} ends {:?} slide {:?} out {:?}",
            s.door.placement,
            game.sim.placed.get(s.door.placement).map(|p| p.destroyed),
            s.door.position.map(|x| x as i32),
            s.door.axis,
            s.door.dist,
            s.door.out,
            s.door.speed,
            s.door.ends,
            s.door.slide,
            s.door.out_dir
        );
        // the camera before the door, outside: across its slide line, away from its building
        let e = s.door.ends;
        let along = Vec2::new(e[1][0] - e[0][0], e[1][1] - e[0][1]).normalize_or_zero();
        let mut n = Vec2::new(-along.y, along.x);
        if let Some(pl) = game.sim.placed.get(s.door.placement) {
            let away = Vec2::new(s.door.position[0] - pl.position[0], s.door.position[2] - pl.position[2]);
            if n.dot(away) < 0.0 {
                n = -n;
            }
        }
        // (KENSHI_DOOR_SIDE: from the other side)
        if std::env::var("KENSHI_DOOR_SIDE").is_ok() {
            n = -n;
        }
        game.yaw = n.x.atan2(n.y);
        // (KENSHI_DOOR_CAM=distance,pitch)
        let c: Vec<f32> = std::env::var("KENSHI_DOOR_CAM").unwrap_or_default().split(',').filter_map(|x| x.parse().ok()).collect();
        game.dist = c.first().copied().unwrap_or(160.0);
        game.pitch = c.get(1).copied().unwrap_or(0.45);
        *pick = Some(d);
    }
    let Some(d) = *pick else { return };
    let k = (*frames - from) / 60;
    let open = [0.0, 0.5, 1.0, 0.5][k as usize % 4];
    game.sim.door_states[d].open = open;
    let p = Vec3::from(game.sim.door_states[d].door.position) - game.zone.origin;
    game.focus.x = p.x;
    game.focus.z = p.z;
}

/// @fn F029335
///
/// Each door's mesh where its door is: its node at its building's place plus, opened `f`, the
/// slide (`Door::slide`, its collision box's "door axis" column: x, -z or up) times f x "door
/// move dist"; a door that "comes out" first moves out (`Door::out_dir`) for the part |out| /
/// (|dist| x 0.25 + |out|) of its opening, then slides the rest (F029383 sets the directions).
fn move_doors(game: Res<Game>, mut doors: Query<(&kenshi_render::zone::DoorPiece, &mut Transform)>, mut index: Local<(usize, HashMap<(usize, usize), usize>)>) {
    let sim = &game.sim;
    if index.0 != sim.door_states.len() || game.is_added() {
        index.1 = sim.door_states.iter().enumerate().map(|(k, d)| ((d.door.placement, d.door.index), k)).collect();
        index.0 = sim.door_states.len();
    }
    // (testing) KENSHI_DOOR_DEBUG: every 2 s, the door pieces and what they find
    let debug = std::env::var("KENSHI_DOOR_DEBUG").is_ok() && sim.tick.is_multiple_of(120);
    for (dp, mut t) in &mut doors {
        let Some(&off) = sim.zone_offsets.get(&dp.zone) else {
            if debug {
                eprintln!("door piece {:?} b{} i{}: no zone offset", dp.zone, dp.building, dp.index);
            }
            continue;
        };
        if debug {
            let found = index.1.get(&(off + dp.building, dp.index)).map(|&k| (k, sim.door_states[k].door.position.map(|v| v as i32), sim.door_states[k].open));
            let p = (t.translation + game.zone.origin).to_array().map(|v| v as i32);
            eprintln!("door piece {:?} b{} i{} at {p:?}: state {found:?}", dp.zone, dp.building, dp.index);
        }
        let Some(d) = index.1.get(&(off + dp.building, dp.index)).map(|&k| &sim.door_states[k]) else { continue };
        let local = door_offset(&d.door, d.open);
        let to = dp.rest.translation + dp.rest.rotation * local;
        if t.translation != to {
            t.translation = to;
        }
    }
}

/// F029335: a door's offset (building frame) opened `open` (0 .. 1).
fn door_offset(door: &kenshi_world::interior::Door, open: f32) -> Vec3 {
    let (slide, out_dir) = (Vec3::from(door.slide), Vec3::from(door.out_dir));
    let (dist, out) = (door.dist, door.out);
    if out == 0.0 {
        return slide * dist * open;
    }
    let f = out.abs() / (dist.abs() * 0.25 + out.abs());
    if open >= f {
        out_dir * out + slide * dist * ((open - f) / (1.0 - f))
    } else {
        out_dir * (out * open / f)
    }
}

/// The player's gates' doors where their doors are (in their building's frame; F029335 as
/// `move_doors`).
fn move_player_doors(game: Res<Game>, mut doors: Query<(&build_ui::PlayerDoorPiece, &mut Transform)>) {
    for (pd, mut t) in &mut doors {
        let Some(d) = game.sim.door_states.get(pd.0) else { continue };
        let to = door_offset(&d.door, d.open);
        if t.translation != to {
            t.translation = to;
        }
    }
}

/// The buildings opened (zone, placement), the floor they show and the interiors open: the
/// PlayerInterface's set (F045556, +0x1a0) and floor (+0x1e4), and each building's interior timer
/// (Building +0x1f0, its +0x28).
#[derive(Resource, Default)]
pub struct OpenBuildings {
    /// The buildings a selected character stands inside (F045556): they show their floors up to
    /// `floor`; every other building shows all of them (F048854 with floor 4).
    open: std::collections::HashSet<BuildingKey>,
    /// The floor shown, 0 - 3: that of the character the camera follows, whenever theirs changes
    /// (F045539, F045597), or of the selected character it is moved to (F045512), or the one the
    /// floor arrows chose (F045507). Otherwise it stays as it is (a new selection or someone
    /// unfollowed going up the stairs leaves it).
    pub floor: i32,
    /// The floor the HUD's arrows chose (FloorArrowUp / Down), shown from the next frame.
    pub pick: Option<i32>,
    /// The main selected character's floor when the pick was made, and now (for the HUD).
    pub picked_at: i32,
    pub own: i32,
    /// The camera was moved to the selected character (F045512): the floor shown becomes theirs.
    pub focused: bool,
    /// The floor of the character the camera follows, as last seen (PlayerInterface +0x290).
    followed: Option<i32>,
    /// The buildings whose interior is open (its parts and its interior furniture shown,
    /// F037853), each with its timer: 10 s while it is in the set (F045556 / F037858), then running
    /// down with the game's frame time while unpaused (F037727); at 0 it closes (F037854 ->
    /// F037736). (assumption: the check of the timer against 0 was not found; F037854 is what
    /// closes an interior)
    interiors: HashMap<BuildingKey, f32>,
    /// The buildings with a part in view this frame (within the objects view range, F037849):
    /// Kenshi's +0x19d, which their effects follow (F037702).
    pub in_view: std::collections::HashSet<BuildingKey>,
}

impl OpenBuildings {
    /// Whether a building is opened (a selected character inside).
    pub fn open_any(&self) -> bool {
        !self.open.is_empty()
    }
}

/// The building (and its floor) a world position stands inside, if any.
/// A building: its zone and placement index.
type BuildingKey = ((i32, i32), usize);

fn building_at(views: &Query<&BuildingView>, p: [f32; 3]) -> Option<(BuildingKey, i32)> {
    view_at(views, p).map(|v| {
        let local = v.to_local.transform_point3(Vec3::from(p));
        ((v.zone, v.building), v.floor_of(local.y))
    })
}

/// The building whose interior a world position stands inside, if any.
fn view_at<'a>(views: &'a Query<&BuildingView>, p: [f32; 3]) -> Option<&'a BuildingView> {
    let test = [p[0], p[1] + kenshi_world::interior::TEST_HEIGHT, p[2]];
    views.iter().find(|v| v.interior.contains(test))
}

/// Building parts (F045556, F045539, F045512, F048854 / F048853, F037858, F037853, F037727,
/// F037736, F037849, F037815).
/// - A building a selected character stands inside opens (F045556; not a ruin): its parts above
///   the floor shown are hidden. The moment no selected character is inside, it shows all its
///   floors again (F048854 with floor 4): roof and walls are back and nothing inside can be seen.
///   The camera's place plays no part.
/// - The floor shown is that of the character the camera follows whenever theirs changes
///   (F045539, F045597), of the selected character it is moved to (F045512), or the floor
///   arrows' (F045507), 0 - 3.
/// - Its interior (its "interior" parts and its interior layout's furniture, [`InteriorContent`])
///   is shown from its opening until 10 s of game time after it left the set (F037858's timer,
///   run down by F037727 while unpaused); a closed one is hidden (F037736), so it is not seen
///   until a selected character goes in again.
/// - Every part farther from the camera than the "objects view range" is not drawn (Ogre
///   rendering distance, F037849) unless its bounds' radius is over 100 (F037815).
///
/// (assumption: a part's distance is the camera's to its bounds' centre less their radius, as
/// Ogre's culling measures; the character's floor is the highest floor of interior parts under its
/// feet, standing in for the physics floor groups; the halved range of type-12 buildings is not
/// ported; a building's interior opened by a character who is not selected (F044071) is not
/// ported: it changes nothing seen, the building's walls and roof being up)
#[allow(clippy::too_many_arguments)]
fn hide_roofs(
    game: Res<Game>,
    time: Res<Time>,
    sky: Res<Sky>,
    views: Query<&BuildingView>,
    cams: Query<&GlobalTransform, MainCam>,
    mut open_res: ResMut<OpenBuildings>,
    mut pieces: Query<(&BuildingPiece, Option<&BuildingFloor>, Has<kenshi_render::zone::InteriorContent>, &mut Visibility)>,
    rig: Res<camera::CameraRig>,
) {
    // the floor a character stands on (Character slot 0x60; outdoors 0), clamped to 0 - 3 as
    // F045539 / F045512 take it
    let floor_of = |i: usize| building_at(&views, game.sim.folk[i].mover.position_units()).map_or(0, |a| a.1).clamp(0, 3);
    let main = game.selected.first().copied();
    // F045597: following someone, the floor shown is theirs whenever it changes (F045539 as it
    // starts)
    match rig.follow {
        Some(i) if i < game.sim.folk.len() => {
            let f = building_at(&views, game.sim.folk[i].mover.position_units()).map_or(0, |a| a.1);
            if open_res.followed != Some(f) {
                open_res.floor = f.clamp(0, 3);
            }
            open_res.followed = Some(f);
        }
        _ => open_res.followed = None,
    }
    // F045512: the camera moved to the selected character shows their floor
    if std::mem::take(&mut open_res.focused)
        && let Some(i) = main
    {
        open_res.floor = floor_of(i);
    }
    // F045507: the floor arrows
    if let Some(f) = open_res.pick.take() {
        open_res.floor = f.clamp(0, 3);
    }
    open_res.own = main.map_or(0, floor_of);
    // F045556: the buildings a selected character stands inside (with an interior, not a ruin)
    let mut open = std::collections::HashSet::new();
    for &i in &game.selected {
        let p = game.sim.folk[i].mover.position_units();
        if let Some(v) = view_at(&views, p).filter(|v| !v.destroyed) {
            open.insert((v.zone, v.building));
        }
        // inside one of the player's buildings with an interior (within its inner footprint,
        // as interior furniture is placed; (assumption) on its ground floor)
        for (k, pb) in game.sim.buildings.iter().enumerate() {
            if pb.done
                && (pb.at[0] - p[0]).hypot(pb.at[2] - p[2]) < pb.radius * 0.6
                && (p[1] - pb.at[1]).abs() < 60.0
                && game.sim.data.get(&pb.record).is_some_and(|r| !r.refs("interior").is_empty())
            {
                open.insert((build_ui::PLAYER_ZONE, k));
            }
        }
    }
    // the interiors: those in the set held open 10 s (F045556 / F037858); the others' timers run
    // down with the game's frame time, none while paused (F037727), and they close at 0
    let dt = if game.paused { 0.0 } else { time.delta_secs() * game.speed };
    for t in open_res.interiors.values_mut() {
        *t -= dt;
    }
    for &b in &open {
        open_res.interiors.insert(b, 10.0);
    }
    open_res.interiors.retain(|_, t| *t > 0.0);
    let floor = open_res.floor;
    let cam = cams.iter().next().map(|c| c.translation());
    let range = sky.0.objects_view_range;
    // (testing) KENSHI_HIDE_OVER=<k>,...: the k-th of the parts over the first selected hidden
    let hide_over: Vec<usize> = std::env::var("KENSHI_HIDE_OVER").map(|v| v.split(',').filter_map(|k| k.parse().ok()).collect()).unwrap_or_default();
    let sel_at = game.selected.first().map(|&i| Vec3::from(game.sim.folk[i].mover.position_units()) - game.zone.origin);
    let mut over_k = 0;
    open_res.in_view.clear();
    for (piece, b, content, mut vis) in &mut pieces {
        // F048854: an opened building's parts above the floor shown are hidden (any other shows
        // all its floors)
        let mut shut = b.is_some_and(|b| open.contains(&(b.zone, b.building)) && b.floor > floor);
        // F037736: a closed interior's parts and furniture are hidden (the player's buildings'
        // interior parts as well)
        let inner = content || b.is_some_and(|b| b.zone == build_ui::PLAYER_ZONE && b.interior);
        if inner && !b.is_some_and(|b| open_res.interiors.contains_key(&(b.zone, b.building))) {
            shut = true;
        }
        if !hide_over.is_empty()
            && let Some(at) = sel_at
            && piece.lo.x <= at.x && at.x <= piece.hi.x && piece.lo.z <= at.z && at.z <= piece.hi.z && piece.hi.y > at.y + 15.0
        {
            shut |= hide_over.contains(&over_k);
            over_k += 1;
        }
        let radius = (piece.hi - piece.lo).length() * 0.5;
        let far = cam.is_some_and(|c| radius <= 100.0 && c.distance((piece.lo + piece.hi) * 0.5) - radius > range);
        if !far && let Some(b) = b {
            open_res.in_view.insert((b.zone, b.building));
        }
        let want = if shut || far { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
    }
    if std::env::var("KENSHI_ROOF_DEBUG").is_ok() && game.sim.tick.is_multiple_of(120) {
        let inside: Vec<_> = game.selected.iter().map(|&i| (i, building_at(&views, game.sim.folk[i].mover.position_units()))).collect();
        eprintln!("roofs: open {:?} floor {floor} interiors {:?} selection {:?}", open, open_res.interiors, inside);
        if let Some(&i) = game.selected.first() {
            let w = game.sim.folk[i].mover.position_units();
            for v in views.iter() {
                let l = v.to_local.transform_point3(Vec3::from(w));
                if (0..3).all(|k| v.interior.lo[k] - 30.0 <= w[k] && w[k] <= v.interior.hi[k] + 30.0) {
                    eprintln!("   near view {:?}: world {:?} lo {:?} hi {:?} tris {} local {l:.0}", (v.zone, v.building), w, v.interior.lo, v.interior.hi, v.interior.triangles.len());
                }
            }
            let at = Vec3::from(game.sim.folk[i].mover.position_units()) - game.zone.origin; eprintln!("   selection local {at:.0}");
            for (piece, b, content, vis) in pieces.iter() {
                if piece.lo.x <= at.x && at.x <= piece.hi.x && piece.lo.z <= at.z && at.z <= piece.hi.z && piece.hi.y > at.y + 15.0 {
                    eprintln!("   over {i}: {:?} content {content} {:?} {:.0}..{:.0}", b.map(|b| (b.zone, b.building, b.floor, b.interior)), *vis, piece.lo, piece.hi);
                }
            }
        }
    }
    open_res.open = open;
}

/// (testing) KENSHI_INSIDE=<frame>[,<k>]: at that frame the first selected character is put
/// inside the nearest building with an interior (the k-th nearest), on its lowest floor, alone
/// selected, and the camera's focus moves there.
fn inside_hook(mut game: ResMut<Game>, views: Query<&BuildingView>, mut frames: Local<u32>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_INSIDE") else { return };
    let a: Vec<usize> = v.split(',').filter_map(|x| x.parse().ok()).collect();
    if a.first() != Some(&(*frames as usize)) {
        return;
    }
    let Some(&me) = game.selected.first() else { return };
    let here = Vec3::from(game.sim.folk[me].mover.position_units());
    let centre = |v: &BuildingView| (Vec3::from(v.interior.lo) + Vec3::from(v.interior.hi)) * 0.5;
    let mut vs: Vec<&BuildingView> = views.iter().filter(|v| !v.destroyed && !v.levels.is_empty()).collect();
    vs.sort_by(|a, b| centre(a).distance(here).total_cmp(&centre(b).distance(here)));
    let Some(v) = vs.get(a.get(1).copied().unwrap_or(0)) else { return };
    // the lowest floor's height (building frame) and so the world's
    let ly = v.levels.iter().map(|l| l.1).fold(f32::MAX, f32::min);
    let to_world = v.to_local.inverse();
    let (lo, hi) = (Vec3::from(v.interior.lo), Vec3::from(v.interior.hi));
    let c = centre(v);
    // the spot inside nearest the middle, on a 24 x 24 grid over its bounds
    let mut best: Option<(f32, [f32; 3])> = None;
    for i in 0..24 {
        for j in 0..24 {
            let x = lo.x + (hi.x - lo.x) * (i as f32 + 0.5) / 24.0;
            let z = lo.z + (hi.z - lo.z) * (j as f32 + 0.5) / 24.0;
            let l = v.to_local.transform_point3(Vec3::new(x, c.y, z));
            let w = to_world.transform_point3(Vec3::new(l.x, ly + 1.0, l.z));
            let p = w.to_array();
            let d = Vec2::new(x - c.x, z - c.z).length();
            if view_at(&views, p).is_some_and(|o| (o.zone, o.building) == (v.zone, v.building)) && best.is_none_or(|b| d < b.0) {
                best = Some((d, p));
            }
        }
    }
    let Some((_, p)) = best else {
        eprintln!("inside: no spot inside {:?}", (v.zone, v.building));
        return;
    };
    game.sim.folk[me].mover.position = p.map(|x| x * 0.1);
    game.selected = vec![me];
    game.focus = Vec3::from(p) - game.zone.origin;
    eprintln!("inside: {me} put in {:?} ({}) at {p:?}", (v.zone, v.building), v.record);
}

/// @fn F030155
///
/// The local lights shown (the deferred light pass's test, F030155, and F037794's visibility):
/// one out of doors only while the daylight (F040781) is 0.1 or less, an indoor one
/// (`IndoorLight`) while its building's interior is open (`OpenBuildings`).
fn light_visibility(
    game: Res<Game>,
    open: Res<OpenBuildings>,
    mut lights: Query<(Option<&kenshi_render::local_lights::IndoorLight>, &mut Visibility), With<kenshi_render::local_lights::LocalLight>>,
) {
    let hour = game.sim.hour() as f32;
    let day = kenshi_ai::perception::daylight(hour, game.sim.sunrise, game.sim.sunset) > 0.1;
    for (inside, mut vis) in &mut lights {
        let shown = match inside {
            Some(l) => open.interiors.contains_key(&(l.zone, l.building)),
            None => !day,
        };
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

/// People (F039194, F037443 / F037452): a character farther from the camera than the "npc range"
/// is not drawn (player characters always are; the 7x range of F037452's flagged bodies is not
/// ported), nor one inside an opened building on a floor above the one shown.
fn people_ranges(
    game: Res<Game>,
    sky: Res<Sky>,
    views: Query<&BuildingView>,
    cams: Query<&GlobalTransform, MainCam>,
    open: Res<OpenBuildings>,
    mut vis: Query<&mut Visibility>,
) {
    let Some(cam) = cams.iter().next().map(|c| c.translation()) else { return };
    let origin = game.zone.origin;
    let range = sky.0.npc_range;
    for p in &game.sim.folk {
        let Some(&e) = game.bodies.get(p.id) else { continue };
        let Ok(mut v) = vis.get_mut(e) else { continue };
        let w = p.mover.position_units();
        let far = !p.player && cam.distance(Vec3::from(w) - origin) > range;
        let upstairs = !open.open.is_empty()
            && building_at(&views, w).is_some_and(|(b, f)| open.open.contains(&b) && f > open.floor);
        let want = if far || upstairs { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
}


impl Game {
    /// The zone (x, y) a scene point lies in.
    fn zone_of(&self, x: f32, z: f32) -> (i32, i32) {
        let w = Vec3::new(x, 0.0, z) + self.zone.origin;
        let k = |v: f32| ((v + WORLD_SIZE / 2.0) / ZONE_SIZE).floor() as i32;
        (k(w.x), k(w.z))
    }

    /// The ground's height at scene (x, z): from the streamed zone holding it, else the first.
    pub(crate) fn ground(&self, x: f32, z: f32) -> f32 {
        match self.zones.get(&self.zone_of(x, z)) {
            Some(zs) => ground_on(&zs.patch, self.zone.origin, x, z),
            None => self.zone.ground(x, z),
        }
    }
}

/// (harness: Kenshi pages its world round the camera) the zones Kenshi keeps loaded (the sim's
/// `world::zones_tick`: the camera's current zone and its neighbours, the player's people's
/// zones, each kept for its timer) load on threads, placed on the first zone's origin, and are
/// spawned as they arrive; one whose timer runs out unloads.
#[allow(clippy::too_many_arguments)]
fn stream_zones(
    mut commands: Commands,
    mut game: ResMut<Game>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut object_materials: ResMut<Assets<KenshiObjectMaterial>>,
    mut terrain_materials: ResMut<Assets<KenshiTerrainMaterial>>,
    mut look: ResMut<TerrainLook>,
    mut waters: ResMut<Assets<KenshiWaterMaterial>>,
    (sky, time, tagged): (Res<Sky>, Res<Time>, Query<(Entity, &ZoneTag)>),
) {
    // the zones Kenshi keeps loaded (the sim's `zones_tick`, Kenshi's rule) by real time
    let focus = game.focus + game.zone.origin;
    game.sim.camera_focus = Some([focus.x, focus.y, focus.z]);
    game.sim.fast_zone_hopping = sky.0.fast_zone_hopping;
    game.sim.zones_driven = true;
    let (_, expired) = game.sim.zones_tick(time.delta_secs());
    let want: std::collections::HashSet<(i32, i32)> = game.sim.live_zones.keys().copied().collect();
    // a save loaded is done loading once its zones are in (ZoneManager +0x08, F051615)
    if game.sim.loading && want.iter().all(|k| game.zones.contains_key(k)) {
        game.sim.loading = false;
    }
    for key in expired {
        game.asked.remove(&key);
        game.zones.remove(&key);
        let mut n = 0;
        for (e, z) in &tagged {
            if z.0 == key {
                commands.entity(e).despawn();
                n += 1;
            }
        }
        eprintln!("zone {}.{} unloaded ({n} entities)", key.0, key.1);
        rebuild_statics(&mut game);
    }
    for key in want {
        {
            if !game.asked.insert(key) {
                continue;
            }
            let foliage = game.foliage.clone();
            let (data, res, root, origin, tx) = (
                game.sim.data.clone(),
                game.res.clone(),
                game.root.clone(),
                game.zone.origin,
                game.loader.clone(),
            );
            spawn_worker(move || {
                let r = Terrain::open(&root)
                    .map_err(|e| e.to_string())
                    .and_then(|mut t| {
                        load_zone_shared(&root, &data, &res, &mut t, key, 0.0, origin)
                    })
                    .map(|mut zs| {
                        if let Some(f) = &foliage {
                            zs.add_foliage(&root, f, key);
                        }
                        zs
                    });
                let _ = tx.send((key, r));
            });
        }
    }
    let arrived: Vec<Loaded> = game
        .loaded
        .lock()
        .map(|rx| rx.try_iter().collect())
        .unwrap_or_default();
    for (key, r) in arrived {
        // unloaded again while it loaded
        if !game.sim.live_zones.contains_key(&key) {
            game.asked.remove(&key);
            continue;
        }
        match r {
            Ok(mut zs) => {
                if !old_terrain() {
                    zs.terrain.clear();
                }
                zs.spawn_kenshi(
                    &mut commands,
                    &assets,
                    &mut meshes,
                    &mut object_materials,
                    &mut terrain_materials,
                    &mut look,
                );
                eprintln!("zone {}.{} streamed in", key.0, key.1);
                spawn_zone_lights(&mut commands, &game.sim.data, &zs.lights, &zs.lights_inside, zs.zone);
                let o = zs.origin;
                game.new_effects.extend(zs.effects.iter().zip(&zs.effects_of).map(|((e, at, r), &b)| (e.clone(), *at + o, *r, Some((zs.zone, b)))));
                spawn_zone_water(&mut commands, &assets, &mut meshes, &mut waters, &mut look, &zs, key);
                game.zones.insert(key, zs);
                rebuild_statics(&mut game);
                // the zone's buildings and residents into the simulation (once per zone)
                let res = game.res.clone();
                game.sim.add_zone(key.0, key.1, &res);
            }
            Err(e) => eprintln!("zone {}.{}: {e}", key.0, key.1),
        }
    }
}

/// A zone's lights (kenshi_render::local_lights), gone with the zone.
fn spawn_zone_lights(commands: &mut Commands, data: &kenshi_data::GameDataContainer, lights: &[(String, Vec3, Quat)], inside: &[Option<usize>], zone: (i32, i32)) {
    let mut n = 0;
    for (k, (light, at, rot)) in lights.iter().enumerate() {
        if let Some(e) = kenshi_render::local_lights::spawn_light(commands, data, light, *at, *rot) {
            commands.entity(e).insert(ZoneTag(zone));
            // an indoor light (F037795 / F037794): hidden till its building's interior opens
            if let Some(&Some(building)) = inside.get(k) {
                commands.entity(e).insert((kenshi_render::local_lights::IndoorLight { zone, building }, Visibility::Hidden));
            }
            n += 1;
        }
    }
    if n > 0 {
        eprintln!("zone {}.{}: {n} lights", zone.0, zone.1);
    }
}

/// Bodies for the people the simulation has that have none yet (start, and squads spawned later).
/// A body for person `id` in outfit `k`: the character, its sword on the prop bone.
fn spawn_body(commands: &mut Commands, game: &Game, id: usize, k: usize) -> Entity {
    let origin = game.zone.origin;
    let p = &game.sim.folk[id];
    let at = Transform::from_translation(Vec3::from(p.mover.position_units()) - origin);
    let (root, joints) = spawn_character_with_joints(
        commands,
        &game.outfits[k],
        &game.uploads[k],
        at,
        "idle_stand_relax",
        p.id as f32 * 0.37,
        None,
    );
    // the joints sheathed weapons hang from (ranged_view::AttachPoints)
    for (j, name) in game.outfits[k].rig.names.iter().enumerate() {
        if (name == "Bip01 Pelvis" || name == "Bip01 Spine2") && j < joints.len() {
            commands.entity(joints[j]).insert(ranged_view::AttachBone(id, name.clone()));
        }
    }
    let prop = game.outfits[k].rig.names.iter().position(|n| n == "Bip01 Prop2");
    if let (Some((hs, mat)), Some(j)) = (&game.sword, prop) {
        for h in hs {
            let e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(mat.clone()), Transform::default(), ranged_view::SwordOf(id))).id();
            commands.entity(joints[j]).add_child(e);
        }
    }
    root
}

/// Bodies for the people the simulation has that have none yet (start, and squads spawned later).
fn spawn_bodies(commands: &mut Commands, game: &mut Game) {
    while game.bodies.len() < game.sim.folk.len() {
        let id = game.bodies.len();
        let p = &game.sim.folk[id];
        let n = game.outfits.len();
        let mut k = if p.player { 0 } else { (1 + p.id % (n - 1).max(1)).min(n - 1) };
        // an animal: its race's body, loaded the first time (rebody)
        if let Some(race) = p.race.as_deref().and_then(|r| game.sim.data.get(r)).map(|r| r.name.clone()) {
            match game.race_outfits.get(&race) {
                Some(&rk) => k = rk,
                None => game.rebody.push((id, CharacterSpec::new(&race))),
            }
        }
        let root = spawn_body(commands, game, id, k);
        game.bodies.push(root);
        game.body_outfit.push(k);
        game.body_scale.push(1.0);
        // their own look, made a few a frame (`dress`); the editor's looks are kept
        if p.race.is_none() && !game.looks.contains_key(&id) {
            game.dress_queue.push_back(id);
        }
    }
    let n = game.sim.folk.len();
    game.playing.resize(n, String::new());
}

/// People given a new look (the character editor): their body loaded, uploaded and swapped in.
#[allow(clippy::too_many_arguments)]
fn rebody(
    mut commands: Commands,
    mut game: ResMut<Game>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut characters: ResMut<Assets<KenshiCharacterMaterial>>,
    mut look: ResMut<TerrainLook>,
) {
    let game = &mut *game;
    let origin = game.zone.origin;
    for (id, spec) in std::mem::take(&mut game.rebody) {
        let plain = spec.head.is_none() && spec.hairs.is_empty() && spec.wear.is_empty();
        if plain && let Some(&k) = game.race_outfits.get(&spec.race) {
            if let Some(&old) = game.bodies.get(id) {
                commands.entity(old).despawn();
            }
            let root = spawn_body(&mut commands, game, id, k);
            game.bodies[id] = root;
            game.body_outfit[id] = k;
            continue;
        }
        let mut asset = match load_character(&game.root, &game.sim.data, &game.res, &spec) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("look for {id} not made: {e}");
                continue;
            }
        };
        strip_tracks(&mut asset, &game.sim.data);
        let handles = upload_character_with(&asset, &mut meshes, &mut bindposes, |surface| {
            characters.add(look.character(&assets, &mut images, surface, origin))
        });
        game.ragdoll_rigs.push(asset.ragdoll.as_ref().map(|r| Arc::new(RagdollRig::new(r, &asset.rig))));
        game.outfits.push(asset);
        game.uploads.push(handles);
        let k = game.outfits.len() - 1;
        if plain && game.sim.folk.get(id).is_some_and(|p| p.race.is_some()) {
            game.race_outfits.insert(spec.race.clone(), k);
        }
        if let Some(&old) = game.bodies.get(id) {
            commands.entity(old).despawn();
        }
        let root = spawn_body(&mut commands, game, id, k);
        game.bodies[id] = root;
        game.body_outfit[id] = k;
    }
}

/// People given their own look (`looks::look_of`), up to three a frame: a look already made is
/// shared; otherwise its body is loaded, sharing the skeleton and animations of the first of its
/// race and sex.
#[allow(clippy::too_many_arguments)]
fn dress(
    mut commands: Commands,
    mut game: ResMut<Game>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut characters: ResMut<Assets<KenshiCharacterMaterial>>,
    mut look: ResMut<TerrainLook>,
) {
    let game = &mut *game;
    let origin = game.zone.origin;
    // those whose clothes changed (put on, taken off, looted) are dressed again
    let changed: Vec<usize> = game
        .worn
        .iter()
        .filter(|(id, was)| game.sim.folk.get(**id).is_some_and(|p| looks::worn(p) != **was))
        .map(|(id, _)| *id)
        .collect();
    for id in changed {
        game.worn.remove(&id);
        if !game.dress_queue.contains(&id) {
            game.dress_queue.push_back(id);
        }
    }
    let mut loads = 0;
    while loads < 3 {
        let Some(id) = game.dress_queue.pop_front() else { break };
        if game.looks.contains_key(&id) || id >= game.bodies.len() {
            continue;
        }
        let Some(spec) = looks::look_of(&game.sim.data, &game.root, &game.sim.folk[id], "Greenlander") else { continue };
        // a named character's body sliders (its body file), as the editor's are applied
        if !game.body_values.contains_key(&id)
            && let Some(c) = game.sim.folk[id].character.clone()
            && let Some(body) = kenshi_game::body_file::body_of(&game.root, &game.sim.data, &c)
        {
            game.body_values.insert(id, body.values.iter().cloned().collect());
        }
        game.worn.insert(id, looks::worn(&game.sim.folk[id]));
        let key = looks::key(&spec);
        if std::env::var("KENSHI_LOOK_DEBUG").is_ok() {
            eprintln!("look for {id} ({}): {key}", game.sim.folk[id].name);
        }
        let k = match game.looks_made.get(&key) {
            Some(&k) => k,
            None => {
                loads += 1;
                let base = game.look_base.get(&(spec.race.clone(), spec.female)).copied();
                let shared = base.map(|b| &game.outfits[b]);
                let mut asset = match kenshi_render::character::load_character_shared(&game.root, &game.sim.data, &game.res, &spec, shared) {
                    Ok(a) => a,
                    Err(e) => {
                        eprintln!("look for {id} not made: {e}");
                        continue;
                    }
                };
                if base.is_none() {
                    strip_tracks(&mut asset, &game.sim.data);
                }
                let handles = upload_character_with(&asset, &mut meshes, &mut bindposes, |surface| {
                    characters.add(look.character(&assets, &mut images, surface, origin))
                });
                match base {
                    Some(b) => {
                        let r = game.ragdoll_rigs[b].clone();
                        game.ragdoll_rigs.push(r);
                    }
                    None => {
                        game.ragdoll_rigs.push(asset.ragdoll.as_ref().map(|r| Arc::new(RagdollRig::new(r, &asset.rig))));
                    }
                }
                game.outfits.push(asset);
                game.uploads.push(handles);
                let k = game.outfits.len() - 1;
                if base.is_none() {
                    game.look_base.insert((spec.race.clone(), spec.female), k);
                }
                game.looks_made.insert(key, k);
                k
            }
        };
        if game.body_outfit.get(id) == Some(&k) {
            continue;
        }
        commands.entity(game.bodies[id]).despawn();
        let root = spawn_body(&mut commands, game, id, k);
        game.bodies[id] = root;
        game.body_outfit[id] = k;
        game.playing[id].clear();
    }
}

/// Each body at its person's size (the editor's Height).
fn body_scales(game: Res<Game>, mut bodies: Query<&mut Transform>) {
    for (id, &e) in game.bodies.iter().enumerate() {
        let s = game.body_scale.get(id).copied().unwrap_or(1.0);
        if let Ok(mut t) = bodies.get_mut(e) {
            t.scale = Vec3::splat(s);
        }
    }
}

fn new_bodies(mut commands: Commands, mut game: ResMut<Game>) {
    if game.bodies.len() < game.sim.folk.len() {
        spawn_bodies(&mut commands, &mut game);
    }
}

#[derive(Resource)]
pub(crate) struct Sky(pub(crate) SkySettings);

/// The "View Distance" option in effect (F032930's callback): the sky's and fog's distance, the
/// camera's far clip (x 10, F041473) and the sky dome kept inside it. (stand-in) The water's
/// reflection camera keeps the distance it was made with until the game is next loaded.
pub(crate) fn set_view_distance(world: &mut World, v: f32) {
    if let Some(mut sky) = world.get_resource_mut::<Sky>() {
        sky.0.view_distance = v;
    }
    let far = world.get_resource::<Sky>().map_or(v * 10.0, |s| s.0.fog_params().x);
    let mut old = None;
    let mut cams = world.query_filtered::<&mut Projection, MainCam>();
    for mut p in cams.iter_mut(world) {
        if let Projection::Perspective(pp) = &mut *p {
            old = Some(pp.far);
            pp.far = far;
        }
    }
    if let Some(old) = old.filter(|o| *o > 0.0) {
        let mut domes = world.query_filtered::<&mut Transform, With<kenshi_render::sky_dome::SkyDome>>();
        for mut t in domes.iter_mut(world) {
            t.scale *= far / old;
        }
    }
}

/// An option the sky settings or the HDR chain hold, in effect as it changes ("npc range",
/// "objects view range", "Fast zone hopping": read each frame; "HeatHaze", "FXAA": the
/// compositors, at the window's close; "Shadow Range": the shadow camera's, each frame,
/// F046665).
pub(crate) fn set_sky_option(world: &mut World, key: &str, v: f32) {
    if let Some(mut sky) = world.get_resource_mut::<Sky>() {
        match key {
            "npc range" => sky.0.npc_range = v,
            "objects view range" => sky.0.objects_view_range = v,
            "Fast zone hopping" => sky.0.fast_zone_hopping = v != 0.0,
            "HeatHaze" => sky.0.heat_haze = v != 0.0,
            "FXAA" => sky.0.fxaa = v != 0.0,
            "Shadow Range" => sky.0.shadow_range = v,
            _ => {}
        }
    }
    if let Some(mut hdr) = world.get_resource_mut::<kenshi_render::hdr::KenshiHdr>() {
        match key {
            "HeatHaze" => hdr.haze_enabled = v != 0.0,
            "FXAA" => hdr.fxaa = v != 0.0,
            _ => {}
        }
    }
    if key == "Shadow Range" {
        let mut suns = world.query_filtered::<Entity, With<DirectionalLight>>();
        let suns: Vec<Entity> = suns.iter(world).collect();
        for e in suns {
            world.entity_mut(e).insert(kenshi_render::shadows::cascade_config(v));
        }
    }
}

/// The shadows' mode and quality in effect (F032951's close: F0125df with the mode and
/// 1024, 2048 or 4096): the sun casting or not, its maps' size.
pub(crate) fn set_shadows(world: &mut World, mode: i32, quality: i32) {
    if let Some(mut s) = world.get_resource_mut::<Sky>() {
        s.0.shadow_mode = mode;
        s.0.shadow_quality = quality;
    }
    world.insert_resource(bevy::light::DirectionalLightShadowMap { size: kenshi_render::shadows::cascade_size(quality) as usize });
    let mut suns = world.query::<&mut DirectionalLight>();
    for mut l in suns.iter_mut(world) {
        l.shadow_maps_enabled = mode != 0;
    }
}

/// The options' "Font Size" in effect (F032942 at the window's close): every font its size plus
/// "font size2"; the texts shown in one of Kenshi's fonts at its old size made the new.
pub(crate) fn set_font_size(world: &mut World, size2: f32) {
    let old = kenshi_gui::res::font_size2();
    if (old - size2).abs() < 0.01 {
        return;
    }
    kenshi_gui::res::set_font_size2(size2);
    let Some(gui) = world.get_resource::<kenshi_gui::GuiRes>() else { return };
    let defs: Vec<(String, f32, f32)> = gui.0.fonts.values().map(|f| (f.file.clone(), f.screen_px_at(old), f.screen_px_at(size2))).collect();
    let paths: std::collections::HashMap<AssetId<Font>, String> = {
        let assets = world.resource::<AssetServer>().clone();
        let mut q = world.query::<&TextFont>();
        q.iter(world)
            .filter_map(|t| match &t.font {
                bevy::text::FontSource::Handle(h) => assets.get_path(h.id()).map(|p| (h.id(), p.path().to_string_lossy().replace('\\', "/"))),
                _ => None,
            })
            .collect()
    };
    let mut q = world.query::<&mut TextFont>();
    for mut t in q.iter_mut(world) {
        let bevy::text::FontSource::Handle(h) = &t.font else { continue };
        let Some(path) = paths.get(&h.id()) else { continue };
        let bevy::text::FontSize::Px(px) = t.font_size else { continue };
        if let Some((_, _, new)) = defs.iter().find(|(f, was, _)| path.ends_with(f.replace('\\', "/").as_str()) && (was - px).abs() < 0.6) {
            t.font_size = bevy::text::FontSize::Px(*new);
        }
    }
}

/// The sun (and so its shadows) follows the game clock every frame, as SkyX moves it each
/// update; the materials' lighting (`relight`) is redone each game minute.
fn sun_follow(game: Res<Game>, sky: Res<Sky>, mut suns: Query<&mut Transform, With<DirectionalLight>>) {
    let dir = sky.0.main_light(game.sim.hour() as f32).0;
    for mut t in &mut suns {
        *t = Transform::default().looking_to(-dir, Vec3::Y);
    }
}

/// The stars' drift (kenshi_render::sky_dome::drift_stars), each frame.
fn stars(game: Res<Game>, time: Res<Time>, mut skies: ResMut<Assets<KenshiSkyMaterial>>, mut offset: Local<f32>) {
    kenshi_render::sky_dome::drift_stars(&mut offset, time.delta_secs(), game.speed, game.paused, &mut skies);
}

/// The main light follows the game clock: recomputed each game minute.
#[allow(clippy::too_many_arguments)]
fn relight(
    game: Res<Game>,
    sky: Res<Sky>,
    mut look: ResMut<TerrainLook>,
    mut skies: ResMut<Assets<KenshiSkyMaterial>>,
    mut moons: ResMut<Assets<KenshiMoonMaterial>>,
    mut suns: Query<&mut Transform, With<DirectionalLight>>,
    mut hdr: ResMut<kenshi_render::hdr::KenshiHdr>,
    mut particles: ResMut<kenshi_render::particles::render::Particles>,
    mut last: Local<i64>,
) {
    let hour = game.sim.hour();
    let minute = (hour * 60.0) as i64;
    // (testing) KENSHI_RELIGHT_EACH_FRAME: the light set again every frame (to time a relight)
    if minute == *last && std::env::var("KENSHI_RELIGHT_EACH_FRAME").is_err() {
        return;
    }
    *last = minute;
    let mut prof = kenshi_render::prof::Marks::start();
    let light = sky.0.terrain_light(hour as f32);
    // MIN_LUMINANCE (F040805) and MAX_LUMINANCE ("exposure max", F040804)
    hdr.min_luminance = sky.0.main_light(hour as f32).3;
    hdr.max_luminance = sky.0.exposure_max;
    // the terrain, objects, characters and water: their shared light (kenshi_render::light_buffer)
    look.relight(light);
    for (_, m) in skies.iter_mut() {
        m.set_light(&light);
    }
    relight_moons(&light, &mut moons);
    // the ambient particles' sunDirection
    particles.sun_y = light.sun_direction.y;
    for mut t in &mut suns {
        *t = Transform::default().looking_to(-light.sun_direction, Vec3::Y);
    }
    prof.mark("relight");
}

/// The particle systems' frame: each running system updated by the frame's time.
/// (testing) `KENSHI_PARTICLE=<system>` starts that system at the focus, once.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn tick_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut game: ResMut<Game>,
    particles: Res<kenshi_render::particles::render::Particles>,
    mut effects: Query<&mut kenshi_render::particles::render::ParticleEffect>,
    mut spawned: Local<bool>,
    mut weather_fx: ResMut<kenshi_render::particles::effects::Effects>,
    weather: Res<WeatherState>,
    sky: Res<Sky>,
    cams: Query<&GlobalTransform, MainCam>,
    mut region_zones: Local<Option<Vec<Vec<(i32, i32)>>>>,
    mut terrain: Local<Option<kenshi_world::Terrain>>,
    open: Res<OpenBuildings>,
) {
    if !*spawned {
        *spawned = true;
        if let Ok(name) = std::env::var("KENSHI_PARTICLE") {
            match particles.instance(&name, 1) {
                Some(mut s) => {
                    s.position = game.focus;
                    s.prepare();
                    s.update(0.0);
                    s.start();
                    commands.spawn(kenshi_render::particles::render::ParticleEffect::new(s));
                }
                None => eprintln!("no particle system {name}"),
            }
        }
    }
    // the game's frame time (none while paused)
    let dt = time.delta_secs() * if game.paused { 0.0 } else { game.speed };
    let mut prof = kenshi_render::prof::Marks::start();
    for mut e in &mut effects {
        e.system.update(dt);
    }
    prof.mark("particles: systems");
    // F037702: the buildings' effects follow their buildings' view
    weather_fx.follow_views(&|k| open.in_view.contains(&k));
    for (id, at, rot, building) in std::mem::take(&mut game.new_effects) {
        if let Some(e) = weather_fx.effect_index(&id) {
            if std::env::var("KENSHI_FOOT_DEBUG").is_ok() {
                eprintln!("building effect {id} at {at:.0}");
            }
            weather_fx.attach_to(e, at, rot, building);
        }
    }
    weather_effects(dt, &game, &weather, &sky, &cams, &mut weather_fx, &mut region_zones, &mut terrain);
    prof.mark("particles: weather and buildings");
    // the effects that hurt, for the sim (kenshi_game::hazards)
    game.sim.harms = weather_fx
        .harmful()
        .into_iter()
        .map(|(affect, at, radius, strength)| kenshi_game::hazards::Harm { affect, at: at.to_array(), radius, strength })
        .collect();
}

/// The running effects' fog volumes (kenshi_render::fog_volume): a pool of hulls, one per
/// volume this frame, the rest hidden.
#[allow(clippy::too_many_arguments)]
fn fog_volumes(
    mut commands: Commands,
    game: Res<Game>,
    mut weather_fx: ResMut<kenshi_render::particles::effects::Effects>,
    look: Res<TerrainLook>,
    mut materials: ResMut<Assets<kenshi_render::fog_volume::KenshiFogVolumeMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut hulls: Query<(&mut Mesh3d, &mut Transform, &mut Visibility)>,
    mut pool: Local<Vec<(Entity, Handle<kenshi_render::fog_volume::KenshiFogVolumeMaterial>)>>,
    mut shapes: Local<Option<(Handle<Mesh>, Handle<Mesh>)>>,
) {
    use kenshi_render::fog_volume::{KenshiFogVolumeMaterial, VolumeShape};
    // a game loaded in the program: the hulls went with the last
    if game.is_added() {
        pool.clear();
    }
    let origin = game.zone.origin;
    let draws = weather_fx.fog_volumes(origin);
    // (testing) KENSHI_FOG_DEBUG: the volumes, now and then
    if std::env::var("KENSHI_FOG_DEBUG").is_ok() && game.sim.tick % 120 == 0 {
        let with_fog: Vec<String> = weather_fx.defs.iter().filter(|d| !d.fog.is_empty()).map(|d| d.id.clone()).collect();
        eprintln!("fog: effects with volumes {with_fog:?}, running {}", weather_fx.running());
        eprintln!("fog: groups {:?}", weather_fx.debug_groups());
        for d in &draws {
            eprintln!("fog volume at {:.0} (world), r {}, fade {:.2}", d.at + origin, d.def.radius, d.fade);
        }
    }
    if draws.is_empty() && pool.is_empty() {
        return;
    }
    // unit_sphere.mesh and cylinder.mesh as the shader's hull: a sphere of radius 1 and a
    // cylinder of radius 1 and height 1 round its middle
    let (sphere, cylinder) = shapes
        .get_or_insert_with(|| (meshes.add(Sphere::new(1.0).mesh().ico(3).unwrap()), meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(24))))
        .clone();
    for (k, d) in draws.iter().enumerate() {
        let (m, transform) = KenshiFogVolumeMaterial::for_volume(d, &look.light, origin);
        let mesh = if d.def.shape == VolumeShape::Sphere { sphere.clone() } else { cylinder.clone() };
        if k >= pool.len() {
            let h = materials.add(m);
            let e = commands
                .spawn((Mesh3d(mesh), MeshMaterial3d(h.clone()), transform, Visibility::Visible, bevy::light::NotShadowCaster))
                .id();
            pool.push((e, h));
            continue;
        }
        let (e, h) = &pool[k];
        if let Some(mut mat) = materials.get_mut(h) {
            *mat = m;
        }
        if let Ok((mut m3, mut t, mut v)) = hulls.get_mut(*e) {
            if m3.0 != mesh {
                m3.0 = mesh;
            }
            *t = transform;
            *v = Visibility::Visible;
        }
    }
    for (e, _) in pool.iter().skip(draws.len()) {
        if let Ok((_, _, mut v)) = hulls.get_mut(*e) {
            *v = Visibility::Hidden;
        }
    }
}

/// A fog feature's drawing: its material, and for a sphere or cylinder the volume it is drawn as
/// (its colour lit anew with the light) and its edge.
type FogFeatureDraw = (Handle<kenshi_render::fog_volume::KenshiFogVolumeMaterial>, Option<(kenshi_render::fog_volume::FogVolumeDraw, f32)>);

/// The map's fog features (kenshi_world::fog_features: the swamps' mists and the other standing
/// fogs), made once when the world is ready (F046692 -> F023589), lit again as the light changes.
/// (testing) KENSHI_NO_FOG_FEATURES leaves them out.
fn fog_features(
    mut commands: Commands,
    game: Res<Game>,
    look: Res<TerrainLook>,
    mut materials: ResMut<Assets<kenshi_render::fog_volume::KenshiFogVolumeMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut made: Local<Option<Vec<FogFeatureDraw>>>,
    mut lit: Local<Vec4>,
) {
    use kenshi_render::fog_volume::{FogVolumeDef, FogVolumeDraw, KenshiFogVolumeMaterial, VolumeShape};
    use kenshi_world::fog_features::FogShape;
    // a game loaded in the program: made again (at its origin)
    if game.is_added() {
        *made = None;
        *lit = Vec4::ZERO;
    }
    let origin = game.zone.origin;
    let made = made.get_or_insert_with(|| {
        let mut out = Vec::new();
        if std::env::var("KENSHI_NO_FOG_FEATURES").is_ok() {
            return out;
        }
        let sphere = meshes.add(Sphere::new(1.0).mesh().ico(3).unwrap());
        let cylinder = meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(24));
        for f in kenshi_world::fog_features::load(&game.root) {
            let volume = |shape, start: Vec3, end: Vec3, radius: f32| FogVolumeDraw {
                def: FogVolumeDef { shape, offset: Vec3::ZERO, offset2: end - start, radius, distance: f.distance, colour: Vec4::from(f.colour), additive: false },
                at: start - origin,
                fade: 1.0,
            };
            match &f.shape {
                FogShape::Planes { edge, planes } => {
                    if let Some((m, mesh)) = KenshiFogVolumeMaterial::for_planes(f.colour, f.distance, *edge, planes, &look.light, origin) {
                        let h = materials.add(m);
                        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(h.clone()), Transform::IDENTITY, bevy::light::NotShadowCaster));
                        out.push((h, None));
                    }
                }
                FogShape::Sphere { centre, radius } => {
                    let d = volume(VolumeShape::Sphere, Vec3::from(*centre), Vec3::from(*centre), *radius);
                    let (m, t) = KenshiFogVolumeMaterial::for_volume(&d, &look.light, origin);
                    let h = materials.add(m);
                    commands.spawn((Mesh3d(sphere.clone()), MeshMaterial3d(h.clone()), t, bevy::light::NotShadowCaster));
                    out.push((h, Some((d, 0.0))));
                }
                FogShape::Cylinder { start, end, radius, edge } => {
                    let d = volume(VolumeShape::Cylinder, Vec3::from(*start), Vec3::from(*end), *radius);
                    let (mut m, t) = KenshiFogVolumeMaterial::for_volume(&d, &look.light, origin);
                    // F023566: edgeBlur 1 / the feature's edge
                    m.params.consts.y = 1.0 / edge.max(1.0);
                    let h = materials.add(m);
                    commands.spawn((Mesh3d(cylinder.clone()), MeshMaterial3d(h.clone()), t, bevy::light::NotShadowCaster));
                    out.push((h, Some((d, 1.0 / edge.max(1.0)))));
                }
            }
        }
        out
    });
    let l = &look.light;
    let now = Vec4::new(l.sun_colour.w, l.sun_real.y, l.fog.w, l.horizon.w);
    if now == *lit {
        return;
    }
    *lit = now;
    for (h, volume) in made.iter() {
        let Some(mut m) = materials.get_mut(h) else { continue };
        match volume {
            None => m.params.set_light(l),
            Some((d, edge)) => {
                let (fresh, _) = KenshiFogVolumeMaterial::for_volume(d, l, origin);
                m.params = fresh.params;
                if *edge > 0.0 {
                    m.params.consts.y = *edge;
                }
            }
        }
    }
}

/// An image's width and RGBA bytes.
type Rgba = (u32, Vec<u8>);

/// One ANIMATION_EVENT of a clip: the fraction of the clip it falls at, its bone, whether it
/// makes a ground effect, and the sound event it posts (with its switch override).
struct AnimEvent {
    at: f32,
    bone: String,
    ground: bool,
    sound: Option<(String, Option<(String, String)>)>,
}

/// ANIMATION records' events by clip name.
#[derive(Default)]
struct FootEvents {
    by_clip: HashMap<String, Vec<AnimEvent>>,
    /// Per person and clip: the phase last frame.
    last: HashMap<(usize, usize), f32>,
    /// The overlay map tiles (new_overlay.X.Y.png), as Ogre has them in memory (BGRA).
    overlays: HashMap<(i32, i32), Option<Rgba>>,
    terrain: Option<kenshi_world::Terrain>,
}

impl FootEvents {
    /// "events" of each ANIMATION: ANIMATION_EVENT records, at their value percent through
    /// the clip: their bone, whether a "ground effect", and the sound "event" they post with
    /// their "override state" / "override value" switch. (The combat cues post the attacks'
    /// and reloads' own: "Attack" and "Reload" are left to them.)
    fn new(data: &kenshi_data::GameDataContainer) -> FootEvents {
        let mut by_clip: HashMap<String, Vec<AnimEvent>> = HashMap::new();
        for r in data.of_type(kenshi_data::ItemType::Animation) {
            let Some(name) = r.strings.get("anim name") else { continue };
            for (id, v) in r.refs("events") {
                let Some(ev) = data.get(id) else { continue };
                let ground = ev.bools.get("ground effect").copied().unwrap_or(false);
                let sound = ev.strings.get("event").filter(|e| !e.is_empty() && *e != "Attack" && *e != "Reload").map(|e| {
                    let over = ev.strings.get("override state").zip(ev.strings.get("override value")).filter(|(s, v)| !s.is_empty() && !v.is_empty()).map(|(s, v)| (s.clone(), v.clone()));
                    (e.clone(), over)
                });
                if !ground && sound.is_none() {
                    continue;
                }
                let bone = ev.strings.get("bone").cloned().unwrap_or_default();
                let at = v[0] as f32 * 0.01;
                let list = by_clip.entry(name.to_lowercase()).or_default();
                if !list.iter().any(|e| e.at == at && e.bone == bone && e.sound.as_ref().map(|s| &s.0) == sound.as_ref().map(|s| &s.0)) {
                    list.push(AnimEvent { at, bone, ground, sound });
                }
            }
        }
        FootEvents { by_clip, ..Default::default() }
    }

    /// @fn F051634
    ///
    /// The terrain layer under a point: the overlay map's pixel there, alpha over 0xa0 4, red
    /// 3, green 2, blue 2 (as Kenshi has it), else 0; None off the map or where no zone is
    /// loaded.
    fn layer(&mut self, root: &std::path::Path, x: f32, z: f32) -> Option<usize> {
        const TILE: f32 = kenshi_world::WORLD_SIZE / 8.0;
        let (fx, fz) = ((x + kenshi_world::WORLD_SIZE / 2.0) / TILE, (z + kenshi_world::WORLD_SIZE / 2.0) / TILE);
        let (tx, tz) = (fx.floor() as i32, fz.floor() as i32);
        if !(0..8).contains(&tx) || !(0..8).contains(&tz) {
            return None;
        }
        let tile = self.overlays.entry((tx, tz)).or_insert_with(|| {
            let p = root.join(format!("data/newland/land/overlaymaps/new_overlay.{tx}.{tz}.png"));
            kenshi_render::particles::kenshi::load_rgba(&p)
        });
        let (w, data) = tile.as_ref()?;
        let h = data.len() as u32 / 4 / w;
        let col = (((fx - tx as f32) * *w as f32) as u32).min(w - 1);
        let row = (((fz - tz as f32) * h as f32) as u32).min(h - 1);
        let i = ((row * w + col) * 4) as usize;
        let (r, g, b, a) = (data[i], data[i + 1], data[i + 2], data[i + 3]);
        Some(if a > 0xa0 {
            4
        } else if r > 0xa0 {
            3
        } else if g > 0xa0 || b > 0xa0 {
            2
        } else {
            0
        })
    }
}

/// Footsteps (AnimationGroundEvent, F020844): each person's clips fire their events as they
/// pass them: a ground event's dust where the foot's bone is (the biome says which effect),
/// and the event's sound there ("Footsteps" with the surface and armour switches,
/// `sound::footstep_switches`; others with their override switch), within 700 units of the
/// camera's focus.
/// (stand-in) Events fire on the clips weighing at least half; the speed Kenshi compares is
/// the movement speed x 10 (as the animation choice takes it); every foot is taken to be on
/// terrain (Kenshi's ground effects need its +0x640 state 11).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn footsteps(
    time: Res<Time>,
    game: Res<Game>,
    look: Res<TerrainLook>,
    bodies: Query<(&kenshi_render::character::Animated, Option<&Blend>)>,
    joints: Query<&GlobalTransform>,
    cams: Query<&bevy::camera::primitives::Frustum, MainCam>,
    mut fx: ResMut<kenshi_render::particles::effects::Effects>,
    mut events: Local<Option<FootEvents>>,
    mut sound: Option<ResMut<sound::Sound>>,
) {
    let ev = events.get_or_insert_with(|| FootEvents::new(&game.sim.data));
    if ev.terrain.is_none() {
        ev.terrain = kenshi_world::Terrain::open(&game.root).ok();
    }
    let Some(frustum) = cams.iter().next() else { return };
    let origin = game.zone.origin;
    let elapsed = time.elapsed_secs();
    for p in &game.sim.folk {
        let Some(&e) = game.bodies.get(p.id) else { continue };
        let Ok((a, blend)) = bodies.get(e) else { continue };
        let clips: Vec<(usize, f32)> = match blend {
            Some(b) => {
                // a clip both playing and fading out counts once, by its heavier time
                let mut v: Vec<(usize, f32, f32)> = Vec::new();
                for c in &b.0 {
                    match v.iter_mut().find(|x| x.0 == c.0) {
                        Some(x) if x.2 < c.2 => *x = *c,
                        Some(_) => {}
                        None => v.push(*c),
                    }
                }
                v.into_iter().filter(|c| c.2 >= 0.5).map(|c| (c.0, c.1)).collect()
            }
            None => vec![(a.current, a.freeze_at.unwrap_or(elapsed + a.offset))],
        };
        for (clip, t) in clips {
            let anim = &a.library[clip];
            if anim.length <= 0.0 {
                continue;
            }
            let phase = (t / anim.length).rem_euclid(1.0);
            let last = ev.last.insert((p.id, clip), phase);
            let Some(last) = last else { continue };
            let Some(list) = ev.by_clip.get(&anim.name.to_lowercase()) else { continue };
            let passed = |f: f32| if phase >= last { last < f && f <= phase } else { last < f || f <= phase };
            let fired: Vec<(String, bool, Option<(String, Option<(String, String)>)>)> = list.iter().filter(|e| passed(e.at)).map(|e| (e.bone.clone(), e.ground, e.sound.clone())).collect();
            for (bone, ground, posts) in fired {
                let foot = a
                    .rig
                    .names
                    .iter()
                    .position(|n| *n == bone)
                    .and_then(|i| joints.get(a.joints[i]).ok())
                    .map(|g| g.translation() + origin)
                    .unwrap_or(Vec3::from(p.mover.position_units()));
                // its sound
                if let (Some((event, over)), Some(s)) = (posts.as_ref(), sound.as_mut())
                    && !game.paused
                    && !p.down
                    && (foot - origin).distance(game.focus) < 700.0
                {
                    let at = foot - origin;
                    if std::env::var("KENSHI_FOOT_DEBUG").is_ok() {
                        eprintln!("foot sound {event} {over:?} for {} ({})", p.id, anim.name);
                    }
                    if event == "Footsteps" {
                        // the terrain's ground type under the foot (F051635: the biome's type
                        // for the layer, F051634)
                        let zone = kenshi_world::zone_of(foot.x, foot.z);
                        let layer = if zone == game.home || game.zones.contains_key(&zone) {
                            ev.layer(&game.root, foot.x, foot.z)
                        } else {
                            None
                        };
                        let terrain = sound::terrain_surface(&game.sim.data, look.biome_at(foot.x, foot.z), layer);
                        let (surface, armour) = sound::footstep_switches(s, &game, p.id, at, terrain);
                        if std::env::var("KENSHI_FOOT_DEBUG").is_ok() {
                            eprintln!("footstep {} surface {surface} (terrain {terrain}, layer {layer:?}) armour {armour}", p.id);
                        }
                        s.post_with(event, Some(at), &[("Surface_Material", surface), ("Armor_Worn", armour)], sound::Channel::Footstep);
                    } else {
                        let sw: Vec<(&str, &str)> = over.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
                        s.post_with(event, Some(at), &sw, sound::Channel::Sfx);
                    }
                }
                if !ground {
                    continue;
                }
                let biome = look.biome_at(foot.x, foot.z).map(str::to_string);
                let zone = kenshi_world::zone_of(foot.x, foot.z);
                let layer = if zone == game.home || game.zones.contains_key(&zone) {
                    ev.layer(&game.root, foot.x, foot.z)
                } else {
                    None
                };
                if std::env::var("KENSHI_FOOT_DEBUG").is_ok() {
                    eprintln!("foot {} {bone} at {foot:.0} speed {:.0} biome {biome:?} layer {layer:?}", anim.name, p.speed * 10.0);
                }
                let ground_y = ev.terrain.as_mut().and_then(|t| t.height_at(foot.x, foot.z).ok()).unwrap_or(foot.y);
                let spawned = fx.ground_event(foot, p.speed * 10.0, biome.as_deref(), layer, WATER_HEIGHT, ground_y, |at| {
                    let s = bevy::math::bounding::BoundingSphere::new(at - origin, 100.0);
                    frustum.intersects_sphere(&bevy::camera::primitives::Sphere { center: s.center, radius: 100.0 }, true)
                });
                if std::env::var("KENSHI_FOOT_DEBUG").is_ok()
                    && let Some(e) = spawned
                {
                    eprintln!("  spawned {}", fx.defs[e].id);
                }
            }
        }
    }
}

/// The weathers' effects (kenshi_render::particles::effects): each region's groups made for
/// its weather when it begins (F051022), then the frame.
#[allow(clippy::too_many_arguments)]
fn weather_effects(
    dt: f32,
    game: &Game,
    weather: &WeatherState,
    sky: &Sky,
    cams: &Query<&GlobalTransform, MainCam>,
    fx: &mut kenshi_render::particles::effects::Effects,
    region_zones: &mut Option<Vec<Vec<(i32, i32)>>>,
    terrain: &mut Option<kenshi_world::Terrain>,
) {
    let (Some(map), Some(cam)) = (weather.map.as_ref(), cams.iter().next()) else { return };
    let region_of_zone = |z: (i32, i32)| -> Option<usize> {
        let b = kenshi_world::biome::biome_of_zone(map, &weather.biomes, z.0, z.1)?;
        let group = &weather.biomes[b].record;
        weather.regions.iter().position(|r| &r.biome_group == group)
    };
    let zones = region_zones.get_or_insert_with(|| {
        let mut out = vec![Vec::new(); weather.regions.len()];
        for x in 0..64 {
            for y in 0..64 {
                if let Some(r) = region_of_zone((x, y)) {
                    out[r].push((x, y));
                }
            }
        }
        out
    });
    if terrain.is_none() {
        *terrain = kenshi_world::Terrain::open(&game.root).ok();
    }
    if std::env::var("KENSHI_FOG_DEBUG").is_ok() && game.sim.tick % 120 == 7 {
        let cur = weather.current;
        eprintln!(
            "fog: regions {}, current {cur:?} with {} zones, camera zone region {:?}, non-empty {}",
            zones.len(),
            cur.and_then(|c| zones.get(c)).map_or(0, |z| z.len()),
            weather.current_zone.and_then(region_of_zone),
            zones.iter().filter(|z| !z.is_empty()).count()
        );
    }
    // each region's weather: its effects, and its wind
    let mut wind = Vec::with_capacity(weather.regions.len());
    for (k, r) in weather.regions.iter().enumerate() {
        match (&r.instance, r.weather()) {
            (Some(i), Some(w)) => {
                fx.weather_began(k, (i.weather, i.start), &w.effects);
                wind.push((i.wind_speed, Vec3::new(i.wind_dir[0], 0.0, i.wind_dir[1])));
            }
            _ => wind.push((0.0, Vec3::ZERO)),
        }
    }
    let origin = game.zone.origin;
    let hour = (game.sim.hours_gone() + game.sim.start_hour()) as f32;
    let (_, sun_colour, _, _) = sky.0.main_light(hour % 24.0);
    let zone_loaded = |z: (i32, i32)| game.zones.contains_key(&z);
    let towns: Vec<Vec3> = game.sim.towns.iter().map(|t| Vec3::from(t.position)).collect();
    let mut ground = |x: f32, z: f32| -> (f32, Vec3) {
        let Some(t) = terrain.as_mut() else { return (0.0, Vec3::Y) };
        let at = std::time::Instant::now();
        let h = t.height_at(x, z).unwrap_or(0.0);
        let s = t.spacing();
        let hx = t.height_at(x + s, z).unwrap_or(h);
        let hz = t.height_at(x, z + s).unwrap_or(h);
        kenshi_render::prof::add("particles: ground heights", at.elapsed().as_secs_f64());
        (h, Vec3::new(h - hx, s, h - hz).normalize_or(Vec3::Y))
    };
    let mut view = kenshi_render::particles::effects::WorldView {
        camera: cam.translation() + origin,
        forward: cam.forward().as_vec3(),
        target: game.focus + origin,
        current_region: weather.current,
        region_of_zone: &region_of_zone,
        zone_loaded: &zone_loaded,
        ground: &mut ground,
        region_zones: zones,
        wind: &wind,
        sun_colour,
        origin,
        towns: &towns,
    };
    fx.update(dt, &mut view);
}

/// The terrain beyond the streamed zones, out to the far clip: per blend cell (512 heightmap
/// samples, two zones a side) a coarse patch (every 32nd sample) with Kenshi's terrain material,
/// holed where a streamed zone's own terrain lies. Harness stand-in for Ogre's terrain LOD (not
/// ported): the coarse patch sits `DISTANT_DROP` units low and the zones' patches carry skirts.
#[derive(Resource)]
struct Distant {
    terrain: Terrain,
    /// Per cell: its entities and which of its four zones were holed.
    cells: HashMap<(u32, u32), (Vec<Entity>, u8)>,
}

const DISTANT_STEP: u32 = 32;
const DISTANT_CELL: u32 = 512;
const DISTANT_DROP: f32 = 5.0;
/// Cells (re)built per frame.
const DISTANT_PER_FRAME: usize = 6;

/// (testing) `KENSHI_OLD_TERRAIN=1`: the zones' own terrain and the coarse distant terrain
/// instead of Kenshi's quadtree terrain.
fn old_terrain() -> bool {
    std::env::var("KENSHI_OLD_TERRAIN").is_ok()
}

/// Kenshi's terrain (kenshi_render::terrain_lod): the quadtree updated from the camera each
/// frame (the viewport's height in pixels its error scale).
#[allow(clippy::too_many_arguments)]
fn lod_terrain(
    mut commands: Commands,
    game: Res<Game>,
    assets: Res<AssetServer>,
    mut lod: ResMut<kenshi_render::terrain_lod::LodTerrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut terrain_materials: ResMut<Assets<KenshiTerrainMaterial>>,
    mut look: ResMut<TerrainLook>,
    cams: Query<&GlobalTransform, MainCam>,
    windows: Query<&Window>,
    mut logged: Local<u32>,
) {
    if old_terrain() {
        return;
    }
    let Some(cam) = cams.iter().next() else { return };
    let k = windows.iter().next().map_or(1080.0, |w| w.resolution.physical_height() as f32);
    let origin = game.zone.origin;
    lod.update(cam.translation() + origin, k, origin, &mut commands, &mut meshes, &mut terrain_materials, &mut look, &assets);
    *logged += 1;
    if *logged % 300 == 1 && std::env::var("KENSHI_LOD_DEBUG").is_ok() {
        eprintln!("terrain nodes held/drawn {:?}", lod.stats());
    }
}

#[allow(clippy::too_many_arguments)]
fn distant_terrain(
    mut commands: Commands,
    game: Res<Game>,
    sky: Res<Sky>,
    assets: Res<AssetServer>,
    mut distant: ResMut<Distant>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut terrain_materials: ResMut<Assets<KenshiTerrainMaterial>>,
    mut look: ResMut<TerrainLook>,
) {
    if !old_terrain() {
        return;
    }
    let origin = game.zone.origin;
    let samples = distant.terrain.samples();
    let spacing = distant.terrain.spacing();
    let n = (samples - 1) / DISTANT_CELL;
    let cell_size = DISTANT_CELL as f32 * spacing;
    let reach = sky.0.fog_params().x + cell_size;
    let focus = game.focus + origin;
    let loaded = |k: (i32, i32)| k == game.home || game.zones.contains_key(&k);
    // which of a cell's zones (2 x 2) are streamed in
    let holes = |cx: u32, cy: u32| {
        let mut m = 0u8;
        for (b, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
            if loaded(((cx * 2 + dx) as i32, (cy * 2 + dy) as i32)) {
                m |= 1 << b;
            }
        }
        m
    };
    let mut want: Vec<(f32, (u32, u32), u8)> = vec![];
    for cy in 0..n {
        for cx in 0..n {
            let centre = |c: u32| (c as f32 + 0.5) * cell_size - WORLD_SIZE / 2.0;
            let d = Vec2::new(centre(cx) - focus.x, centre(cy) - focus.z).length();
            if d < reach {
                want.push((d, (cx, cy), holes(cx, cy)));
            }
        }
    }
    // drop the cells out of reach
    let keep: std::collections::HashSet<(u32, u32)> = want.iter().map(|w| w.1).collect();
    distant.cells.retain(|k, (es, _)| {
        if keep.contains(k) {
            return true;
        }
        for e in es.drain(..) {
            commands.entity(e).despawn();
        }
        false
    });
    want.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut built = 0;
    for (_, (cx, cy), mask) in want {
        if distant.cells.get(&(cx, cy)).is_some_and(|(_, m)| *m == mask) {
            continue;
        }
        if built == DISTANT_PER_FRAME {
            break;
        }
        built += 1;
        if let Some((es, _)) = distant.cells.remove(&(cx, cy)) {
            for e in es {
                commands.entity(e).despawn();
            }
        }
        let side = DISTANT_CELL / DISTANT_STEP + 1;
        let Ok(patch) = distant.terrain.patch(
            cx * DISTANT_CELL,
            cy * DISTANT_CELL,
            side,
            side,
            DISTANT_STEP,
        ) else {
            continue;
        };
        let zone = |v: f32| ((v + WORLD_SIZE / 2.0) / ZONE_SIZE).floor() as i32;
        let pieces = kenshi_render::terrain_pieces_where(
            &patch,
            Some(origin + Vec3::Y * DISTANT_DROP),
            DISTANT_CELL,
            300.0,
            |x, z| !loaded((zone(x), zone(z))),
        );
        let mut es = vec![];
        for (tile, cell, mesh) in pieces {
            let m = look.material(&assets, tile, cell, origin);
            es.push(
                commands
                    .spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(terrain_materials.add(m)),
                        bevy::light::NotShadowCaster,
                        kenshi_render::reflection::reflected(),
                    ))
                    .id(),
            );
        }
        distant.cells.insert((cx, cy), (es, mask));
    }
}

/// A zone's water plane (F050961), when its ground dips under the water: one plane per biome
/// pass of its blend cell.
fn spawn_zone_water(
    commands: &mut Commands,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    waters: &mut Assets<KenshiWaterMaterial>,
    look: &mut TerrainLook,
    zs: &ZoneScene,
    key: (i32, i32),
) {
    if !zs.patch.heights.iter().any(|&h| h < WATER_HEIGHT) || std::env::var("KENSHI_NO_WATER").is_ok() {
        return;
    }
    let origin = zs.origin;
    let centre = |k: i32| (k as f32 + 0.5) * ZONE_SIZE - WORLD_SIZE / 2.0;
    let (cx, cz) = (centre(key.0), centre(key.1));
    let cell_size = WORLD_SIZE / look.blend.n as f32;
    let cell = (
        ((cx + WORLD_SIZE / 2.0) / cell_size) as u32,
        ((cz + WORLD_SIZE / 2.0) / cell_size) as u32,
    );
    let mesh = meshes.add(Plane3d::default().mesh().size(ZONE_WATER_SIZE, ZONE_WATER_SIZE));
    for m in look.water(assets, cell, origin) {
        commands.spawn((
            ZoneTag(key),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(waters.add(m)),
            Transform::from_xyz(cx - origin.x, WATER_HEIGHT - origin.y, cz - origin.z),
            bevy::light::NotShadowCaster,
        ));
    }
}

/// gameTime every frame: the water's clock and the heat haze's.
fn water_clock(
    game: Res<Game>,
    mut shared: ResMut<kenshi_render::light_buffer::SharedLight>,
    mut hdr: ResMut<kenshi_render::hdr::KenshiHdr>,
    mut lights: ResMut<kenshi_render::local_lights::LightClock>,
) {
    // the sky's hours (day x 24 + hour) for the light effects (F030155)
    lights.hours = game.sim.start_hour() + game.sim.hours_gone();
    let t = game.sim.hours_gone() as f32;
    // gameTime for the heat haze too, every frame (F040805 sets it each frame)
    hdr.game_time = t;
    // the water's clock: its gameTime in the shared light buffer
    shared.game_hours = t;
}

/// The moons each frame (F040806 -> F040808), round the camera.
fn planets(
    game: Res<Game>,
    cams: Query<(&Transform, &Projection), MainCam>,
    mut planets: Query<(&Planet, &mut Transform), Without<Camera3d>>,
) {
    let Some((t, Projection::Perspective(p))) = cams.iter().next() else {
        return;
    };
    // the game's minute count: (day x 24 + hour) x 60 + minute, from the clock's start
    let minutes = ((game.sim.hours_gone() + game.sim.start_hour()) * 60.0).floor() as f32;
    place_planets(&mut planets, t.translation, p.near, p.far, minutes);
}

/// The weather (kenshi_world::weather) and what it tells the sky controller: the sky colour
/// multiplier and cloud density moving to the weather's over 30 time units (F040786; F040805
/// steps them by the frame time), and the wind the clouds drift by (F040785: the controller's
/// +0x40; F040805 moves the clouds' position by it each frame).
#[derive(Resource)]
struct WeatherState {
    regions: Vec<kenshi_world::weather::WeatherRegion>,
    map: Option<kenshi_world::biome::BiomeMap>,
    biomes: Vec<kenshi_world::biome::Biome>,
    current: Option<usize>,
    rng: WeatherRng,
    sky_mult: Vec3,
    cloud: f32,
    sky_rate: Vec3,
    cloud_rate: f32,
    timer: f32,
    duration: f32,
    active: bool,
    wind: Vec2,
    cloud_pos: Vec2,
    /// F051184: the four region slots (fog), the zone the camera's region was taken in, the
    /// camera's last position, and the outputs.
    slots: [kenshi_world::weather::FogSlot; 4],
    current_zone: Option<(i32, i32)>,
    last_pos: Option<Vec3>,
    outputs: kenshi_world::weather::WeatherOutputs,
}

/// rand() / RAND_MAX (a fixed-seed linear congruential stand-in for the CRT's rand).
struct WeatherRng(u32);

impl kenshi_world::weather::Random for WeatherRng {
    fn unit(&mut self) -> f32 {
        // MSVC's rand: seed = seed x 214013 + 2531011, (seed >> 16) & 0x7fff
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        ((self.0 >> 16) & 0x7fff) as f32 / 32767.0
    }
}

impl WeatherState {
    fn new(data: &kenshi_data::GameDataContainer) -> WeatherState {
        let gc = data.get("110-gamedata.quack");
        let days_per_year = gc
            .and_then(|g| g.ints.get("days per year").copied())
            .or_else(|| gc.and_then(|g| g.floats.get("days per year").map(|v| *v as i32)))
            .unwrap_or(300);
        WeatherState {
            regions: kenshi_world::weather::weather_regions(data, days_per_year),
            map: None,
            biomes: kenshi_world::biome::biomes(data),
            current: None,
            rng: WeatherRng(1),
            sky_mult: Vec3::ONE,
            cloud: 0.25,
            sky_rate: Vec3::ZERO,
            cloud_rate: 0.0,
            timer: 0.0,
            duration: 0.0,
            active: false,
            wind: Vec2::ONE,
            cloud_pos: Vec2::ZERO,
            slots: Default::default(),
            current_zone: None,
            last_pos: None,
            outputs: Default::default(),
        }
    }

    /// F040786: move the sky colour multiplier and cloud density to these over `duration`.
    fn transition(&mut self, sky: Vec3, cloud: f32, duration: f32) {
        self.timer = 0.0;
        self.duration = duration;
        if duration == 0.0 {
            self.sky_mult = sky;
            self.cloud = cloud;
            self.sky_rate = Vec3::ZERO;
            self.cloud_rate = 0.0;
        } else {
            self.sky_rate = (sky - self.sky_mult) / duration;
            self.cloud_rate = (cloud - self.cloud) / duration;
        }
        self.active = true;
    }
}

#[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
fn weather_tick(
    mut game: ResMut<Game>,
    time: Res<Time>,
    mut weather: ResMut<WeatherState>,
    mut sky: ResMut<Sky>,
    mut skies: ResMut<Assets<KenshiSkyMaterial>>,
    mut hdr: ResMut<kenshi_render::hdr::KenshiHdr>,
    cams: Query<&Projection, MainCam>,
) {
    if let Some(Projection::Perspective(pp)) = cams.iter().next() {
        hdr.near = pp.near;
        hdr.far = pp.far;
        let up = (pp.fov * 0.5).tan();
        hdr.tan_half_fov = Vec2::new(up * pp.aspect_ratio, up);
    }
    let w = &mut *weather;
    if w.map.is_none() {
        w.map = kenshi_world::biome::BiomeMap::load(&game.root);
    }
    let hours = game.sim.hours_gone() + game.sim.start_hour();
    let day = (hours / 24.0).floor() as i32;
    for r in w.regions.iter_mut() {
        if r.instance.is_none() {
            r.begin(day, hours, &mut w.rng);
        } else {
            r.update(day, hours, &mut w.rng);
        }
    }
    // F051184: the region of a world point: its zone's biome group (the map's colour at the
    // zone's centre)
    let focus = game.focus + game.zone.origin;
    let region_of_zone = |w: &WeatherState, z: (i32, i32)| -> Option<usize> {
        let m = w.map.as_ref()?;
        let b = kenshi_world::biome::biome_of_zone(m, &w.biomes, z.0, z.1)?;
        let group = &w.biomes[b].record;
        w.regions.iter().position(|r| &r.biome_group == group)
    };
    // the squared distance from the camera to a zone's square, over 1300^2 (F051177)
    let zone_rect = |z: (i32, i32)| {
        let lo = Vec2::new(
            z.0 as f32 * ZONE_SIZE - WORLD_SIZE / 2.0,
            z.1 as f32 * ZONE_SIZE - WORLD_SIZE / 2.0,
        );
        (lo, lo + Vec2::splat(ZONE_SIZE))
    };
    let out_of = |z: (i32, i32), p: Vec3| {
        let (lo, hi) = zone_rect(z);
        let dx = if p.x < lo.x { p.x - lo.x } else if p.x > hi.x { hi.x - p.x } else { 0.0 };
        let dz = if p.z < lo.y { p.z - lo.y } else if p.z > hi.y { hi.y - p.z } else { 0.0 };
        dx * dx + dz * dz
    };
    // the four samples round the camera, their regions' weights
    let (a, b) = (Vec3::new(1300.0, 0.0, 1300.0), Vec3::new(1300.0, 0.0, -1300.0));
    let mut weights = [0.0f32; 4];
    for s in [focus - a, focus - b, focus + b, focus + a] {
        let z = kenshi_world::zone_of(s.x, s.z);
        let Some(region) = region_of_zone(w, z) else { continue };
        let d = (out_of(z, focus) / 1_690_000.0).min(1.0);
        if 1.0 - d <= 0.0 {
            continue;
        }
        let slot = match w.slots.iter().position(|sl| sl.region == Some(region)) {
            Some(k) => k,
            None => {
                let k = w.slots.iter().position(|sl| sl.region.is_none()).unwrap_or(0);
                w.slots[k] = kenshi_world::weather::FogSlot::new(region);
                k
            }
        };
        weights[slot] += 1.0 - d;
    }
    let sum: f32 = weights.iter().sum();
    for k in 0..4 {
        if weights[k] == 0.0 {
            w.slots[k].region = None;
        } else {
            weights[k] /= sum;
        }
        if let Some(r) = w.slots[k].region {
            let region = w.regions[r].clone();
            w.slots[k].update(&region, hours);
        }
    }
    // the current region, kept until the camera is 500 units out of its zone
    let here = kenshi_world::zone_of(focus.x, focus.z);
    let mut jump = false;
    match w.current_zone {
        Some(z) if region_of_zone(w, z) != region_of_zone(w, here) => {
            if out_of(z, focus) >= 250_000.0 {
                w.current_zone = Some(here);
            }
            if let Some(last) = w.last_pos
                && (last - focus).length_squared() > 8000.0
            {
                jump = true;
            }
        }
        Some(_) => {}
        None => w.current_zone = Some(here),
    }
    w.last_pos = Some(focus);
    let current = w.current_zone.and_then(|z| region_of_zone(w, z));
    // the weather and ground of the zones round the player's characters, for the sim
    // (kenshi_game::hazards): the region's weather's affect type and strength, its biome
    // group's acidic ground and water
    let mut climate = kenshi_game::hazards::Climates::new();
    for z in kenshi_game::hazards::zones_round(&game.sim) {
        let Some(r) = region_of_zone(w, z) else { continue };
        let region = &w.regions[r];
        let group = game.sim.data.get(&region.biome_group);
        let f = |k: &str| group.and_then(|g| g.floats.get(k).copied()).unwrap_or(0.0);
        let (affect, strength) = region
            .instance
            .as_ref()
            .and_then(|i| region.seasons.get(region.season).and_then(|s| s.weathers.get(i.weather)).map(|wt| (wt.affect_type, i.strength)))
            .unwrap_or((0, 0.0));
        climate.insert(z, kenshi_game::hazards::Climate { affect, strength, acid_ground: f("acidic ground"), acid_water: f("acidic water") });
    }
    game.sim.climate = climate;
    let switched = current != w.current;
    w.current = current;
    // KENSHI_WEATHER=<WEATHER name> (testing): that weather in the camera's region, once
    if switched
        && let (Some(k), Ok(name)) = (current, std::env::var("KENSHI_WEATHER"))
        && let Some(r) = game
            .sim
            .data
            .of_type(kenshi_data::ItemType::Weather)
            .find(|r| r.name == name)
    {
        let weather = kenshi_world::weather::Weather::from_record(r, 1.0, [100_000, 100_000]);
        w.regions[k].force(weather, hours, &mut w.rng);
        w.outputs.wetness = 0.0;
    }
    if let Some(k) = current {
        let changed = w.regions[k].changed || switched;
        w.regions[k].changed = false;
        if let Some(weather) = w.regions[k].weather().cloned() {
            if changed {
                let name = game.sim.data.get(&weather.record).map_or("?", |r| r.name.as_str());
                eprintln!("weather: {name} (clouds {})", weather.clouds_density);
                w.transition(Vec3::from(weather.sky_colour_mult), weather.clouds_density, 30.0);
            }
            if let Some(i) = &w.regions[k].instance {
                w.wind = Vec2::new(i.wind_dir[0], i.wind_dir[1]) * i.wind_speed;
            }
        }
    }
    // F040805: the transition and the clouds' drift, by the frame time (the game's)
    let dt = time.delta_secs() * if game.paused { 0.0 } else { game.speed };
    if w.active {
        w.timer += dt;
        if w.timer >= w.duration {
            w.active = false;
        }
        w.sky_mult = (w.sky_mult + w.sky_rate * dt).clamp(Vec3::ZERO, Vec3::ONE);
        w.cloud = (w.cloud + w.cloud_rate * dt).clamp(0.0, 1.0);
        sky.0.set_cloud_density(w.cloud);
        sky.0.sky_mult = w.sky_mult;
    }
    w.cloud_pos += w.wind * dt;
    if let Some(k) = w.current {
        let slots: Vec<(kenshi_world::weather::FogSlot, f32)> =
            w.slots.iter().copied().zip(weights).collect();
        let sun_y = sky.0.sun_direction(hours as f32).y;
        let region = w.regions[k].clone();
        w.outputs.update(&slots, &region, dt, sun_y, jump);
        // the base's wind and rain (assumption: the weather where the camera is), for farms and
        // wind generators
        game.sim.rain = w.outputs.rain;
        game.sim.wind = region.instance.as_ref().map_or(0.0, |i| i.wind_speed * 0.01);
        sky.0.weather = w.outputs;
        hdr.heat_haze = w.outputs.heat_haze;
    }
    for (_, m) in skies.iter_mut() {
        m.params.wind = Vec4::new(w.cloud_pos.x, w.cloud_pos.y, 0.0, 0.0);
    }
}

/// People whose squad is away travelling the world (`kenshi_game::world`) are not drawn.
fn hide_away(game: Res<Game>, mut vis: Query<&mut Visibility>) {
    for p in &game.sim.folk {
        let Some(&e) = game.bodies.get(p.id) else { continue };
        let Ok(mut v) = vis.get_mut(e) else { continue };
        let want = if p.away { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
}

/// @fn F038266
///
/// A person's look rolled as Kenshi rolls a new NPC's (F021092 the Body sliders, F021095 the Face
/// ones and their poses; `appearance`), unless it has its own (the editor's, a body file's): on a
/// generator seeded by its number and CHARACTER, so it looks the same each time.
fn roll_looks(mut game: ResMut<Game>) {
    let ids: Vec<usize> = game.looks.keys().copied().filter(|id| !game.body_values.contains_key(id)).collect();
    if ids.is_empty() {
        return;
    }
    let deviation = appearance::deviation(&game);
    for id in ids {
        let Some(spec) = game.looks.get(&id).cloned() else { continue };
        let Some(p) = game.sim.folk.get(id) else { continue };
        if p.race.is_some() {
            continue;
        }
        let salt = p.character.as_deref().map_or(0, |c| c.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)));
        let mut values = HashMap::new();
        if let Some(l) = appearance::limits(&game, &spec.race, spec.female) {
            let mut r = appearance::seeded(id, salt);
            appearance::roll(&l.body, deviation, &mut r, &mut values);
            appearance::roll(&l.face, deviation, &mut r, &mut values);
        }
        game.body_values.insert(id, values);
    }
}

/// F021065: each head's face poses at the person's weights (its values' pose names; the
/// editor's, a body file's or rolled), baked into a head mesh of its own when they change (as
/// Ogre blends poses in software), the shared plain head while they are all nothing.
fn face_morphs(game: Res<Game>, children: Query<&Children>, mut heads: Query<(&mut kenshi_render::character::MorphBase, &mut Mesh3d)>, mut meshes: ResMut<Assets<Mesh>>) {
    for p in &game.sim.folk {
        if p.away {
            continue;
        }
        let (Some(&root), Some(values)) = (game.bodies.get(p.id), game.body_values.get(&p.id)) else { continue };
        let Ok(kids) = children.get(root) else { continue };
        for &c in kids {
            let Ok((mut m, mut mesh)) = heads.get_mut(c) else { continue };
            let Some(names) = m.base.morph_target_names() else { continue };
            let want: Vec<f32> = names.iter().map(|n| values.get(n).copied().unwrap_or(0.0)).collect();
            if want == m.weights {
                continue;
            }
            if std::env::var("KENSHI_MORPH_DEBUG").is_ok() {
                eprintln!("face morphs {}: {:?}", p.id, &want[..want.len().min(6)]);
            }
            let handle = if want.iter().all(|&w| w == 0.0) {
                Some(m.plain.clone())
            } else {
                kenshi_render::baked(&m.base, |n| values.get(n).copied().unwrap_or(0.0)).map(|b| meshes.add(b))
            };
            if let Some(h) = handle {
                mesh.0 = h;
            }
            m.weights = want;
        }
    }
}

/// Each body's bone sizes (`kenshi_render::body_shape`, F037414): from its sliders (the
/// editor's, a body file's, or rolled as Kenshi rolls a new NPC's, `roll_looks`), its muscle and
/// thinness (F037377), its lost limbs' bones sized to nothing.
/// - muscle (AppearanceHuman +0x1a0): invlerp(the highest of Strength, Weapon Smithing and Armour
///   Smithing / 100, 0.2, 0.99) (F050480, not clamped: the weak are slighter);
/// - thinness (+0x1a8): its hunger's (Character +0x4b8, the hunger level 0 .. 3: a malnourished
///   squad's people start at 1.7, F038266): 1.5 x (1 - (level - 1) within 0 .. 1), at most 1, so
///   the hungry (under 2) look thinner and the starving thinnest.
fn body_shapes(mut commands: Commands, game: Res<Game>, bodies: Query<(&kenshi_render::character::Animated, Option<&kenshi_render::body_shape::BoneSizes>)>) {
    for p in &game.sim.folk {
        if p.away {
            continue;
        }
        let Some(&root) = game.bodies.get(p.id) else { continue };
        let Ok((anim, current)) = bodies.get(root) else { continue };
        let lost: [bool; 4] = std::array::from_fn(|s| game.sim.limb_lost(p.id, s));
        let values = game.body_values.get(&p.id);
        if values.is_none() && !lost.iter().any(|&l| l) {
            if current.is_some() {
                commands.entity(root).remove::<kenshi_render::body_shape::BoneSizes>();
            }
            continue;
        }
        let empty = HashMap::new();
        let female = game.looks.get(&p.id).is_some_and(|l| l.female);
        use kenshi_char::StatsEnumerated::{StatSmithingArmour, StatSmithingWeapon, StatStrength};
        let f = &game.sim.arena.f[p.id];
        let strong = f.level(StatStrength).max(f.level(StatSmithingWeapon)).max(f.level(StatSmithingArmour));
        let muscle = (strong * 0.01 - 0.2) / (0.99 - 0.2);
        let level = f.medical.hunger.level;
        let thin = ((1.0 - (level - 1.0).clamp(0.0, 1.0)) * 1.5).clamp(0.0, 1.0);
        let Some(sizes) = kenshi_render::body_shape::bone_sizes(&anim.rig.names, values.unwrap_or(&empty), female, muscle, thin, lost) else {
            if std::env::var("KENSHI_EDITOR_DEBUG").is_ok() {
                eprintln!("body shape: rig of {} bones: {:?}", anim.rig.names.len(), anim.rig.names);
            }
            continue;
        };
        if std::env::var("KENSHI_EDITOR_DEBUG").is_ok() && current != Some(&sizes) {
            let v = values.unwrap_or(&empty);
            eprintln!("body shape {} ({root:?}): Height {:?} Arm bulk {:?} Frame {:?}, muscle {muscle}", p.id, v.get("Height"), v.get("Arm bulk"), v.get("Frame"));
        }
        if current != Some(&sizes) {
            commands.entity(root).insert(sizes);
        }
    }
}

/// (debugging) The debug lines drawn over everything.
fn gizmo_setup(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.depth_bias = -1.0;
    config.line.width = 2.0;
}

/// (debugging) Ctrl+F9 or KENSHI_NAV_DEBUG: the navmesh round the camera drawn over the world: the
/// edges a character cannot cross (no face beyond, no link to another section) red, building
/// sections' other edges yellow.
fn nav_debug(mut on: Local<Option<bool>>, mut frame: Local<u32>, keys: Res<ButtonInput<KeyCode>>, game: Res<Game>, mut gizmos: Gizmos) {
    let shown = on.get_or_insert_with(|| std::env::var("KENSHI_NAV_DEBUG").is_ok());
    if (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) && keys.just_pressed(KeyCode::F9) {
        *shown = !*shown;
    }
    if !*shown {
        return;
    }
    // once: the placed buildings near the camera with and without a navmesh section of their own
    *frame += 1;
    if *frame == 300 {
        let f = (game.focus + game.zone.origin) * 0.1;
        let ground = game.ground(game.focus.x, game.focus.z) + game.zone.origin.y;
        eprintln!("navcut at focus {f:?} (ground {:.1} m): {:?}", ground * 0.1, game.sim.nav.cutters_at(f.x, f.z));
        let (mut blocked, mut open, mut pieces, mut area_open, mut area_pieces) = (0, 0, 0, 0.0f32, 0.0f32);
        for s in game.sim.nav.sections.values().filter(|s| s.info.is_none()) {
            for fi in 0..s.faces.len() as u32 {
                let c: Vec<[f32; 3]> = s.corners(fi).collect();
                if c.is_empty() {
                    continue;
                }
                let (cx, cz) = (c.iter().map(|v| v[0]).sum::<f32>() / c.len() as f32, c.iter().map(|v| v[2]).sum::<f32>() / c.len() as f32);
                if (cx - f.x).hypot(cz - f.z) > 60.0 {
                    continue;
                }
                let a = (0..c.len()).map(|i| { let (p, q) = (c[i], c[(i + 1) % c.len()]); p[0] * q[2] - q[0] * p[2] }).sum::<f32>().abs() * 0.5;
                if s.uid >= kenshi_world::navcut::CUT_UID {
                    pieces += 1;
                    area_pieces += a;
                } else if s.faces[fi as usize].kind == kenshi_world::navmesh::FACE_BLOCKED {
                    blocked += 1;
                } else {
                    open += 1;
                    area_open += a;
                }
            }
        }
        let origin = game.zone.origin;
        let g = |x: f32, z: f32| (game.ground(x * 10.0 - origin.x, z * 10.0 - origin.z) + origin.y) * 0.1;
        eprintln!("navcut check (asked, found, through): {:?}", game.sim.nav.check_cutters(f.x, f.z, 150.0, &g));
        if let Some(&me) = game.selected.first() {
            let p = game.sim.folk[me].mover.position_units().map(|v| v * 0.1);
            eprintln!("floors check ((low asked, found), (high asked, found)): {:?}", game.sim.nav.check_floors(p, 120.0));
        }
        eprintln!("navcut near: {open} open ground faces ({area_open:.0} m2), {blocked} cut, {pieces} pieces ({area_pieces:.0} m2)");
    }
    if std::env::var("KENSHI_NAV_MATCH").is_ok() && *frame == 300 {
        let f = game.focus + game.zone.origin;
        let secs: Vec<[f32; 3]> = game.sim.nav.sections.values().filter_map(|s| s.building_position()).collect();
        for s in game.sim.nav.sections.values().filter(|s| s.building_position().is_some_and(|q| (q[0] - f.x).hypot(q[2] - f.z) < 1500.0)).take(12) {
            let v0 = s.vertices.first().copied().unwrap_or_default();
            eprintln!("navsec {:?} first vertex (units) {:?}", s.info, v0.map(|v| v * 10.0));
        }
        for (k, p) in game.sim.placed.iter().enumerate() {
            let at = game.sim.placed_at[k];
            if (at[0] - f.x).hypot(at[2] - f.z) > 1500.0 {
                continue;
            }
            let d = secs.iter().map(|q| (q[0] - at[0]).hypot(q[2] - at[2])).fold(f32::MAX, f32::min);
            let name = game.sim.data.get(&p.building).map_or("?", |r| r.name.as_str());
            eprintln!("navmatch {k} {name} at {at:?}: nearest section {d:.1}");
        }
    }
    let origin = game.zone.origin;
    let focus = game.focus + origin;
    let to_scene = |p: [f32; 3]| Vec3::new(p[0] * 10.0, p[1] * 10.0 + 0.5, p[2] * 10.0) - origin;
    for s in game.sim.nav.sections.values() {
        let building = s.info.is_some();
        let edges = s.faces.iter().filter(|f| f.kind != kenshi_world::navmesh::FACE_BLOCKED).flat_map(|f| f.start_edge..f.start_edge + f.num_edges);
        for k in edges {
            let e = &s.edges[k as usize];
            let (a, b) = (s.vertices[e.a as usize], s.vertices[e.b as usize]);
            if (a[0] * 10.0 - focus.x).hypot(a[2] * 10.0 - focus.z) > 700.0 {
                continue;
            }
            let wall = e.opposite_face < 0 && !s.links.contains_key(&k);
            if wall && s.uid >= kenshi_world::navcut::CUT_UID {
                gizmos.line(to_scene(a), to_scene(b), Color::srgb(1.0, 0.2, 1.0));
            } else if wall {
                gizmos.line(to_scene(a), to_scene(b), Color::srgb(1.0, 0.1, 0.1));
            } else if building {
                gizmos.line(to_scene(a), to_scene(b), Color::srgba(1.0, 0.9, 0.2, 0.6));
            } else if std::env::var("KENSHI_NAV_GROUND").is_ok() {
                gizmos.line(to_scene(a), to_scene(b), Color::srgba(0.2, 0.6, 1.0, 0.6));
            }
        }
    }
}

/// (testing) KENSHI_RAY_TEST: every two seconds, where a click at the middle of the screen
/// lands: the terrain only, and with the navmesh's floors.
fn ray_test(mut frames: Local<u32>, game: Res<Game>, windows: Query<&Window>, cams: Query<(&Camera, &GlobalTransform), MainCam>) {
    *frames += 1;
    if std::env::var("KENSHI_RAY_TEST").is_err() || !(*frames).is_multiple_of(120) {
        return;
    }
    let (Ok(win), Ok((cam, at))) = (windows.single(), cams.single()) else { return };
    let c = Vec2::new(win.width() * 0.5, win.height() * 0.5);
    let Ok(ray) = cam.viewport_to_world(at, c) else { return };
    eprintln!("ray test: terrain {:?} click {:?}", terrain_at(&game, ray.origin, *ray.direction), ground_at(cam, at, &game, c));
}

/// The character editor's Posture, Shoulder set and Neck position on every person's body
/// (F037432 -> F037400): the skeleton's "postures", "shoulder set" and "neck set" clips enabled
/// at full weight, each held at its length x the value x 0.01. A person without editor values
/// takes its race's middles (the race's "editor limits" Body ranges' "mid", by gender).
fn postures(
    mut commands: Commands,
    game: Res<Game>,
    bodies: Query<&Animated>,
    mut shown: Local<HashMap<usize, Vec<(usize, f32)>>>,
    mut mids: Local<HashMap<(String, bool), [f32; 3]>>,
) {
    const NAMES: [(&str, &str); 3] = [("Posture", "postures"), ("Shoulder set", "shoulder set"), ("Neck position", "neck set")];
    if game.is_added() {
        shown.clear();
    }
    for p in &game.sim.folk {
        let (Some(&e), Some(look)) = (game.bodies.get(p.id), game.looks.get(&p.id)) else { continue };
        if p.race.is_some() {
            continue;
        }
        let Ok(a) = bodies.get(e) else { continue };
        let key = (look.race.clone(), look.female);
        let mid = *mids.entry(key).or_insert_with(|| race_posture_mids(&game, &look.race, look.female));
        let values = game.body_values.get(&p.id);
        let mut want = Vec::new();
        for (k, (value_name, clip)) in NAMES.iter().enumerate() {
            let v = values.and_then(|m| m.get(*value_name).copied()).unwrap_or(mid[k]);
            if let Some(i) = a.library.iter().position(|c| c.name.eq_ignore_ascii_case(clip)) {
                want.push((i, a.library[i].length * v * 0.01));
            }
        }
        if shown.get(&p.id) != Some(&want) {
            commands.entity(e).insert(kenshi_render::character::Posture(want.clone()));
            shown.insert(p.id, want);
        }
    }
}

/// A race's Posture, Shoulder set and Neck position middles (its "editor limits" file's Body
/// ranges for the gender), 50 where it has none.
fn race_posture_mids(game: &Game, race: &str, female: bool) -> [f32; 3] {
    let mut out = [50.0; 3];
    let rec = game.sim.data.get(race).or_else(|| game.sim.data.of_type(kenshi_data::ItemType::Race).find(|r| r.name == race));
    let Some(file) = rec.and_then(|r| r.files.get("editor limits")).and_then(|f| game.res.find(f)) else { return out };
    let Ok(text) = std::fs::read_to_string(game.root.join(file)) else { return out };
    let doc = kenshi_gui::xml::parse(&text);
    let gender = if female { "Female" } else { "Male" };
    for c in doc.descendants().into_iter().filter(|c| c.name == "Config" && c.attr("gender") == Some(gender)) {
        for cat in c.kids("Category").filter(|k| k.attr("name") == Some("Body")) {
            for r in cat.kids("Range") {
                let num = |k: &str| r.attr(k).and_then(|v| v.parse::<f32>().ok());
                let (Some(name), Some(min), Some(max)) = (r.attr("name"), num("min"), num("max")) else { continue };
                let mid = num("mid").unwrap_or((min + max) * 0.5);
                match name {
                    "Posture" => out[0] = mid,
                    "Shoulder set" => out[1] = mid,
                    "Neck position" => out[2] = mid,
                    _ => {}
                }
            }
        }
    }
    out
}

/// (testing) KENSHI_NO_UI: every interface root hidden (the HUD, panels, name tags, tips), for
/// clean world screenshots (trailer shots).
fn no_ui_shots(mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>)>) {
    if std::env::var("KENSHI_NO_UI").is_err() {
        return;
    }
    for mut v in &mut roots {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
}
