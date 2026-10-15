//! Kenshi's main menu (Kenshi_MainMenu.layout): New Game picks one of the data's game starts
//! (NEW_GAME_STARTOFF: its squad, its towns) in Kenshi_NewGamePanel.layout and loads that world
//! behind Kenshi_LoadingScreen.layout; Exit quits. Continue loads the save last saved or loaded
//! (settings.cfg "continue", else the newest; a restarted program's KENSHI_LOAD), Load and Import
//! open their panels (saves_ui) and load the save chosen. The new game panel's ADVANCED opens the
//! advanced options (`advanced`), given to the new world.
//!
//! In a game the Esc menu's New Game opens the same panel over it (F048550); its BEGIN, and a
//! save loaded from the game, unload the game first (`super::unload_world`), then load the new
//! world behind the loading screen as the main menu does (`Menu::pending`).
use super::saves_ui::{Kind, SavesDone, SavesPanel};
use super::{Loading, WorldLoad, build_world};
use bevy::prelude::*;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_gui::{Built, GuiClick, GuiRes, GuiScroll, SetCaption, SetVisible};
use kenshi_io::mods::Install;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

/// Where Kenshi is: Steam's library folders (libraryfolders.vdf), Steam's default folder, or this
/// folder when it holds the game's data.
pub fn find_install() -> Option<PathBuf> {
    let ok = |p: &PathBuf| p.join("data").is_dir() && p.join("data/gui").is_dir();
    if let Ok(here) = std::env::current_dir()
        && ok(&here)
    {
        return Some(here);
    }
    let steams = ["C:/Program Files (x86)/Steam", "C:/Program Files/Steam"];
    for steam in steams {
        let steam = PathBuf::from(steam);
        let mut libraries = vec![steam.clone()];
        if let Ok(vdf) = std::fs::read_to_string(steam.join("steamapps/libraryfolders.vdf")) {
            for line in vdf.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("\"path\"") {
                    let p = rest.trim().trim_matches('"').replace("\\\\", "/");
                    libraries.push(PathBuf::from(p));
                }
            }
        }
        for lib in libraries {
            let p = lib.join("steamapps/common/Kenshi");
            if ok(&p) {
                return Some(p);
            }
        }
    }
    None
}

/// A game start as the menu offers it.
#[derive(Clone, Debug)]
pub struct Start {
    pub name: String,
    pub description: String,
    /// Its "difficulty" and "style" (the new game panel's Difficulty and Play Style lines).
    pub difficulty: String,
    pub style: String,
    /// The squad's size (its template's leader and squad counts).
    pub members: usize,
    /// Where it may begin (one at random): its zone and the point (world units, x and z):
    /// its "start pos" when it forces it, else its towns' positions (F046699).
    pub spots: Vec<((i32, i32), [f32; 2])>,
    /// Its squad templates (the characters' kits, each its own people).
    pub squads: Vec<String>,
    /// The money it starts with (cats).
    pub money: i32,
    /// Its "faction relations" (faction, value) and "research" done.
    pub relations: Vec<(String, i32)>,
    pub research: Vec<String>,
    /// Its "force race" (race record names): the only races the editor offers; empty = any.
    pub races: Vec<String>,
}

/// @fn F046699
///
/// The game starts the base game has (a start a mod adds is not offered), with where each
/// begins: its "start pos X" / "start pos Z" when "force start pos", else one of its "town"
/// refs' positions at random; a start with no town begins at a town of type 2 or 3 whose faction
/// is not the player's enemy, at random. (Not ported: the game's test F04d2f7 of each place.)
fn starts(root: &std::path::Path) -> Vec<Start> {
    let Ok(order) = Install::new(root).game_order() else { return Vec::new() };
    let Ok(data) = GameDataContainer::load(&order) else { return Vec::new() };
    // the base game's own starts (its core files'), the player's mods' left out
    let mut core_order = order.clone();
    core_order.files.retain(|f| f.core);
    let base: std::collections::HashSet<String> = GameDataContainer::load(&core_order)
        .map(|c| c.of_type(ItemType::NewGameStartoff).map(|s| s.string_id.clone()).collect())
        .unwrap_or_default();
    let towns = kenshi_world::town::towns(root, &data);
    let spot = |t: &kenshi_world::town::Town| (kenshi_world::zone_of(t.position[0], t.position[2]), [t.position[0], t.position[2]]);
    // a start without towns: towns of type 2 or 3 of a faction not the player's enemy
    let player = data.get(kenshi_game::sim::PLAYER_FACTION).map(|r| kenshi_ai::faction::FactionRelations::from_data(r, &data));
    let open: Vec<((i32, i32), [f32; 2])> = towns
        .iter()
        .filter(|t| matches!(t.kind, 2 | 3))
        .filter(|t| !player.as_ref().is_some_and(|r| r.is_enemy(t.faction.as_deref())))
        .map(spot)
        .collect();
    let mut out: Vec<Start> = data
        .of_type(ItemType::NewGameStartoff)
        .filter(|s| base.contains(&s.string_id))
        .map(|s| {
            let members = s
                .refs("squad")
                .iter()
                .filter_map(|(sid, _)| data.get(sid))
                .map(|t| {
                    let n: i32 = t.refs("leader").iter().chain(t.refs("squad")).map(|(_, v)| v[0].max(1)).sum();
                    n.max(1) as usize
                })
                .sum::<usize>()
                .max(1);
            let forced = s.bools.get("force start pos").copied().unwrap_or(false);
            let spots: Vec<((i32, i32), [f32; 2])> = if forced {
                let n = |k: &str| s.ints.get(k).map(|&v| v as f32).or_else(|| s.floats.get(k).copied()).unwrap_or(0.0);
                let (x, z) = (n("start pos X"), n("start pos Z"));
                vec![(kenshi_world::zone_of(x, z), [x, z])]
            } else {
                let listed: Vec<_> = s.refs("town").iter().filter_map(|(sid, _)| towns.iter().find(|t| &t.record == sid)).map(spot).collect();
                if listed.is_empty() { open.clone() } else { listed }
            };
            Start {
                squads: s.refs("squad").iter().map(|(sid, _)| sid.clone()).collect(),
                money: s.ints.get("money").copied().unwrap_or(0),
                relations: s.refs("faction relations").iter().map(|(f, v)| (f.clone(), v[0])).collect(),
                research: s.refs("research").iter().map(|(r, _)| r.clone()).collect(),
                races: s.refs("force race").iter().filter_map(|(r, _)| data.get(r)).map(|r| r.name.clone()).collect(),
                name: s.name.clone(),
                description: s.strings.get("description").cloned().unwrap_or_default(),
                difficulty: s.strings.get("difficulty").cloned().unwrap_or_default(),
                style: s.strings.get("style").cloned().unwrap_or_default(),
                members,
                spots,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[derive(Resource)]
pub struct Menu {
    root: PathBuf,
    ui: Built,
    panel: Option<Built>,
    starts: Vec<Start>,
    starts_rx: Mutex<Option<Receiver<Vec<Start>>>>,
    pick: usize,
    loading: Mutex<Option<Receiver<WorldLoad>>>,
    /// A save being loaded (its state is put on the world as it is made).
    restoring: bool,
    /// The new game's advanced options, and their window when open (its rows' value texts).
    advanced: kenshi_game::advanced::Advanced,
    advanced_ui: Option<AdvancedUi>,
    /// Opened over a game (the Esc menu's New Game, F048550): BEGIN unloads the game first.
    in_game: bool,
    /// The new game panel to open on the next frame.
    want_panel: bool,
    /// What to load once the game before is unloaded (`super::unload_world`).
    pending: Option<After>,
}

/// What follows unloading a game: a save loaded (ours, the squad gathered or not; or with
/// import options one in Kenshi's layout), or the new game panel's start begun.
pub enum After {
    Load(PathBuf, bool, Option<kenshi_game::kenshi_save::ImportOptions>),
    Begin,
}

/// The advanced options window: its widgets, each slider's value text and scroll bar, the
/// tooltip's text.
#[derive(Clone)]
struct AdvancedUi {
    ui: Built,
    values: Vec<Entity>,
    sliders: Vec<Entity>,
    tip: Option<Entity>,
}

impl Menu {
    /// The main menu, the game starts read in the background.
    pub fn new(root: PathBuf) -> Menu {
        let (tx, rx) = channel();
        let r = root.clone();
        super::spawn_worker(move || {
            let _ = tx.send(starts(&r));
        });
        Menu { starts_rx: Mutex::new(Some(rx)), ..Menu::bare(root) }
    }

    fn bare(root: PathBuf) -> Menu {
        Menu {
            root,
            ui: Built::default(),
            panel: None,
            starts: Vec::new(),
            starts_rx: Mutex::new(None),
            pick: 0,
            loading: Mutex::new(None),
            restoring: false,
            advanced: Default::default(),
            advanced_ui: None,
            in_game: false,
            want_panel: false,
            pending: None,
        }
    }

    /// The new game panel over a running game (F048550's NewGameButton: F048543 shows it).
    pub fn in_game(root: PathBuf) -> Menu {
        Menu { in_game: true, want_panel: true, ..Menu::new(root) }
    }

    /// Only the loading screen, for a save loaded from inside a game (no starts read).
    pub fn loading(root: PathBuf, after: After) -> Menu {
        Menu { pending: Some(after), ..Menu::bare(root) }
    }

    /// The game is unloaded (its widgets with it): `after` is to load next.
    pub fn unloaded(&mut self, after: After) {
        self.in_game = false;
        self.want_panel = false;
        self.panel = None;
        self.advanced_ui = None;
        self.ui = Built::default();
        self.pending = Some(after);
    }

    /// Close the new game panel (and its advanced window).
    pub fn close_panel(&mut self, commands: &mut Commands) {
        if let Some(a) = self.advanced_ui.take() {
            for r in a.ui.roots {
                commands.entity(r).try_despawn();
            }
        }
        if let Some(p) = self.panel.take() {
            for r in p.roots {
                commands.entity(r).try_despawn();
            }
        }
    }

    /// Whether the advanced options window is open.
    pub fn advanced_open(&self) -> bool {
        self.advanced_ui.is_some()
    }
}

/// The menu's camera (the world's replaces it).
#[derive(Component)]
pub struct MenuCamera;

/// Everything the menu spawned.
#[derive(Component)]
pub struct MenuUi;

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

pub fn open(mut commands: Commands, gui: Res<GuiRes>, assets: Res<AssetServer>, mut menu: ResMut<Menu>, windows: Query<&Window>) {
    commands.spawn((Camera2d, MenuCamera));
    main_menu(&mut commands, &gui, &assets, &mut menu, window_size(&windows));
}

/// The main menu's widgets (Kenshi_MainMenu.layout, F048551).
fn main_menu(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, menu: &mut Menu, size: [f32; 2]) {
    let defs = gui.0.layout("Kenshi_MainMenu.layout");
    let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(MenuUi);
    }
    // Continue and Load need a save of ours, Import one of ours or Kenshi's
    let ours = !kenshi_game::save::slots().is_empty() || std::env::var_os("KENSHI_LOAD").is_some();
    let any = ours || !kenshi_game::save::kenshi_slots().is_empty();
    for (name, on) in [("ContinueButton", ours), ("LoadGameButton", ours), ("ImportGameButton", any)] {
        if !on && let Some(e) = ui.get(name) {
            commands.entity(e).insert(kenshi_gui::ui::Disabled);
        }
    }
    if let Some(v) = ui.get("VersionText") {
        commands.queue(SetCaption(v, format!("Project Okran {}", env!("CARGO_PKG_VERSION"))));
    }
    menu.ui = ui;
}

fn show_start(commands: &mut Commands, menu: &Menu) {
    let Some(panel) = &menu.panel else { return };
    // F048538: Difficulty, Cash and Play Style, then the description
    let (title, text) = match menu.starts.get(menu.pick) {
        Some(s) => (
            s.name.clone(),
            format!("Difficulty: {}
Cash: c.{}
Play Style: {}

{}", s.difficulty, super::hud_ui::num(s.money as i64), s.style, s.description),
        ),
        None => ("Loading starts...".to_string(), String::new()),
    };
    if let Some(t) = panel.get("TitleTextBox") {
        commands.queue(SetCaption(t, title));
    }
    if let Some(d) = panel.get("DescriptionText") {
        commands.queue(SetCaption(d, text));
    }
}

/// The loading screen over the menu, with `text` under it.
fn loading_screen(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, size: [f32; 2], text: Option<String>) {
    let defs = gui.0.layout("Kenshi_LoadingScreen.layout");
    let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert((MenuUi, GlobalZIndex(200)));
    }
    if let Some(t) = ui.get("LoadingText") {
        commands.queue(SetCaption(t, "Loading...".to_string()));
    }
    if let (Some(t), Some(text)) = (ui.get("MessageText"), text) {
        commands.queue(SetCaption(t, text));
    }
}

/// The zone a place (units) is in.
fn zone_of(p: [f32; 3]) -> (i32, i32) {
    let k = |v: f32| ((v + kenshi_world::WORLD_SIZE / 2.0) / kenshi_world::ZONE_SIZE).floor() as i32;
    (k(p[0]), k(p[2]))
}

/// Load save `path`: one of ours (the squad gathered with `reset`), or with `import` one in
/// Kenshi's layout started afresh; the world built behind the loading screen.
fn begin_load(menu: &mut Menu, commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, size: [f32; 2], path: &std::path::Path, reset: bool, import: Option<kenshi_game::kenshi_save::ImportOptions>) -> Result<(), String> {
    if menu.loading.lock().is_ok_and(|l| l.is_some()) {
        return Ok(());
    }
    let (zone, n, money, load) = match import {
        Some(opt) => {
            let k = kenshi_game::kenshi_save::read(path)?;
            let chars: Vec<&kenshi_game::kenshi_save::KChar> = k.squads.iter().flat_map(|s| &s.characters).collect();
            let at = chars.first().map_or(k.camera, |c| c.position);
            (zone_of(at), chars.len().max(1), k.money, Loading::Kenshi(Box::new(k), opt))
        }
        None => {
            let save = kenshi_game::save::read_slot(path).map_err(|e| e.to_string())?;
            (save.zone, save.players.len().max(1), save.money, Loading::Save(Box::new(save), reset))
        }
    };
    let name = Some(kenshi_game::save::slot_name(&path)).unwrap_or_default();
    eprintln!("load {name}: {n} characters in zone {}.{}", zone.0, zone.1);
    // ours is the one Continue loads next
    if import.is_none() {
        super::set_continue(&menu.root, &name);
    }
    let (tx, rx) = channel();
    let root = menu.root.clone();
    super::spawn_worker(move || {
        let extras = vec![format!("player:{n}"), format!("money:{money}")];
        let load = build_world(root.clone(), vec![root.to_string_lossy().into_owned()], zone, extras, load);
        let _ = tx.send(load);
    });
    if let Ok(mut l) = menu.loading.lock() {
        *l = Some(rx);
    }
    menu.restoring = true;
    loading_screen(commands, gui, assets, size, None);
    Ok(())
}

/// @fn F048549
///
/// The new game panel (Kenshi_NewGamePanel.layout) at (0.5, 0.05) of the screen on the main menu
/// (F048551), at (0.6, 0.05) over a game (F048550); its captions and the start shown.
fn open_panel(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, menu: &mut Menu, size: [f32; 2]) {
    let mut defs = gui.0.layout("Kenshi_NewGamePanel.layout");
    if let Some(d) = defs.first_mut()
        && let kenshi_gui::res::Pos::Real(r) = d.pos
    {
        let x = if menu.in_game { 0.6 } else { 0.5 };
        d.pos = kenshi_gui::res::Pos::Real([x, 0.05, r[2], r[3]]);
    }
    let panel = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    let holder = panel.roots[0];
    commands.entity(holder).insert((GlobalZIndex(90), MenuUi));
    // the start's description inside the description panel
    if let Some(d) = panel.get("DescriptionPanel") {
        let t = commands
            .spawn((
                Text::new(""),
                TextFont {
                    font: gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or_else(Default::default, |f| {
                        bevy::text::FontSource::Handle(assets.load(f.file.clone()))
                    }),
                    font_size: bevy::text::FontSize::Px(gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or(15.0, |f| f.screen_px())),
                    ..default()
                },
                TextColor(Color::srgb(0.69, 0.65, 0.55)),
                Node { width: Val::Percent(100.0), ..default() },
            ))
            .id();
        commands.entity(d).insert(kenshi_gui::Caption(t)).add_child(t);
    }
    let mut panel = panel;
    if let Some(d) = panel.get("DescriptionPanel") {
        panel.named.push(("DescriptionText".to_string(), d));
    }
    if let Some(l) = panel.get("lbChooseBeginning") {
        commands.queue(SetCaption(l, "Choose your beginning".to_string()));
    }
    if let Some(b) = panel.get("BeginButton") {
        commands.queue(SetCaption(b, "BEGIN".to_string()));
    }
    if let Some(b) = panel.get("CancelButton") {
        commands.queue(SetCaption(b, "CANCEL".to_string()));
    }
    if let Some(b) = panel.get("AdvancedButton") {
        commands.queue(SetCaption(b, "ADVANCED OPTIONS".to_string()));
    }
    panel.roots = vec![holder];
    menu.panel = Some(panel);
    // (testing) KENSHI_START=<name>: that start picked
    if let Ok(want) = std::env::var("KENSHI_START")
        && let Some(k) = menu.starts.iter().position(|s| s.name.eq_ignore_ascii_case(&want))
    {
        menu.pick = k;
    }
    show_start(commands, menu);
}

/// The start picked begun: its world built behind the loading screen.
fn begin_new(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, menu: &mut Menu, size: [f32; 2]) {
    let Some(start) = menu.starts.get(menu.pick).cloned() else { return };
    // one of the start's places at random (F046699: randInt over them)
    let pick = if start.spots.is_empty() {
        0
    } else {
        (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()) as usize) % start.spots.len()
    };
    let (zone, at) = start.spots.get(pick).copied().unwrap_or(((34, 46), [f32::NAN; 2]));
    eprintln!("new game: {} ({} people) in zone {}.{} at {at:?}", start.name, start.members, zone.0, zone.1);
    let (tx, rx) = channel();
    let root = menu.root.clone();
    let advanced = menu.advanced.to_extra();
    let text = format!("{}\n\n{}", start.name, start.description);
    super::spawn_worker(move || {
        let mut extras = vec![format!("player:{}", start.members), format!("money:{}", start.money), advanced];
        if at[0].is_finite() {
            extras.insert(0, format!("spawn:{},{}", at[0], at[1]));
        }
        extras.extend(start.squads.iter().map(|t| format!("kit:{t}")));
        extras.extend(start.relations.iter().map(|(f, v)| format!("relation:{f}={v}")));
        extras.extend(start.research.iter().map(|r| format!("research:{r}")));
        let load = build_world(root.clone(), vec![root.to_string_lossy().into_owned()], zone, extras, Loading::New);
        let _ = tx.send(load);
    });
    if let Ok(mut l) = menu.loading.lock() {
        *l = Some(rx);
    }
    menu.restoring = false;
    // the loading screen over the menu
    loading_screen(commands, gui, assets, size, Some(text));
}

#[allow(clippy::too_many_arguments)]
pub fn buttons(
    mut commands: Commands,
    mut clicks: MessageReader<GuiClick>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut menu: ResMut<Menu>,
    windows: Query<&Window>,
    mut exit: MessageWriter<AppExit>,
    mut saves: ResMut<SavesPanel>,
    mut done: MessageReader<SavesDone>,
) {
    let size = window_size(&windows);
    let idle = menu.loading.lock().is_ok_and(|l| l.is_none());
    // what a game unloaded for (super::unload_world): the menu's camera under the loading screen
    if let Some(after) = menu.pending.take() {
        commands.spawn((Camera2d, MenuCamera));
        match after {
            After::Load(path, reset, import) => {
                if let Err(e) = begin_load(&mut menu, &mut commands, &gui, &assets, size, &path, reset, import) {
                    eprintln!("can't load {}: {e}", path.display());
                    // nothing to go back to: the main menu
                    *menu = Menu::new(menu.root.clone());
                    main_menu(&mut commands, &gui, &assets, &mut menu, size);
                    return;
                }
            }
            After::Begin => begin_new(&mut commands, &gui, &assets, &mut menu, size),
        }
        return;
    }
    // the save panels' choices (over a game, the game's own `saves_done` takes them)
    if !menu.in_game {
        for d in done.read() {
            let (path, reset, import) = match d {
                SavesDone::Load(p, reset) => (p.clone(), *reset, None),
                SavesDone::Import(p, opt) => (p.clone(), false, Some(*opt)),
                SavesDone::Save(_) => continue,
            };
            if import.is_none() {
                saves.current = Some(kenshi_game::save::slot_name(&path));
            }
            if let Err(e) = begin_load(&mut menu, &mut commands, &gui, &assets, size, &path, reset, import) {
                eprintln!("can't load {}: {e}", path.display());
            }
        }
    }
    // the starts, once read
    let got = menu.starts_rx.lock().ok().and_then(|rx| rx.as_ref().and_then(|r| r.try_recv().ok()));
    if let Some(s) = got {
        menu.starts = s;
        if let Ok(mut rx) = menu.starts_rx.lock() {
            *rx = None;
        }
        show_start(&mut commands, &menu);
    }
    if menu.want_panel {
        menu.want_panel = false;
        if menu.panel.is_none() {
            open_panel(&mut commands, &gui, &assets, &mut menu, size);
        }
    }
    for c in clicks.read() {
        // over a game only the new game panel's buttons are the menu's
        let ours = menu.panel.as_ref().is_some_and(|p| p.named.iter().any(|(_, e)| *e == c.widget));
        if menu.in_game && !ours {
            continue;
        }
        match c.name.as_str() {
            "ExitButton" => {
                exit.write(AppExit::Success);
            }
            "CreditsButton" => {
                if let Some(p) = menu.ui.get("CreditsPanel") {
                    commands.queue(SetVisible(p, true));
                }
                if let Some(t) = menu.ui.get("CreditsPanelContent") {
                    commands.queue(SetCaption(
                        t,
                        "Kenshi by Lo-Fi Games.\n\nThis is a rewrite that runs on your own copy of the game's data.".to_string(),
                    ));
                }
            }
            "NewGameButton" if menu.panel.is_none() => open_panel(&mut commands, &gui, &assets, &mut menu, size),
            "LeftButton" | "RightButton" if !menu.starts.is_empty() => {
                let n = menu.starts.len();
                menu.pick = if c.name == "LeftButton" { (menu.pick + n - 1) % n } else { (menu.pick + 1) % n };
                show_start(&mut commands, &menu);
            }
            "LoadGameButton" if !saves.is_open() => saves.open(&mut commands, &gui, &assets, Kind::Load, size),
            "ImportGameButton" if !saves.is_open() => saves.open(&mut commands, &gui, &assets, Kind::Import, size),
            "CancelButton" if ours => {
                menu.close_panel(&mut commands);
                // over a game: back to the game's Esc menu
                if menu.in_game {
                    commands.remove_resource::<Menu>();
                }
            }
            "ContinueButton" if idle => {
                // a restarted program's save (KENSHI_LOAD), else the one Continue loads
                let (path, reset, import) = match std::env::var_os("KENSHI_LOAD") {
                    Some(p) => (
                        PathBuf::from(p),
                        std::env::var("KENSHI_LOAD_RESET").is_ok_and(|v| v == "1"),
                        std::env::var("KENSHI_IMPORT").ok().map(|f| super::import_of_flags(&f)),
                    ),
                    None => {
                        let last = kenshi_io::cfg::load(&menu.root, "settings.cfg").get("continue").map(str::to_string);
                        match kenshi_game::save::continue_slot(last.as_deref()) {
                            Some(s) => (s.path, false, None),
                            None => continue,
                        }
                    }
                };
                if import.is_none() {
                    saves.current = Some(kenshi_game::save::slot_name(&path));
                }
                if let Err(e) = begin_load(&mut menu, &mut commands, &gui, &assets, size, &path, reset, import) {
                    eprintln!("can't load {}: {e}", path.display());
                }
            }
            "BeginButton" if !menu.starts.is_empty() && idle => {
                if menu.in_game {
                    // the game unloaded first, then the start begun (`pending`)
                    commands.insert_resource(super::Unload(After::Begin));
                } else {
                    begin_new(&mut commands, &gui, &assets, &mut menu, size);
                }
            }
            _ => {}
        }
    }
}

/// An advanced options row (its index: the sliders, then the ticks).
#[derive(Component)]
pub struct AdvancedRow(usize);

fn row_def(kind: &str, skin: &str, name: &str, caption: &str, pos: [f32; 4], props: &[(&str, &str)]) -> kenshi_gui::WidgetDef {
    let mut p: Vec<(String, String)> = props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    if !caption.is_empty() {
        p.push(("Caption".into(), caption.into()));
    }
    kenshi_gui::WidgetDef { kind: kind.into(), skin: skin.into(), pos: kenshi_gui::res::Pos::Px(pos), align: Default::default(), name: name.into(), layer: None, props: p, user: Vec::new(), children: Vec::new() }
}

/// @fn F048535
///
/// The advanced options window (Kenshi_AdvancedOptionsWindow.layout, "Advanced options"), opened
/// and closed by the new game panel's ADVANCED, under the panel at its left edge (the panel's
/// constructor F048549 places it): a row per option in its Datapanel, a slider
/// with its value for each number, a tick box for each choice (`kenshi_game::advanced`), the
/// hovered one's tooltip in its TooltipPanel; Esc closes it.
/// (assumption) The rows are laid out as the options window's (`options_ui`), the game making
/// both with its options panel.
#[allow(clippy::too_many_arguments)]
pub fn advanced(
    mut commands: Commands,
    mut clicks: MessageReader<GuiClick>,
    mut scrolls: MessageReader<GuiScroll>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut menu: ResMut<Menu>,
    windows: Query<&Window>,
    keys: Res<ButtonInput<KeyCode>>,
    rows: Query<(&Interaction, &AdvancedRow)>,
) {
    use kenshi_game::advanced::{SLIDERS, TICKS};
    let size = window_size(&windows);
    let mut toggle = false;
    let mut ticked = None;
    for c in clicks.read() {
        if c.name == "AdvancedButton" {
            toggle = true;
        }
        if let Some(k) = c.name.strip_prefix("AdvancedTick").and_then(|k| k.parse::<usize>().ok()) {
            ticked = Some((k, c.widget));
        }
    }
    let close = menu.advanced_ui.is_some() && (toggle || keys.just_pressed(KeyCode::Escape) || menu.panel.is_none());
    if close {
        let a = menu.advanced_ui.take().unwrap();
        for r in a.ui.roots {
            commands.entity(r).try_despawn();
        }
        return;
    }
    if toggle && menu.advanced_ui.is_none() {
        let mut defs = gui.0.layout("Kenshi_AdvancedOptionsWindow.layout");
        // F048549 / F048551: under the new game panel, at its left
        let panel_h = gui.0.layout("Kenshi_NewGamePanel.layout").first().and_then(|d| match d.pos {
            kenshi_gui::res::Pos::Real(r) => Some(r[3]),
            _ => None,
        });
        if let Some(d) = defs.first_mut()
            && let kenshi_gui::res::Pos::Real(r) = d.pos
        {
            let x = if menu.in_game { 0.6 } else { 0.5 };
            d.pos = kenshi_gui::res::Pos::Real([x, 0.05 + panel_h.unwrap_or(0.509259), r[2], r[3]]);
            d.props.retain(|(k, _)| k != "Caption");
            d.props.push(("Caption".into(), "Advanced options".into()));
        }
        let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
        for r in &ui.roots {
            commands.entity(*r).insert(GlobalZIndex(120));
        }
        let mut values = Vec::new();
        let mut sliders = Vec::new();
        let mut tip = None;
        if let Some(panel) = ui.get("Datapanel") {
            let width = ui.sizes.get(&panel).map_or(500.0, |s| s[0]);
            let scale = kenshi_gui::res::ui_scale();
            let row_h = 38.0 * scale;
            let text = |name: &str, caption: &str, pos: [f32; 4], align: &str| {
                row_def("TextBox", "Kenshi_TextboxStandardText", name, caption, pos, &[("TextAlign", align), ("FontName", "Kenshi_StandardFont_Large"), ("TextColour", "0.85 0.8 0.7")])
            };
            let a = menu.advanced.clone();
            let mut a2 = a.clone();
            for k in 0..SLIDERS.len() + TICKS.len() {
                let holder = commands
                    .spawn((
                        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(k as f32 * row_h), width: Val::Px(width), height: Val::Px(row_h), ..default() },
                        Interaction::default(),
                        AdvancedRow(k),
                        ZIndex(3),
                    ))
                    .id();
                commands.entity(panel).add_child(holder);
                let cx = width * 0.52;
                let defs = if k < SLIDERS.len() {
                    let (name, _, min, max) = SLIDERS[k];
                    let v = a2.slider(k).copied().unwrap_or(1.0);
                    let pos = ((v - min) / (max - min) * 1000.0).round().to_string();
                    vec![
                        text("Label", name, [8.0 * scale, 0.0, width * 0.5, row_h], "Left VCenter"),
                        row_def("ScrollBar", "Kenshi_ScrollBar", &format!("AdvancedSlider{k}"), "", [cx, 4.0 * scale, width * 0.32, row_h - 8.0 * scale], &[("Range", "1000"), ("ScrollPosition", &pos)]),
                        text("Number", &format!("{v:.2}"), [cx + width * 0.34, 0.0, width * 0.13, row_h], "Center"),
                    ]
                } else {
                    let t = k - SLIDERS.len();
                    vec![
                        text("Label", TICKS[t].0, [8.0 * scale, 0.0, width * 0.5, row_h], "Left VCenter"),
                        row_def("Button", "Kenshi_TickBoxSkin", &format!("AdvancedTick{t}"), "", [cx, (row_h - 33.0 * scale) * 0.5, 35.0 * scale, 33.0 * scale], &[]),
                    ]
                };
                let built = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, Some(holder), [width, row_h]);
                if let Some(s) = built.get(&format!("AdvancedSlider{k}"))
                    && let Some(v) = a2.slider(k)
                {
                    let (_, _, min, max) = SLIDERS[k];
                    commands.queue(kenshi_gui::ui::SetScroll(s, ((*v - min) / (max - min) * 1000.0).clamp(0.0, 1000.0)));
                }
                if k >= SLIDERS.len()
                    && let Some(b) = built.get(&format!("AdvancedTick{}", k - SLIDERS.len()))
                {
                    commands.entity(b).insert(kenshi_gui::ui::Checked(a2.tick(k - SLIDERS.len()).copied().unwrap_or(false)));
                }
                values.push(built.get("Number").unwrap_or(Entity::PLACEHOLDER));
                if k < SLIDERS.len() {
                    sliders.push(built.get(&format!("AdvancedSlider{k}")).unwrap_or(Entity::PLACEHOLDER));
                }
            }
            let _ = a;
        }
        if let Some(t) = ui.get("TooltipPanel") {
            let font = gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
            let e = commands
                .spawn((
                    Node { position_type: PositionType::Absolute, left: Val::Px(10.0), right: Val::Px(10.0), top: Val::Px(8.0), ..default() },
                    Text::new(String::new()),
                    TextFont { font: bevy::text::FontSource::Handle(font.0), font_size: bevy::text::FontSize::Px(font.1), ..default() },
                    TextColor(Color::srgb(0.078, 0.031, 0.024)),
                    ZIndex(3),
                ))
                .id();
            commands.entity(e).insert(kenshi_gui::Caption(e));
            commands.entity(t).add_child(e);
            tip = Some(e);
        }
        menu.advanced_ui = Some(AdvancedUi { ui, values, sliders, tip });
        return;
    }
    let Some(AdvancedUi { values, sliders, tip, .. }) = menu.advanced_ui.clone() else { return };
    // a slider moved: its value (kept to the slider's range) and its number
    for s in scrolls.read() {
        let Some(k) = sliders.iter().position(|e| *e == s.widget) else { continue };
        let (_, _, min, max) = SLIDERS[k];
        let v = min + (s.pos / 1000.0).clamp(0.0, 1.0) * (max - min);
        // to the hundredth, as shown
        let v = (v * 100.0).round() / 100.0;
        if let Some(f) = menu.advanced.slider(k) {
            *f = v;
        }
        if let Some(&n) = values.get(k) {
            commands.queue(SetCaption(n, format!("{v:.2}")));
        }
    }
    if let Some((t, b)) = ticked
        && let Some(v) = menu.advanced.tick(t)
    {
        *v = !*v;
        let now = *v;
        commands.entity(b).insert(kenshi_gui::ui::Checked(now));
    }
    if let Some(t) = tip {
        let hovered = rows.iter().find(|(i, _)| **i != Interaction::None).map(|(_, r)| r.0);
        let text = hovered.map_or("", |k| if k < SLIDERS.len() { SLIDERS[k].1 } else { TICKS[k - SLIDERS.len()].1 });
        commands.queue(SetCaption(t, text.to_string()));
    }
}

/// The world, once loaded, replaces the menu.
pub fn arrive(mut commands: Commands, menu: ResMut<Menu>, ui: Query<Entity, With<MenuUi>>, cams: Query<Entity, With<MenuCamera>>) {
    let got = menu.loading.lock().ok().and_then(|rx| rx.as_ref().and_then(|r| r.try_recv().ok()));
    let Some(load) = got else { return };
    for e in ui.iter().chain(cams.iter()) {
        commands.entity(e).despawn();
    }
    let restoring = menu.restoring;
    let races = menu.starts.get(menu.pick).map(|s| s.races.clone()).unwrap_or_default();
    commands.queue(move |w: &mut World| {
        load.install(w);
        w.remove_resource::<Menu>();
        // a new game's characters made first (a loaded game's are on the new world already)
        if !restoring {
            w.insert_resource(super::editor::Pending(races));
        }
    });
}

/// (testing) KENSHI_MENU_SCRIPT="NewGameButton@60,shot:out.png@90,...": press named buttons and
/// take screenshots at those frames.
pub fn script(mut frames: Local<u32>, mut commands: Commands, widgets: Query<(Entity, &kenshi_gui::GuiWidget)>, mut out: MessageWriter<GuiClick>) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    *frames += 1;
    let Ok(s) = std::env::var("KENSHI_MENU_SCRIPT") else { return };
    for step in s.split(',') {
        let Some((what, at)) = step.rsplit_once('@') else { continue };
        if at.parse::<u32>().ok() != Some(*frames) {
            continue;
        }
        if let Some(path) = what.strip_prefix("shot:") {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(PathBuf::from(path)));
        } else if let Some((e, _)) = widgets.iter().find(|(_, w)| w.name == what) {
            out.write(GuiClick { widget: e, name: what.to_string() });
        }
    }
}

/// (testing) KENSHI_UI_SCRIPT, as KENSHI_MENU_SCRIPT in the game: "list@300" prints the named
/// buttons shown, "InventoryButton@320" presses one (the last of that name), "SaveRow0@330"
/// picks a save panel's row, "shot:out.png@340" takes a screenshot, "quit@400" ends the
/// program; frames from the world being ready (counting on in a game loaded after).
pub fn ui_script(
    mut frames: Local<u32>,
    mut commands: Commands,
    widgets: Query<(Entity, &kenshi_gui::GuiWidget, &InheritedVisibility)>,
    mut out: MessageWriter<GuiClick>,
    mut exit: MessageWriter<AppExit>,
    mut rows: Query<(&mut Interaction, &super::saves_ui::SaveRow)>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Ok(s) = std::env::var("KENSHI_UI_SCRIPT") else { return };
    *frames += 1;
    for step in s.split(',') {
        let Some((what, at)) = step.rsplit_once('@') else { continue };
        if at.parse::<u32>().ok() != Some(*frames) {
            continue;
        }
        if let Some(path) = what.strip_prefix("shot:") {
            eprintln!("ui: shot {path}");
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(PathBuf::from(path)));
        } else if what == "quit" {
            eprintln!("ui: quit");
            exit.write(AppExit::Success);
        } else if let Some(k) = what.strip_prefix("SaveRow").and_then(|k| k.parse::<usize>().ok()) {
            if let Some((mut i, _)) = rows.iter_mut().find(|(_, r)| r.0 == k) {
                eprintln!("ui: pick save row {k}");
                *i = Interaction::Pressed;
            }
        } else if what == "list" {
            let mut names: Vec<String> =
                widgets.iter().filter(|(_, w, v)| v.get() && w.kind == "Button" && !w.name.is_empty()).map(|(_, w, _)| w.name.clone()).collect();
            names.sort();
            names.dedup();
            eprintln!("ui: buttons shown: {}", names.join(" "));
        } else if let Some((e, _, _)) = widgets.iter().filter(|(_, w, v)| w.name == what && v.get()).last() {
            eprintln!("ui: press {what}");
            out.write(GuiClick { widget: e, name: what.to_string() });
        } else {
            eprintln!("ui: no {what} shown");
        }
    }
}
