//! The Esc menu (Kenshi_MainMenuPopupPanel.layout: New Game, Save Game, Load Game, Options, Exit
//! Game, Resume; the game pauses while it is up) and the options window (Kenshi_OptionsPanel.layout,
//! F032950; its tabs General, Gameplay, Graphics, Audio and Controls as F032960 fills them, each
//! option a settings.cfg key F032944 reads and writes, the key bindings controls.cfg's).
//!
//! Settings are saved to the rewrite's own copies (kenshi_io::cfg, %APPDATA%\kenshi-rewrite); the
//! install's files are never written.
//! The rows, their ranges, choices and tooltips are F032960's (read from its code: too large for
//! the decompiler), the graphics tab's last rows the compositors.cfg ticks, the Mods tab the
//! active mods with their versions. The resolution and screen mode are Ogre's kenshi.cfg
//! options (the rewrite's copy read over the install's). A choice is a drop-down (`Drop`);
//! (assumption) Reset Tutorials forgets the tutorials seen.
//! Most options take effect when the game next starts (the damage floaters, the sound volumes and the movement marker at once);
//! a key binding is used at once (bindings). New Game opens the new game panel over the game
//! (menu::Menu::in_game; F048550 -> F048543), Save and Load open their panels (saves_ui); a game
//! begun or loaded replaces this one in the program (super::unload_world).
use super::Game;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use kenshi_gui::{Built, GuiClick, GuiRes, GuiScroll, SetCaption};
use kenshi_io::cfg::{self, Cfg};

#[derive(Clone, Copy)]
enum Kind {
    /// A heading line (F042410 with text).
    Header,
    /// An empty line (F042410 / F042408 with no text).
    Blank,
    Tick,
    /// min, max (F042419: a scroll bar of 100 steps a unit, the value written as Ogre writes a
    /// float).
    Slider(f32, f32),
    /// The values, in order, with their names (F042414's items: their index, the decals'
    /// resolution).
    Choice(&'static [(&'static str, &'static str)]),
    /// A button (F042415) and what it does.
    Action(&'static str),
    /// A kenshi.cfg (Ogre's) option: the resolution, a choice of the screen's video modes.
    Resolution,
    /// A kenshi.cfg tick: the option and the value ticked ("Full Screen" "Yes", "Border" "None").
    VideoTick(&'static str),
}

struct Opt {
    tab: u8,
    key: &'static str,
    label: &'static str,
    kind: Kind,
    tip: &'static str,
}

const fn o(tab: u8, key: &'static str, label: &'static str, kind: Kind, tip: &'static str) -> Opt {
    Opt { tab, key, label, kind, tip }
}

/// The tabs (F032960; the sixth, Mods, lists the active mods).
const TABS: [&str; 6] = ["GENERAL", "GAMEPLAY", "GRAPHICS", "AUDIO", "CONTROLS", "MODS"];

use Kind::*;
/// The rows as F032960 makes them, in its order, with their settings.cfg keys (F032944) and
/// tooltips; the graphics tab ends with a tick per post-processing compositor
/// (data/materials/compositors/compositors.cfg), the controls tab with the key bindings.
const OPTS: &[Opt] = &[
    // General
    o(0, "tutorials", "Tutorials", Tick, "Enable/Disable in-game tutorials"),
    o(0, "", "", Action("Reset Tutorials"), ""),
    o(0, "User save location", "User Save Location", Tick, ""),
    o(0, "Auto save", "Auto save", Tick, "Turns on automatic saves"),
    o(0, "Auto save time (minutes)", "Auto save time (minutes)", Slider(1.0, 30.0), ""),
    o(0, "Show names", "Show character names", Tick, "Shows the names of your characters on screen, hovering above their heads."),
    o(
        0,
        "Movement marker",
        "Show movement marker",
        Tick,
        "Shows horrible green marker to show you exactly where you clicked.  Useful when you are new to the game, but you should turn them off once you are used to it.",
    ),
    o(
        0,
        "Rotation marker",
        "Show rotation marker",
        Tick,
        "Shows horrible green marker to show camera rotation.  Useful when you are new to the game, but you should turn them off once you are used to it because it looks horrible.",
    ),
    o(0, "Edge scrolling", "Edge scrolling", Tick, "Makes the camera move when you move the mouse pointer to the edge of the screen"),
    o(
        0,
        "censorship",
        "Language censorship",
        Tick,
        "Filters out most of the bad language, if you don't like that sort of thing ruining your violent blood-bath and slavery experience.",
    ),
    o(
        0,
        "floaters",
        "Show damage floaters",
        Choice(&[("0", "Off"), ("1", "Simple info"), ("2", "Full info")]),
        "These are the floating text numbers that show the damage when a character is hit.  It also shows in brackets the amount of damage that was stopped by armour.  [Full Info] will show separate numbers for each damage type.",
    ),
    o(0, "font size2", "Font Size", Slider(-10.0, 32.0), ""),
    o(0, "", "", Blank, ""),
    o(
        0,
        "Fast zone hopping",
        "Fast zone hopping",
        Tick,
        "Allows you to jump between characters in different map zones without any loading time because it keeps a larger area in memory.  WARNING: will use more RAM and reduce frame rate",
    ),
    // Gameplay
    o(1, "", "Population settings (affects performance)", Header, ""),
    o(
        1,
        "Squad size multiplier",
        "Squad size multiplier",
        Slider(0.5, 3.0),
        "This multiplies the number of NPCs in each squad (Does not affect number of squads).  Setting higher than 1.0 means MORE characters and bigger battles.  WARNING! Will adversely affect performance.",
    ),
    o(
        1,
        "Global population multiplier",
        "Global population multiplier (num NPC squads)",
        Slider(0.5, 4.0),
        "This multiplies the number of squads in the world (Does not affect squad size).  Setting higher than 1.0 means MORE characters.  WARNING! Will adversely affect performance.",
    ),
    o(1, "", "Difficulty settings", Header, ""),
    o(
        1,
        "raidSizeMult",
        "Town raid events: size",
        Slider(0.25, 4.0),
        "Affects the size (and therefore difficulty) of attacking forces when nearby factions attack your town. WARNING! Making the raids bigger will affect performance",
    ),
    o(1, "raidFrequencyMult", "Town raid events: frequency", Slider(0.25, 3.0), "Affects the frequency of the raid events.  (Values > 1 means more frequent attacks)"),
    o(
        1,
        "attacks",
        "Town attacks frequency",
        Choice(&[("0", "Bombardment"), ("1", "High"), ("2", "Normal"), ("3", "Fewer"), ("4", "Rare"), ("5", "Never")]),
        "This is a separate thing to the raid events.  This is for the attacks by bandits and enemies that just happen to be passing by your base and decide to attack it, trying to take it over. The old default was [High]",
    ),
    o(
        1,
        "Dismemberment",
        "Dismemberment",
        Choice(&[("0", "Never"), ("1", "Rare"), ("2", "Frequent")]),
        "Affects how often limbs are lost.  [Frequent] means that a limb is lost whenever its HP drops below -100.  [Rare] means limbs are only lost when HIT and the damage goes below -100 (a rare occurence).  With either setting limbs can still be eaten off also.  [Never] means limb loss is disabled completely (boring)",
    ),
    // Graphics
    o(2, "Video Mode", "Resolution (need restart)", Resolution, ""),
    o(2, "Full Screen", "Full screen (need restart)", VideoTick("Yes"), "Fullscreen mode, use only if Borderless isn't working for you"),
    o(2, "Border", "Borderless (need restart)", VideoTick("None"), "Borderless is the recommended screen mode, it is a type of fullscreen that allows safer alt-tabbing"),
    o(
        2,
        "view distance",
        "View Distance",
        Slider(1500.0, 12000.0),
        "Increases the distance you can see, but has a negative effect on performance.  Recommend you don't set it extremely high or low.",
    ),
    o(2, "terrain detail", "Terrain Detail", Slider(0.0, 1.0), "Increases the quality of the terrain, but can have a negative effect on performance."),
    o(2, "grass range", "Grass View Range", Slider(0.0, 3.0), ""),
    o(2, "grass density", "Grass Density", Slider(0.0, 2.0), ""),
    o(2, "foliage range", "Foliage View Range", Slider(0.1, 4.0), ""),
    o(2, "npc range", "NPC View Range", Slider(1000.0, 8000.0), ""),
    o(2, "objects view range", "Objects View Range", Slider(1000.0, 8000.0), ""),
    o(2, "feature range", "Terrain Feature Range", Slider(0.0, 8.0), ""),
    o(2, "distant town range", "Distant Towns Range", Slider(0.0, 8.0), ""),
    o(
        2,
        "generate distant towns",
        "Generate Distant Towns",
        Tick,
        "Generate distant town meshes for towns that do not yet have them, such as player built towns and modded towns.\nThis may cause issues on some hardware.",
    ),
    o(
        2,
        "texture resolution gimping",
        "Texture Quality (need restart)",
        Choice(&[("0", "Maximum"), ("1", "High"), ("2", "Medium"), ("3", "Low"), ("4", "Fugly")]),
        "Increases texture quality, doesn't tend to affect performance but will affect memory use and loading times. /n WARNING!  If you don't have enough video RAM to handle the setting you choose then the game will crash randomly because that's just how Microsoft designed it.  Recommend > 4GB VRAM for max setting",
    ),
    o(
        2,
        "terrain patch size",
        "Terrain Chunk Size (need restart)",
        Choice(&[("0", "Small"), ("1", "Large")]),
        "The size of terrain chunks. [Large] will make the game run much faster when looking at the horizon for faster computers.  Slower systems using a shorter view distance may find it actually runs slower, in which case set it to [Small].  [Small] will also help if you have problems with running out of video RAM, which causes crashes",
    ),
    o(
        2,
        "water reflection",
        "Water reflection",
        Choice(&[("0", "Disabled"), ("1", "Landscape"), ("2", "Characters"), ("3", "Buildings"), ("4", "Everything")]),
        "Determines what is reflected in the water.  Turning this off will give a good performance boost",
    ),
    o(2, "shadow mode", "Shadows", Choice(&[("0", "Disabled"), ("1", "CSM"), ("2", "RTWSM")]), "Turn off shadows if you have problems with performance"),
    o(2, "shadow quality", "Shadow Quality", Choice(&[("0", "1024 (poor)"), ("1", "2048 (nice)"), ("2", "4096 (some video cards may freak out)")]), ""),
    o(
        2,
        "Shadow Range",
        "Shadow Range",
        Slider(1000.0, 9000.0),
        "Setting a longer range means that shadows are visible for a longer distance, but also reduces the shadows resolution/quality.  Find a balance that you like.",
    ),
    o(2, "Blood", "Blood", Tick, ""),
    o(2, "Decal Range", "Decal Range", Slider(0.0, 5000.0), ""),
    o(2, "Decal Resolution", "Decal Resolution", Choice(&[("512", "512"), ("1024", "1024"), ("2048", "2048")]), "The quality of blood-splats on the terrain"),
    o(
        2,
        "harpoonLimit",
        "Harpoon limit",
        Slider(10.0, 500.0),
        "The max number of harpoons/arrows that are left in the ground.  Reduce this if you have frame rate drops around heavy turret areas.",
    ),
    o(2, "", "", Blank, ""),
    // Audio
    o(3, "Music volume", "Music volume", Slider(0.0, 1.0), ""),
    o(3, "Ambient volume", "Ambient volume", Slider(0.0, 1.0), ""),
    o(3, "Footstep volume", "Footstep volume", Slider(0.0, 1.0), ""),
    o(3, "Sfx volume", "SFX volume", Slider(0.0, 1.0), ""),
    o(3, "VO volume", "VO volume", Slider(0.0, 1.0), ""),
    o(3, "UI volume", "UI volume", Slider(0.0, 1.0), ""),
    o(3, "", "", Blank, ""),
    o(3, "Music frequency", "Music Frequency (minutes)", Slider(1.0, 20.0), "Affects the overall amount of time that music plays in the game"),
    // Controls
    o(4, "invert X", "Invert mouse X", Tick, ""),
    o(4, "invert Y", "Invert mouse Y", Tick, ""),
    o(4, "swap mouse buttons", "Swap mouse buttons", Tick, "Swap left and right mouse buttons on the GUI"),
    o(
        4,
        "hardware_mouse",
        "Hardware Mouse",
        Tick,
        "Turning this on will use the windows hardware mouse pointer, which looks a bit weird but has the advantage that it's not affected by the frame-rate.  (REQURIES RESTART)",
    ),
    o(4, "camera speed", "Camera Move Speed", Slider(150.0, 1000.0), ""),
    o(4, "mouse speed pan", "Camera Rotate Speed X", Slider(2.0, 50.0), ""),
    o(4, "mouse speed tilt", "Camera Rotate Speed Y", Slider(2.0, 50.0), ""),
    o(4, "camera zoom", "Camera Zoom Speed", Slider(10.0, 200.0), ""),
    o(4, "", "", Blank, ""),
    o(4, "", "Key bindings", Header, ""),
];

/// The key binding rows (F032960): controls.cfg's actions with the window's names, in its
/// order; an empty action a heading. (not ported) The Tools section's editor keys do nothing
/// here.
const BINDINGS: &[(&str, &str)] = &[
    ("mouse_select", "Selection"),
    ("mouse_command", "Issue command"),
    ("pause", "Pause Game"),
    ("screenshot", "Take screenshot"),
    ("quicksave", "Quicksave"),
    ("quickload", "Quickload"),
    ("speed_1", "Normal game speed"),
    ("speed_2", "Faster game speed"),
    ("speed_3", "Fastest game speed"),
    ("floor_up", "Up one floor"),
    ("floor_down", "Down one floor"),
    ("highlight", "Highlight Items"),
    ("mouse_rotate", "Rotate camera"),
    ("camera_forward", "Move camera forward"),
    ("camera_back", "Move camera backward"),
    ("camera_left", "Move camera left"),
    ("camera_right", "Move camera right"),
    ("camera_tilt+", "Tilt camera up"),
    ("camera_tilt-", "Tilt camera down"),
    ("camera_rotate_left", "Rotate camera left"),
    ("camera_rotate_right", "Rotate camera right"),
    ("camera_zoom_in", "Zoom in"),
    ("camera_zoom_out", "Zoom out"),
    ("toggle_fps_camera", "Free camera mode"),
    ("toggle_camera_grid", "Toggle camera grid"),
    ("toggle_map", "Toggle map"),
    ("toggle_inventory", "Toggle character inventory"),
    ("toggle_stats", "Toggle character stats"),
    ("toggle_research", "Toggle research"),
    ("toggle_faction", "Toggle faction window"),
    ("toggle_bar", "Toggle main gui"),
    ("stop_movement", "Stop selected characters actions"),
    ("toggle_block", "Toggle block only mode"),
    ("toggle_hold", "Toggle hold mode"),
    ("toggle_passive", "Toggle passive mode"),
    ("toggle_jobs", "Toggle jobs"),
    ("toggle_ranged", "Toggle ranged mode"),
    ("toggle_sneak", "Toggle stealth mode"),
    ("cycle_run_speed", "Cycle movement speed"),
    ("medic", "Medic order"),
    ("rescue", "Rescue order"),
    ("prospect", "Prospect order"),
    ("focus_char", "Focus selected character"),
    ("select_all", "Select all characters"),
    ("select_0", "Select character 1"),
    ("select_1", "Select character 2"),
    ("select_2", "Select character 3"),
    ("select_3", "Select character 4"),
    ("select_4", "Select character 5"),
    ("select_5", "Select character 6"),
    ("select_6", "Select character 7"),
    ("select_7", "Select character 8"),
    ("select_8", "Select character 9"),
    ("select_9", "Select character 10"),
    ("character_next", "Select next character"),
    ("character_prev", "Select previous character"),
    ("change_squad", "Cycle squads"),
    ("", "Build mode"),
    ("toggle_build", "Toggle build mode"),
    ("build_apply", "Confirm build mode"),
    ("build_undo", "Undo last building"),
    ("build_rotate_left", "Rotate building left"),
    ("build_rotate_right", "Rotate building right"),
    ("build_move_up", "Move building up"),
    ("build_move_down", "Move building down"),
    ("build_tilt_increase", "Increase building slope"),
    ("build_tilt_decrease", "Decrease building slope"),
    ("", "Tools"),
    ("editor_toggle", "Toggle game editor"),
    ("rebuild_navmesh", "Rebuild navmesh"),
    ("reload_biomes", "Reload biome data"),
    ("gizmo_move", "Editor gizmo: move"),
    ("gizmo_rotate", "Editor gizmo: rotate"),
    ("gizmo_scale", "Editor gizmo: scale"),
];

/// The options as the game holds them: F032944 reads settings.cfg into its globals (at
/// 0x142133490) and the game's systems read those as they run. Here the install's settings.cfg
/// with the rewrite's copy over it, kept by the options window as each option changes
/// (`changed`); `changes` counts them.
#[derive(Resource, Clone, Default)]
pub struct Settings {
    pub cfg: Cfg,
    pub changes: u64,
}

/// F032944's defaults for the options it reads when settings.cfg lacks them (its ticks, and
/// the numbers a reader here takes).
pub const DEFAULTS: &[(&str, &str)] = &[
    ("Dismemberment", "1"),
    ("attacks", "2"),
    ("Show names", "1"),
    ("Edge scrolling", "1"),
    ("Movement marker", "1"),
    ("Rotation marker", "1"),
    ("censorship", "0"),
    ("Fast zone hopping", "0"),
    ("view distance", "6000"),
    ("terrain detail", "1"),
    ("terrain distant", "0"),
    ("terrain threshold", "750"),
    ("terrain patch size", "1"),
    ("grass range", "1"),
    ("grass density", "1"),
    ("foliage range", "1"),
    ("npc range", "3000"),
    ("objects view range", "4000"),
    ("feature range", "2"),
    ("distant town range", "6"),
    ("generate distant towns", "0"),
    ("mouse speed pan", "6"),
    ("mouse speed tilt", "6"),
    ("camera speed", "500"),
    ("camera zoom", "125"),
    ("floaters", "1"),
    ("font size2", "3"),
    ("invert X", "0"),
    ("invert Y", "1"),
    ("swap mouse buttons", "0"),
    ("hardware_mouse", "0"),
    ("tutorials", "1"),
    ("texture resolution gimping", "1"),
    ("water reflection", "1"),
    ("reflection range", "0.6"),
    ("shadow mode", "0"),
    ("shadow quality", "0"),
    ("Shadow Range", "5000"),
    ("harpoonLimit", "60"),
    ("Blood", "1"),
    ("Decal Range", "1000"),
    ("Decal Resolution", "1024"),
    ("Music volume", "0.5"),
    ("Ambient volume", "0.5"),
    ("Footstep volume", "0.5"),
    ("Sfx volume", "0.5"),
    ("UI volume", "0.5"),
    ("VO volume", "0.5"),
    ("Music frequency", "5"),
    ("Squad size multiplier", "1"),
    ("Global population multiplier", "1"),
    ("Auto save time (minutes)", "10"),
    ("raidSizeMult", "1"),
    ("raidFrequencyMult", "1"),
    ("Auto save", "1"),
    ("User save location", "1"),
];

/// An option's default (`DEFAULTS`).
pub fn default_of(key: &str) -> Option<&'static str> {
    DEFAULTS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

impl Settings {
    pub fn load(root: &std::path::Path) -> Settings {
        Settings { cfg: cfg::load(root, "settings.cfg"), changes: 0 }
    }

    /// A number (atof's: what does not read as one is the default).
    pub fn f32(&self, key: &str) -> f32 {
        let d = default_of(key).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        self.cfg.get(key).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d)
    }

    /// A tick (an int, on when not 0; what does not read as one is the default).
    pub fn on(&self, key: &str) -> bool {
        let d = default_of(key).is_some_and(|v| v != "0");
        self.cfg.get(key).map_or(d, |v| match v.trim() {
            "true" => true,
            "false" => false,
            t => t.parse::<f32>().map_or(d, |n| n != 0.0),
        })
    }
}

/// An option changed: kept (`Settings`), written to the rewrite's settings.cfg (what is there
/// now with this key set, so values the game writes meanwhile, "continue" and the autosave
/// index, stay) and put into effect where the game reads it as it runs (`apply_live`).
/// (stand-in) Kenshi writes settings.cfg once, as the window closes (F032954 -> F032951); the
/// rewrite writes each change at once, so nothing is lost if it stops with the window open.
fn changed(commands: &mut Commands, root: &std::path::Path, key: &str, value: &str) {
    let (k, v, root) = (key.to_string(), value.to_string(), root.to_path_buf());
    commands.queue(move |world: &mut World| {
        let mut c = cfg::load(&root, "settings.cfg");
        c.set(&k, &v);
        let _ = cfg::save("settings.cfg", &c);
        if let Some(mut st) = world.get_resource_mut::<Settings>() {
            st.cfg.set(&k, &v);
            st.changes += 1;
        }
        apply_live(world, &k, &v);
        if std::env::var("KENSHI_OPTIONS_DEBUG").is_ok() {
            debug_state(world, &format!("{k}={v}"));
        }
    });
}

/// (testing) KENSHI_OPTIONS_DEBUG: what the options are in effect as, after a change.
fn debug_state(world: &mut World, what: &str) {
    let mut far = None;
    let mut cams = world.query_filtered::<&Projection, super::MainCam>();
    for p in cams.iter(world) {
        if let Projection::Perspective(pp) = p {
            far = Some(pp.far);
        }
    }
    let sky = world.get_resource::<super::Sky>().map(|s| (s.0.view_distance, s.0.npc_range, s.0.objects_view_range, s.0.fast_zone_hopping, s.0.shadow_range, s.0.heat_haze, s.0.fxaa, s.0.shadow_mode, s.0.shadow_quality));
    let hdr = world.get_resource::<kenshi_render::hdr::KenshiHdr>().map(|h| (h.haze_enabled, h.fxaa));
    let map = world.get_resource::<bevy::light::DirectionalLightShadowMap>().map(|m| m.size);
    let mut suns = world.query::<&DirectionalLight>();
    let shadows: Vec<bool> = suns.iter(world).map(|l| l.shadow_maps_enabled).collect();
    let lod = world.get_resource::<kenshi_render::terrain_lod::LodTerrain>().map(|l| format!("{:?}", l.detail));
    let marker = world.get_resource::<super::move_marker::MarkerOptions>().map(|m| (m.movement, m.rotation));
    let floaters = world.get_resource::<super::floaters::FloaterMode>().map(|f| f.0);
    let game = world.get_resource::<Game>().map(|g| {
        (g.sim.population.as_ref().map(|p| p.mult), g.sim.world.raid_frequency, g.sim.arena.sh.harpoon_limit, g.sim.squad_size_mult, g.sim.arena.sh.mc.dismemberment)
    });
    eprintln!(
        "options: {what}: far {far:?} sky(view,npc,objects,hop,shadow range,haze,fxaa,mode,quality) {sky:?} hdr(haze,fxaa) {hdr:?} shadow map {map:?} suns {shadows:?} terrain {lod:?} markers {marker:?} floaters {floaters:?} game(pop,raids,harpoons,squad,dismember) {game:?} font size2 {} censorship {}",
        kenshi_gui::res::font_size2(),
        CENSORSHIP.load(std::sync::atomic::Ordering::Relaxed)
    );
}

/// The options Kenshi puts into effect as they change (the rows' callbacks in F032960, and
/// the systems reading the globals as they run): the markers, the floaters, the sound volumes
/// and the music's frequency (F032926), the view distance and terrain detail (F032930: the
/// far clip, F041473, and the terrain's detail, F045916), the NPC and object view ranges and
/// zone hopping (read each frame), the harpoon limit (F033871), the population (F048140), the
/// raids' frequency (F050802), the squad size (F038271) and dismemberment (F040472): see
/// `sync_game` for the last.
fn apply_live(world: &mut World, key: &str, value: &str) {
    let n = value.trim().parse::<f32>().ok();
    match key {
        "Movement marker" | "Rotation marker" => {
            if let Some(mut m) = world.get_resource_mut::<super::move_marker::MarkerOptions>() {
                let on = n.is_none_or(|v| v != 0.0);
                if key == "Movement marker" {
                    m.movement = on;
                } else {
                    m.rotation = on;
                }
            }
        }
        "floaters" => {
            if let Some(mut f) = world.get_resource_mut::<super::floaters::FloaterMode>() {
                f.0 = n.map_or(1, |v| v as i32);
            }
        }
        "Music frequency" => {
            if let (Some(mut snd), Some(v)) = (world.get_resource_mut::<super::sound::Sound>(), n) {
                snd.set_music_frequency(v);
            }
        }
        k if k.ends_with(" volume") => {
            if let Some(mut snd) = world.get_resource_mut::<super::sound::Sound>() {
                snd.set_volume(k, n.unwrap_or(0.5));
            }
        }
        "view distance" => {
            if let Some(v) = n {
                super::set_view_distance(world, v);
            }
        }
        "terrain detail" => {
            let st = world.get_resource::<Settings>().cloned().unwrap_or_default();
            if let Some(mut lod) = world.get_resource_mut::<kenshi_render::terrain_lod::LodTerrain>() {
                lod.detail = kenshi_render::terrain_lod::Detail::from_options(st.f32("terrain detail"), st.f32("terrain distant"), st.f32("terrain threshold"));
            }
        }
        "npc range" | "objects view range" | "Fast zone hopping" | "Shadow Range" => super::set_sky_option(world, key, n.unwrap_or(0.0)),
        "censorship" => set_censorship(n.is_some_and(|v| v != 0.0)),
        _ => {}
    }
}

/// The options' "Language censorship" (0x142133575) as the dialogue reads it.
static CENSORSHIP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_censorship(on: bool) {
    CENSORSHIP.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// @fn F040907
///
/// A line of dialogue as the game shows it: with "Language censorship" on, each of these words
/// replaced wherever it stands, in this order (a part of a longer word too: "shitting" first,
/// then "shit"); off, as written.
pub fn censor(text: &str) -> String {
    const SWAPS: [(&str, &str); 17] = [
        ("shitting", "crapping"),
        ("Shitting", "Crapping"),
        ("shitty", "crappy"),
        ("Shitty", "Crappy"),
        ("shit", "crap"),
        ("Shit", "Crap"),
        ("SHIT", "CRAP"),
        ("fuck", "bugger"),
        ("FUCK", "BUGGER"),
        ("Fuck", "Bugger"),
        ("Bollocks", "Damn"),
        ("bollocks", "damn"),
        ("BOLLOCKS", "DAMN"),
        ("asshole", "idiot"),
        ("Asshole", "Idiot"),
        ("ASSHOLE", "IDIOT"),
        ("asswipe", "idiot"),
    ];
    if !CENSORSHIP.load(std::sync::atomic::Ordering::Relaxed) {
        return text.to_string();
    }
    let mut s = text.to_string();
    for (bad, mild) in SWAPS {
        s = s.replace(bad, mild);
    }
    s
}

/// @fn F046180
/// @fn F046181
///
/// "Swap mouse buttons" (0x14213346b): the input handler's mouse presses and releases take
/// the left button for the right and the right for the left, everywhere (the GUI and the
/// world), before anything reads them.
pub fn swap_mouse(st: Option<Res<Settings>>, mut b: ResMut<ButtonInput<MouseButton>>) {
    if !st.is_some_and(|s| s.on("swap mouse buttons")) {
        return;
    }
    let state = |b: &ButtonInput<MouseButton>, m: MouseButton| (b.pressed(m), b.just_pressed(m), b.just_released(m));
    let (l, r) = (state(&b, MouseButton::Left), state(&b, MouseButton::Right));
    if l == r {
        return;
    }
    for (m, (pressed, jp, jr)) in [(MouseButton::Left, r), (MouseButton::Right, l)] {
        b.reset(m);
        if pressed || jp || jr {
            b.press(m);
            if !jp {
                b.clear_just_pressed(m);
            }
            if jr && !pressed {
                b.release(m);
            }
        }
    }
}

/// The options Kenshi puts into effect as the window closes (F032946 -> F032942, F032954 ->
/// F032951): the font size (every font its loaded size + "font size2"), the shadows' mode and
/// quality (F0125df: 1024, 2048 or 4096), the compositors' ticks (HeatHaze, FXAA).
fn apply_on_close(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        let Some(st) = world.get_resource::<Settings>().cloned() else { return };
        super::set_font_size(world, st.f32("font size2"));
        super::set_shadows(world, st.f32("shadow mode") as i32, st.f32("shadow quality") as i32);
        super::set_sky_option(world, "HeatHaze", if st.on("HeatHaze") { 1.0 } else { 0.0 });
        super::set_sky_option(world, "FXAA", if st.on("FXAA") { 1.0 } else { 0.0 });
        if std::env::var("KENSHI_OPTIONS_DEBUG").is_ok() {
            debug_state(world, "closed");
        }
    });
}

/// The options the game's own state reads as it runs, kept in it each frame (Kenshi's systems
/// read the globals live): "Global population multiplier" (ZoneManager, F048140),
/// "raidFrequencyMult" (F050802), "raidSizeMult" (F050803), "harpoonLimit" (F033871), "Squad size multiplier"
/// (F038271), "Dismemberment" (F040472, F040473: 0 never, 1 a hit's, 2 any limb under its
/// most health). A game loaded takes them at once.
pub fn sync_game(st: Res<Settings>, mut game: ResMut<Game>) {
    let (pop, raids, raid_size, harpoons, squad, dismember) = (
        st.f32("Global population multiplier"),
        st.f32("raidFrequencyMult"),
        st.f32("raidSizeMult"),
        st.f32("harpoonLimit").clamp(10.0, 500.0) as usize,
        st.f32("Squad size multiplier"),
        st.f32("Dismemberment") as i32,
    );
    let sim = &game.sim;
    let same = sim.population.as_ref().is_none_or(|p| p.mult == pop)
        && sim.world.raid_frequency == raids
        && sim.world.raid_size == raid_size
        && sim.arena.sh.harpoon_limit == harpoons
        && sim.squad_size_mult == squad
        && sim.arena.sh.mc.dismemberment == dismember;
    if same {
        return;
    }
    let sim = &mut game.sim;
    if let Some(p) = sim.population.as_mut() {
        p.mult = pop;
    }
    sim.world.raid_frequency = raids;
    sim.world.raid_size = raid_size;
    sim.arena.sh.harpoon_limit = harpoons;
    sim.squad_size_mult = squad;
    sim.arena.sh.mc.dismemberment = dismember;
}

impl Menus {
    /// The main menu's OPTIONS (Kenshi_MainMenu.layout's OptionsButton, F048549's): the
    /// options window over the title screen.
    pub fn open_options(&mut self, commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, root: &std::path::Path, size: [f32; 2], monitors: &Query<&bevy::window::Monitor>) {
        if self.options.is_none() {
            self.options = Some(open_options(commands, gui, assets, root, size, video_modes(monitors)));
        }
    }

}

/// The Esc menu and the options window.
#[derive(Resource, Default)]
pub struct Menus {
    pause: Option<Built>,
    /// The game's paused state from before the Esc menu.
    was_paused: bool,
    options: Option<Options>,
    /// Some window other than these was open last frame (Esc closes it, not opening the menu).
    other_open: bool,
}


struct Options {
    /// The install the window reads and writes the options of.
    root: std::path::PathBuf,
    ui: Built,
    tabs: Built,
    tab: u8,
    settings: Cfg,
    controls: Cfg,
    /// Ogre's kenshi.cfg (the screen's resolution and mode), the screen's video modes, the
    /// compositors (compositors.cfg), the active mods (names and versions).
    video: Cfg,
    modes: Vec<String>,
    compositors: Vec<String>,
    mods: Vec<(String, i32)>,
    /// The rows of the tab shown: holder, the option (or binding) and its control entities.
    rows: Vec<Row>,
    list: Entity,
    scroll: f32,
    /// A binding waiting for its key.
    binding: Option<usize>,
    /// A choice's list dropped down.
    drop: Option<Drop>,
}

/// A combo box's list shown (MyGUI ComboBox::showList, Kenshi_ComboBox's List: a Kenshi_ListBox
/// on the Popup layer): the row (in `Options::rows`) it is for, the list's root, how far it is
/// scrolled and whether its rows are made.
struct Drop {
    row: usize,
    root: Entity,
    scroll: usize,
}

/// The drop-down list's rows (build_ui::ListRow's tag).
const DROP: u8 = 91;

struct Row {
    holder: Entity,
    opt: Option<usize>,
    bind: Option<usize>,
    /// A compositor's tick, or the key bindings' Defaults button.
    comp: Option<usize>,
    defaults: bool,
    control: Option<Entity>,
    number: Option<Entity>,
}

fn text_def(name: &str, caption: &str, pos: [f32; 4], font: &str, align: &str) -> kenshi_gui::WidgetDef {
    kenshi_gui::WidgetDef {
        kind: "TextBox".into(),
        skin: "Kenshi_TextboxStandardText".into(),
        pos: kenshi_gui::res::Pos::Px(pos),
        align: Default::default(),
        name: name.into(),
        layer: None,
        // (assumption) the light parchment tone Kenshi's option labels draw in; the textbox skin alone
        // gave dark brown text on the dark panel
        props: vec![("Caption".into(), caption.into()), ("TextAlign".into(), align.into()), ("FontName".into(), font.into()), ("TextColour".into(), "0.85 0.8 0.7".into())],
        user: Vec::new(),
        children: Vec::new(),
    }
}

fn button_def(name: &str, skin: &str, caption: &str, pos: [f32; 4]) -> kenshi_gui::WidgetDef {
    kenshi_gui::WidgetDef {
        kind: "Button".into(),
        skin: skin.into(),
        pos: kenshi_gui::res::Pos::Px(pos),
        align: Default::default(),
        name: name.into(),
        layer: None,
        props: if caption.is_empty() { Vec::new() } else { vec![("Caption".into(), caption.into())] },
        user: Vec::new(),
        children: Vec::new(),
    }
}

/// A float as Ogre's StringConverter::toString writes it (precision 6, F042345): six
/// significant digits, no trailing zeros.
/// A choice's combo box (F042391: F043440's ComboBox in Kenshi_ComboBox, its edit static), its
/// text the value's name.
fn combo_def(text: &str, pos: [f32; 4]) -> kenshi_gui::WidgetDef {
    let mut d = button_def("Control", "Kenshi_ComboBox", "", pos);
    d.kind = "ComboBox".into();
    // (its text goes on its Client part, `combo_text`, not on the box)
    d.props = Vec::new();
    d.user = vec![("ComboText".into(), text.to_string())];
    d
}

/// A combo box's text (its Client part's).
fn combo_text(commands: &mut Commands, combo: Entity, text: String) {
    commands.queue(move |world: &mut World| {
        if let Some(c) = world.get::<kenshi_gui::ui::Parts>(combo).and_then(|p| p.0.get("Client").copied()) {
            SetCaption(c, text).apply(world);
        }
    });
}

/// The names a combo box's list holds and the one set: a choice's names, or the screen's video
/// modes.
fn combo_items(o: &Options, op: &Opt) -> (Vec<String>, Option<usize>) {
    match op.kind {
        Choice(cs) => {
            let cur = o.settings.get(op.key).unwrap_or("0").to_string();
            (cs.iter().map(|(_, n)| n.to_string()).collect(), cs.iter().position(|(k, _)| *k == cur))
        }
        Resolution => {
            let cur = o.video.get(op.key).unwrap_or("");
            (o.modes.clone(), o.modes.iter().position(|m| cur.starts_with(m.as_str())))
        }
        _ => (Vec::new(), None),
    }
}

/// The list's line height (Kenshi_ListBox's HeightLine) and its frame round the rows (the
/// template's 165 x 135 about its 120 x 95 client), unscaled.
const LINE: f32 = 18.0;
const FRAME: [f32; 4] = [15.0, 20.0, 30.0, 20.0];

/// @fn F043440
///
/// A combo box's list shown (MyGUI ComboBox::showList): as wide as the box, under it (over it
/// when it would leave the screen), as tall as its rows and frame need up to MaxListLength 200;
/// the item set chosen. (not ported) Its smooth show (a fade in), its scroll bar (the wheel
/// scrolls it).
fn show_drop(commands: &mut Commands, o: &mut Options, k: usize, at: Rect, screen: [f32; 2], gui: &GuiRes, assets: &AssetServer) {
    let Some(op) = o.rows[k].opt.map(|i| &OPTS[i]) else { return };
    let (items, chosen) = combo_items(o, op);
    let scale = kenshi_gui::res::ui_scale();
    let h = ((FRAME[1] + FRAME[3] + items.len() as f32 * LINE).min(200.0) * scale).floor();
    let top = if at.max.y + h > screen[1] { at.min.y - h } else { at.max.y };
    let def = kenshi_gui::WidgetDef {
        kind: "ListBox".into(),
        skin: "Kenshi_ListBox".into(),
        pos: kenshi_gui::res::Pos::Px([at.min.x, top, at.width(), h]),
        align: Default::default(),
        name: "DropList".into(),
        layer: None,
        props: Vec::new(),
        user: Vec::new(),
        children: Vec::new(),
    };
    let built = kenshi_gui::ui::spawn(commands, &gui.0, assets, &[def], None, screen);
    let Some(&root) = built.roots.first() else { return };
    // the Popup layer, over the options window
    commands.entity(root).insert((GlobalZIndex(90), RelativeCursorPosition::default(), Interaction::default(), bevy::ui::FocusPolicy::Block));
    let visible = (((h / scale - FRAME[1] - FRAME[3]) / LINE).floor().max(1.0)) as usize;
    let scroll = chosen.map_or(0, |c| c.saturating_sub(visible - 1)).min(items.len().saturating_sub(visible));
    o.drop = Some(Drop { row: k, root, scroll });
    drop_rows(commands, root, items, chosen, scroll, gui, assets);
}

/// The list's rows from `scroll` on, in its frame (a ListRow(DROP, k) each).
fn drop_rows(commands: &mut Commands, root: Entity, items: Vec<String>, chosen: Option<usize>, scroll: usize, gui: &GuiRes, assets: &AssetServer) {
    let font = gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
    commands.queue(move |world: &mut World| {
        let Some(parts) = world.get::<kenshi_gui::ui::Parts>(root).cloned() else { return };
        let Some(&client) = parts.0.get("Client") else { return };
        if let Some(&bar) = parts.0.get("VScroll") {
            world.entity_mut(bar).insert(Visibility::Hidden);
        }
        // the rows made before go
        let old: Vec<Entity> = world.get::<Children>(client).map(|c| c.iter().collect()).unwrap_or_default();
        for e in old {
            world.entity_mut(e).despawn();
        }
        let rows: Vec<(String, bool)> = items.iter().skip(scroll).map(|n| (n.clone(), true)).collect();
        let mut c = world.commands();
        let column = super::build_ui::list_rows(&mut c, client, DROP, &rows, chosen.and_then(|k| k.checked_sub(scroll)), &font, (0.0, 0.0, 100.0, 100.0));
        let _ = column;
        world.flush();
    });
}

fn hide_drop(commands: &mut Commands, o: &mut Options) {
    if let Some(d) = o.drop.take() {
        commands.entity(d.root).try_despawn();
    }
}

fn ogre_float(v: f32) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let digits = (5 - v.abs().log10().floor() as i32).max(0) as usize;
    let s = format!("{v:.digits$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

fn value_text(kind: Kind, v: f32) -> String {
    match kind {
        Slider(..) => ogre_float(v),
        _ => String::new(),
    }
}

/// The compositors the graphics tab ticks (data/materials/compositors/compositors.cfg: a name a
/// line, # a comment).
fn compositors(root: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("data/materials/compositors/compositors.cfg")).unwrap_or_default();
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect()
}

/// The active mods, each with its file's version (the Mods tab).
fn active_mods(root: &std::path::Path) -> Vec<(String, i32)> {
    let order = kenshi_io::mods::Install::new(root).load_order().map(|o| o.files).unwrap_or_default();
    order
        .into_iter()
        .filter(|f| !f.core)
        .map(|f| {
            let v = kenshi_io::fcs::read_header(&f.path).map_or(0, |h| h.version);
            (f.name.trim_end_matches(".mod").to_string(), v)
        })
        .collect()
}

fn choice_name(kind: Kind, v: &str) -> String {
    match kind {
        Choice(cs) => cs.iter().find(|(k, _)| *k == v).map_or(v.to_string(), |(_, n)| n.to_string()),
        _ => String::new(),
    }
}

/// The screen's video modes as Ogre names them ("1920 x 1080 @ 32-bit colour"), largest first.
fn video_modes(monitors: &Query<&bevy::window::Monitor>) -> Vec<String> {
    let mut sizes: Vec<(u32, u32)> = monitors.iter().flat_map(|m| m.video_modes.iter().map(|v| (v.physical_size.x, v.physical_size.y))).collect();
    sizes.sort_by(|a, b| b.cmp(a));
    sizes.dedup();
    sizes.into_iter().map(|(w, h)| format!("{w} x {h} @ 32-bit colour")).collect()
}

/// The rows of `o.tab`, made again.
fn fill(commands: &mut Commands, o: &mut Options, gui: &GuiRes, assets: &AssetServer, width: f32) {
    hide_drop(commands, o);
    for r in o.rows.drain(..) {
        commands.entity(r.holder).despawn();
    }
    let scale = kenshi_gui::res::ui_scale();
    let row_h = 44.0 * scale;
    let mut k = 0usize;
    let mut add = |commands: &mut Commands, defs: Vec<kenshi_gui::WidgetDef>, opt: Option<usize>, bind: Option<usize>, control: &str, number: &str, (comp, defaults): (Option<usize>, bool)| {
        let holder = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(k as f32 * row_h),
                    width: Val::Px(width),
                    height: Val::Px(row_h),
                    ..default()
                },
                RelativeCursorPosition::default(),
            ))
            .id();
        commands.entity(o.list).add_child(holder);
        let built = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, Some(holder), [width, row_h]);
        // a combo box: pressed anywhere it drops its list (its edit is static)
        if let Some(c) = built.get(control)
            && defs.iter().any(|d| d.name == control && d.skin == "Kenshi_ComboBox")
        {
            commands.entity(c).insert((Interaction::default(), RelativeCursorPosition::default()));
            if let Some(text) = defs.iter().find(|d| d.name == control).and_then(|d| d.user.iter().find(|(k, _)| k == "ComboText")).map(|(_, v)| v.clone()) {
                combo_text(commands, c, text);
            }
        }
        o.rows.push(Row { holder, opt, bind, comp, defaults, control: built.get(control), number: built.get(number) });
        k += 1;
    };
    let (lw, cx) = (width * 0.48, width * 0.5);
    for (i, op) in OPTS.iter().enumerate().filter(|(_, op)| op.tab == o.tab) {
        let label = |f: &str| text_def("Label", op.label, [10.0 * scale, 0.0, lw, row_h], f, "Left VCenter");
        let v = o.settings.get(op.key).unwrap_or("").to_string();
        let tick = || button_def("Control", "Kenshi_TickBoxSkin", "", [cx, (row_h - 33.0 * scale) * 0.5, 35.0 * scale, 33.0 * scale]);
        let defs = match op.kind {
            Header => vec![text_def("Label", op.label, [0.0, 0.0, width, row_h], "Kenshi_PaintedTextFont_Large", "Left VCenter")],
            Blank => Vec::new(),
            Tick | VideoTick(_) => vec![label("Kenshi_StandardFont_Large"), tick()],
            // F042415: the button 0.2 of the row wide, its label empty
            Action(caption) => vec![button_def("Control", "Kenshi_Button2", caption, [cx, 4.0 * scale, width * 0.2, row_h - 8.0 * scale])],
            Resolution => {
                let shown = o.video.get(op.key).unwrap_or("").to_string();
                vec![label("Kenshi_StandardFont_Large"), combo_def(&shown, [cx, 4.0 * scale, width * 0.4, row_h - 8.0 * scale])]
            }
            Slider(min, max) => {
                let mut bar = button_def("Control", "Kenshi_ScrollBar", "", [cx, 4.0 * scale, width * 0.36, row_h - 8.0 * scale]);
                bar.kind = "ScrollBar".into();
                // F042370: 100 steps a unit
                bar.props.push(("Range".into(), format!("{}", ((max - min) * 100.0).round())));
                let f = v.parse::<f32>().unwrap_or(min);
                vec![
                    label("Kenshi_StandardFont_Large"),
                    bar,
                    text_def("Number", &value_text(op.kind, f), [cx + width * 0.37, 0.0, width * 0.12, row_h], "Kenshi_StandardFont_Large", "Center"),
                ]
                .into_iter()
                .map(|mut d| {
                    if d.name == "Control" {
                        d.props.push(("ScrollPosition".into(), format!("{}", ((f - min) * 100.0).round())));
                    }
                    d
                })
                .collect()
            }
            Choice(_) => vec![label("Kenshi_StandardFont_Large"), combo_def(&choice_name(op.kind, &v), [cx, 4.0 * scale, width * 0.3, row_h - 8.0 * scale])],
        };
        add(commands, defs, Some(i), None, "Control", "Number", (None, false));
    }
    // the graphics tab's compositor ticks (F032960's loop over compositors.cfg's names)
    if o.tab == 2 {
        for (c, name) in o.compositors.clone().iter().enumerate() {
            let defs = vec![
                text_def("Label", name, [10.0 * scale, 0.0, lw, row_h], "Kenshi_StandardFont_Large", "Left VCenter"),
                button_def("Control", "Kenshi_TickBoxSkin", "", [cx, (row_h - 33.0 * scale) * 0.5, 35.0 * scale, 33.0 * scale]),
            ];
            add(commands, defs, None, None, "Control", "", (Some(c), false));
        }
    }
    if o.tab == 4 {
        for (b, (action, name)) in BINDINGS.iter().enumerate() {
            // an empty action: a heading (Build mode, Tools)
            if action.is_empty() {
                add(commands, vec![text_def("Label", name, [0.0, 0.0, width, row_h], "Kenshi_PaintedTextFont_Large", "Left VCenter")], None, None, "", "", (None, false));
                continue;
            }
            let keys = o.controls.all(action).join(", ");
            let defs = vec![
                text_def("Label", name, [10.0 * scale, 0.0, lw, row_h], "Kenshi_StandardFont_Large", "Left VCenter"),
                button_def("Control", "Kenshi_Button1", if keys.is_empty() { "-" } else { &keys }, [cx, 4.0 * scale, width * 0.3, row_h - 8.0 * scale]),
            ];
            add(commands, defs, None, Some(b), "Control", "", (None, false));
        }
        // the bindings' Defaults (Kenshi_Button2)
        add(commands, vec![button_def("Control", "Kenshi_Button2", "Defaults", [cx, 4.0 * scale, width * 0.2, row_h - 8.0 * scale])], None, None, "Control", "", (None, true));
    }
    // the Mods tab: "[Active mods]" in #afa68b, then "- name" and "Version: n" a mod
    if o.tab == 5 {
        add(commands, vec![text_def("Label", "[Active mods]", [10.0 * scale, 0.0, lw, row_h], "Kenshi_StandardFont_Large", "Left VCenter")], None, None, "", "", (None, false));
        for (name, v) in o.mods.clone() {
            let defs = vec![
                text_def("Label", &format!("- {name}"), [10.0 * scale, 0.0, lw, row_h], "Kenshi_StandardFont_Large", "Left VCenter"),
                text_def("Number", &format!("Version: {v}"), [cx, 0.0, width * 0.4, row_h], "Kenshi_StandardFont_Large", "Left VCenter"),
            ];
            add(commands, defs, None, None, "", "", (None, false));
        }
    }
    // the sliders where their values are
    for r in &o.rows {
        if let (Some(i), Some(c)) = (r.opt, r.control)
            && let Slider(min, max) = OPTS[i].kind
        {
            let f = Settings { cfg: o.settings.clone(), changes: 0 }.f32(OPTS[i].key).clamp(min, max);
            commands.queue(kenshi_gui::ui::SetScroll(c, ((f - min) * 100.0).clamp(0.0, (max - min) * 100.0)));
        }
    }
    o.scroll = 0.0;
}

fn open_options(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, root: &std::path::Path, size: [f32; 2], modes: Vec<String>) -> Options {
    let mut defs = gui.0.layout("Kenshi_OptionsPanel.layout");
    for d in &mut defs {
        if let kenshi_gui::res::Pos::Real(r) = d.pos {
            d.pos = kenshi_gui::res::Pos::Real([(1.0 - r[2]) * 0.5, (1.0 - r[3]) * 0.5, r[2], r[3]]);
        }
    }
    let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(80));
    }
    let place = ui.get("OptionsTab").or_else(|| ui.roots.first().copied()).unwrap_or(Entity::PLACEHOLDER);
    let tab_size = ui.sizes.get(&place).copied().unwrap_or([size[0] * 0.6, size[1] * 0.68]);
    let scale = kenshi_gui::res::ui_scale();
    let bh = tab_size[1] * 0.06;
    let buttons: Vec<kenshi_gui::WidgetDef> =
        TABS.iter().enumerate().map(|(k, t)| button_def(&format!("OptionsTab{k}"), "Kenshi_Button1", t, [k as f32 * 144.0 * scale, 0.0, 140.0 * scale, bh])).collect();
    let tabs = kenshi_gui::ui::spawn(commands, &gui.0, assets, &buttons, Some(place), tab_size);
    let list = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(bh + 6.0 * scale),
            width: Val::Px(tab_size[0]),
            height: Val::Px(tab_size[1] - bh - 6.0 * scale),
            overflow: Overflow::clip(),
            ..default()
        })
        .id();
    let inner = commands.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(tab_size[0]), ..default() }).id();
    commands.entity(list).add_child(inner);
    commands.entity(place).add_child(list);
    let mut o = Options {
        root: root.to_path_buf(),
        ui,
        tabs,
        tab: 0,
        settings: cfg::load(root, "settings.cfg"),
        controls: cfg::load(root, "controls.cfg"),
        video: cfg::load(root, "kenshi.cfg"),
        modes,
        compositors: compositors(root),
        mods: active_mods(root),
        rows: Vec::new(),
        list: inner,
        scroll: 0.0,
        binding: None,
        drop: None,
    };
    fill(commands, &mut o, gui, assets, tab_size[0] - 20.0 * scale);
    o
}

fn close_options(commands: &mut Commands, mut o: Options) {
    hide_drop(commands, &mut o);
    apply_on_close(commands);
    for r in o.rows {
        commands.entity(r.holder).despawn();
    }
    for r in o.tabs.roots.iter().chain(&o.ui.roots) {
        commands.entity(*r).despawn();
    }
}

/// A tick as the window shows it (`Settings::on`'s reading of it).
fn tick_on(c: &Cfg, key: &str) -> bool {
    Settings { cfg: c.clone(), changes: 0 }.on(key)
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut menus: ResMut<Menus>,
    mut game: Option<ResMut<Game>>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clicks: MessageReader<GuiClick>,
    mut scrolls: MessageReader<GuiScroll>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window>,
    hovers: Query<&RelativeCursorPosition>,
    mut nodes: Query<&mut Node>,
    (inv, stats, build, research, slaves): (
        Option<Res<super::inventory_ui::InventoryWindow>>,
        Option<Res<super::stats_ui::StatsWindow>>,
        Option<Res<super::build_ui::BuildMode>>,
        Option<Res<super::research_ui::ResearchWindow>>,
        Option<Res<super::slave_ui::SlaveTrade>>,
    ),
    (mut frames, monitors, mut tutorials, parts, placed, drop_rows_q, mouse): (
        Local<u32>,
        Query<&bevy::window::Monitor>,
        ResMut<super::tutorial_ui::Tutorials>,
        Query<&kenshi_gui::ui::Parts>,
        Query<(&ComputedNode, &bevy::ui::UiGlobalTransform)>,
        Query<(&Interaction, &super::build_ui::ListRow), Changed<Interaction>>,
        Res<ButtonInput<MouseButton>>,
    ),
    mut saves: ResMut<super::saves_ui::SavesPanel>,
    mut new_game: Option<ResMut<super::menu::Menu>>,
) {
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let mut clicked: Vec<GuiClick> = clicks.read().cloned().collect();
    // (testing) KENSHI_OPTIONS_SCRIPT="Escape@300,OptionsButton@330,OptionsTab2@360,Combo1@400":
    // Esc or a named button of these windows (or the tab's n-th combo box) pressed at those frames
    *frames += 1;
    let mut esc_scripted = false;
    if let Ok(v) = std::env::var("KENSHI_OPTIONS_SCRIPT") {
        for step in v.split(',') {
            let Some((what, at)) = step.rsplit_once('@') else { continue };
            if at.parse::<u32>().ok() != Some(*frames) {
                continue;
            }
            if what == "Escape" {
                esc_scripted = true;
                continue;
            }
            // "Set:<key>=<value>": an option changed as the window changes it
            if let Some((k, v)) = what.strip_prefix("Set:").and_then(|kv| kv.split_once('=')) {
                let root = game.as_ref().map(|g| g.root.clone()).or_else(|| menus.options.as_ref().map(|o| o.root.clone()));
                if let Some(root) = root {
                    if let Some(o) = menus.options.as_mut() {
                        o.settings.set(k, v);
                    }
                    changed(&mut commands, &root, k, v);
                }
                continue;
            }
            // "Debug": what the options are in effect as, printed
            if what == "Debug" {
                commands.queue(|world: &mut World| debug_state(world, "now"));
                continue;
            }
            // "Close": the window's CLOSE (its close-time options)
            if what == "Close" {
                if let Some(o) = menus.options.take() {
                    close_options(&mut commands, o);
                }
                continue;
            }
            let found = menus
                .pause
                .iter()
                .chain(menus.options.iter().map(|o| &o.ui))
                .chain(menus.options.iter().map(|o| &o.tabs))
                .find_map(|b| b.get(what));
            // "Combo<n>": the tab's n-th combo box
            let combo = what.strip_prefix("Combo").and_then(|n| n.parse::<usize>().ok()).and_then(|n| {
                let o = menus.options.as_ref()?;
                o.rows.iter().filter(|r| r.opt.is_some_and(|i| matches!(OPTS[i].kind, Choice(_) | Resolution))).nth(n).and_then(|r| r.control)
            });
            if let Some(e) = found.or(combo) {
                clicked.push(GuiClick { widget: e, name: if combo.is_some() { "Control".to_string() } else { what.to_string() } });
            }
        }
    }
    let others = inv.is_some_and(|w| w.is_open())
        || stats.is_some_and(|w| w.is_open())
        || build.is_some_and(|w| w.is_open())
        || research.is_some_and(|w| w.tab().is_some())
        || slaves.is_some_and(|w| w.is_open());
    let other_was = std::mem::replace(&mut menus.other_open, others);
    // a binding takes the next key pressed (Escape cancels)
    if let Some(o) = menus.options.as_mut()
        && let Some(b) = o.binding
    {
        // a modifier is held for the key it goes with; let go alone, it is the key itself
        const MODS: [KeyCode; 6] = [KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::AltLeft, KeyCode::AltRight];
        let held = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);
        let prefix = [(KeyCode::ControlLeft, KeyCode::ControlRight, "Ctrl+"), (KeyCode::ShiftLeft, KeyCode::ShiftRight, "Shift+"), (KeyCode::AltLeft, KeyCode::AltRight, "Alt+")]
            .iter()
            .filter(|(a, b, _)| held(*a, *b))
            .map(|(_, _, p)| *p)
            .collect::<String>();
        let pick = keys
            .get_just_pressed()
            .find(|k| !MODS.contains(k))
            .map(|k| (*k, prefix))
            .or_else(|| keys.get_just_released().find(|k| MODS.contains(k)).map(|k| (*k, String::new())));
        if let Some((k, prefix)) = pick {
            let k = &k;
            o.binding = None;
            if *k != KeyCode::Escape {
                let name = format!("{prefix}{}", super::bindings::key_name(*k));
                o.controls.set_all(BINDINGS[b].0, &[name.clone()]);
                let _ = cfg::save("controls.cfg", &o.controls);
                commands.insert_resource(super::bindings::Bindings::from_cfg(&o.controls));
                if let Some(r) = o.rows.iter().find(|r| r.bind == Some(b))
                    && let Some(c) = r.control
                {
                    commands.queue(SetCaption(c, name));
                }
            } else if let Some(r) = o.rows.iter().find(|r| r.bind == Some(b))
                && let Some(c) = r.control
            {
                commands.queue(SetCaption(c, o.controls.all(BINDINGS[b].0).join(", ")));
            }
        }
        return;
    }
    let esc = keys.just_pressed(KeyCode::Escape) || esc_scripted;
    // a save panel over the menu takes Esc and the clicks (saves_ui)
    if saves.is_open() {
        return;
    }
    // Esc: the options window closes, then the new game panel (its advanced window first:
    // menu::advanced), then the menu; with nothing else open, the menu opens. On the title
    // screen (no game), only the options window closes
    if esc && game.is_none() {
        if let Some(o) = menus.options.take() {
            close_options(&mut commands, o);
        }
        return;
    }
    if esc && let Some(game) = game.as_mut() {
        if let Some(m) = new_game.as_mut() {
            if !m.advanced_open() {
                m.close_panel(&mut commands);
                commands.remove_resource::<super::menu::Menu>();
            }
            return;
        }
        if let Some(o) = menus.options.take() {
            close_options(&mut commands, o);
            return;
        }
        if let Some(p) = menus.pause.take() {
            for r in &p.roots {
                commands.entity(*r).despawn();
            }
            game.paused = menus.was_paused;
            return;
        }
        if !others && !other_was {
            let defs = gui.0.layout("Kenshi_MainMenuPopupPanel.layout");
            let built = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
            for r in &built.roots {
                commands.entity(*r).insert(GlobalZIndex(70));
            }
            menus.pause = Some(built);
            menus.was_paused = game.paused;
            game.paused = true;
            return;
        }
    }
    // the Esc menu's buttons
    if menus.pause.is_some()
        && menus.options.is_none()
        && let Some(game) = game.as_mut()
    {
        for c in &clicked {
            match c.name.as_str() {
                "ResumeButton" => {
                    if let Some(p) = menus.pause.take() {
                        for r in &p.roots {
                            commands.entity(*r).despawn();
                        }
                    }
                    game.paused = menus.was_paused;
                }
                "OptionsButton" => menus.options = Some(open_options(&mut commands, &gui, &assets, &game.root, size, video_modes(&monitors))),
                "SaveGameButton" => saves.open(&mut commands, &gui, &assets, super::saves_ui::Kind::Save, size),
                "LoadGameButton" => saves.open(&mut commands, &gui, &assets, super::saves_ui::Kind::Load, size),
                "NewGameButton" if new_game.is_none() => {
                    // F048550 -> F048543: the new game panel over the game (menu::Menu::in_game)
                    commands.insert_resource(super::menu::Menu::in_game(game.root.clone()));
                }
                "ExitGameButton" => {
                    // F048526: asked first
                    game.ask = Some(super::message_box::Ask {
                        title: "Exit".into(),
                        text: "Are you sure you want to exit?".into(),
                        question: super::message_box::Question::Exit,
                    });
                }
                _ => {}
            }
        }
    }
    let Some(o) = menus.options.as_mut() else { return };
    let tab_width = o.ui.get("OptionsTab").and_then(|p| o.ui.sizes.get(&p)).map_or(size[0] * 0.6, |s| s[0]) - 20.0 * kenshi_gui::res::ui_scale();
    // a combo box's arrow is the box
    for c in &mut clicked {
        if c.name == "Button"
            && let Some(r) = o.rows.iter().find(|r| r.control.and_then(|e| parts.get(e).ok()).and_then(|p| p.0.get("Button")) == Some(&c.widget))
            && let Some(e) = r.control
        {
            c.widget = e;
            c.name = "Control".into();
        }
    }
    // the list dropped down: an item picked sets the choice (eventComboAccept, F042391's
    // delegate) and the list goes; pressed anywhere else (the list losing focus) it goes too;
    // the wheel over it scrolls it
    let mut over_drop = false;
    if let Some(d) = o.drop.as_ref() {
        over_drop = hovers.get(d.root).is_ok_and(|h| h.cursor_over());
        let picked = drop_rows_q.iter().find(|(i, r)| **i == Interaction::Pressed && r.0 == DROP).map(|(_, r)| r.1 + d.scroll);
        let k = d.row;
        if let Some(pick) = picked
            && let Some(op) = o.rows[k].opt.map(|i| &OPTS[i])
        {
            let (items, _) = combo_items(o, op);
            match op.kind {
                Choice(cs) if pick < cs.len() => {
                    let new = cs[pick].0.to_string();
                    o.settings.set(op.key, &new);
                    changed(&mut commands, &o.root, op.key, &new);
                }
                Resolution if pick < o.modes.len() => {
                    let m = o.modes[pick].clone();
                    o.video.set(op.key, &m);
                    let _ = cfg::save("kenshi.cfg", &o.video);
                }
                _ => {}
            }
            if let (Some(c), Some(name)) = (o.rows[k].control, items.get(pick)) {
                combo_text(&mut commands, c, name.clone());
            }
            hide_drop(&mut commands, o);
            return;
        }
        let on_box = o.rows[k].control.is_some_and(|c| clicked.iter().any(|x| x.widget == c));
        if mouse.just_pressed(MouseButton::Left) && !over_drop && !on_box {
            hide_drop(&mut commands, o);
        }
    }
    if over_drop && let Some(d) = o.drop.as_mut() {
        let notches: f32 = wheel.read().map(|w| w.y.signum()).sum();
        let k = d.row;
        if notches != 0.0
            && let Some(op) = o.rows[k].opt.map(|i| &OPTS[i])
        {
            let (items, chosen) = combo_items(o, op);
            let d = o.drop.as_mut().unwrap();
            let h = nodes.get(d.root).ok().and_then(|n| if let Val::Px(h) = n.height { Some(h) } else { None }).unwrap_or(200.0);
            let visible = (((h / kenshi_gui::res::ui_scale() - FRAME[1] - FRAME[3]) / LINE).floor().max(1.0)) as usize;
            let max = items.len().saturating_sub(visible);
            let s = (d.scroll as i64 - notches as i64).clamp(0, max as i64) as usize;
            if s != d.scroll {
                d.scroll = s;
                let root = d.root;
                drop_rows(&mut commands, root, items, chosen, s, &gui, &assets);
            }
        }
    }
    for c in &clicked {
        if c.name == "CloseButton" {
            let o = menus.options.take().unwrap();
            close_options(&mut commands, o);
            return;
        }
        if let Some(k) = c.name.strip_prefix("OptionsTab").and_then(|k| k.parse::<u8>().ok()) {
            o.tab = k;
            fill(&mut commands, o, &gui, &assets, tab_width);
            continue;
        }
        let Some(r) = o.rows.iter().find(|r| r.control == Some(c.widget)) else { continue };
        if let Some(b) = r.bind {
            o.binding = Some(b);
            commands.queue(SetCaption(c.widget, "Press a key...".into()));
            continue;
        }
        // the bindings' Defaults: the install's controls.cfg again
        if r.defaults {
            o.controls = Cfg::parse(&std::fs::read_to_string(o.root.join("controls.cfg")).unwrap_or_default());
            let _ = cfg::save("controls.cfg", &o.controls);
            commands.insert_resource(super::bindings::Bindings::from_cfg(&o.controls));
            fill(&mut commands, o, &gui, &assets, tab_width);
            break;
        }
        // a compositor's tick
        if let Some(k) = r.comp {
            let key = o.compositors[k].clone();
            let on = o.settings.get(&key).is_none_or(|v| v.trim() != "0");
            o.settings.set(&key, if on { "0" } else { "1" });
            changed(&mut commands, &o.root, &key, if on { "0" } else { "1" });
            continue;
        }
        let Some(i) = r.opt else { continue };
        let op = &OPTS[i];
        if matches!(op.kind, Choice(_)) || (matches!(op.kind, Resolution) && !o.modes.is_empty()) {
            let k = o.rows.iter().position(|x| x.control == Some(c.widget)).unwrap_or(0);
            let again = o.drop.as_ref().is_some_and(|d| d.row == k);
            hide_drop(&mut commands, o);
            if !again && let Ok((n, t)) = placed.get(c.widget) {
                let inv = n.inverse_scale_factor;
                let min = (t.translation - n.size * 0.5) * inv;
                show_drop(&mut commands, o, k, Rect::from_corners(min, min + n.size * inv), size, &gui, &assets);
            }
            continue;
        }
        match op.kind {
            Action("Reset Tutorials") => {
                tutorials.reset(&mut commands);
                continue;
            }
            // kenshi.cfg's: saved in the rewrite's copy (read at the next start)
            VideoTick(yes) => {
                let on = o.video.get(op.key) == Some(yes);
                let other = if op.key == "Border" { "Default" } else { "No" };
                o.video.set(op.key, if on { other } else { yes });
                let _ = cfg::save("kenshi.cfg", &o.video);
                continue;
            }
            _ => {}
        }
        let new = match op.kind {
            Tick => if tick_on(&o.settings, op.key) { "0".to_string() } else { "1".to_string() },
            _ => continue,
        };
        o.settings.set(op.key, &new);
        changed(&mut commands, &o.root, op.key, &new);
        match op.kind {
            Tick => {
                commands.entity(c.widget).insert(kenshi_gui::ui::Checked(new == "1"));
            }
            _ => {}
        }
    }
    // the tick boxes as set
    for r in &o.rows {
        let Some(c) = r.control else { continue };
        let on = match (r.opt.map(|i| &OPTS[i]), r.comp) {
            (Some(op), _) if matches!(op.kind, Tick) => tick_on(&o.settings, op.key),
            (Some(Opt { kind: VideoTick(yes), key, .. }), _) => o.video.get(key) == Some(*yes),
            (None, Some(k)) => o.settings.get(&o.compositors[k]).is_none_or(|v| v.trim() != "0"),
            _ => continue,
        };
        commands.entity(c).insert(kenshi_gui::ui::Checked(on));
    }
    // sliders
    for s in scrolls.read() {
        let Some(r) = o.rows.iter().find(|r| r.control == Some(s.widget)) else { continue };
        let Some(i) = r.opt else { continue };
        let op = &OPTS[i];
        if let Slider(min, max) = op.kind {
            // F042370: the bar's position in hundredths from the minimum
            let v = (min + s.pos.round() / 100.0).clamp(min, max);
            let text = ogre_float(v);
            if o.settings.get(op.key) == Some(text.as_str()) {
                continue;
            }
            o.settings.set(op.key, &text);
            changed(&mut commands, &o.root, op.key, &text);
            if let Some(n) = r.number {
                commands.queue(SetCaption(n, value_text(op.kind, v)));
            }
        }
    }
    // the wheel scrolls the rows
    let row_h = 44.0 * kenshi_gui::res::ui_scale();
    let total = o.rows.len() as f32 * row_h;
    for w in wheel.read() {
        if over_drop {
            continue;
        }
        o.scroll = (o.scroll - w.y.signum() * row_h * 3.0).clamp(0.0, (total - row_h * 8.0).max(0.0));
    }
    if let Ok(mut n) = nodes.get_mut(o.list) {
        n.top = Val::Px(-o.scroll);
    }
    // the tooltip: the option under the pointer
    if let Some(tip) = o.ui.get("TooltipPanel") {
        let hovered = o.rows.iter().find(|r| hovers.get(r.holder).is_ok_and(|h| h.cursor_over()));
        let text = match hovered.and_then(|r| r.opt) {
            // F032947: the save location's tip names where saves go. (stand-in) The rewrite's
            // saves (its own format) always go to its folder, whichever this is: the tick is
            // kept but changes nothing
            Some(i) if OPTS[i].key == "User save location" => {
                format!("Save games to user directory instead of the kenshi install path.\nCurrent location: {}", kenshi_game::save::dir().display())
            }
            Some(i) => OPTS[i].tip.to_string(),
            None => String::new(),
        };
        commands.queue(SetCaption(tip, text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_old_and_odd_files() {
        // an old settings.cfg without the newer keys, a tick written as a float, garbage
        let st = Settings { cfg: Cfg::parse("Show names=0\nBlood=1.000\nview distance=abc\nShadow Range=3000\n"), changes: 0 };
        assert!(!st.on("Show names"));
        assert!(st.on("Blood"));
        // missing: F032944's defaults
        assert!(st.on("Edge scrolling") && !st.on("censorship") && st.on("tutorials"));
        assert_eq!(st.f32("view distance"), 6000.0);
        assert_eq!(st.f32("Shadow Range"), 3000.0);
        assert_eq!(st.f32("font size2"), 3.0);
        assert_eq!(st.f32("Decal Resolution"), 1024.0);
    }

    #[test]
    fn swapped_buttons_trade_places() {
        use bevy::ecs::system::RunSystemOnce;
        let mut w = World::new();
        w.insert_resource(Settings { cfg: Cfg::parse("swap mouse buttons=1\n"), changes: 0 });
        let mut b = ButtonInput::<MouseButton>::default();
        b.press(MouseButton::Left);
        w.insert_resource(b);
        w.run_system_once(swap_mouse).unwrap();
        let b = w.resource::<ButtonInput<MouseButton>>();
        assert!(b.just_pressed(MouseButton::Right) && b.pressed(MouseButton::Right));
        assert!(!b.pressed(MouseButton::Left) && !b.just_pressed(MouseButton::Left));
        // off: left alone
        let mut w = World::new();
        w.insert_resource(Settings { cfg: Cfg::parse("swap mouse buttons=0\n"), changes: 0 });
        let mut b = ButtonInput::<MouseButton>::default();
        b.press(MouseButton::Left);
        w.insert_resource(b);
        w.run_system_once(swap_mouse).unwrap();
        assert!(w.resource::<ButtonInput<MouseButton>>().just_pressed(MouseButton::Left));
    }

    #[test]
    fn censorship_swaps_as_kenshi_does() {
        set_censorship(false);
        assert_eq!(censor("Shit, that fucking asshole"), "Shit, that fucking asshole");
        set_censorship(true);
        assert_eq!(censor("Shit, that fucking asshole is shitting"), "Crap, that buggering idiot is crapping");
        set_censorship(false);
    }
}
