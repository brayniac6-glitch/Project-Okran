//! The Esc menu (Kenshi_MainMenuPopupPanel.layout: New Game, Save Game, Load Game, Options, Exit
//! Game, Resume; the game pauses while it is up) and the options window (Kenshi_OptionsPanel.layout,
//! F032950; its tabs General, Gameplay, Graphics, Audio and Controls as F032960 fills them, each
//! option a settings.cfg key F032944 reads and writes, the key bindings controls.cfg's).
//!
//! Settings are saved to the rewrite's own copies (kenshi_io::cfg, %APPDATA%\kenshi-rewrite); the
//! install's files are never written.
//! (stand-in) F032960 is too large to decompile here: the rows are its strings' options in its
//! order; the sliders' ranges are assumptions; its combo boxes are buttons that step through
//! their choices; the tooltips are the rewrite's own words. Most options take effect when the
//! game next starts (the damage floaters, the sound volumes and the movement marker at once);
//! the key bindings are saved but the game's keys are not yet read from them. New Game from the
//! Esc menu starts the program again at the main menu; Save and Load open their panels
//! (saves_ui).
use super::Game;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use kenshi_gui::{Built, GuiClick, GuiRes, GuiScroll, SetCaption};
use kenshi_io::cfg::{self, Cfg};

#[derive(Clone, Copy)]
enum Kind {
    Header,
    Tick,
    /// min, max, step, shown as a percentage.
    Slider(f32, f32, f32, bool),
    /// The values, in order, with their names.
    Choice(&'static [(&'static str, &'static str)]),
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

const TABS: [&str; 5] = ["GENERAL", "GAMEPLAY", "GRAPHICS", "AUDIO", "CONTROLS"];

use Kind::*;
const OPTS: &[Opt] = &[
    // General
    o(0, "tutorials", "Tutorials", Tick, "Show the in-game tutorials."),
    o(0, "User save location", "User Save Location", Tick, "Save games in your user folder rather than the game's folder."),
    o(0, "Auto save", "Auto save", Tick, "Save the game automatically."),
    o(0, "Auto save time (minutes)", "Auto save time (minutes)", Slider(1.0, 60.0, 1.0, false), "Minutes between automatic saves."),
    o(0, "Show names", "Show character names", Tick, "Show your characters' names over their heads."),
    o(0, "Movement marker", "Show movement marker", Tick, "Show the green marker where you order a move."),
    o(0, "Rotation marker", "Show rotation marker", Tick, "Show the green marker while the camera turns."),
    o(0, "Edge scrolling", "Edge scrolling", Tick, "Move the camera when the pointer reaches the edge of the screen."),
    o(0, "censorship", "Language censorship", Tick, "Filter out most of the bad language."),
    o(0, "floaters", "Show damage floaters", Choice(&[("0", "Off"), ("1", "Simple info"), ("2", "Full info")]), "The numbers that rise over a character when hit, with what armour stopped in brackets."),
    o(0, "font size2", "Font Size", Slider(0.0, 4.0, 1.0, false), "The size of the interface's text."),
    o(0, "Fast zone hopping", "Fast zone hopping", Tick, "Keep more of the world loaded so jumping between characters far apart has no loading time."),
    // Gameplay
    o(1, "", "Population settings (affects performance)", Header, ""),
    o(1, "Squad size multiplier", "Squad size multiplier", Slider(0.25, 3.0, 0.05, false), "Multiplies the number of people in each squad."),
    o(1, "Global population multiplier", "Global population multiplier (num NPC squads)", Slider(0.25, 3.0, 0.05, false), "Multiplies the number of squads in the world."),
    o(1, "", "Difficulty settings", Header, ""),
    o(1, "raidSizeMult", "Town raid events: size", Slider(0.25, 3.0, 0.05, false), "The size of the forces that attack your town."),
    o(1, "raidFrequencyMult", "Town raid events: frequency", Slider(0.25, 3.0, 0.05, false), "How often raids on your town happen (above 1 more often)."),
    o(1, "attacks", "Town attacks frequency", Choice(&[("0", "Never"), ("1", "Fewer"), ("2", "Normal"), ("3", "Bombardment")]), "How often bandits and enemies attack your town besides the raid events."),
    o(1, "Dismemberment", "Dismemberment", Choice(&[("0", "Off"), ("1", "Rare"), ("2", "Frequent")]), "How often limbs are lost."),
    // Graphics
    o(2, "view distance", "View Distance", Slider(2000.0, 20000.0, 500.0, false), "How far you can see; far costs performance."),
    o(2, "terrain detail", "Terrain Detail", Slider(0.0, 1.0, 0.05, true), "The quality of the terrain."),
    o(2, "grass range", "Grass View Range", Slider(0.0, 3.0, 0.1, false), "How far grass is drawn."),
    o(2, "grass density", "Grass Density", Slider(0.0, 2.0, 0.1, false), "How thick the grass is."),
    o(2, "foliage range", "Foliage View Range", Slider(0.0, 3.0, 0.1, false), "How far plants and rocks are drawn."),
    o(2, "npc range", "NPC View Range", Slider(1000.0, 10000.0, 250.0, false), "How far people are drawn."),
    o(2, "objects view range", "Objects View Range", Slider(1000.0, 10000.0, 250.0, false), "How far small objects are drawn."),
    o(2, "feature range", "Terrain Feature Range", Slider(0.0, 4.0, 1.0, false), "How far large rocks and cliffs are drawn."),
    o(2, "distant town range", "Distant Towns Range", Slider(0.0, 10.0, 1.0, false), "How far away towns are drawn on the horizon."),
    o(2, "generate distant towns", "Generate Distant Towns", Tick, "Make horizon models for towns that have none (player and modded towns)."),
    o(2, "texture resolution gimping", "Texture Quality (need restart)", Choice(&[("0", "Maximum"), ("1", "Medium"), ("2", "Fugly")]), "Texture resolution; more uses more video memory."),
    o(2, "terrain patch size", "Terrain Chunk Size (need restart)", Choice(&[("0", "Small"), ("1", "Large")]), "Large chunks are faster at long view distances."),
    o(2, "water reflection", "Water reflection", Choice(&[("0", "Disabled"), ("1", "Characters"), ("2", "Buildings"), ("3", "Everything")]), "What the water reflects; off is faster."),
    o(2, "shadow mode", "Shadows", Choice(&[("0", "Off"), ("1", "Normal"), ("2", "RTWSM")]), "Turn shadows off if the game runs slowly."),
    o(2, "shadow quality", "Shadow Quality", Choice(&[("0", "1024 (poor)"), ("1", "2048 (nice)"), ("2", "4096")]), "The shadows' resolution."),
    o(2, "Shadow Range", "Shadow Range", Slider(500.0, 5000.0, 100.0, false), "How far shadows reach; longer is softer."),
    o(2, "Blood", "Blood", Tick, "Blood splats on the ground."),
    o(2, "harpoonLimit", "Harpoon limit", Slider(0.0, 200.0, 5.0, false), "How many bolts and harpoons stay stuck in the ground."),
    o(2, "HeatHaze", "HeatHaze", Tick, "The shimmer of heat in the air."),
    o(2, "FXAA", "FXAA", Tick, "Smooth jagged edges."),
    // Audio
    o(3, "Music volume", "Music volume", Slider(0.0, 1.0, 0.05, true), ""),
    o(3, "Ambient volume", "Ambient volume", Slider(0.0, 1.0, 0.05, true), ""),
    o(3, "Footstep volume", "Footstep volume", Slider(0.0, 1.0, 0.05, true), ""),
    o(3, "Sfx volume", "SFX volume", Slider(0.0, 1.0, 0.05, true), ""),
    o(3, "UI volume", "UI volume", Slider(0.0, 1.0, 0.05, true), ""),
    o(3, "VO volume", "VO volume", Slider(0.0, 1.0, 0.05, true), ""),
    o(3, "Music frequency", "Music Frequency (minutes)", Slider(0.0, 30.0, 1.0, false), "How long between pieces of music."),
    // Controls
    o(4, "invert X", "Invert mouse X", Tick, ""),
    o(4, "invert Y", "Invert mouse Y", Tick, ""),
    o(4, "swap mouse buttons", "Swap mouse buttons", Tick, "Swap the left and right buttons on the interface."),
    o(4, "hardware_mouse", "Hardware Mouse", Tick, "Use the Windows pointer: it moves without lag when the game is slow."),
    o(4, "mouse speed pan", "Camera Rotate Speed X", Slider(1.0, 20.0, 0.5, false), ""),
    o(4, "mouse speed tilt", "Camera Rotate Speed Y", Slider(1.0, 20.0, 0.5, false), ""),
    o(4, "camera zoom", "Camera Zoom Speed", Slider(10.0, 300.0, 5.0, false), ""),
    o(4, "", "Key bindings", Header, ""),
];

/// controls.cfg's actions with the option window's names (F032960's key binding rows).
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
    ("toggle_crafting", "Toggle faction window"),
    ("toggle_bar", "Toggle main gui"),
    ("stop_movement", "Stop selected characters actions"),
    ("toggle_block", "Toggle block only mode"),
    ("toggle_hold", "Toggle hold mode"),
    ("toggle_passive", "Toggle passive mode"),
    ("toggle_ranged", "Toggle ranged mode"),
    ("toggle_sneak", "Toggle stealth mode"),
    ("toggle_taunt", "Toggle taunt mode"),
    ("cycle_run_speed", "Cycle movement speed"),
    ("medic", "Medic order"),
    ("rescue", "Rescue order"),
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
    ("toggle_build", "Toggle build mode"),
    ("build_apply", "Confirm build mode"),
    ("build_undo", "Undo last building"),
    ("build_rotate_left", "Rotate building left"),
    ("build_rotate_right", "Rotate building right"),
    ("build_move_up", "Move building up"),
    ("build_move_down", "Move building down"),
    ("build_tilt_increase", "Increase building slope"),
    ("build_tilt_decrease", "Decrease building slope"),
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

fn value_text(kind: Kind, v: f32) -> String {
    match kind {
        Slider(_, _, _, true) => format!("{:.0}%", v * 100.0),
        Slider(_, _, step, false) if step >= 1.0 => format!("{v:.0}"),
        Slider(..) => format!("{v:.2}"),
        _ => String::new(),
    }
}

fn choice_name(kind: Kind, v: &str) -> String {
    match kind {
        Choice(cs) => cs.iter().find(|(k, _)| *k == v).map_or(v.to_string(), |(_, n)| n.to_string()),
        _ => String::new(),
    }
}

/// The rows of `o.tab`, made again.
fn fill(commands: &mut Commands, o: &mut Options, gui: &GuiRes, assets: &AssetServer, width: f32) {
    for r in o.rows.drain(..) {
        commands.entity(r.holder).despawn();
    }
    let scale = kenshi_gui::res::ui_scale();
    let row_h = 44.0 * scale;
    let mut k = 0usize;
    let mut add = |commands: &mut Commands, defs: Vec<kenshi_gui::WidgetDef>, opt: Option<usize>, bind: Option<usize>, control: &str, number: &str| {
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
        o.rows.push(Row { holder, opt, bind, control: built.get(control), number: built.get(number) });
        k += 1;
    };
    let (lw, cx) = (width * 0.48, width * 0.5);
    for (i, op) in OPTS.iter().enumerate().filter(|(_, op)| op.tab == o.tab) {
        let label = |f: &str| text_def("Label", op.label, [10.0 * scale, 0.0, lw, row_h], f, "Left VCenter");
        let v = o.settings.get(op.key).unwrap_or("").to_string();
        let defs = match op.kind {
            Header => vec![text_def("Label", op.label, [0.0, 0.0, width, row_h], "Kenshi_PaintedTextFont_Large", "Left VCenter")],
            Tick => vec![label("Kenshi_StandardFont_Large"), button_def("Control", "Kenshi_TickBoxSkin", "", [cx, (row_h - 33.0 * scale) * 0.5, 35.0 * scale, 33.0 * scale])],
            Slider(min, max, _, _) => {
                let mut bar = button_def("Control", "Kenshi_ScrollBar", "", [cx, 4.0 * scale, width * 0.36, row_h - 8.0 * scale]);
                bar.kind = "ScrollBar".into();
                bar.props.push(("Range".into(), "1000".into()));
                let f = v.parse::<f32>().unwrap_or(min);
                vec![
                    label("Kenshi_StandardFont_Large"),
                    bar,
                    text_def("Number", &value_text(op.kind, f), [cx + width * 0.37, 0.0, width * 0.12, row_h], "Kenshi_StandardFont_Large", "Center"),
                ]
                .into_iter()
                .map(|mut d| {
                    if d.name == "Control" {
                        d.props.push(("ScrollPosition".into(), format!("{}", ((f - min) / (max - min) * 1000.0).round())));
                    }
                    d
                })
                .collect()
            }
            Choice(_) => vec![label("Kenshi_StandardFont_Large"), button_def("Control", "Kenshi_Button1", &choice_name(op.kind, &v), [cx, 4.0 * scale, width * 0.3, row_h - 8.0 * scale])],
        };
        add(commands, defs, Some(i), None, "Control", "Number");
    }
    if o.tab == 4 {
        for (b, (action, name)) in BINDINGS.iter().enumerate() {
            let keys = o.controls.all(action).join(", ");
            let defs = vec![
                text_def("Label", name, [10.0 * scale, 0.0, lw, row_h], "Kenshi_StandardFont_Large", "Left VCenter"),
                button_def("Control", "Kenshi_Button1", if keys.is_empty() { "-" } else { &keys }, [cx, 4.0 * scale, width * 0.3, row_h - 8.0 * scale]),
            ];
            add(commands, defs, None, Some(b), "Control", "");
        }
    }
    // the sliders where their values are
    for r in &o.rows {
        if let (Some(i), Some(c)) = (r.opt, r.control)
            && let Slider(min, max, _, _) = OPTS[i].kind
        {
            let f = o.settings.f32(OPTS[i].key, min);
            commands.queue(kenshi_gui::ui::SetScroll(c, ((f - min) / (max - min) * 1000.0).clamp(0.0, 1000.0)));
        }
    }
    o.scroll = 0.0;
}

fn open_options(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, game: &Game, size: [f32; 2]) -> Options {
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
fn apply_now(key: &str, value: &str, floaters: &mut Option<ResMut<super::floaters::FloaterMode>>, sound: &mut Option<ResMut<super::sound::Sound>>) {
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
    (inv, stats, build, research, mut floaters, mut sound, slaves): (
        Option<Res<super::inventory_ui::InventoryWindow>>,
        Option<Res<super::stats_ui::StatsWindow>>,
        Option<Res<super::build_ui::BuildMode>>,
        Option<Res<super::research_ui::ResearchWindow>>,
        Option<ResMut<super::floaters::FloaterMode>>,
        Option<ResMut<super::sound::Sound>>,
        Option<Res<super::slave_ui::SlaveTrade>>,
    ),
    mut exit: MessageWriter<AppExit>,
    mut frames: Local<u32>,
    mut saves: ResMut<super::saves_ui::SavesPanel>,
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
    // Esc: the options window closes, then the menu; with nothing else open, the menu opens
    if esc {
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
                "OptionsButton" => menus.options = Some(open_options(&mut commands, &gui, &assets, &game, size)),
                "SaveGameButton" => saves.open(&mut commands, &gui, &assets, super::saves_ui::Kind::Save, size),
                "LoadGameButton" => saves.open(&mut commands, &gui, &assets, super::saves_ui::Kind::Load, size),
                "NewGameButton" => {
                    // (stand-in) the program started again at its main menu
                    if let Ok(exe) = std::env::current_exe() {
                        let mut c = std::process::Command::new(exe);
                        c.arg(&game.root).current_dir(&game.root);
                        for v in super::LOAD_VARS {
                            c.env_remove(v);
                        }
                        let _ = c.spawn();
                    }
                    exit.write(AppExit::Success);
                }
                "ExitGameButton" => {
                    exit.write(AppExit::Success);
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
        let Some(i) = r.opt else { continue };
        let op = &OPTS[i];
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
        apply_now(op.key, &new, &mut floaters, &mut sound);
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
        if let (Some(i), Some(c)) = (r.opt, r.control)
            && matches!(OPTS[i].kind, Tick)
        {
            commands.entity(c).insert(kenshi_gui::ui::Checked(o.settings.get(OPTS[i].key).is_some_and(|v| v.trim() == "1")));
        }
    }
    // sliders
    for s in scrolls.read() {
        let Some(r) = o.rows.iter().find(|r| r.control == Some(s.widget)) else { continue };
        let Some(i) = r.opt else { continue };
        let op = &OPTS[i];
        if let Slider(min, max, step, _) = op.kind {
            let v = min + (max - min) * (s.pos / 1000.0).clamp(0.0, 1.0);
            let v = (v / step).round() * step;
            let text = if step >= 1.0 { format!("{v:.0}") } else { format!("{v:.3}") };
            o.settings.set(op.key, &text);
            let _ = cfg::save("settings.cfg", &o.settings);
            apply_now(op.key, &text, &mut floaters, &mut sound);
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

