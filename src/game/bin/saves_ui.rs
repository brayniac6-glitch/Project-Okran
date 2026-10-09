//! The Save, Load and Import panels (Kenshi_SaveGamePanel, Kenshi_LoadGamePanel,
//! Kenshi_ImportGamePanel; F034863 .. F034866) and Kenshi's message box (Kenshi_MessageBox,
//! F042114) for their questions.
//!
//! Each lists the saves in its GamesList (a MultiListBox, F034863): a "Name" column (170 wide,
//! taking the room left) and a "Date" column (180, "{1,datetime}"), sorted by date, newest
//! first. Delete with a save chosen asks "Are you sure you want to delete this save game?"
//! ("Delete Save"); the save in use can't be deleted (F034862).
//! - Save: SAVE only with a name typed (F034833), the characters a folder name can't take dropped
//!   as they are typed; picking a save puts its name in the box; a name already used asks
//!   "Overwrite save '{1}'?" ("Save game", F034854).
//! - Load: LOAD loads the chosen save; "Reset squad positions" gathers the squad.
//! - Import: Kenshi's own saves (its save folder, read only) as well as ours; IMPORT starts a new
//!   world from the chosen one with what is ticked (reset positions, buildings, research, dead
//!   NPCs, relations).
//!
//! (assumption) The info panel shows the chosen save's day and time, money and characters;
//! picking a save (not double-clicking) chooses it; ADVANCED OPTIONS (the import's ticks are
//! all shown already) does nothing; the message box's Yes and No are its first and last
//! buttons. The date is written as ICU's medium date and time ("8 Oct 2026, 15:45:12").
use bevy::prelude::*;
use kenshi_game::kenshi_save::ImportOptions;
use kenshi_game::save::{self, Slot};
use kenshi_gui::ui::{Checked, Disabled, EditText};
use kenshi_gui::{Built, GuiClick, GuiEdit, GuiRes, SetCaption, SetVisible};
use std::path::PathBuf;

/// The list rows' text colours (Kenshi_ListBoxItemSkin: normal, checked).
const ROW_TEXT: Color = Color::srgb(0.078, 0.031, 0.024);
const ROW_CHECKED: Color = Color::srgb(0.626, 0.605, 0.578);
/// The list's line height (Kenshi_MultiSubListBox "HeightLine"), its header's height and the
/// Date column's width (F034863), in design pixels.
const LINE: f32 = 21.0;
const HEADER: f32 = 30.0;
const DATE_W: f32 = 180.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Save,
    Load,
    Import,
}

/// What a panel asks the game to do.
#[derive(Message, Clone, Debug)]
pub enum SavesDone {
    /// Save under this name.
    Save(String),
    /// Load this save of ours (its folder), gathering the squad or not.
    Load(PathBuf, bool),
    /// Start a world from this save (ours or Kenshi's) with these options.
    Import(PathBuf, ImportOptions),
}

#[derive(Resource, Default)]
pub struct SavesPanel {
    open: Option<Open>,
    ask: Option<Ask>,
    /// The save the running game came from (it can't be deleted).
    pub current: Option<String>,
    /// Closed panels' roots, hidden and despawned next frame (the GUI may still restyle the
    /// button that closed them this frame).
    dying: Vec<Entity>,
}

struct Open {
    kind: Kind,
    ui: Built,
    slots: Vec<Slot>,
    rows: Vec<Entity>,
    chosen: Option<usize>,
    /// Reset squad positions, buildings, research, dead NPCs, relations.
    ticks: [bool; 5],
    info: Option<Entity>,
    font: (Handle<Font>, f32),
}

struct Ask {
    ui: Built,
    what: Asked,
}

enum Asked {
    Overwrite(String),
    Delete(usize),
    Note,
}

/// A list row: its save's place in the list.
#[derive(Component)]
pub struct SaveRow(usize);

const TICKS: [&str; 5] = ["ResetTickBox", "BuildingsTickBox", "ResearchTickBox", "NPCTickBox", "RelationsTickBox"];

impl SavesPanel {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Open panel `kind` (closing one open), its middle at the screen's (F034863).
    pub fn open(&mut self, commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, kind: Kind, size: [f32; 2]) {
        self.close(commands);
        let file = match kind {
            Kind::Save => "Kenshi_SaveGamePanel.layout",
            Kind::Load => "Kenshi_LoadGamePanel.layout",
            Kind::Import => "Kenshi_ImportGamePanel.layout",
        };
        let mut defs = gui.0.layout(file);
        if let Some(d) = defs.first_mut()
            && let kenshi_gui::res::Pos::Real(r) = d.pos
        {
            d.pos = kenshi_gui::res::Pos::Real([0.5 - r[2] * 0.5, 0.5 - r[3] * 0.5, r[2], r[3]]);
        }
        let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
        for r in &ui.roots {
            commands.entity(*r).insert(GlobalZIndex(95));
        }
        let slots = match kind {
            Kind::Save | Kind::Load => save::slots(),
            Kind::Import => {
                let mut v: Vec<Slot> = save::slots().into_iter().filter(|s| s.path.join("quick.save").is_file()).collect();
                v.extend(save::kenshi_slots());
                v
            }
        };
        let ticks = [false, true, true, true, true];
        for (k, t) in TICKS.iter().enumerate() {
            if let Some(e) = ui.get(t) {
                commands.entity(e).insert(Checked(ticks[k]));
            }
        }
        for b in ["SaveButton", "LoadButton", "ImportButton", "AdvancedButton"] {
            if let Some(e) = ui.get(b) {
                commands.entity(e).insert(Disabled);
            }
        }
        let fd = gui.0.fonts.get("Kenshi_StandardFont_Medium");
        let font = (fd.map_or_else(Handle::default, |f| assets.load(f.file.clone())), fd.map_or(15.0, |f| f.screen_px()));
        // the info panel's text
        let info = ui.get("InfoPanel").map(|p| {
            let t = commands
                .spawn((
                    Text::new(""),
                    TextFont { font: bevy::text::FontSource::Handle(font.0.clone()), font_size: bevy::text::FontSize::Px(font.1), ..default() },
                    TextColor(Color::srgb(0.69, 0.65, 0.55)),
                    Node { width: Val::Percent(100.0), ..default() },
                ))
                .id();
            commands.entity(p).add_child(t);
            t
        });
        let mut o = Open { kind, ui, slots, rows: Vec::new(), chosen: None, ticks, info, font };
        fill(commands, &mut o, gui, assets);
        self.open = Some(o);
    }

    pub fn close(&mut self, commands: &mut Commands) {
        if let Some(a) = self.ask.take() {
            self.bury(commands, a.ui.roots);
        }
        if let Some(o) = self.open.take() {
            self.bury(commands, o.ui.roots);
        }
    }

    fn bury(&mut self, commands: &mut Commands, roots: Vec<Entity>) {
        for r in roots {
            commands.entity(r).insert(Visibility::Hidden);
            self.dying.push(r);
        }
    }

    /// Kenshi's message box: `title`, `text`, Yes and No (or OK alone).
    fn ask(&mut self, commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, size: [f32; 2], title: &str, text: &str, what: Asked) {
        if let Some(a) = self.ask.take() {
            self.bury(commands, a.ui.roots);
        }
        let mut defs = gui.0.layout("Kenshi_MessageBox.layout");
        if let Some(d) = defs.first_mut()
            && let kenshi_gui::res::Pos::Real(r) = d.pos
        {
            d.pos = kenshi_gui::res::Pos::Real([0.5 - r[2] * 0.5, 0.5 - r[3] * 0.5, r[2], r[3]]);
        }
        let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
        for r in &ui.roots {
            commands.entity(*r).insert(GlobalZIndex(120));
        }
        if let Some(e) = ui.get("Root.Caption") {
            commands.queue(SetCaption(e, title.to_string()));
        }
        if let Some(e) = ui.get("MessageText") {
            commands.queue(SetCaption(e, text.to_string()));
        }
        let note = matches!(what, Asked::Note);
        for (b, cap, shown) in [("ButtonA", "Yes", !note), ("ButtonB", "OK", note), ("ButtonC", "No", !note)] {
            if let Some(e) = ui.get(b) {
                commands.queue(SetCaption(e, cap.to_string()));
                commands.queue(SetVisible(e, shown));
            }
        }
        self.ask = Some(Ask { ui, what });
    }
}

/// A time as ICU's medium date and time ("8 Oct 2026, 15:45:12"), in local time.
pub fn date_text(t: std::time::SystemTime) -> String {
    let [y, mo, d, h, mi, s] = local_time(t);
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{d} {} {y}, {h:02}:{mi:02}:{s:02}", MONTHS[(mo as usize).clamp(1, 12) - 1])
}

#[cfg(windows)]
fn local_time(t: std::time::SystemTime) -> [u32; 6] {
    #[repr(C)]
    struct FileTime {
        lo: u32,
        hi: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct SysTime {
        year: u16,
        month: u16,
        dow: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        ms: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToLocalFileTime(a: *const FileTime, b: *mut FileTime) -> i32;
        fn FileTimeToSystemTime(a: *const FileTime, b: *mut SysTime) -> i32;
    }
    let ticks = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64 / 100) + 116_444_736_000_000_000;
    let ft = FileTime { lo: ticks as u32, hi: (ticks >> 32) as u32 };
    let mut local = FileTime { lo: 0, hi: 0 };
    let mut st = SysTime::default();
    // SAFETY: both calls only write their out-structures, laid out as the API declares them.
    let ok = unsafe { FileTimeToLocalFileTime(&ft, &mut local) != 0 && FileTimeToSystemTime(&local, &mut st) != 0 };
    if !ok {
        return utc(t);
    }
    [st.year as u32, st.month as u32, st.day as u32, st.hour as u32, st.minute as u32, st.second as u32]
}

#[cfg(not(windows))]
fn local_time(t: std::time::SystemTime) -> [u32; 6] {
    utc(t)
}

/// A time in UTC (year, month, day, hour, minute, second).
fn utc(t: std::time::SystemTime) -> [u32; 6] {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // civil from days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    [y as u32, m as u32, d as u32, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32]
}

/// The list: its column headers, then a row a save.
fn fill(commands: &mut Commands, o: &mut Open, gui: &GuiRes, assets: &AssetServer) {
    for r in o.rows.drain(..) {
        commands.entity(r).despawn();
    }
    let Some(list) = o.ui.get("GamesList") else { return };
    let scale = kenshi_gui::res::ui_scale();
    let size = o.ui.sizes.get(&list).copied().unwrap_or([400.0, 300.0]);
    let (inner_w, date_w) = (size[0] - 10.0 * scale, DATE_W * scale);
    let name_w = (inner_w - date_w).max(40.0);
    // the headers (Kenshi_MultiListButton), Date sorted (its arrow down)
    let header = commands.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(5.0 * scale), top: Val::Px(5.0 * scale), width: Val::Px(inner_w), height: Val::Px(HEADER * scale), ..default() }).id();
    commands.entity(list).add_child(header);
    let mk = |name: &str, caption: &str, pos: [f32; 4]| kenshi_gui::WidgetDef {
        kind: "Button".into(),
        skin: "Kenshi_MultiListButton".into(),
        pos: kenshi_gui::res::Pos::Px(pos),
        align: Default::default(),
        name: name.into(),
        layer: None,
        props: vec![("Caption".into(), caption.into())],
        user: Vec::new(),
        children: Vec::new(),
    };
    let defs = vec![mk("NameColumn", "Name", [0.0, 0.0, name_w, HEADER * scale]), mk("DateColumn", "Date", [name_w, 0.0, date_w, HEADER * scale])];
    let built = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, Some(header), [inner_w, HEADER * scale]);
    // only the sorted column shows its arrow
    if let Some(n) = built.get("NameColumn") {
        commands.entity(n).insert(Disabled);
    }
    o.rows.push(header);
    // the rows, under the headers
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(5.0 * scale),
                top: Val::Px((HEADER + 5.0) * scale),
                width: Val::Px(inner_w),
                height: Val::Px((size[1] - (HEADER + 10.0) * scale).max(0.0)),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ZIndex(2),
        ))
        .id();
    commands.entity(list).add_child(column);
    o.rows.push(column);
    for (k, s) in o.slots.iter().enumerate() {
        let on = o.chosen == Some(k);
        let colour = if on { ROW_CHECKED } else { ROW_TEXT };
        let row = commands
            .spawn((
                Button,
                SaveRow(k),
                Node { width: Val::Percent(100.0), height: Val::Px(LINE * scale), flex_shrink: 0.0, align_items: AlignItems::Center, ..default() },
                BackgroundColor(if on { Color::srgba(0.12, 0.07, 0.05, 0.85) } else { Color::NONE }),
            ))
            .id();
        let name = if s.kenshi { format!("{} (Kenshi)", s.name) } else { s.name.clone() };
        for (text, left, w) in [(name, 3.0 * scale, name_w - 3.0 * scale), (date_text(s.when), name_w + 3.0 * scale, date_w - 3.0 * scale)] {
            let t = commands
                .spawn((
                    Text::new(text),
                    TextFont { font: bevy::text::FontSource::Handle(o.font.0.clone()), font_size: bevy::text::FontSize::Px(o.font.1), ..default() },
                    TextColor(colour),
                    TextLayout::no_wrap(),
                    Node { position_type: PositionType::Absolute, left: Val::Px(left), width: Val::Px(w), overflow: Overflow::clip(), ..default() },
                ))
                .id();
            commands.entity(row).add_child(t);
        }
        commands.entity(column).add_child(row);
    }
}

/// The chosen save's day and time, money and characters.
fn describe(o: &Open) -> String {
    let Some(s) = o.chosen.and_then(|k| o.slots.get(k)) else { return String::new() };
    let mut out = format!("{}\n{}\n\n", s.name, date_text(s.when));
    if s.path.join("quick.save").is_file() {
        match kenshi_game::kenshi_save::read(&s.path) {
            Ok(k) => {
                out += &format!("Day {}, {:02}:{:02}\n{} cats\n\n", k.time.0, k.time.1, k.time.2, k.money);
                let names: Vec<&str> = k.squads.iter().flat_map(|q| q.characters.iter().map(|c| c.name.as_str())).collect();
                out += &format!("{} characters\n{}", names.len(), names.iter().take(8).cloned().collect::<Vec<_>>().join("\n"));
                if names.len() > 8 {
                    out += "\n...";
                }
            }
            Err(e) => out += &e,
        }
    } else if let Ok(g) = save::read_slot(&s.path) {
        out += &format!("{} cats\n\n{} characters\n", g.money, g.players.len());
        out += &g.players.iter().take(8).map(|p| p.name.as_str()).collect::<Vec<_>>().join("\n");
    }
    out
}

/// Choose save `k`: its row lit, its name in the save box, the panel's button on.
fn choose(commands: &mut Commands, o: &mut Open, k: Option<usize>, gui: &GuiRes, assets: &AssetServer, boxes: &mut Query<&mut EditText>) {
    o.chosen = k;
    fill(commands, o, gui, assets);
    if let Some(i) = o.info {
        commands.entity(i).insert(Text::new(describe(o)));
    }
    for b in ["LoadButton", "ImportButton"] {
        if let Some(e) = o.ui.get(b) {
            if k.is_some() {
                commands.entity(e).remove::<Disabled>();
            } else {
                commands.entity(e).insert(Disabled);
            }
        }
    }
    if o.kind == Kind::Save
        && let Some(s) = k.and_then(|k| o.slots.get(k))
        && let Some(e) = o.ui.get("SaveEditBox")
    {
        if let Ok(mut t) = boxes.get_mut(e) {
            t.text = s.name.clone();
        }
        commands.queue(SetCaption(e, s.name.clone()));
        if let Some(b) = o.ui.get("SaveButton") {
            commands.entity(b).remove::<Disabled>();
        }
    }
}

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut panel: ResMut<SavesPanel>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut clicks: MessageReader<GuiClick>,
    rows: Query<(&Interaction, &SaveRow), Changed<Interaction>>,
    mut edits: MessageReader<GuiEdit>,
    mut boxes: Query<&mut EditText>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut done: MessageWriter<SavesDone>,
) {
    let size = window_size(&windows);
    let clicked: Vec<GuiClick> = clicks.read().cloned().collect();
    let p = &mut *panel;
    for e in p.dying.drain(..) {
        if let Ok(mut c) = commands.get_entity(e) {
            c.despawn();
        }
    }
    // the message box's answer first
    if let Some(a) = &p.ask {
        let hit = |n: &str| clicked.iter().any(|c| a.ui.get(n) == Some(c.widget));
        let (yes, no) = (hit("ButtonA") || hit("ButtonB"), hit("ButtonC"));
        if !yes && !no {
            return;
        }
        let a = p.ask.take().unwrap();
        p.bury(&mut commands, a.ui.roots);
        if yes {
            match a.what {
                Asked::Overwrite(name) => {
                    p.close(&mut commands);
                    done.write(SavesDone::Save(name));
                }
                Asked::Delete(k) => {
                    let Some(o) = p.open.as_mut() else { return };
                    if let Some(s) = o.slots.get(k).cloned() {
                        match save::delete_slot(&s) {
                            Ok(()) => {
                                o.slots.remove(k);
                            }
                            Err(e) => eprintln!("can't delete {}: {e}", s.name),
                        }
                    }
                    choose(&mut commands, o, None, &gui, &assets, &mut boxes);
                }
                Asked::Note => {}
            }
        }
        return;
    }
    let Some(o) = p.open.as_mut() else { return };
    let hits: Vec<String> = clicked.iter().filter(|c| o.ui.get(&c.name) == Some(c.widget)).map(|c| c.name.clone()).collect();
    let mine = |n: &str| hits.iter().any(|h| h == n);
    if mine("CancelButton") || keys.just_pressed(KeyCode::Escape) {
        p.close(&mut commands);
        return;
    }
    // a row picked
    if let Some(k) = rows.iter().find(|(i, _)| **i == Interaction::Pressed).map(|(_, r)| r.0) {
        choose(&mut commands, o, Some(k), &gui, &assets, &mut boxes);
    }
    // the ticks
    for (k, t) in TICKS.iter().enumerate() {
        if mine(t)
            && let Some(e) = o.ui.get(t)
        {
            o.ticks[k] = !o.ticks[k];
            commands.entity(e).insert(Checked(o.ticks[k]));
        }
    }
    // the save name typed: what a folder can't take dropped, SAVE only with a name
    let edit = o.ui.get("SaveEditBox");
    for ev in edits.read() {
        if Some(ev.widget) != edit {
            continue;
        }
        let clean: String = ev.text.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
        if clean != ev.text {
            if let Ok(mut t) = boxes.get_mut(ev.widget) {
                t.text = clean.clone();
            }
            commands.queue(SetCaption(ev.widget, clean.clone()));
        }
        if let Some(b) = o.ui.get("SaveButton") {
            if clean.trim().is_empty() {
                commands.entity(b).insert(Disabled);
            } else {
                commands.entity(b).remove::<Disabled>();
            }
        }
    }
    let typing = boxes.iter().any(|t| t.focused);
    // Delete: the chosen save, asked first (not the one in use, nor Kenshi's)
    if (keys.just_pressed(KeyCode::Delete) || (!typing && keys.just_pressed(KeyCode::Backspace)))
        && let Some(k) = o.chosen
        && let Some(s) = o.slots.get(k)
    {
        if s.kenshi || p.current.as_deref() == Some(s.name.as_str()) {
            p.ask(&mut commands, &gui, &assets, size, "Delete Save", "This save file is currently in use and cannot be deleted", Asked::Note);
        } else {
            p.ask(&mut commands, &gui, &assets, size, "Delete Save", "Are you sure you want to delete this save game?", Asked::Delete(k));
        }
        return;
    }
    if mine("SaveButton") || (o.kind == Kind::Save && typing && keys.just_pressed(KeyCode::Enter)) {
        let name = edit.and_then(|e| boxes.get(e).ok()).map(|t| save::clean_name(&t.text)).unwrap_or_default();
        if name.is_empty() {
            return;
        }
        if o.slots.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            let text = format!("Overwrite save '{name}'?");
            p.ask(&mut commands, &gui, &assets, size, "Save game", &text, Asked::Overwrite(name));
        } else {
            p.close(&mut commands);
            done.write(SavesDone::Save(name));
        }
        return;
    }
    if mine("LoadButton")
        && let Some(s) = o.chosen.and_then(|k| o.slots.get(k))
    {
        let (path, reset) = (s.path.clone(), o.ticks[0]);
        p.close(&mut commands);
        done.write(SavesDone::Load(path, reset));
        return;
    }
    if mine("ImportButton")
        && let Some(s) = o.chosen.and_then(|k| o.slots.get(k))
    {
        let t = o.ticks;
        let opt = ImportOptions { reset_positions: t[0], buildings: t[1], research: t[2], dead_npcs: t[3], relations: t[4] };
        let path = s.path.clone();
        p.close(&mut commands);
        done.write(SavesDone::Import(path, opt));
    }
}
