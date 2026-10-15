//! Kenshi's character editor (Kenshi_CharacterEditor.layout) after a new game loads: each of the
//! squad's characters gets a name, a race and subrace, a gender and a look (the race's editor
//! limits file: its Face and Body ranges; hair and beard from the race's hairs), shown live on the
//! character; Confirm starts the game.
//!
//! (stand-in) races are grouped by their body mesh's folder (human: Greenlander, Scorchlander;
//! stick_person: the Hive; robot: Skeleton; bone: Shek), Kenshi's own grouping not being found.
//! The Body sliders size the bones (`body_shapes`, F037414), the Face sliders move the head's
//! poses (`appearance`, F021065); RANDOMISE rolls the tab's sliders as Kenshi does (F021064).
//! EXPORT and IMPORT save and read looks (`SavedLook`).
use super::{Game, WorldReady};
use bevy::prelude::*;
use kenshi_data::ItemType;
use kenshi_gui::{Built, GuiClick, GuiEdit, GuiRes, GuiScroll, SetCaption};
use kenshi_render::character::CharacterSpec;
use std::collections::HashMap;

/// Open the editor once the world is ready.
#[derive(Resource)]
pub struct Pending(pub Vec<String>);

#[derive(Clone, Debug)]
struct RaceChoice {
    name: String,
    /// The subraces: race record names.
    subs: Vec<String>,
}

#[derive(Clone, Debug)]
struct Range {
    name: String,
    min: f32,
    max: f32,
    mid: f32,
}

#[derive(Clone, Debug, Default)]
struct Look {
    name: String,
    race: usize,
    sub: usize,
    female: bool,
    hair: usize,
    beard: usize,
    values: HashMap<String, f32>,
}

/// A look saved by EXPORT, read back by IMPORT: names, not indices, so it survives other races'
/// lists. (stand-in) Kenshi's own character files are not ported: these are the rewrite's,
/// in %APPDATA%\kenshi-rewrite\characters, one RON file each named after the character.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
struct SavedLook {
    name: String,
    race: String,
    female: bool,
    hair: Option<String>,
    beard: Option<String>,
    values: HashMap<String, f32>,
}

fn looks_dir() -> std::path::PathBuf {
    kenshi_io::cfg::user_dir().join("characters")
}

/// The saved looks, by file name.
fn saved_looks() -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(looks_dir()).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ron")).collect()).unwrap_or_default();
    v.sort();
    v
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Body,
    Face,
    Hair,
}

#[derive(Resource)]
pub struct Editor {
    ui: Built,
    races: Vec<RaceChoice>,
    chars: Vec<usize>,
    idx: usize,
    looks: Vec<Look>,
    tab: Tab,
    /// The tab's rows: their entities, and each slider's scroll bar and range name.
    rows: Vec<Entity>,
    sliders: Vec<(Entity, String, Option<Entity>)>,
    /// The look changed: rebuild the body.
    rebuild: bool,
    /// The tab's rows need making again.
    refresh_rows: bool,
    /// CLOTHES: the characters shown in what they wear (else bare, to see the body and face).
    clothes: bool,
    /// The character the camera was last turned to face.
    framed: Option<usize>,
    /// The rows' list (moved up as it scrolls), its scroll bar, the rows scrolled past, the
    /// rows' height and how many show at once.
    list: Option<Entity>,
    vbar: Option<Entity>,
    scroll: f32,
    row_h: f32,
    visible: usize,
    /// The stage (its root) and where it stands.
    stage: Option<Entity>,
    stage_at: Option<Vec3>,
    /// The camera's haze before the stage's (put back on leaving).
    saved_fog: Option<DistanceFog>,
    /// Editing a character already in the world (DA_CHARACTER_EDITOR, the plastic surgeon):
    /// (assumption) its race, subrace and gender kept.
    locked: bool,
    /// IMPORT: the saved look it reads next (each press the next one).
    import_next: usize,
}

const NAMES: [&str; 24] = [
    "Beep", "Hamut", "Ruka", "Kral", "Daro", "Seto", "Ishi", "Mara", "Tinfist", "Kana", "Burn", "Ozu", "Shiro",
    "Yuki", "Sadneil", "Ihara", "Ryu", "Moll", "Grim", "Lin", "Tasha", "Bo", "Esata", "Vex",
];

fn races(data: &kenshi_data::GameDataContainer) -> Vec<RaceChoice> {
    let mut groups: Vec<(String, RaceChoice)> = Vec::new();
    for r in data.of_type(ItemType::Race) {
        if !r.bools.get("playable").copied().unwrap_or(false) || r.name.starts_with('-') {
            continue;
        }
        let mesh = r.files.get("male mesh").cloned().unwrap_or_default().replace('\\', "/").to_lowercase();
        let folder = mesh.rsplit('/').nth(1).unwrap_or("").to_string();
        let display = match folder.as_str() {
            "human" => "Human",
            "stick_person" => "Hive",
            "robot" => "Skeleton",
            "bone" => "Shek",
            "fishman" => "Fishman",
            _ => r.name.as_str(),
        }
        .to_string();
        match groups.iter_mut().find(|(f, _)| *f == folder) {
            Some((_, g)) => g.subs.push(r.name.clone()),
            None => groups.push((folder, RaceChoice { name: display, subs: vec![r.name.clone()] })),
        }
    }
    let order = ["Human", "Shek", "Hive", "Skeleton"];
    let mut out: Vec<RaceChoice> = groups.into_iter().map(|(_, g)| g).collect();
    out.sort_by_key(|g| order.iter().position(|o| *o == g.name).unwrap_or(order.len()));
    for g in &mut out {
        // Greenlander before Scorchlander, the plain kinds before the odd ones
        g.subs.sort_by_key(|s| (s.contains("Flayed") as u8, s.clone()));
    }
    out
}

impl Editor {
    fn race_record<'a>(&self, game: &'a Game) -> Option<&'a kenshi_data::GameData> {
        let l = &self.looks[self.idx];
        let name = self.races.get(l.race)?.subs.get(l.sub)?;
        game.sim.data.of_type(ItemType::Race).find(|r| &r.name == name)
    }

    /// The race's editor ranges for the gender, by tab.
    fn ranges(&self, game: &Game, tab: Tab) -> Vec<Range> {
        let Some(r) = self.race_record(game) else { return Vec::new() };
        let Some(file) = r.files.get("editor limits").and_then(|f| game.res.find(f)) else { return Vec::new() };
        let Ok(text) = std::fs::read_to_string(game.root.join(file)) else { return Vec::new() };
        let doc = kenshi_gui::xml::parse(&text);
        let gender = if self.looks[self.idx].female { "Female" } else { "Male" };
        let cat = match tab {
            Tab::Body => "Body",
            Tab::Face => "Face",
            Tab::Hair => return Vec::new(),
        };
        let num = |e: &kenshi_gui::xml::Elem, k: &str| e.attr(k).and_then(|v| v.parse::<f32>().ok());
        doc.descendants()
            .into_iter()
            .filter(|c| c.name == "Config" && c.attr("gender") == Some(gender))
            .flat_map(|c| c.kids("Category").filter(|k| k.attr("name") == Some(cat)).flat_map(|k| k.kids("Range")).collect::<Vec<_>>())
            .filter_map(|r| {
                let (min, max) = (num(r, "min")?, num(r, "max")?);
                Some(Range { name: r.attr("name")?.to_string(), min, max, mid: num(r, "mid").unwrap_or((min + max) * 0.5) })
            })
            .collect()
    }

    fn hairs(&self, game: &Game) -> (Vec<String>, Vec<String>) {
        let Some(r) = self.race_record(game) else { return (Vec::new(), Vec::new()) };
        let all: Vec<String> = r.refs("hairs").iter().filter_map(|(sid, _)| game.sim.data.get(sid)).map(|h| h.name.clone()).collect();
        let beards: Vec<String> = all.iter().filter(|n| n.to_lowercase().contains("beard")).cloned().collect();
        let hair: Vec<String> = all.into_iter().filter(|n| !n.to_lowercase().contains("beard")).collect();
        (hair, beards)
    }

    /// The character's look as a body to load.
    fn spec(&self, game: &Game) -> Option<CharacterSpec> {
        let l = &self.looks[self.idx];
        let race = self.races.get(l.race)?.subs.get(l.sub)?.clone();
        let mut spec = CharacterSpec::new(&race);
        spec.female = l.female;
        let (hair, beards) = self.hairs(game);
        if let Some(h) = hair.get(l.hair) {
            spec.hairs.push(h.clone());
        }
        if !l.female
            && l.beard > 0
            && let Some(b) = beards.get(l.beard - 1)
        {
            spec.hairs.push(b.clone());
        }
        // the clothes the character wears (its inventory's equipped armour), with CLOTHES on
        if self.clothes
            && let Some(&id) = self.chars.get(self.idx)
        {
            spec.wear = game.sim.folk[id]
                .inventory
                .equipped()
                .filter(|i| i.kind == Some(ItemType::Armour))
                .map(|i| i.name.clone())
                .collect();
        }
        Some(spec)
    }
}

fn caption(commands: &mut Commands, ui: &Built, name: &str, text: impl Into<String>) {
    if let Some(e) = ui.get(name) {
        commands.queue(SetCaption(e, text.into()));
    }
}

/// Open the editor over the loaded world.
#[allow(clippy::too_many_arguments)]
pub fn open(
    mut commands: Commands,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut game: ResMut<Game>,
    windows: Query<&Window>,
    _ready: Res<WorldReady>,
    pending: Res<Pending>,
) {
    commands.remove_resource::<Pending>();
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let mut races = races(&game.sim.data);
    // the game start's "force race": only those subraces (and their races) offered
    // (assumption: a forced race the list lacks leaves the list whole)
    let mut forced = races.clone();
    for g in &mut forced {
        g.subs.retain(|s| pending.0.contains(s));
    }
    forced.retain(|g| !g.subs.is_empty());
    if !forced.is_empty() {
        races = forced;
    }
    let chars = game.selected.clone();
    let looks = chars
        .iter()
        .enumerate()
        .map(|(k, _)| Look { name: NAMES[k % NAMES.len()].to_string(), ..Default::default() })
        .collect();
    start(&mut commands, &gui, &assets, &mut game, size, races, chars, looks, false);
}

/// DA_CHARACTER_EDITOR (F040912 0xb: the conversation ends, the editor opens on the one spoken
/// for): the editor on that character as it looks now, its race, subrace and gender kept.
/// (testing) KENSHI_SURGERY=<frame>: at that frame the editor opens on the first selected
/// character as the surgeon's does.
pub fn open_surgery(mut commands: Commands, gui: Res<GuiRes>, assets: Res<AssetServer>, mut game: ResMut<Game>, windows: Query<&Window>, mut frames: Local<u32>) {
    *frames += 1;
    if std::env::var("KENSHI_SURGERY").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames) {
        game.surgery = game.selected.first().copied();
    }
    let Some(id) = game.surgery.take() else { return };
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let races = races(&game.sim.data);
    let race_name = game.sim.race_of(id).and_then(|r| game.sim.data.get(&r)).map(|r| r.name.clone()).unwrap_or_default();
    let (race, sub) = races.iter().enumerate().find_map(|(k, g)| g.subs.iter().position(|s| *s == race_name).map(|j| (k, j))).unwrap_or((0, 0));
    // its sliders as they are (the face ones from its poses where a body file keeps only those)
    let mut values = game.body_values.get(&id).cloned().unwrap_or_default();
    if let Some(r) = game.sim.race_of(id)
        && let Some(l) = super::appearance::limits(&game, &r, game.sim.is_female(id))
    {
        super::appearance::fill_sliders(&l, &mut values);
    }
    let look = Look {
        name: game.sim.folk[id].name.clone(),
        race,
        sub,
        female: game.sim.is_female(id),
        values,
        ..Default::default()
    };
    let worn: Vec<String> = game.looks.get(&id).map(|s| s.hairs.clone()).unwrap_or_default();
    start(&mut commands, &gui, &assets, &mut game, size, races, vec![id], vec![look], true);
    // its hair and beard as they are
    commands.queue(move |w: &mut World| {
        w.resource_scope(|w, mut ed: Mut<Editor>| {
            let game = w.resource::<Game>();
            let (hair, beards) = ed.hairs(game);
            if let Some(k) = worn.iter().find_map(|h| hair.iter().position(|x| x == h)) {
                ed.looks[0].hair = k;
            }
            if let Some(k) = worn.iter().find_map(|h| beards.iter().position(|x| x == h)) {
                ed.looks[0].beard = k + 1;
            }
            if std::env::var("KENSHI_EDITOR_DEBUG").is_ok() {
                eprintln!("editor: surgery on hairs {worn:?}: hair {} of {hair:?}, beard {} of {beards:?}", ed.looks[0].hair, ed.looks[0].beard);
            }
            ed.rebuild = true;
            ed.refresh_rows = true;
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn start(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, game: &mut Game, size: [f32; 2], races: Vec<RaceChoice>, chars: Vec<usize>, looks: Vec<Look>, locked: bool) {
    let defs = gui.0.layout("Kenshi_CharacterEditor.layout");
    let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(80));
    }
    for (name, text) in [
        ("lbRace", "Race"),
        ("lbSubRace", "Subrace"),
        ("lbGender", "Gender"),
        ("lbChangeCharacter", "Character"),
        ("lbRaceDescription", "Race Description"),
        ("lbRaceStats", "Race Stats"),
        ("BodyTabButton", "BODY"),
        ("FaceTabButton", "FACE"),
        ("HairTabButton", "HAIR"),
        ("RandomiseButton", "RANDOMISE"),
        ("RandomiseAllButton", "RANDOMISE ALL"),
        ("ResetButton", "RESET"),
        ("ConfirmButton", "CONFIRM"),
        ("ImportButton", "IMPORT"),
        ("ExportButton", "EXPORT"),
        ("ClothesButton", "CLOTHES"),
    ] {
        caption(commands, &ui, name, text);
    }
    // a character in the world keeps its race and gender
    if locked {
        for b in ["RaceLeftButton", "RaceRightButton", "SubRaceLeftButton", "SubRaceRightButton", "GenderLeftButton", "GenderRightButton"] {
            if let Some(e) = ui.get(b) {
                commands.entity(e).insert(kenshi_gui::ui::Disabled);
            }
        }
    }
    game.paused = true;
    commands.insert_resource(Editor {
        ui,
        races,
        chars,
        idx: 0,
        looks,
        tab: Tab::Body,
        rows: Vec::new(),
        sliders: Vec::new(),
        rebuild: true,
        refresh_rows: true,
        clothes: false,
        framed: None,
        list: None,
        vbar: None,
        scroll: 0.0,
        row_h: 55.0,
        visible: 0,
        stage: None,
        stage_at: None,
        saved_fog: None,
        locked,
        import_next: 0,
    });
}

/// The current character's Body and Face values (every range, its middle where unset) onto its
/// body: its bones are sized from them (`body_shapes`).
fn sync_body(ed: &Editor, game: &mut Game) {
    let Some(&id) = ed.chars.get(ed.idx) else { return };
    let mids: Vec<(String, f32)> = [Tab::Body, Tab::Face].iter().flat_map(|&t| ed.ranges(game, t)).map(|r| (r.name, r.mid)).collect();
    let mut values = kenshi_render::body_shape::with_defaults(&ed.looks[ed.idx].values, &mids);
    // the face sliders as the head's poses (F021065)
    if let Some(r) = ed.race_record(game)
        && let Some(l) = super::appearance::limits(game, &r.string_id, ed.looks[ed.idx].female)
    {
        for s in &l.face {
            let v = values.get(&s.name).copied().unwrap_or(s.mid);
            super::appearance::put_poses(s, v, &mut values);
        }
    }
    if std::env::var("KENSHI_EDITOR_DEBUG").is_ok() {
        let mut l: Vec<_> = ed.looks[ed.idx].values.iter().collect();
        l.sort_by(|a, b| a.0.cmp(b.0));
        eprintln!("editor: sync {l:?}");
    }
    game.body_values.insert(id, values);
}

/// The panels' texts for the current character.
fn texts(commands: &mut Commands, ed: &Editor, game: &Game) {
    let l = &ed.looks[ed.idx];
    let race = ed.races.get(l.race);
    caption(commands, &ed.ui, "RaceText", race.map_or("", |r| r.name.as_str()));
    caption(commands, &ed.ui, "SubRaceText", race.and_then(|r| r.subs.get(l.sub)).map_or("", |s| s.as_str()));
    caption(commands, &ed.ui, "GenderText", if l.female { "Female" } else { "Male" });
    caption(commands, &ed.ui, "CharIndexText", format!("{}/{}", ed.idx + 1, ed.chars.len()));
    caption(commands, &ed.ui, "NameText", l.name.clone());
    if let Some(r) = ed.race_record(game) {
        caption(commands, &ed.ui, "DescriptionPanelText", r.strings.get("description").cloned().unwrap_or_default());
        if let Some(p) = ed.ui.get("StatsPanelText") {
            commands.queue(SetRows(p, race_stats(r, ed.chars.get(ed.idx).map(|&id| &game.sim.arena.f[id].medical))));
        }
    }
}

/// The race stats panel's rows (F039457): "Hit Points:" and the character's health, "No
/// bleeding" for a race that does not bleed, then each skill the race is good or bad at with its
/// change ("{1,num}%").
/// (assumption) Kenshi's race skill multipliers are taken as "stats good" / "stats bad" at 1.2
/// and 0.8 (the +20% its editor shows), listed in the record's order, good then bad; the health
/// is the most a body part of the character takes.
fn race_stats(r: &kenshi_data::GameData, medical: Option<&kenshi_char::medical::Medical>) -> Vec<(String, String, Color)> {
    let hp = medical.map_or(100.0, |m| m.parts.iter().map(|p| p.max_health()).fold(0.0, f32::max));
    let mut rows = vec![("Hit Points:".to_string(), format!("{}", (hp + 0.5) as i32), STAT_TEXT)];
    if r.floats.get("bleed rate").copied().unwrap_or(1.0) <= 0.0 {
        rows.push(("No bleeding".to_string(), String::new(), STAT_GOOD));
    }
    for (key, mult) in [("stats good", 1.2f32), ("stats bad", 0.8)] {
        for k in 0..8 {
            let Some(&id) = r.ints.get(&format!("{key}{k}")) else { break };
            let Some(n) = super::stats_ui::stat_name(id) else { continue };
            let pct = ((mult - 1.0) * 100.0).round() as i32;
            let (v, c) = if pct > 0 { (format!("+{pct}%"), STAT_GOOD) } else { (format!("{pct}%"), STAT_BAD) };
            rows.push((format!("{n}"), v, c));
        }
    }
    rows
}

/// The stats panel's colours (assumption: Kenshi's colour globals are not read yet).
const STAT_TEXT: Color = Color::srgb(0.78, 0.73, 0.62);
const STAT_GOOD: Color = Color::srgb(0.62, 0.78, 0.52);
const STAT_BAD: Color = Color::srgb(0.85, 0.42, 0.36);

/// A plain panel's rows: a label at its left and a value in a column at its right (made again
/// each time).
struct SetRows(Entity, Vec<(String, String, Color)>);

impl Command for SetRows {
    type Out = ();
    fn apply(self, world: &mut World) {
        if let Some(old) = world.get::<Children>(self.0).map(|c| c.iter().collect::<Vec<_>>()) {
            for c in old {
                world.entity_mut(c).despawn();
            }
        }
        let font = world.get_resource::<GuiRes>().and_then(|g| g.0.fonts.get("Kenshi_StandardFont_Medium").map(|f| (f.file.clone(), f.screen_px())));
        let font = font.map(|(file, px)| (world.resource::<AssetServer>().load::<Font>(file), px));
        let tf = || {
            let mut tf = TextFont { font_size: bevy::text::FontSize::Px(font.as_ref().map_or(15.0, |f| f.1)), ..default() };
            if let Some((h, _)) = &font {
                tf.font = bevy::text::FontSource::Handle(h.clone());
            }
            tf
        };
        let col = world
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                ..default()
            })
            .id();
        for (label, value, colour) in self.1 {
            let a = world.spawn((Node { width: Val::Percent(68.0), ..default() }, Text::new(label), tf(), TextColor(STAT_TEXT))).id();
            let b = world.spawn((Node { width: Val::Percent(32.0), ..default() }, Text::new(value), tf(), TextColor(colour))).id();
            let row = world.spawn(Node { flex_direction: FlexDirection::Row, width: Val::Percent(100.0), ..default() }).add_children(&[a, b]).id();
            world.entity_mut(col).add_child(row);
        }
        world.entity_mut(self.0).add_child(col);
    }
}

/// The current tab's rows: a Kenshi_Slider per range (Hair: hair and beard pickers).
fn rows(commands: &mut Commands, ed: &mut Editor, game: &Game, gui: &kenshi_gui::Gui, assets: &AssetServer, sizes: &Query<&ComputedNode>) {
    for r in ed.rows.drain(..) {
        commands.entity(r).despawn();
    }
    ed.sliders.clear();
    let _ = sizes;
    if let Some(l) = ed.list.take() {
        commands.entity(l).despawn();
    }
    if let Some(b) = ed.vbar.take() {
        commands.entity(b).despawn();
    }
    let Some(content) = ed.ui.get("TabsPanelContent") else { return };
    // the content's size as the layout made it (the window's pixels)
    let [full_w, full_h] = ed.ui.sizes.get(&content).copied().unwrap_or([400.0, 600.0]);
    // the rows: Kenshi_Slider's 55 high (the layouts' 1080 lines, scaled), down the content
    // beside a Kenshi_ScrollBarV (17 wide)
    let scale = kenshi_gui::res::ui_scale();
    let row_h = 55.0 * scale;
    let bar_w = 17.0 * scale;
    let width = full_w - bar_w - 4.0 * scale;
    commands.queue(move |w: &mut World| {
        if let Some(mut n) = w.get_mut::<Node>(content) {
            n.overflow = Overflow::clip();
        }
    });
    let entries: Vec<(String, f32, f32, f32)> = match ed.tab {
        Tab::Hair => {
            let (hair, beards) = ed.hairs(game);
            let l = &ed.looks[ed.idx];
            let mut v = vec![("Hair".to_string(), 0.0, (hair.len().max(1) - 1) as f32, l.hair as f32)];
            if !l.female {
                v.push(("Beard".to_string(), 0.0, beards.len() as f32, l.beard as f32));
            }
            v
        }
        tab => ed
            .ranges(game, tab)
            .into_iter()
            .map(|r| {
                let v = ed.looks[ed.idx].values.get(&r.name).copied().unwrap_or(r.mid);
                (r.name, r.min, r.max, v)
            })
            .collect(),
    };
    if std::env::var("KENSHI_EDITOR_DEBUG").is_ok() {
        eprintln!("editor: rows {:?}: {:?}", ed.tab, entries);
    }
    let n = entries.len();
    ed.row_h = row_h;
    ed.visible = ((full_h / row_h).floor() as usize).max(1);
    let max_scroll = n.saturating_sub(ed.visible) as f32;
    ed.scroll = ed.scroll.clamp(0.0, max_scroll);
    let list = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(-ed.scroll * row_h),
            width: Val::Px(width),
            height: Val::Px(n as f32 * row_h),
            ..default()
        })
        .id();
    commands.entity(content).add_child(list);
    ed.list = Some(list);
    // the scroll bar, its range the rows that do not fit
    let bar = kenshi_gui::WidgetDef {
        kind: "ScrollBar".into(),
        skin: "Kenshi_ScrollBarV".into(),
        pos: kenshi_gui::res::Pos::Real([1.0 - bar_w / full_w, 0.0, bar_w / full_w, 1.0]),
        align: Default::default(),
        name: "EditorRowsBar".into(),
        layer: None,
        props: vec![("Range".into(), format!("{max_scroll}"))],
        user: Vec::new(),
        children: Vec::new(),
    };
    let built = kenshi_gui::ui::spawn(commands, gui, assets, &[bar], Some(content), [full_w, full_h]);
    if let Some(b) = built.get("EditorRowsBar") {
        commands.queue(kenshi_gui::ui::SetScroll(b, ed.scroll));
        if max_scroll <= 0.0 {
            commands.queue(kenshi_gui::ui::SetVisible(b, false));
        }
        ed.vbar = Some(b);
    }
    for (k, (name, min, max, value)) in entries.into_iter().enumerate() {
        let holder = commands
            .spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(k as f32 * row_h),
                width: Val::Px(width),
                height: Val::Px(row_h),
                ..default()
            })
            .id();
        commands.entity(list).add_child(holder);
        let mut def = gui.templates.get("Kenshi_Slider").cloned().unwrap_or_else(|| kenshi_gui::WidgetDef {
            kind: "Widget".into(),
            skin: "PanelEmpty".into(),
            pos: kenshi_gui::res::Pos::Real([0.0, 0.0, 1.0, 1.0]),
            align: Default::default(),
            name: String::new(),
            layer: None,
            props: Vec::new(),
            user: Vec::new(),
            children: Vec::new(),
        });
        // the template at its own size (455 x 55), stretched to the row
        let design = match def.pos {
            kenshi_gui::res::Pos::Px(p) => [p[2], p[3]],
            _ => [width, row_h],
        };
        def.pos = kenshi_gui::res::Pos::Px([0.0, 0.0, design[0], design[1]]);
        def.align = kenshi_gui::res::Align::parse("Stretch");
        let range = (max - min).round().max(1.0);
        // the slider's own range, the row's label and number
        fn set(d: &mut kenshi_gui::WidgetDef, name: &str, key: &str, value: &str) {
            if d.name == name {
                d.props.retain(|(k, _)| k != key);
                d.props.push((key.to_string(), value.to_string()));
            }
            for c in &mut d.children {
                set(c, name, key, value);
            }
        }
        set(&mut def, "ElementText", "Caption", &name);
        set(&mut def, "NumberText", "Caption", &format!("{value:.0}"));
        set(&mut def, "Slider", "Range", &format!("{range}"));
        let built = kenshi_gui::ui::spawn_design(commands, gui, assets, &[def], Some(holder), design, [width, row_h]);
        if let Some(bar) = built.get("Slider") {
            commands.queue(kenshi_gui::ui::SetScroll(bar, value - min));
            ed.sliders.push((bar, name.clone(), built.get("NumberText")));
        }
        ed.rows.push(holder);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut ed: ResMut<Editor>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut clicks: MessageReader<GuiClick>,
    mut scrolls: MessageReader<GuiScroll>,
    mut edits: MessageReader<GuiEdit>,
    sizes: Query<&ComputedNode>,
    time: Res<Time>,
    mut bodies: Query<&mut Transform>,
    windows: Query<&Window>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    (mut meshes, mut objects, mut standard, mut look): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<kenshi_render::object_material::KenshiObjectMaterial>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<kenshi_render::terrain_material::TerrainLook>,
    ),
) {
    let mut changed = false;
    let mut rng = (time.elapsed_secs_f64() * 1e6) as u64 | 1;
    // (testing) KENSHI_EDITOR_SET="Height=120;Arm bulk=145": those values on the first character
    if let Ok(v) = std::env::var("KENSHI_EDITOR_SET")
        && !ed.looks.is_empty()
        && !ed.looks[0].values.contains_key("__set")
    {
        for kv in v.split(';') {
            if let Some((k, x)) = kv.split_once('=')
                && let Ok(x) = x.trim().parse::<f32>()
            {
                ed.looks[0].values.insert(k.trim().to_string(), x);
            }
        }
        ed.looks[0].values.insert("__set".to_string(), 1.0);
        ed.refresh_rows = true;
    }
    for e in edits.read() {
        if Some(e.widget) == ed.ui.get("NameText") {
            let i = ed.idx;
            ed.looks[i].name = e.text.clone();
        }
    }
    // the rows' list scrolled: by its bar, or the wheel over the right panel
    let mut scroll_to = None;
    let over_rows = windows.iter().next().and_then(|w| Some(w.cursor_position()?.x > w.width() * 0.7)).unwrap_or(false);
    let notches: f32 = wheel.read().map(|w| w.y.signum()).sum();
    if over_rows && notches != 0.0 {
        scroll_to = Some(ed.scroll - notches);
        if let Some(b) = ed.vbar {
            commands.queue(kenshi_gui::ui::SetScroll(b, ed.scroll - notches));
        }
    }
    for s in scrolls.read() {
        if Some(s.widget) == ed.vbar {
            scroll_to = Some(s.pos);
            continue;
        }
        let Some((_, name, number)) = ed.sliders.iter().find(|(b, _, _)| *b == s.widget).cloned() else { continue };
        if std::env::var("KENSHI_EDITOR_DEBUG").is_ok() {
            eprintln!("editor: scroll {name} to {}", s.pos);
        }
        let i = ed.idx;
        match (ed.tab, name.as_str()) {
            (Tab::Hair, "Hair") => {
                ed.looks[i].hair = s.pos.round() as usize;
                ed.rebuild = true;
            }
            (Tab::Hair, "Beard") => {
                ed.looks[i].beard = s.pos.round() as usize;
                ed.rebuild = true;
            }
            (tab, _) => {
                let min = ed.ranges(&game, tab).iter().find(|r| r.name == name).map_or(0.0, |r| r.min);
                ed.looks[i].values.insert(name.clone(), min + s.pos);
                // the number beside the slider, and the body (its bones, F037414)
                if let Some(n) = number {
                    commands.queue(SetCaption(n, format!("{:.0}", min + s.pos)));
                }
                sync_body(&ed, &mut game);
            }
        }
    }
    for c in clicks.read() {
        let i = ed.idx;
        let n_races = ed.races.len().max(1);
        let n_subs = ed.races.get(ed.looks[i].race).map_or(1, |r| r.subs.len().max(1));
        let step = |v: usize, n: usize, d: i32| ((v as i32 + d).rem_euclid(n as i32)) as usize;
        match c.name.as_str() {
            "RaceLeftButton" | "RaceRightButton" | "SubRaceLeftButton" | "SubRaceRightButton" | "GenderLeftButton" | "GenderRightButton" if ed.locked => continue,
            "RaceLeftButton" | "RaceRightButton" => {
                let d = if c.name == "RaceLeftButton" { -1 } else { 1 };
                ed.looks[i].race = step(ed.looks[i].race, n_races, d);
                ed.looks[i].sub = 0;
                ed.looks[i].hair = 0;
                ed.looks[i].beard = 0;
                ed.rebuild = true;
                ed.refresh_rows = true;
            }
            "SubRaceLeftButton" | "SubRaceRightButton" => {
                let d = if c.name == "SubRaceLeftButton" { -1 } else { 1 };
                ed.looks[i].sub = step(ed.looks[i].sub, n_subs, d);
                ed.rebuild = true;
                ed.refresh_rows = true;
            }
            "GenderLeftButton" | "GenderRightButton" => {
                let single = ed.race_record(&game).and_then(|r| r.bools.get("single gender").copied()).unwrap_or(false);
                if !single {
                    ed.looks[i].female = !ed.looks[i].female;
                    ed.looks[i].hair = 0;
                    ed.looks[i].beard = 0;
                    ed.rebuild = true;
                    ed.refresh_rows = true;
                }
            }
            "CharLeftArrow" | "CharRightArrow" if !ed.chars.is_empty() => {
                let d = if c.name == "CharLeftArrow" { -1 } else { 1 };
                ed.idx = step(ed.idx, ed.chars.len(), d);
                ed.refresh_rows = true;
                let id = ed.chars[ed.idx];
                game.focus = Vec3::from(game.sim.folk[id].mover.position_units()) - game.zone.origin;
            }
            "ClothesButton" => {
                ed.clothes = !ed.clothes;
                ed.rebuild = true;
            }
            "BodyTabButton" => {
                ed.tab = Tab::Body;
                ed.refresh_rows = true;
            }
            "FaceTabButton" => {
                ed.tab = Tab::Face;
                ed.refresh_rows = true;
            }
            "HairTabButton" => {
                ed.tab = Tab::Hair;
                ed.refresh_rows = true;
            }
            "RandomiseButton" | "RandomiseAllButton" => {
                let tabs: &[Tab] = if c.name == "RandomiseAllButton" { &[Tab::Body, Tab::Face] } else { std::slice::from_ref(&ed.tab) };
                let tabs = tabs.to_vec();
                // F039463 / F039464: the tab's sliders rolled as Kenshi rolls them (F021064)
                let deviation = super::appearance::deviation(&game);
                let limits = ed.race_record(&game).and_then(|r| super::appearance::limits(&game, &r.string_id, ed.looks[i].female));
                let mut r = || {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    (rng >> 40) as f32 / (1u64 << 24) as f32
                };
                for tab in tabs {
                    let sliders = match (&limits, tab) {
                        (Some(l), Tab::Body) => l.body.clone(),
                        (Some(l), Tab::Face) => l.face.clone(),
                        _ => Vec::new(),
                    };
                    super::appearance::roll(&sliders, deviation, &mut r, &mut ed.looks[i].values);
                }
                if c.name == "RandomiseAllButton" || ed.tab == Tab::Hair {
                    let (hair, beards) = ed.hairs(&game);
                    ed.looks[i].hair = (rng as usize) % hair.len().max(1);
                    ed.looks[i].beard = ((rng >> 8) as usize) % (beards.len() + 1);
                    ed.rebuild = true;
                }
                ed.refresh_rows = true;
            }
            // EXPORT: the look saved under the character's name
            "ExportButton" => {
                let l = ed.looks[i].clone();
                let (hair, beards) = ed.hairs(&game);
                let saved = SavedLook {
                    name: l.name.clone(),
                    race: ed.races.get(l.race).and_then(|g| g.subs.get(l.sub)).cloned().unwrap_or_default(),
                    female: l.female,
                    hair: hair.get(l.hair).cloned(),
                    beard: l.beard.checked_sub(1).and_then(|b| beards.get(b)).cloned(),
                    values: l.values.clone(),
                };
                let file: String = l.name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
                let path = looks_dir().join(format!("{}.ron", if file.trim().is_empty() { "character" } else { file.trim() }));
                let ok = std::fs::create_dir_all(looks_dir()).is_ok() && ron::ser::to_string_pretty(&saved, Default::default()).ok().is_some_and(|t| std::fs::write(&path, t).is_ok());
                eprintln!("editor: exported {} to {} ({ok})", l.name, path.display());
                continue;
            }
            // IMPORT: the next saved look (its race and gender only where they may change)
            "ImportButton" => {
                let files = saved_looks();
                if files.is_empty() {
                    continue;
                }
                let path = &files[ed.import_next % files.len()];
                ed.import_next += 1;
                let Some(saved) = std::fs::read_to_string(path).ok().and_then(|t| ron::from_str::<SavedLook>(&t).ok()) else { continue };
                if !ed.locked
                    && let Some((r, sub)) = ed.races.iter().enumerate().find_map(|(k, g)| g.subs.iter().position(|x| *x == saved.race).map(|j| (k, j)))
                {
                    ed.looks[i].race = r;
                    ed.looks[i].sub = sub;
                    ed.looks[i].female = saved.female;
                }
                ed.looks[i].name = saved.name.clone();
                ed.looks[i].values = saved.values.clone();
                let (hair, beards) = ed.hairs(&game);
                ed.looks[i].hair = saved.hair.as_ref().and_then(|h| hair.iter().position(|x| x == h)).unwrap_or(0);
                ed.looks[i].beard = saved.beard.as_ref().and_then(|b| beards.iter().position(|x| x == b)).map_or(0, |b| b + 1);
                eprintln!("editor: imported {}", path.display());
                ed.rebuild = true;
                ed.refresh_rows = true;
            }
            "ResetButton" => {
                ed.looks[i].values.clear();
                ed.looks[i].hair = 0;
                ed.looks[i].beard = 0;
                ed.rebuild = true;
                ed.refresh_rows = true;
            }
            "ConfirmButton" => {
                // names onto the characters; the game starts
                // each in their clothes again
                ed.clothes = true;
                for (k, &id) in ed.chars.clone().iter().enumerate() {
                    game.sim.folk[id].name = ed.looks[k].name.clone();
                    ed.idx = k;
                    if let Some(spec) = ed.spec(&game) {
                        game.looks.insert(id, spec.clone());
                        game.sim.folk[id].female = Some(spec.female);
                        let race_id = game.sim.data.of_type(kenshi_data::ItemType::Race).find(|r| r.name == spec.race).map(|r| r.string_id.clone());
                game.sim.folk[id].look_race = race_id;
                        game.rebody.push((id, spec));
                    }
                }
                for r in &ed.ui.roots {
                    commands.entity(*r).despawn();
                }
                if let Some(s) = ed.stage {
                    commands.entity(s).despawn();
                }
                if let Some(old) = ed.saved_fog.take() {
                    commands.queue(move |w: &mut World| {
                        let mut q = w.query::<&mut DistanceFog>();
                        for mut f in q.iter_mut(w) {
                            *f = old.clone();
                        }
                    });
                }
                game.stage = None;
                // the camera back on the squad where they stand
                if let Some(&id) = ed.chars.first() {
                    game.focus = Vec3::from(game.sim.folk[id].mover.position_units()) - game.zone.origin;
                }
                game.paused = false;
                game.dist = super::ZOOM_START;
                game.pitch = super::PITCH_START;
                commands.remove_resource::<Editor>();
                return;
            }
            _ => continue,
        }
        changed = true;
    }
    if let Some(to) = scroll_to {
        let rows = ed.sliders.len().max(if ed.tab == Tab::Hair { 2 } else { 0 });
        ed.scroll = to.clamp(0.0, rows.saturating_sub(ed.visible) as f32);
        if let Some(l) = ed.list {
            let top = -ed.scroll * ed.row_h;
            commands.queue(move |w: &mut World| {
                if let Some(mut n) = w.get_mut::<Node>(l) {
                    n.top = Val::Px(top);
                }
            });
        }
    }
    if ed.rebuild {
        ed.rebuild = false;
        if let (Some(spec), Some(&id)) = (ed.spec(&game), ed.chars.get(ed.idx)) {
            game.looks.insert(id, spec.clone());
            game.sim.folk[id].female = Some(spec.female);
            let race_id = game.sim.data.of_type(kenshi_data::ItemType::Race).find(|r| r.name == spec.race).map(|r| r.string_id.clone());
                game.sim.folk[id].look_race = race_id;
            game.rebody.push((id, spec));
        }
        changed = true;
    }
    if ed.refresh_rows {
        ed.refresh_rows = false;
        let ed = &mut *ed;
        rows(&mut commands, ed, &game, &gui.0, &assets, &sizes);
        changed = true;
    }
    if changed {
        sync_body(&ed, &mut game);
        texts(&mut commands, &ed, &game);
    }
    // the character being made on the editor's stage (F039470, F039441): Kenshi's editor
    // platform 3000 below where the camera was, the character on it facing the camera; the
    // camera free to turn (middle mouse) and zoom within reach of them
    if let Some(&id) = ed.chars.get(ed.idx) {
        if ed.stage_at.is_none() {
            let at = Vec3::from(game.sim.folk[id].mover.position_units()) - game.zone.origin;
            // F039441 stages it at the height -3000, under the world, where Kenshi draws its sandy
            // ground; (stand-in) here it stands as far above the ground instead, on its own sand
            // floor, the world's underside and shadow not over it
            let floor = game.ground(at.x, at.z) + STAGE_HEIGHT;
            if std::env::var("KENSHI_EDITOR_DEBUG").is_ok() {
                eprintln!("editor: stage at {:.0} {floor:.0} {:.0} (origin {:.0}, ground {:.0})", at.x, at.z, game.zone.origin, game.ground(at.x, at.z));
            }
            ed.stage_at = Some(Vec3::new(at.x, floor, at.z));
            if let Some(stage) = ed.stage_at {
                ed.stage = Some(spawn_stage(&mut commands, &game, &assets, &mut meshes, &mut objects, &mut standard, &mut look, stage));
                // the stage's haze over the far sand
                commands.queue(|w: &mut World| {
                    let mut q = w.query::<&mut DistanceFog>();
                    let mut old = None;
                    for mut f in q.iter_mut(w) {
                        old.get_or_insert_with(|| f.clone());
                        *f = DistanceFog { color: STAGE_FOG, falloff: FogFalloff::Linear { start: 90.0, end: 900.0 }, ..default() };
                    }
                    if let Some(mut ed) = w.get_resource_mut::<Editor>() {
                        ed.saved_fog = old;
                    }
                });
            }
        }
        let stage = ed.stage_at.unwrap_or_default();
        game.stage = Some((id, stage));
        if std::env::var("KENSHI_EDITOR_DEBUG").is_ok()
            && let Some(t) = game.bodies.get(id).and_then(|&b| bodies.get(b).ok())
        {
            eprintln!("editor: body of {id} at {:.1} scale {:.2} (stage {stage:.1})", t.translation, t.scale);
        }
        if ed.framed != Some(id) {
            let first = ed.framed.is_none();
            ed.framed = Some(id);
            if first {
                game.dist = FRAME_DIST;
                game.pitch = FRAME_PITCH;
                game.yaw = 0.0;
            }
            // the character turned to face the camera
            let (s, c) = game.yaw.sin_cos();
            game.sim.folk[id].facing = [s, 0.0, c];
            if let Some(mut t) = game.bodies.get(id).and_then(|&b| bodies.get_mut(b).ok()) {
                t.rotation = Quat::from_rotation_y(game.yaw);
            }
        }
        game.dist = game.dist.clamp(15.0, 70.0);
        // the character in the middle of the room the panels leave (Kenshi's editor camera sits
        // to one side of them, F039441): the camera's centre moved to their right
        let aspect = windows.iter().next().map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
        let gap_mid = (NAME_PANEL_W + BODY_PANEL_X) * 0.5;
        let off = (1.0 - 2.0 * gap_mid) * (std::f32::consts::FRAC_PI_4 * 0.5).tan() * aspect * game.dist;
        let (s, c) = game.yaw.sin_cos();
        game.focus = stage + Vec3::new(c, 0.0, -s) * off;
    }
}

/// F039441: the editor's stage 3000 from the world's ground (Kenshi's below it, here above).
const STAGE_HEIGHT: f32 = 3000.0;
/// (stand-in) The stage's floor: Kenshi's desert ground texture ("DesertDarkStones", the sand and
/// dark stones its editor shows), tiled every FLOOR_TILE across FLOOR_SIZE, at the platform's
/// feet (its mesh reaches 26.6 down).
const FLOOR_TEXTURE: &str = "data/newland/land/textures/DesertDarkStones_DIF.dds";
const FLOOR_NORMALS: &str = "data/newland/land/textures/DesertDarkStones_NML.dds";
const FLOOR_TILE: f32 = 40.0;
const FLOOR_SIZE: f32 = 40000.0;
const FLOOR_DROP: f32 = 26.0;
/// (assumption) The haze over the stage's sand, as Kenshi's editor shows it.
const STAGE_FOG: Color = Color::srgb(0.66, 0.71, 0.66);
/// BUILDING_PART "character_editor_platform_part" (F039470): the platform the characters stand on.
const PLATFORM: &str = "5370-platform_otto.mod";

/// The stage: the platform part's meshes, turned so its front faces the camera's first side.
fn spawn_stage(
    commands: &mut Commands,
    game: &Game,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    objects: &mut Assets<kenshi_render::object_material::KenshiObjectMaterial>,
    standard: &mut Assets<StandardMaterial>,
    look: &mut kenshi_render::terrain_material::TerrainLook,
    at: Vec3,
) -> Entity {
    let root = commands.spawn((Transform::from_translation(at).with_rotation(Quat::from_rotation_y(std::f32::consts::PI)), Visibility::default())).id();
    // the floor
    let mut plane = Plane3d::default().mesh().size(FLOOR_SIZE, FLOOR_SIZE).build();
    if let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) = plane.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        let k = FLOOR_SIZE / FLOOR_TILE;
        for t in uv.iter_mut() {
            *t = [t[0] * k, t[1] * k];
        }
    }
    let _ = plane.generate_tangents();
    let floor = standard.add(StandardMaterial {
        base_color_texture: Some(assets.load(FLOOR_TEXTURE)),
        normal_map_texture: Some(assets.load(FLOOR_NORMALS)),
        // (assumption) the grey-green the editor's sand shows in Kenshi
        base_color: Color::srgb(0.84, 0.9, 0.86),
        perceptual_roughness: 1.0,
        reflectance: 0.1,
        ..default()
    });
    let f = commands.spawn((Mesh3d(meshes.add(plane)), MeshMaterial3d(floor), Transform::from_xyz(0.0, -FLOOR_DROP, 0.0))).id();
    commands.entity(root).add_child(f);
    let Some(r) = game.sim.data.get(PLATFORM) else { return root };
    let mat = r.refs("material").first().and_then(|(sid, _)| game.sim.data.get(sid));
    let o = kenshi_render::object_material::ObjectLook::from_record(mat, |f| game.res.find(f)).for_building();
    let Some(file) = r.files.get("phs or mesh").and_then(|f| game.res.find(f)) else { return root };
    let Ok(mesh) = kenshi_io::ogre_mesh::read(&game.root.join(&file)) else { return root };
    let origin = game.zone.origin;
    let ground = look.ground_colour(at.x + origin.x, at.z + origin.z);
    for (_, m) in kenshi_render::to_bevy(&mesh) {
        if std::env::var("KENSHI_EDITOR_DEBUG").is_ok()
            && let Some(a) = bevy::camera::primitives::MeshAabb::compute_aabb(&m)
        {
            eprintln!("editor: platform mesh {:?} .. {:?}", a.min(), a.max());
        }
        let colours = m.attribute(Mesh::ATTRIBUTE_COLOR).is_some();
        let h = objects.add(look.object(assets, &o, origin, colours, ground));
        let e = commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(h), Transform::default())).id();
        commands.entity(root).add_child(e);
    }
    root
}

/// The editor camera: (assumption, from Kenshi's editor as seen) the whole character, a little
/// above level, in front of them.
const FRAME_DIST: f32 = 30.0;
const FRAME_PITCH: f32 = 0.38;
/// Kenshi_CharacterEditor.layout: NamePanel's width and Root's left (screen fractions).
const NAME_PANEL_W: f32 = 0.161458;
const BODY_PANEL_X: f32 = 0.7;


/// (testing) KENSHI_EDITOR_TYPE="frame@text": that many frames after the editor opens, its name
/// box is clicked into (as a click does: its text kept) and `text` typed (key presses, as the
/// keyboard sends them; each '<' a Backspace).
pub fn type_script(
    mut frames: Local<u32>,
    ed: Option<Res<Editor>>,
    mut boxes: Query<&mut kenshi_gui::EditText>,
    windows: Query<Entity, With<Window>>,
    mut keys: MessageWriter<bevy::input::keyboard::KeyboardInput>,
) {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};
    let Some(ed) = ed else { return };
    let Ok(v) = std::env::var("KENSHI_EDITOR_TYPE") else { return };
    let Some((at, text)) = v.split_once('@') else { return };
    *frames += 1;
    if at.parse::<u32>().ok() != Some(*frames) {
        return;
    }
    let (Some(name), Ok(window)) = (ed.ui.get("NameText"), windows.single()) else { return };
    if let Ok(mut t) = boxes.get_mut(name) {
        t.focused = true;
    }
    for c in text.chars() {
        let s: bevy::platform::prelude::String = c.to_string();
        let back = c == '<';
        keys.write(KeyboardInput {
            key_code: if back { KeyCode::Backspace } else { KeyCode::KeyA },
            logical_key: if back { Key::Backspace } else { Key::Character(s.clone().into()) },
            state: ButtonState::Pressed,
            text: (!back).then(|| s.into()),
            repeat: false,
            window,
        });
    }
}
