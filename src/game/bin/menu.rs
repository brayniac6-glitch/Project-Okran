//! Kenshi's main menu (Kenshi_MainMenu.layout): New Game picks one of the data's game starts
//! (NEW_GAME_STARTOFF: its squad, its towns) in Kenshi_NewGamePanel.layout and loads that world
//! behind Kenshi_LoadingScreen.layout; Exit quits. Continue loads the save last saved or loaded
//! (settings.cfg "continue", else the newest; a restarted program's KENSHI_LOAD), Load and Import
//! open their panels (saves_ui) and load the save chosen.
use super::saves_ui::{Kind, SavesDone, SavesPanel};
use super::{Loading, WorldLoad, build_world};
use bevy::prelude::*;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption, SetVisible};
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
    /// The squad's size (its template's leader and squad counts).
    pub members: usize,
    /// Its towns' zones.
    pub zones: Vec<(i32, i32)>,
    /// Its squad template (the characters' kit).
    pub squad: Option<String>,
    /// The money it starts with (cats).
    pub money: i32,
    /// Its "faction relations" (faction, value) and "research" done.
    pub relations: Vec<(String, i32)>,
    pub research: Vec<String>,
    /// Its "force race" (race record names): the only races the editor offers; empty = any.
    pub races: Vec<String>,
}

/// The game starts the data has, with their towns placed.
fn starts(root: &std::path::Path) -> Vec<Start> {
    let Ok(order) = Install::new(root).game_order() else { return Vec::new() };
    let Ok(data) = GameDataContainer::load(&order) else { return Vec::new() };
    let towns = kenshi_world::town::towns(root, &data);
    let mut out: Vec<Start> = data
        .of_type(ItemType::NewGameStartoff)
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
            let zones = s
                .refs("town")
                .iter()
                .filter_map(|(sid, _)| towns.iter().find(|t| &t.record == sid))
                .map(|t| kenshi_world::zone_of(t.position[0], t.position[2]))
                .collect();
            Start {
                squad: s.refs("squad").first().map(|(sid, _)| sid.clone()),
                money: s.ints.get("money").copied().unwrap_or(0),
                relations: s.refs("faction relations").iter().map(|(f, v)| (f.clone(), v[0])).collect(),
                research: s.refs("research").iter().map(|(r, _)| r.clone()).collect(),
                races: s.refs("force race").iter().filter_map(|(r, _)| data.get(r)).map(|r| r.name.clone()).collect(),
                name: s.name.clone(),
                description: s.strings.get("description").cloned().unwrap_or_default(),
                members,
                zones,
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
}

impl Menu {
    pub fn new(root: PathBuf) -> Menu {
        let (tx, rx) = channel();
        let r = root.clone();
        super::spawn_worker(move || {
            let _ = tx.send(starts(&r));
        });
        Menu { root, ui: Built::default(), panel: None, starts: Vec::new(), starts_rx: Mutex::new(Some(rx)), pick: 0, loading: Mutex::new(None), restoring: false }
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
    let size = window_size(&windows);
    let defs = gui.0.layout("Kenshi_MainMenu.layout");
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
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
    let (title, text) = match menu.starts.get(menu.pick) {
        Some(s) => (s.name.clone(), s.description.clone()),
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
    // the save panels' choices
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
    // the starts, once read
    let got = menu.starts_rx.lock().ok().and_then(|rx| rx.as_ref().and_then(|r| r.try_recv().ok()));
    if let Some(s) = got {
        menu.starts = s;
        if let Ok(mut rx) = menu.starts_rx.lock() {
            *rx = None;
        }
        show_start(&mut commands, &menu);
    }
    for c in clicks.read() {
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
            "NewGameButton" if menu.panel.is_none() => {
                // the new game panel beside the menu's buttons (its root placed there)
                let mut defs = gui.0.layout("Kenshi_NewGamePanel.layout");
                if let Some(d) = defs.first_mut() {
                    d.pos = kenshi_gui::res::Pos::Real([0.48, 0.25, 0.3, 0.509259]);
                }
                let panel = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
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
                    commands.entity(b).insert(kenshi_gui::ui::Disabled);
                }
                panel.roots = vec![holder];
                menu.panel = Some(panel);
                show_start(&mut commands, &menu);
            }
            "LeftButton" | "RightButton" if !menu.starts.is_empty() => {
                let n = menu.starts.len();
                menu.pick = if c.name == "LeftButton" { (menu.pick + n - 1) % n } else { (menu.pick + 1) % n };
                show_start(&mut commands, &menu);
            }
            "LoadGameButton" if !saves.is_open() => saves.open(&mut commands, &gui, &assets, Kind::Load, size),
            "ImportGameButton" if !saves.is_open() => saves.open(&mut commands, &gui, &assets, Kind::Import, size),
            "CancelButton" if menu.panel.as_ref().is_some_and(|p| p.get("CancelButton") == Some(c.widget)) => {
                if let Some(p) = menu.panel.take() {
                    for r in p.roots {
                        commands.entity(r).despawn();
                    }
                }
            }
            "ContinueButton" if menu.loading.lock().is_ok_and(|l| l.is_none()) => {
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
            "BeginButton" if !menu.starts.is_empty() && menu.loading.lock().is_ok_and(|l| l.is_none()) => {
                let start = menu.starts[menu.pick].clone();
                // one of the start's towns at random (assumption: as the game picks among them); a
                // start with none at the default zone
                let pick = if start.zones.is_empty() {
                    0
                } else {
                    (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()) as usize) % start.zones.len()
                };
                let zone = start.zones.get(pick).copied().unwrap_or((34, 46));
                eprintln!("new game: {} ({} people) in zone {}.{}", start.name, start.members, zone.0, zone.1);
                let (tx, rx) = channel();
                let root = menu.root.clone();
                super::spawn_worker(move || {
                    let mut extras = vec![format!("player:{}", start.members), format!("money:{}", start.money)];
                    extras.extend(start.squad.map(|t| format!("kit:{t}")));
                    extras.extend(start.relations.iter().map(|(f, v)| format!("relation:{f}={v}")));
                    extras.extend(start.research.iter().map(|r| format!("research:{r}")));
                    let load = build_world(root.clone(), vec![root.to_string_lossy().into_owned()], zone, extras, Loading::New);
                    let _ = tx.send(load);
                });
                if let Ok(mut l) = menu.loading.lock() {
                    *l = Some(rx);
                }
                // the loading screen over the menu
                loading_screen(&mut commands, &gui, &assets, size, Some(format!("{}\n\n{}", start.name, start.description)));
            }
            _ => {}
        }
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
