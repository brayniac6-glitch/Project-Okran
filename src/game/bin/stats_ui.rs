//! The character stats window (Kenshi_StatsWindow.layout, "toggle_stats": C, controls.cfg): the selected
//! character's attributes, its skills in four columns, and derived statistics, each skill with
//! its level and a bar of the way to the next. The layout's panels are datapanels the game fills
//! (F047305); the stats' names and their groups (Weapons, Combat, Ranged, Thievery, Athletics,
//! Sciences, Trades; derived: Primary weapon, Secondary weapon, Turrets, General) are the game's
//! table (F047300). (stand-in) Which group a stat is in and the column each group goes to follow
//! the game's screen as players know it; the description panel is not filled; derived figures
//! the rewrite has no system for are left out.
use super::Game;
use bevy::prelude::*;
use kenshi_char::StatsEnumerated::{self, *};
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption};

#[derive(Resource, Default)]
pub struct StatsWindow {
    open: Option<Open>,
}

impl StatsWindow {
    /// Whether the stats window is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

struct Open {
    person: usize,
    ui: Built,
    /// The rows made: (stat, the level text, the bar's fill).
    rows: Vec<(StatsEnumerated, Entity, Entity)>,
    /// The derived figures' value texts, in DERIVED's order.
    derived: Vec<Entity>,
    last: f32,
}

const ATTRIBUTES: [(StatsEnumerated, &str); 4] =
    [(StatStrength, "Strength"), (StatToughness, "Toughness"), (StatDexterity, "Dexterity"), (StatPerception, "Perception")];

/// A heading and its stats (with their names).
type Group = (&'static str, &'static [(StatsEnumerated, &'static str)]);

/// The skill columns (Skills1Panel .. Skills4Panel): headings and their stats.
const COLUMNS: [&[Group]; 4] = [
    &[
        ("COMBAT", &[(StatMeleeAttack, "Melee attack"), (StatMeleeDefence, "Melee defence"), (StatDodge, "Dodge"), (StatMartialarts, "Martial arts")]),
        ("RANGED", &[(StatCrossbows, "Crossbows"), (StatTurrets, "Turrets"), (StatFriendlyFire, "Precision shooting")]),
    ],
    &[(
        "WEAPONS",
        &[
            (StatKatanas, "Katanas"),
            (StatSabres, "Sabres"),
            (StatHackers, "Hackers"),
            (StatHeavyweapons, "Heavy weapons"),
            (StatBlunt, "Blunt"),
            (StatPolearms, "Polearms"),
        ],
    )],
    &[
        ("THIEVERY", &[(StatStealth, "Stealth"), (StatLockpicking, "Lockpicking"), (StatThieving, "Thievery"), (StatAssassination, "Assassination")]),
        ("ATHLETICS", &[(StatAthletics, "Athletics"), (StatSwimming, "Swimming"), (StatSurvival, "Survival")]),
    ],
    &[
        ("SCIENCES", &[(StatMedic, "Field medic"), (StatEngineering, "Engineer"), (StatRobotics, "Robotics"), (StatScience, "Science")]),
        (
            "TRADES",
            &[
                (StatSmithingWeapon, "Weapon smith"),
                (StatSmithingArmour, "Armour smith"),
                (StatSmithingBow, "Crossbow smith"),
                (StatLabouring, "Labouring"),
                (StatFarming, "Farming"),
                (StatCooking, "Cooking"),
            ],
        ),
    ],
];

/// A stat's name as the window shows it.
pub fn stat_name(id: i32) -> Option<&'static str> {
    ATTRIBUTES
        .iter()
        .chain(COLUMNS.iter().flat_map(|c| c.iter()).flat_map(|(_, stats)| stats.iter()))
        .find(|(s, _)| *s as i32 == id)
        .map(|(_, n)| *n)
}

/// The derived statistics the window shows: the game's groups and labels (F047300) the rewrite
/// has the figures for.
const DERIVED: [(&str, &[&str]); 2] = [
    ("PRIMARY WEAPON", &["Primary Weapon Damage", "Primary Weapon Speed"]),
    ("GENERAL", &["Max Carry Weight", "Damage Resistance", "KO Threshold", "Max Run Speed", "Current Run Speed"]),
];

/// Kenshi_TextboxStandardText's colour: the text on the light panels.
const DARK: Color = Color::srgb(0.078, 0.031, 0.024);
const TEXT: Color = Color::srgb(0.78, 0.73, 0.62);
const HEADING: Color = Color::srgb(0.38, 0.28, 0.17);

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

/// The window's fonts: the datapanels' (Kenshi_StandardFont_Medium) and the headings'
/// (Kenshi_PaintedTextFont_Small).
#[derive(Clone)]
struct Fonts {
    text: (Handle<Font>, f32),
    heading: (Handle<Font>, f32),
}

fn text(commands: &mut Commands, s: &str, font: &(Handle<Font>, f32), colour: Color) -> Entity {
    commands
        .spawn((
            Text::new(s),
            TextFont { font: bevy::text::FontSource::Handle(font.0.clone()), font_size: bevy::text::FontSize::Px(font.1), ..default() },
            TextColor(colour),
        ))
        .id()
}

/// A skill row: its name, level and progress bar. Returns (row, level text, bar fill).
fn row(commands: &mut Commands, name: &str, font: &(Handle<Font>, f32), colour: Color) -> (Entity, Entity, Entity) {
    let label = text(commands, name, font, colour);
    let level = text(commands, "0", font, colour);
    let fill = commands
        .spawn((Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.62, 0.5, 0.26))))
        .id();
    let bar = commands
        .spawn((
            Node { width: Val::Percent(100.0), height: Val::Px(3.0), ..default() },
            BackgroundColor(Color::srgba(0.1, 0.09, 0.07, 0.8)),
        ))
        .add_child(fill)
        .id();
    let line = commands
        .spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::SpaceBetween, ..default() })
        .add_children(&[label, level])
        .id();
    let r = commands
        .spawn(Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), margin: UiRect::bottom(Val::Px(3.0)), ..default() })
        .add_children(&[line, bar])
        .id();
    (r, level, fill)
}

/// A layout panel made a column its rows stack down (its place and size kept).
struct Column(Entity);

impl Command for Column {
    type Out = ();
    fn apply(self, world: &mut World) {
        if let Some(mut n) = world.get_mut::<Node>(self.0) {
            n.flex_direction = FlexDirection::Column;
            n.overflow = Overflow::clip();
        }
    }
}

fn column(commands: &mut Commands, panel: Entity) {
    commands.queue(Column(panel));
}

#[allow(clippy::too_many_arguments)]
pub fn toggle(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    act: super::bindings::Actions,
    mut win: ResMut<StatsWindow>,
    game: Res<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    mut clicks: MessageReader<GuiClick>,
    nodes: Query<&Node>,
) {
    let close_clicked = win.open.is_some() && clicks.read().any(|c| c.name == "Button");
    let pressed = act.just("toggle_stats");
    if let Some(o) = &win.open
        && (pressed || close_clicked || keys.just_pressed(KeyCode::Escape))
    {
        for r in &o.ui.roots {
            commands.entity(*r).despawn();
        }
        win.open = None;
        return;
    }
    if !pressed {
        return;
    }
    let Some(&person) = game.selected.first() else { return };
    let size = window_size(&windows);
    let mut defs = gui.0.layout("Kenshi_StatsWindow.layout");
    if let Some(d) = defs.first_mut()
        && let kenshi_gui::res::Pos::Real(r) = d.pos
    {
        d.pos = kenshi_gui::res::Pos::Real([(1.0 - r[2]) * 0.5, 0.12, r[2], r[3]]);
        let name = game.sim.folk[person].name.clone();
        d.props.retain(|(k, _)| k != "Caption");
        d.props.push(("Caption".into(), if name.is_empty() { "CHARACTER STATS".into() } else { format!("{} - STATS", name.to_uppercase()) }));
    }
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(60));
    }
    for name in ["lbAttributes", "lbSkills", "lbDescription", "lbDerivedStats"] {
        if let Some(e) = ui.get(name) {
            let caption = match name {
                "lbAttributes" => "ATTRIBUTES",
                "lbSkills" => "SKILLS",
                "lbDescription" => "DESCRIPTION",
                _ => "DERIVED STATISTICS",
            };
            commands.queue(SetCaption(e, caption.to_string()));
        }
    }
    let font = |name: &str| gui.0.fonts.get(name).map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
    let fonts = Fonts { text: font("Kenshi_StandardFont_Medium"), heading: font("Kenshi_PaintedTextFont_Small") };
    let mut rows = Vec::new();
    if let Some(panel) = ui.get("AttributesPanel") {
        let _ = nodes.get(panel);
        column(&mut commands, panel);
        for (s, name) in ATTRIBUTES {
            let (r, level, fill) = row(&mut commands, name, &fonts.text, TEXT);
            commands.entity(panel).add_child(r);
            rows.push((s, level, fill));
        }
    }
    for (k, groups) in COLUMNS.iter().enumerate() {
        let Some(panel) = ui.get(&format!("Skills{}Panel", k + 1)) else { continue };
        column(&mut commands, panel);
        for (heading, stats) in groups.iter() {
            let h = text(&mut commands, heading, &fonts.heading, HEADING);
            commands.entity(h).insert(Node { margin: UiRect::vertical(Val::Px(2.0)), ..default() });
            commands.entity(panel).add_child(h);
            for (s, name) in stats.iter() {
                let (r, level, fill) = row(&mut commands, name, &fonts.text, DARK);
                commands.entity(panel).add_child(r);
                rows.push((*s, level, fill));
            }
        }
    }
    // the derived statistics: a heading per group, a line per figure (label, value)
    let mut derived = Vec::new();
    if let Some(panel) = ui.get("StatisticsPanel") {
        column(&mut commands, panel);
        for (heading, labels) in DERIVED {
            let h = text(&mut commands, heading, &fonts.heading, HEADING);
            commands.entity(h).insert(Node { margin: UiRect::vertical(Val::Px(2.0)), ..default() });
            commands.entity(panel).add_child(h);
            for label in labels.iter() {
                let l = text(&mut commands, label, &fonts.text, DARK);
                let v = text(&mut commands, "", &fonts.text, DARK);
                let line = commands
                    .spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::SpaceBetween, ..default() })
                    .add_children(&[l, v])
                    .id();
                commands.entity(panel).add_child(line);
                derived.push(v);
            }
        }
    }
    win.open = Some(Open { person, ui, rows, derived, last: -1.0 });
}

/// The levels, a few times a second (they train as the character acts).
pub fn update(mut win: ResMut<StatsWindow>, game: Res<Game>, time: Res<Time>, mut texts: Query<&mut Text>, mut nodes: Query<&mut Node>) {
    let Some(o) = win.open.as_mut() else { return };
    let now = time.elapsed_secs();
    if now - o.last < 0.25 {
        return;
    }
    o.last = now;
    let f = &game.sim.arena.f[o.person];
    let p = &game.sim.folk[o.person];
    for &(s, level, fill) in &o.rows {
        let v = f.level(s);
        if let Ok(mut t) = texts.get_mut(level) {
            let shown = format!("{}", v.floor() as i32);
            if t.0 != shown {
                t.0 = shown;
            }
        }
        if let Ok(mut n) = nodes.get_mut(fill) {
            n.width = Val::Percent(v.fract() * 100.0);
        }
    }
    // the derived figures
    let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
    let strength = kenshi_char::encumbrance::strength_level(f.level(StatStrength), 1.0);
    let capacity = ec.carry_weight_mult * strength + ec.encumbrance_base;
    let hc = &game.sim.arena.sh.hc;
    let t = (f.level(StatToughness) * 0.01).clamp(0.0, 1.0);
    let resistance = kenshi_char::medical::lerp(t, hc.damage_resistance_min, hc.damage_resistance_max);
    let ko = kenshi_char::medical::Medical::ko_point(&f.stats, &game.sim.arena.sh.mc);
    let values = [
        format!("x{:.2}", f.wielded.cut_factor.max(f.wielded.blunt_factor)),
        format!("x{:.2}", f.attack_speed),
        format!("{:.0} kg", capacity),
        format!("{:.0}%", resistance * 100.0),
        format!("{:.0}", ko),
        format!("{:.0} mph", f.run * 0.2237),
        format!("{:.0} mph", p.speed * 0.1 * 2.237),
    ];
    for (e, v) in o.derived.iter().zip(values) {
        if let Ok(mut t) = texts.get_mut(*e)
            && t.0 != v
        {
            t.0 = v;
        }
    }
}

/// (testing) KENSHI_STATS_AT=<frame>: C pressed at that frame.
pub fn script(mut frames: Local<u32>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    *frames += 1;
    if std::env::var("KENSHI_STATS_AT").ok().and_then(|f| f.parse::<u32>().ok()) == Some(*frames) {
        keys.press(KeyCode::KeyC);
    }
}
