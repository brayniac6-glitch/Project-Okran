//! Kenshi's tutorials (TutorialManager: F049922 the registry, F049781 the panel, F049790 the
//! tutorials window).
//!
//! A tutorial is a title and its pages. One that starts is not shown at once: it waits as a tip
//! (F049784: a Kenshi_TutorialTooltipPanel row, its name on NameButton and an X), the tips
//! stacked up from the bottom of the tips panel (TooltipsPanel's place, the screen's left;
//! F049741), a new one flashing white and sounding "Thing_Complete". Its name shows it in the
//! panel (Kenshi_TutorialPanel.layout's TutorialWindow, F049783 -> F049778): one page at a
//! time, "1 / 5" under it, PREVIOUS, CONTINUE (FINISH on the last) and DISMISS. FINISH, DISMISS
//! and the tip's X end it (F049758: its tip gone, done); the window's own close only hides it.
//! A tutorial is done once (the seen ones kept, Kenshi in ./__tutorials.data, the rewrite in
//! %APPDATA%\kenshi-rewrite\tutorials.txt) and starts only with the options' "Tutorials" on. The main panel's HLP opens the tutorials window
//! (Kenshi_TutorialsWindow.layout, "TUTORIALS"): every tutorial, ACTIVATE showing the one
//! chosen again.
//!
//! The texts are read from the player's own kenshi_x64.exe (`tutorial_table`); a game whose
//! texts are not where this build's are has no tutorials.
//!
//! (stand-in) What starts each tutorial is not traced; the rewrite starts these when it sees:
//! a new game (Basic Controls, then GUI overview, then What do I do?), a town found, a character
//! hurt, fighting, knocked out, hungry, imprisoned, enslaved, in disguise, a recruit, build
//! mode, a production building built, the prospecting window. (assumption) A new tip's white
//! flash fades over a second.
//!
//! A page may point at the GUI it speaks of (`pointer`): a yellow box over that widget.
use super::Game;
use super::build_ui::{ListRow, list_rows};
use super::tutorial_table::{POINTERS, TUTORIALS};
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
    /// The tutorials window's list scrolled down this many rows (the mouse wheel over it).
    scroll: usize,
    loaded: bool,
    /// The tips waiting (F049784): the tutorial, its row's widgets, its flash and how bright.
    tips: Vec<(usize, Built, Option<Entity>, f32)>,
}

impl Tutorials {
    /// Whether the tutorials window (HLP) is open.
    pub fn window_open(&self) -> bool {
        self.window.is_some()
    }
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

    /// The options' Reset Tutorials: every tutorial shown again.
    /// @fn F032929
    ///
    /// The options' Reset Tutorials: none seen any more (Kenshi deletes ./__tutorials.data),
    /// the tips waiting gone, none queued.
    pub fn reset(&mut self, commands: &mut Commands) {
        self.seen.clear();
        self.save_seen();
        for (_, ui, ..) in self.tips.drain(..) {
            for r in ui.roots {
                commands.entity(r).try_despawn();
            }
        }
        self.queue.clear();
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
    (build, prospect, time, settings): (Option<Res<super::build_ui::BuildMode>>, Res<super::prospect_ui::ProspectWindow>, Res<Time>, Option<Res<super::options_ui::Settings>>),
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    (mut last, mut hooked, mut hooked_open): (Local<f32>, Local<bool>, Local<bool>),
    (mut sound, mut nodes): (Option<ResMut<super::sound::Sound>>, Query<&mut Node>),
) {
    let size = window_size(&windows);
    if !tut.loaded {
        tut.loaded = true;
        tut.list = texts(&game.root);
        tut.seen = std::fs::read_to_string(seen_path()).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
        tut.seen_state = (game.sim.folk.iter().filter(|p| p.player && p.race.is_none()).count(), game.sim.world.discovered.len());
        // (testing) KENSHI_TUTORIAL=<id>: that one now, seen or not
        if let Ok(id) = std::env::var("KENSHI_TUTORIAL") {
            tut.seen.remove(&id);
            tut.want(&id, true);
        }
    }
    // the options' "Tutorials" (F049785 as it changes: off, the panel hidden and the tips
    // waiting gone, not seen; none started while off)
    let on = settings.as_ref().is_none_or(|s| s.on("tutorials"));
    if !on {
        if let Some((_, _, ui)) = tut.showing.take() {
            for r in ui.roots {
                commands.entity(r).try_despawn();
            }
        }
        for (_, ui, ..) in tut.tips.drain(..) {
            for r in ui.roots {
                commands.entity(r).try_despawn();
            }
        }
    }
    let mut clicked: Vec<GuiClick> = clicks.read().cloned().collect();
    // (testing) KENSHI_TUTORIALS_OPEN=<seconds>[,<rows>]: the tutorials window opened then, its
    // list scrolled that many rows
    if let Ok(v) = std::env::var("KENSHI_TUTORIALS_OPEN")
        && !*hooked_open
    {
        let mut it = v.split(',');
        let at: f32 = it.next().and_then(|t| t.parse().ok()).unwrap_or(10.0);
        if time.elapsed_secs() > at {
            *hooked_open = true;
            clicked.push(GuiClick { widget: Entity::PLACEHOLDER, name: "ShortcutHelpButton".into() });
            tut.scroll = it.next().and_then(|r| r.parse().ok()).unwrap_or(0);
        }
    }
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
        // a recruit: one more person (an animal bought is not one)
        let people = mine.iter().filter(|p| p.race.is_none()).count();
        if people > tut.seen_state.0 {
            want.push("Tutorial_HiredRecruit");
        }
        tut.seen_state = (people, sim.world.discovered.len());
        for p in &mine {
            let m = &sim.arena.f[p.id].medical;
            if m.parts.iter().any(|q| q.flesh - q.stun < q.max_health() * 0.9) {
                want.push("Tutorial_Injured_Intro");
            }
            if sim.arena.f[p.id].active {
                want.push("Tutorial_UnderAttack");
            }
            if p.playing_dead {
                want.push("Tutorial_PlayingDead");
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
    // the wheel scrolls the window's list (a row a notch), as Kenshi_ListBox does
    let notches: f32 = wheel.read().map(|w| w.y.signum()).sum();
    if tut.window.is_some() && notches != 0.0 {
        // no further than leaves the list's last rows filling it (its client height over the
        // rows' 18 px)
        let visible = tut
            .window
            .as_ref()
            .and_then(|(ui, ..)| ui.get("TutorialList").and_then(|l| ui.sizes.get(&l)))
            .map_or(12, |s| (s[1] * 0.96 / (18.0 * kenshi_gui::res::ui_scale())).floor().max(1.0) as usize);
        let most = tut.list.len().saturating_sub(visible);
        let to = (tut.scroll as f32 - notches).clamp(0.0, most as f32) as usize;
        if to != tut.scroll {
            tut.scroll = to;
            redraw_list(&mut commands, &mut tut, &gui, &assets);
        }
    }
    if tut.window.is_some()
        && let Some(k) = rows.iter().find(|(i, r)| **i == Interaction::Pressed && r.0 == LIST).map(|(_, r)| r.1 + tut.scroll)
    {
        if let Some(w) = tut.window.as_mut() {
            w.1 = Some(k);
            w.3 = 0;
        }
        redraw_list(&mut commands, &mut tut, &gui, &assets);
    }
    // ACTIVATE: the chosen one a tip again
    if let Some(k) = activate {
        tut.queue.retain(|q| *q != k);
        tut.queue.push_front(k);
    }
    // the panel's buttons
    if let Some((k, page, ui)) = tut.showing.as_ref().map(|(k, p, u)| (*k, *p, u.clone())) {
        let hit = |n: &str| clicked.iter().any(|c| ui.get(n) == Some(c.widget));
        // the window's own close only hides it (F049742); its tip stays
        let closed = clicked.iter().any(|c| c.name == "Button" && ui.named.iter().any(|(_, e)| *e == c.widget));
        if closed {
            tut.showing = None;
            for r in ui.roots {
                commands.entity(r).try_despawn();
            }
            return;
        }
        let (next, prev, dismiss) = (hit("NextTutorialButton"), hit("PrevTutorialButton"), hit("DismissButton"));
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
            // FINISH or DISMISS: done (F049758)
            finish(&mut commands, &mut tut, k);
        }
    }
    // the tips' rows: a name shows its tutorial, an X ends it
    let mut show: Option<usize> = None;
    let mut end: Option<usize> = None;
    for c in &clicked {
        for (k, ui, ..) in &tut.tips {
            if ui.get("NameButton") == Some(c.widget) {
                show = Some(*k);
            }
            if ui.get("CloseButton") == Some(c.widget) {
                end = Some(*k);
            }
        }
    }
    if let Some(k) = end {
        finish(&mut commands, &mut tut, k);
    }
    // the ones started become tips (F049784), a new one flashing, "Thing_Complete"
    let mut added = false;
    while let Some(k) = tut.queue.pop_front() {
        if tut.tips.iter().any(|t| t.0 == k) {
            continue;
        }
        let mut defs = gui.0.layout("Kenshi_TutorialTooltipPanel.layout");
        let scale = kenshi_gui::res::ui_scale();
        // F049774: the panel's width, the layout's height at the UI scale
        let (w, h) = ((size[0] * 0.130208).floor(), (25.0 * scale).floor());
        if let Some(d) = defs.first_mut() {
            d.pos = kenshi_gui::res::Pos::Px([0.0, 0.0, 250.0, 25.0]);
            d.align = kenshi_gui::res::Align::parse("Stretch");
        }
        // the row's place, its parts fitted to it by their align
        let holder = commands
            .spawn((Node { position_type: PositionType::Absolute, width: Val::Px(w), height: Val::Px(h), ..default() }, GlobalZIndex(84)))
            .id();
        let mut ui = kenshi_gui::ui::spawn_design(&mut commands, &gui.0, &assets, &defs, Some(holder), [250.0, 25.0], [w, h]);
        ui.roots = vec![holder];
        if let Some(b) = ui.get("NameButton") {
            commands.queue(SetCaption(b, tut.list[k].title.clone()));
        }
        // its flash: a WhiteSkin over it, white
        let flash = ui.roots.first().map(|&r| {
            let f = commands
                .spawn((
                    Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(Color::WHITE),
                    Pickable::IGNORE,
                ))
                .id();
            commands.entity(r).add_child(f);
            f
        });
        tut.tips.push((k, ui, flash, 1.0));
        added = true;
    }
    if added && let Some(s) = sound.as_mut() {
        s.post("Thing_Complete", super::sound::Channel::Ui);
    }
    // laid up from the tips panel's bottom (TooltipsPanel: 0.462963 of the screen), the last at
    // the bottom (F049741)
    let scale = kenshi_gui::res::ui_scale();
    let h = (25.0 * scale).floor();
    let bottom = size[1] * 0.462963;
    let dt = time.delta_secs();
    let n = tut.tips.len();
    for (i, (_, ui, flash, a)) in tut.tips.iter_mut().enumerate() {
        if let Some(&r) = ui.roots.first()
            && let Ok(mut node) = nodes.get_mut(r)
        {
            node.left = Val::Px(0.0);
            node.top = Val::Px(bottom - (n - i) as f32 * h);
        }
        if let Some(f) = *flash {
            *a = (*a - dt).max(0.0);
            commands.entity(f).insert(BackgroundColor(Color::srgba(1.0, 1.0, 1.0, *a)));
            if *a <= 0.0 {
                commands.entity(f).try_despawn();
                *flash = None;
            }
        }
    }
    let Some(k) = show else { return };
    if let Some((_, _, ui)) = tut.showing.take() {
        for r in ui.roots {
            commands.entity(r).try_despawn();
        }
    }
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

/// The box a tutorial page points with (TutorialGUI +0xd0 the case, +0xe0 the box, +0xe8 its
/// flash's way, +0xec its alpha).
#[derive(Default)]
pub struct Pointer {
    case: u8,
    node: Option<Entity>,
    way: f32,
    alpha: f32,
}

/// @fn F049763
/// @fn F049782
///
/// The page shown's pointer (`POINTERS`): a WhiteSkin box in #e8e920 (0.909, 0.913, 0.125) on
/// the Top layer, not taking the mouse, over the widget the case names (F049757:
/// `hud_ui::Hud::pointer_target`, `ResearchWindow::pointer_target`). A new case puts it there
/// at alpha 0; each frame its alpha goes by its way x 0.4 a second within 0.1 .. 0.4, turning
/// at each end (its way 1 at first), and it shows only while its widget does. No case (another
/// page, or none shown) hides it, its way 1 and alpha 0 again (F049728). (stand-in) Kenshi
/// places the box when the page starts (the GUI overview's second page again each frame); here
/// each frame, so it follows a widget that moves. (assumption) The frame's time is the
/// global at 0x142133790.
pub fn pointer(
    mut commands: Commands,
    tut: Res<Tutorials>,
    hud: Res<super::hud_ui::Hud>,
    research: Res<super::research_ui::ResearchWindow>,
    time: Res<Time>,
    widgets: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform, &InheritedVisibility)>,
    mut ptr: Local<Pointer>,
) {
    let case = tut
        .showing
        .as_ref()
        .and_then(|(k, page, _)| POINTERS.iter().find(|(id, _)| *id == tut.list[*k].id).and_then(|(_, c)| c.get(*page).copied()))
        .unwrap_or(0);
    if case != ptr.case {
        ptr.case = case;
        if case == 0 {
            ptr.way = 1.0;
            ptr.alpha = 0.0;
        } else {
            ptr.alpha = 0.0;
        }
    }
    if ptr.way == 0.0 {
        ptr.way = 1.0;
    }
    // (a game unloaded took the box with it)
    if ptr.node.is_some_and(|n| widgets.get(n).is_err() && commands.get_entity(n).is_err()) {
        ptr.node = None;
    }
    // (WhiteSkin is Kenshi_UI.png's white corner: a plain fill)
    let node = *ptr.node.get_or_insert_with(|| {
        commands.spawn((Node { position_type: PositionType::Absolute, ..default() }, BackgroundColor(Color::NONE), GlobalZIndex(TOP), Visibility::Hidden, Pickable::IGNORE, bevy::ui::FocusPolicy::Pass)).id()
    });
    if case == 0 {
        commands.entity(node).try_insert(Visibility::Hidden);
        return;
    }
    // the widget and its rect (physical pixels)
    let target = hud.pointer_target(case).or_else(|| research.pointer_target(case).map(|e| super::hud_ui::PointerTarget { main: e, rects: vec![e], shape: super::hud_ui::PointerShape::One }));
    let rect_of = |e: Entity| {
        widgets.get(e).ok().map(|(c, t, _)| {
            let min = t.translation - c.size * 0.5;
            (Rect::from_corners(min, min + c.size), c.inverse_scale_factor)
        })
    };
    let placed = target.as_ref().and_then(|t| {
        let rects: Vec<(Rect, f32)> = t.rects.iter().filter_map(|&e| rect_of(e)).collect();
        let (first, inv) = *rects.first()?;
        let r = match t.shape {
            super::hud_ui::PointerShape::One => first,
            super::hud_ui::PointerShape::ToBottom => Rect::new(first.min.x, first.min.y, first.max.x, 1080.0),
            super::hud_ui::PointerShape::Span => Rect::new(first.min.x, first.min.y, first.max.x, rects.last().map_or(first.max.y, |l| l.0.max.y)),
            super::hud_ui::PointerShape::Union => rects.iter().fold(first, |a, (b, _)| a.union(*b)),
        };
        let shown = widgets.get(t.main).is_ok_and(|(_, _, v)| v.get());
        Some((r, inv, shown))
    });
    let Some((r, inv, shown)) = placed else {
        commands.entity(node).try_insert(Visibility::Hidden);
        return;
    };
    // its flash
    let dt = time.delta_secs();
    ptr.alpha = (ptr.alpha + ptr.way * dt * 0.4).clamp(0.1, 0.4);
    if ptr.way > 0.0 {
        if ptr.alpha >= 0.4 {
            ptr.way = -1.0;
        }
    } else if ptr.alpha <= 0.1 {
        ptr.way = 1.0;
    }
    let alpha = ptr.alpha;
    commands.entity(node).try_insert((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(r.min.x * inv),
            top: Val::Px(r.min.y * inv),
            width: Val::Px(r.width().max(0.0) * inv),
            height: Val::Px((r.max.y - r.min.y).max(0.0) * inv),
            ..default()
        },
        if shown { Visibility::Visible } else { Visibility::Hidden },
        BackgroundColor(Color::srgba(0.909, 0.913, 0.125, alpha)),
    ));
}

/// Kenshi's Top layer (over the tooltips' ToolTip, core_layer.xml).
const TOP: i32 = 310;

/// @fn F049758
///
/// Tutorial `k` done: hidden if shown, its tip gone, kept as seen.
fn finish(commands: &mut Commands, tut: &mut Tutorials, k: usize) {
    if tut.showing.as_ref().is_some_and(|s| s.0 == k)
        && let Some((_, _, ui)) = tut.showing.take()
    {
        for r in ui.roots {
            commands.entity(r).try_despawn();
        }
    }
    if let Some(at) = tut.tips.iter().position(|t| t.0 == k) {
        let (_, ui, ..) = tut.tips.remove(at);
        for r in ui.roots {
            commands.entity(r).try_despawn();
        }
    }
    let id = tut.list[k].id.to_string();
    tut.seen.insert(id);
    tut.save_seen();
}

/// The tutorials window's list: every tutorial's title, the chosen one marked; its page beside
/// the list with "n / m".
fn redraw_list(commands: &mut Commands, tut: &mut Tutorials, gui: &GuiRes, assets: &AssetServer) {
    let scroll = tut.scroll;
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
    let names: Vec<(String, bool)> = tut.list.iter().skip(scroll).map(|t| (t.title.clone(), true)).collect();
    // (Kenshi_ListBox's client area; the rows from the scroll on)
    *rows = Some(list_rows(commands, list, LIST, &names, chosen.and_then(|c| c.checked_sub(scroll)), &font, (4.0, 2.0, 92.0, 96.0)));
}
