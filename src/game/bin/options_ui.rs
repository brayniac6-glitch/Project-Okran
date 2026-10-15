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
//! options (the rewrite's copy read over the install's). (stand-in) Its combo boxes are buttons
//! that step through their choices; (assumption) Reset Tutorials forgets the tutorials seen.
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
}

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
                vec![label("Kenshi_StandardFont_Large"), button_def("Control", "Kenshi_Button1", &shown, [cx, 4.0 * scale, width * 0.4, row_h - 8.0 * scale])]
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
            Choice(_) => vec![label("Kenshi_StandardFont_Large"), button_def("Control", "Kenshi_Button1", &choice_name(op.kind, &v), [cx, 4.0 * scale, width * 0.3, row_h - 8.0 * scale])],
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
            let f = o.settings.f32(OPTS[i].key, min);
            commands.queue(kenshi_gui::ui::SetScroll(c, ((f - min) * 100.0).clamp(0.0, (max - min) * 100.0)));
        }
    }
    o.scroll = 0.0;
}

fn open_options(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, game: &Game, size: [f32; 2], modes: Vec<String>) -> Options {
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
        ui,
        tabs,
        tab: 0,
        settings: cfg::load(&game.root, "settings.cfg"),
        controls: cfg::load(&game.root, "controls.cfg"),
        video: cfg::load(&game.root, "kenshi.cfg"),
        modes,
        compositors: compositors(&game.root),
        mods: active_mods(&game.root),
        rows: Vec::new(),
        list: inner,
        scroll: 0.0,
        binding: None,
    };
    fill(commands, &mut o, gui, assets, tab_size[0] - 20.0 * scale);
    o
}

fn close_options(commands: &mut Commands, o: Options) {
    for r in o.rows {
        commands.entity(r.holder).despawn();
    }
    for r in o.tabs.roots.iter().chain(&o.ui.roots) {
        commands.entity(*r).despawn();
    }
}

/// The options that act at once: the damage floaters, the sound volumes, the movement marker.
fn apply_now(key: &str, value: &str, floaters: &mut Option<ResMut<super::floaters::FloaterMode>>, sound: &mut Option<ResMut<super::sound::Sound>>, markers: &mut Option<ResMut<super::move_marker::MarkerOptions>>) {
    if let Some(m) = markers {
        match key {
            "Movement marker" => m.movement = value.trim() != "0",
            "Rotation marker" => m.rotation = value.trim() != "0",
            _ => {}
        }
    }
    if key == "floaters"
        && let Some(f) = floaters
    {
        f.0 = value.parse().unwrap_or(1);
    }
    if key.ends_with(" volume")
        && let Some(s) = sound
    {
        s.set_volume(key, value.parse().unwrap_or(0.5));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut menus: ResMut<Menus>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clicks: MessageReader<GuiClick>,
    mut scrolls: MessageReader<GuiScroll>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window>,
    hovers: Query<&RelativeCursorPosition>,
    mut nodes: Query<&mut Node>,
    (inv, stats, build, research, mut floaters, mut sound, slaves, mut markers): (
        Option<Res<super::inventory_ui::InventoryWindow>>,
        Option<Res<super::stats_ui::StatsWindow>>,
        Option<Res<super::build_ui::BuildMode>>,
        Option<Res<super::research_ui::ResearchWindow>>,
        Option<ResMut<super::floaters::FloaterMode>>,
        Option<ResMut<super::sound::Sound>>,
        Option<Res<super::slave_ui::SlaveTrade>>,
        Option<ResMut<super::move_marker::MarkerOptions>>,
    ),
    (mut frames, monitors, mut tutorials): (Local<u32>, Query<&bevy::window::Monitor>, ResMut<super::tutorial_ui::Tutorials>),
    mut saves: ResMut<super::saves_ui::SavesPanel>,
    mut new_game: Option<ResMut<super::menu::Menu>>,
) {
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let mut clicked: Vec<GuiClick> = clicks.read().cloned().collect();
    // (testing) KENSHI_OPTIONS_SCRIPT="Escape@300,OptionsButton@330,OptionsTab2@360": Esc or a
    // named button of these windows pressed at those frames
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
            let found = menus
                .pause
                .iter()
                .chain(menus.options.iter().map(|o| &o.ui))
                .chain(menus.options.iter().map(|o| &o.tabs))
                .find_map(|b| b.get(what));
            if let Some(e) = found {
                clicked.push(GuiClick { widget: e, name: what.to_string() });
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
    // menu::advanced), then the menu; with nothing else open, the menu opens
    if esc {
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
    if menus.pause.is_some() && menus.options.is_none() {
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
                "OptionsButton" => menus.options = Some(open_options(&mut commands, &gui, &assets, &game, size, video_modes(&monitors))),
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
            o.controls = Cfg::parse(&std::fs::read_to_string(game.root.join("controls.cfg")).unwrap_or_default());
            let _ = cfg::save("controls.cfg", &o.controls);
            commands.insert_resource(super::bindings::Bindings::from_cfg(&o.controls));
            fill(&mut commands, o, &gui, &assets, tab_width);
            break;
        }
        // a compositor's tick
        if let Some(k) = r.comp {
            let key = o.compositors[k].clone();
            let on = o.settings.get(&key).is_some_and(|v| v.trim() == "1");
            o.settings.set(&key, if on { "0" } else { "1" });
            let _ = cfg::save("settings.cfg", &o.settings);
            continue;
        }
        let Some(i) = r.opt else { continue };
        let op = &OPTS[i];
        match op.kind {
            Action("Reset Tutorials") => {
                tutorials.reset();
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
            Resolution if !o.modes.is_empty() => {
                let cur = o.video.get(op.key).unwrap_or("");
                let at = o.modes.iter().position(|m| cur.starts_with(m.as_str())).map_or(0, |k| (k + 1) % o.modes.len());
                let m = o.modes[at].clone();
                o.video.set(op.key, &m);
                let _ = cfg::save("kenshi.cfg", &o.video);
                commands.queue(SetCaption(c.widget, m));
                continue;
            }
            _ => {}
        }
        let cur = o.settings.get(op.key).unwrap_or("0").to_string();
        let new = match op.kind {
            Tick => if cur.trim() == "1" { "0".to_string() } else { "1".to_string() },
            Choice(cs) => {
                let at = cs.iter().position(|(k, _)| *k == cur).unwrap_or(0);
                cs[(at + 1) % cs.len()].0.to_string()
            }
            _ => continue,
        };
        o.settings.set(op.key, &new);
        let _ = cfg::save("settings.cfg", &o.settings);
        apply_now(op.key, &new, &mut floaters, &mut sound, &mut markers);
        match op.kind {
            Tick => {
                commands.entity(c.widget).insert(kenshi_gui::ui::Checked(new == "1"));
            }
            Choice(_) => commands.queue(SetCaption(c.widget, choice_name(op.kind, &new))),
            _ => {}
        }
    }
    // the tick boxes as set
    for r in &o.rows {
        let Some(c) = r.control else { continue };
        let on = match (r.opt.map(|i| &OPTS[i]), r.comp) {
            (Some(op), _) if matches!(op.kind, Tick) => o.settings.get(op.key).is_some_and(|v| v.trim() == "1"),
            (Some(Opt { kind: VideoTick(yes), key, .. }), _) => o.video.get(key) == Some(*yes),
            (None, Some(k)) => o.settings.get(&o.compositors[k]).is_some_and(|v| v.trim() == "1"),
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
            o.settings.set(op.key, &text);
            let _ = cfg::save("settings.cfg", &o.settings);
            apply_now(op.key, &text, &mut floaters, &mut sound, &mut markers);
            if let Some(n) = r.number {
                commands.queue(SetCaption(n, value_text(op.kind, v)));
            }
        }
    }
    // the wheel scrolls the rows
    let row_h = 44.0 * kenshi_gui::res::ui_scale();
    let total = o.rows.len() as f32 * row_h;
    for w in wheel.read() {
        o.scroll = (o.scroll - w.y.signum() * row_h * 3.0).clamp(0.0, (total - row_h * 8.0).max(0.0));
    }
    if let Ok(mut n) = nodes.get_mut(o.list) {
        n.top = Val::Px(-o.scroll);
    }
    // the tooltip: the option under the pointer
    if let Some(tip) = o.ui.get("TooltipPanel") {
        let hovered = o.rows.iter().find(|r| hovers.get(r.holder).is_ok_and(|h| h.cursor_over()));
        let text = hovered.and_then(|r| r.opt).map_or("", |i| OPTS[i].tip);
        commands.queue(SetCaption(tip, text.to_string()));
    }
}

