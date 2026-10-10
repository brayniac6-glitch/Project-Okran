//! Kenshi's tutorials (TutorialManager: F049922 the registry, F049781 the panel, F049790 the
//! tutorials window).
//!
//! A tutorial is a title and its pages; the panel (Kenshi_TutorialPanel.layout's
//! TutorialWindow) shows one page at a time, "1 / 5" under it, PREVIOUS, CONTINUE (FINISH on the
//! last) and DISMISS. A tutorial is shown once (the seen ones kept, Kenshi in
//! ./__tutorials.data, the rewrite in %APPDATA%\kenshi-rewrite\tutorials.txt) and only with
//! the options' "Tutorials" on. The main panel's HLP opens the tutorials window
//! (Kenshi_TutorialsWindow.layout, "TUTORIALS"): every tutorial, ACTIVATE showing the one
//! chosen again.
//!
//! The texts are read from the player's own kenshi_x64.exe (`tutorial_table`); a game whose
//! texts are not where this build's are has no tutorials.
//!
//! (stand-in) What starts each tutorial is not traced; the rewrite starts these when it sees:
//! a new game (Basic Controls, then GUI overview, then What do I do?), a town found, a character
//! hurt, fighting, knocked out, hungry, imprisoned, enslaved, in disguise, a recruit, build
//! mode, a production building built, the prospecting window. The pages' pointers at the GUI
//! (TooltipsPanel, Kenshi_TutorialTooltipPanel) are not shown.
use super::Game;
use super::build_ui::{ListRow, list_rows};
use super::tutorial_table::TUTORIALS;
use bevy::prelude::*;
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption, SetVisible};
use std::collections::{HashSet, VecDeque};

/// The tutorials window's list rows.
const LIST: u8 = 40;

pub struct Tutorial {
    pub id: &'static str,
    pub title: String,
    pub pages: Vec<String>,
}

#[derive(Resource, Default)]
pub struct Tutorials {
    list: Vec<Tutorial>,
    seen: HashSet<String>,
    queue: VecDeque<usize>,
    /// The tutorial shown, its page, its panel.
    showing: Option<(usize, usize, Built)>,
    /// The tutorials window: its widgets, the chosen tutorial, the list's rows, the page shown.
    window: Option<(Built, Option<usize>, Option<Entity>, usize)>,
    /// What the triggers saw last (the player's count, towns found).
    seen_state: (usize, usize),
    loaded: bool,
}

/// A NUL-ended string at virtual address `va` of a PE image.
fn c_string(exe: &[u8], va: u64) -> Option<String> {
    let pe = u32::from_le_bytes(exe.get(0x3c..0x40)?.try_into().ok()?) as usize;
    let n = u16::from_le_bytes(exe.get(pe + 6..pe + 8)?.try_into().ok()?) as usize;
    let opt = u16::from_le_bytes(exe.get(pe + 20..pe + 22)?.try_into().ok()?) as usize;
    let base = u64::from_le_bytes(exe.get(pe + 48..pe + 56)?.try_into().ok()?);
    let rva = va.checked_sub(base)? as usize;
    for i in 0..n {
        let s = pe + 24 + opt + 40 * i;
        let vaddr = u32::from_le_bytes(exe.get(s + 12..s + 16)?.try_into().ok()?) as usize;
        let raw_size = u32::from_le_bytes(exe.get(s + 16..s + 20)?.try_into().ok()?) as usize;
        let raw = u32::from_le_bytes(exe.get(s + 20..s + 24)?.try_into().ok()?) as usize;
        if (vaddr..vaddr + raw_size).contains(&rva) {
            let at = raw + rva - vaddr;
            let end = exe.get(at..)?.iter().position(|&b| b == 0)? + at;
            let bytes = &exe[at..end];
            // the game's texts are plain text: anything else is not where this build's are
            if bytes.is_empty() || bytes.iter().any(|&b| b < 0x20 && b != b'\n') {
                return None;
            }
            return Some(String::from_utf8_lossy(bytes).into_owned());
        }
    }
    None
}

/// The tutorials' texts from the install's game.
pub fn texts(root: &std::path::Path) -> Vec<Tutorial> {
    let Ok(exe) = std::fs::read(root.join("kenshi_x64.exe")) else { return Vec::new() };
    TUTORIALS
        .iter()
        .filter_map(|(id, _, title, pages)| {
            let title = c_string(&exe, *title)?;
            let pages = pages.iter().map(|p| c_string(&exe, *p)).collect::<Option<Vec<_>>>()?;
            Some(Tutorial { id, title, pages })
        })
        .collect()
}

fn seen_path() -> std::path::PathBuf {
    kenshi_io::cfg::user_dir().join("tutorials.txt")
}

impl Tutorials {
    /// Start tutorial `id` (once; with the option on).
    fn want(&mut self, id: &str, on: bool) {
        if !on || self.seen.contains(id) {
            return;
        }
        if let Some(k) = self.list.iter().position(|t| t.id == id)
            && !self.queue.contains(&k)
            && self.showing.as_ref().is_none_or(|s| s.0 != k)
        {
            self.queue.push_back(k);
        }
    }

    fn save_seen(&self) {
        let mut v: Vec<&String> = self.seen.iter().collect();
        v.sort();
        let _ = std::fs::create_dir_all(kenshi_io::cfg::user_dir());
        let _ = std::fs::write(seen_path(), v.iter().map(|s| format!("{s}\n")).collect::<String>());
    }
}

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

fn show_page(commands: &mut Commands, t: &Tutorial, page: usize, ui: &Built) {
    if let Some(e) = ui.get("TutorialWindow.Caption") {
        commands.queue(SetCaption(e, t.title.clone()));
    }
    if let Some(e) = ui.get("TutorialDescription") {
        commands.queue(SetCaption(e, t.pages.get(page).cloned().unwrap_or_default()));
    }
    if let Some(e) = ui.get("PagingText") {
        commands.queue(SetCaption(e, format!("{} / {}", page + 1, t.pages.len())));
    }
    let last = page + 1 >= t.pages.len();
    if let Some(e) = ui.get("NextTutorialButton") {
        commands.queue(SetCaption(e, if last { "FINISH" } else { "CONTINUE" }.to_string()));
    }
    if let Some(e) = ui.get("PrevTutorialButton") {
        commands.queue(SetVisible(e, page > 0));
    }
    if let Some(e) = ui.get("DismissButton") {
        commands.queue(SetCaption(e, "DISMISS".to_string()));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut tut: ResMut<Tutorials>,
    game: Res<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut clicks: MessageReader<GuiClick>,
    windows: Query<&Window>,
    rows: Query<(&Interaction, &ListRow), Changed<Interaction>>,
    (build, prospect, time): (Option<Res<super::build_ui::BuildMode>>, Res<super::prospect_ui::ProspectWindow>, Res<Time>),
    (mut last, mut hooked): (Local<f32>, Local<bool>),
) {
    let size = window_size(&windows);
    if !tut.loaded {
        tut.loaded = true;
        tut.list = texts(&game.root);
        tut.seen = std::fs::read_to_string(seen_path()).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
        tut.seen_state = (game.sim.folk.iter().filter(|p| p.player).count(), game.sim.world.discovered.len());
        // (testing) KENSHI_TUTORIAL=<id>: that one now, seen or not
        if let Ok(id) = std::env::var("KENSHI_TUTORIAL") {
            tut.seen.remove(&id);
            tut.want(&id, true);
        }
    }
    let on = kenshi_io::cfg::load(&game.root, "settings.cfg").get("tutorials").is_none_or(|v| v.trim() != "0");
    let mut clicked: Vec<GuiClick> = clicks.read().cloned().collect();
    // (testing) KENSHI_TUTORIALS_WINDOW: the tutorials window opened (as HLP), the second chosen
    if std::env::var("KENSHI_TUTORIALS_WINDOW").is_ok() && !*hooked && time.elapsed_secs() > 5.0 {
        *hooked = true;
        clicked.push(GuiClick { widget: Entity::PLACEHOLDER, name: "ShortcutHelpButton".into() });
    }
    // the triggers, once a second
    let now = time.elapsed_secs();
    if now - *last > 1.0 {
        *last = now;
        let sim = &game.sim;
        let mine: Vec<&kenshi_game::sim::Person> = sim.folk.iter().filter(|p| p.player && !p.dead).collect();
        let mut want: Vec<&str> = Vec::new();
        if game.sim.hours_gone() < 0.5 {
            want.extend(["Tutorial_BasicControls", "Tutorial_GUIOverview", "Tutorial_WhatDoIDo"]);
        }
        if sim.world.discovered.len() > tut.seen_state.1 {
            want.push("Tutorial_WhatDoIDo_InTown");
        }
        if mine.len() > tut.seen_state.0 {
            want.push("Tutorial_HiredRecruit");
        }
        tut.seen_state = (mine.len(), sim.world.discovered.len());
        for p in &mine {
            let m = &sim.arena.f[p.id].medical;
            if m.parts.iter().any(|q| q.flesh - q.stun < q.max_health() * 0.9) {
                want.push("Tutorial_Injured_Intro");
            }
            if sim.arena.f[p.id].active {
                want.push("Tutorial_UnderAttack");
            }
            if p.down {
                want.push("Tutorial_KOd");
                if mine.iter().any(|q| !q.down) {
                    want.push("Tutorial_KOdWithNonUnconsciousPC");
                }
            }
            if m.hunger.level < 1.0 {
                want.push("Tutorial_YouAreHungry");
            }
            if p.prison.as_ref().is_some_and(|q| !q.slave) {
                want.push("Tutorial_EscapePrision");
            }
            if p.slave.is_some() {
                want.push("Tutorial_BeignSlave");
            }
            if p.disguised.is_some() {
                want.push("Tutorial_Disguises");
            }
        }
        if build.is_some() {
            want.push("Tutorial_BuildMode");
        }
        if sim.buildings.iter().enumerate().any(|(b, pb)| pb.done && sim.is_production(b)) {
            want.push("Tutorial_ProductionBuilding");
        }
        if prospect.is_open() {
            want.push("Tutorial_Prospecting");
        }
        for id in want {
            tut.want(id, on);
        }
    }
    // the tutorials window: HLP opens and closes it, ACTIVATE shows the one chosen
    let mut activate = None;
    for c in &clicked {
        match c.name.as_str() {
            "ShortcutHelpButton" => {
                if let Some((w, ..)) = tut.window.take() {
                    for r in w.roots {
                        commands.entity(r).try_despawn();
                    }
                } else {
                    let mut defs = gui.0.layout("Kenshi_TutorialsWindow.layout");
                    if let Some(d) = defs.first_mut()
                        && let kenshi_gui::res::Pos::Real(r) = d.pos
                    {
                        d.pos = kenshi_gui::res::Pos::Real([0.5 - r[2] * 0.5, 0.2, r[2], r[3]]);
                    }
                    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
                    for r in &ui.roots {
                        commands.entity(*r).insert(GlobalZIndex(65));
                    }
                    // F049790: the list, the chosen one's pages beside it (<< >>), ACTIVATE
                    if let Some(e) = ui.get("Root.Caption") {
                        commands.queue(SetCaption(e, "TUTORIALS".to_string()));
                    }
                    if let Some(e) = ui.get("ActivateButton") {
                        commands.queue(SetCaption(e, "ACTIVATE".to_string()));
                    }
                    tut.window = Some((ui, Some(0), None, 0));
                    redraw_list(&mut commands, &mut tut, &gui, &assets);
                }
            }
            "ActivateButton" => activate = tut.window.as_ref().and_then(|w| w.1),
            "NextTutorialButton" | "PrevTutorialButton" if tut.window.as_ref().is_some_and(|(w, ..)| w.get(&c.name) == Some(c.widget)) => {
                let d = if c.name == "NextTutorialButton" { 1isize } else { -1 };
                let n = tut.window.as_ref().and_then(|w| w.1).and_then(|k| tut.list.get(k)).map_or(1, |t| t.pages.len());
                if let Some(w) = tut.window.as_mut() {
                    w.3 = (w.3 as isize + d).clamp(0, n as isize - 1) as usize;
                }
                redraw_list(&mut commands, &mut tut, &gui, &assets);
            }
            "Button" if tut.window.as_ref().is_some_and(|(w, ..)| w.named.iter().any(|(_, e)| *e == c.widget)) => {
                if let Some((w, ..)) = tut.window.take() {
                    for r in w.roots {
                        commands.entity(r).try_despawn();
                    }
                }
            }
            _ => {}
        }
    }
    if tut.window.is_some()
        && let Some(k) = rows.iter().find(|(i, r)| **i == Interaction::Pressed && r.0 == LIST).map(|(_, r)| r.1)
    {
        if let Some(w) = tut.window.as_mut() {
            w.1 = Some(k);
            w.3 = 0;
        }
        redraw_list(&mut commands, &mut tut, &gui, &assets);
    }
    if let Some(k) = activate {
        if let Some((_, _, ui)) = tut.showing.take() {
            for r in ui.roots {
                commands.entity(r).try_despawn();
            }
        }
        tut.queue.retain(|q| *q != k);
        tut.queue.push_front(k);
    }
    // the panel's buttons
    if let Some((k, page, ui)) = tut.showing.as_ref().map(|(k, p, u)| (*k, *p, u.clone())) {
        let hit = |n: &str| clicked.iter().any(|c| ui.get(n) == Some(c.widget));
        // (the window's own close button dismisses too)
        let closed = clicked.iter().any(|c| c.name == "Button" && ui.named.iter().any(|(_, e)| *e == c.widget));
        let (next, prev, dismiss) = (hit("NextTutorialButton"), hit("PrevTutorialButton"), hit("DismissButton") || closed);
        let pages = tut.list[k].pages.len();
        let turn = if prev && page > 0 {
            Some(page - 1)
        } else if next && page + 1 < pages {
            Some(page + 1)
        } else {
            None
        };
        if let Some(p) = turn {
            show_page(&mut commands, &tut.list[k], p, &ui);
            if let Some(s) = tut.showing.as_mut() {
                s.1 = p;
            }
        } else if next || dismiss {
            tut.showing = None;
            for r in ui.roots {
                commands.entity(r).try_despawn();
            }
            let id = tut.list[k].id.to_string();
            tut.seen.insert(id);
            tut.save_seen();
        }
        return;
    }
    // the next one waiting
    let Some(k) = tut.queue.pop_front() else { return };
    let mut defs = gui.0.layout("Kenshi_TutorialPanel.layout");
    // the tutorial window by the screen's left, under the information panel's top (assumption)
    for d in &mut defs {
        for c in &mut d.children {
            if c.name == "TutorialWindow"
                && let kenshi_gui::res::Pos::Real(r) = c.pos
            {
                c.pos = kenshi_gui::res::Pos::Real([0.01, 0.12, r[2], r[3]]);
                // a caption, so the window's caption part is named (TutorialWindow.Caption)
                c.props.push(("Caption".into(), String::new()));
            }
        }
    }
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert((GlobalZIndex(85), Pickable::IGNORE));
    }
    if let Some(e) = ui.get("TooltipsPanel") {
        commands.queue(SetVisible(e, false));
    }
    show_page(&mut commands, &tut.list[k], 0, &ui);
    tut.showing = Some((k, 0, ui));
}

/// The tutorials window's list: every tutorial's title, the chosen one marked; its page beside
/// the list with "n / m".
fn redraw_list(commands: &mut Commands, tut: &mut Tutorials, gui: &GuiRes, assets: &AssetServer) {
    let Some((ui, chosen, rows, page)) = tut.window.as_mut() else { return };
    if let Some(t) = chosen.and_then(|k| tut.list.get(k)) {
        if let Some(e) = ui.get("TutorialDescription") {
            commands.queue(SetCaption(e, format!("{}\n\n{}", t.title, t.pages.get(*page).cloned().unwrap_or_default())));
        }
        if let Some(e) = ui.get("PagingText") {
            commands.queue(SetCaption(e, format!("{} / {}", *page + 1, t.pages.len())));
        }
    }
    for (w, c) in [("NextTutorialButton", ">>"), ("PrevTutorialButton", "<<")] {
        if let Some(e) = ui.get(w) {
            commands.queue(SetCaption(e, c.to_string()));
        }
    }
    if let Some(r) = rows.take() {
        commands.entity(r).try_despawn();
    }
    let Some(list) = ui.get("TutorialList") else { return };
    let font = gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
    let names: Vec<(String, bool)> = tut.list.iter().map(|t| (t.title.clone(), true)).collect();
    // (Kenshi_ListBox's client area)
    *rows = Some(list_rows(commands, list, LIST, &names, *chosen, &font, (4.0, 2.0, 92.0, 96.0)));
}
