//! The game's main panel (Kenshi_MainPanel.layout): the time and money, game speed buttons and
//! the paused banner, the selected character's name, status and body part life bars, the
//! squad's members to pick from, and the shortcut and order buttons that are ported (inventory,
//! sneak), the outpost's town panel (F043058) and the bounty panel; the status box and the
//! state box tell more on hover (F047087's descriptions, `status_line`).
use super::{Game, SPEEDS};
use bevy::prelude::*;
use kenshi_gui::{Built, EditText, GuiClick, GuiEdit, GuiRes, SetAlpha, SetCaption, SetProgress, SetVisible};
use std::collections::HashMap;

#[derive(Resource)]
pub struct Hud {
    ui: Built,
    /// The squad's portraits made: (their roots, the button, whom it selects, the face widget);
    /// the tab.
    squad: Vec<(Vec<Entity>, Entity, usize, Option<Entity>)>,
    /// The names over the portraits as last written (a name given later, or renamed, is
    /// written again): (the NameText widget, the name).
    squad_names: Vec<(Option<Entity>, String)>,
    /// The squads' tabs (one per squad, in their order) and the names and shown squad they were
    /// made for; the squad shown, and the first selected when last looked.
    tab: Option<Built>,
    tabs_for: (Vec<String>, usize),
    pub active_squad: usize,
    seen_first: Option<usize>,
    last: f32,
    /// (testing) KENSHI_STANCE given.
    stance_tested: bool,
    banner: Banner,
    /// What each datapanel shows now (rebuilt when it changes).
    rows: HashMap<Entity, Vec<Row>>,
    /// The tips set on the medical bars' tooltip widgets (set again when they change).
    tips: HashMap<Entity, String>,
    fonts: Fonts,
    /// The extending panel (MainBarGUI +0x1a0 ExtendingPanel, its button +0x198): folded away
    /// (+0x1bc) or not, how far slid (0 out .. 1 folded), and its layout top (+0x1a8).
    folded: bool,
    fold: f32,
    unfolded_top: Option<f32>,
    /// Hidden by "Toggle main gui" (controls.cfg toggle_bar).
    hidden: bool,
    /// The jobs panel's rows (`jobs_panel`).
    jobs: Jobs,
    /// The last portrait clicked: whom and when (a second click on it soon after is a double
    /// click).
    portrait_click: Option<(usize, f32)>,
}

/// What a tutorial page's pointer frames (`tutorial_ui::pointer`, F049757): the widget it
/// follows (the box hidden while it is) and the widgets whose rects make the box.
pub struct PointerTarget {
    pub main: Entity,
    pub rects: Vec<Entity>,
    pub shape: PointerShape,
}

pub enum PointerShape {
    /// The widget's rect.
    One,
    /// Its rect down to y 1080 (the screen's bottom at 1080p; its height 1080 less its top).
    ToBottom,
    /// The first's left, top and width, down to the last's bottom.
    Span,
    /// Round them all.
    Union,
}

impl Hud {
    /// The main panel hidden by "Toggle main gui" (toggle_bar).
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// @fn F049757
    ///
    /// The main panel's widgets a tutorial pointer case frames: 1 the squad panel (the GUI's
    /// +0x130), 2 NamePanel to the bottom, 3 StatsPanel, 6 the medical panel (+0x188), 7
    /// StatusPanel, 8 its bars 1 to 3 (the vitals, LifeBar2..4), 9 bars 4 to 7 (the limbs,
    /// LifeBar5..8), 10 bar 0 (blood, LifeBar1), 16 SpeedButtonsPanel, 17 TimeMoneyPanel, 18
    /// BuildPanel, 19 the bounty panel (+0x118), 20 ShortcutsPanel, 21 ShortcutSquadButton, 22
    /// ShortcutStatsButton, 23 ShortcutTechButton, 24 OrdersPanel, 25 round the orders' Block,
    /// Hold, Chase, Taunt, Ranged and Sneak buttons (its +0x58 .. +0x88 but Passive's +0x68),
    /// 26 JobsPanel, 27 the first job's row, 28 the Obedient Slave job's row (task 187; none
    /// without it), 29 OrdersMedicButton, 30 OrdersRescueButton. (assumption) +0x130, +0x188
    /// and +0x118 are SquadPanel, MedicalPanel and BountyPanel. (stand-in) The job rows are
    /// those shown (scrolled past, none). (not ported) 4 and 5 (StatsText's Power and Workers
    /// items), 33 (an InputColumn), 34.
    pub fn pointer_target(&self, case: u8) -> Option<PointerTarget> {
        let one = |name: &str| self.ui.get(name).map(|e| PointerTarget { main: e, rects: vec![e], shape: PointerShape::One });
        let span = |a: usize, b: usize| {
            let first = self.ui.get(&format!("LifeBar{a}"))?;
            let last = self.ui.get(&format!("LifeBar{b}"))?;
            Some(PointerTarget { main: first, rects: vec![first, last], shape: PointerShape::Span })
        };
        let row = |k: Option<usize>| {
            let (e, _) = *k.and_then(|k| k.checked_sub(self.jobs.scroll)).and_then(|k| self.jobs.rows.get(k))?;
            Some(PointerTarget { main: e, rects: vec![e], shape: PointerShape::One })
        };
        match case {
            1 => one("SquadPanel"),
            2 => one("NamePanel").map(|t| PointerTarget { shape: PointerShape::ToBottom, ..t }),
            3 => one("StatsPanel"),
            6 => one("MedicalPanel"),
            7 => one("StatusPanel"),
            8 => span(2, 4),
            9 => span(5, 8),
            10 => one("LifeBar1"),
            16 => one("SpeedButtonsPanel"),
            17 => one("TimeMoneyPanel"),
            18 => one("BuildPanel"),
            19 => one("BountyPanel"),
            20 => one("ShortcutsPanel"),
            21 => one("ShortcutSquadButton"),
            22 => one("ShortcutStatsButton"),
            23 => one("ShortcutTechButton"),
            24 => one("OrdersPanel"),
            25 => {
                let main = self.ui.get("OrdersPanel")?;
                let rects = ["OrdersBlockButton", "OrdersHoldButton", "OrdersChaseButton", "OrdersTauntButton", "OrdersRangedButton", "OrdersSneakButton"].iter().filter_map(|n| self.ui.get(n)).collect();
                Some(PointerTarget { main, rects, shape: PointerShape::Union })
            }
            26 => one("JobsPanel"),
            27 => row((!self.jobs.tasks.is_empty()).then_some(0)),
            28 => row(self.jobs.tasks.iter().position(|&t| t == OBEDIENT_SLAVE)),
            29 => one("OrdersMedicButton"),
            30 => one("OrdersRescueButton"),
            _ => None,
        }
    }
}

/// (testing) KENSHI_PORTRAIT_CLICKS=<k>@<frame>,...: the k-th squad portrait clicked at those
/// frames.
pub fn portrait_clicks(hud: Res<Hud>, mut out: MessageWriter<GuiClick>, mut frames: Local<u32>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_PORTRAIT_CLICKS") else { return };
    for step in v.split(',') {
        let Some((k, at)) = step.split_once('@') else { continue };
        if at.parse::<u32>().ok() == Some(*frames)
            && let Some(&(_, button, _, _)) = k.parse::<usize>().ok().and_then(|k| hud.squad.get(k))
        {
            out.write(GuiClick { widget: button, name: String::new() });
        }
    }
}

/// The jobs panel (OrdersPanel's JobsPanel ItemBox, F043076): whose jobs the rows show and what
/// (each row's text and whether it is drawn at full alpha) at which scroll, the rows (their
/// containers and remove buttons), the list scrolled by how many rows, and the row being
/// dragged (its place and where the press was).
#[derive(Default)]
struct Jobs {
    shown: Option<(usize, Vec<(String, bool)>, usize)>,
    rows: Vec<(Entity, Entity)>,
    /// The jobs listed (their task ids), as the rows were last made.
    tasks: Vec<u16>,
    scroll: usize,
    drag: Option<(usize, Vec2)>,
    /// (testing) KENSHI_JOBS given.
    tested: bool,
}

/// A jobs panel row (OrderCellView): its job's place in the list.
#[derive(Component)]
pub struct JobRow(usize);

/// The fonts the panel's own texts use.
struct Fonts {
    medium: (Handle<Font>, f32),
    painted: (Handle<Font>, f32),
}

impl Fonts {
    fn load(gui: &kenshi_gui::Gui, assets: &AssetServer) -> Fonts {
        let f = |name: &str| {
            gui.fonts.get(name).map_or((Handle::default(), 14.0), |d| (assets.load(d.file.clone()), d.screen_px()))
        };
        Fonts { medium: f("Kenshi_StandardFont_Medium"), painted: f("Kenshi_PaintedTextFont_Small") }
    }
}

/// Kenshi's light panel text (the banner's "TextColour").
pub(super) const LIGHT: Color = Color::srgb(0.871, 0.845, 0.81);
/// Faded panel text.
pub(super) const FADED: Color = Color::srgb(0.6, 0.56, 0.5);
/// The game's colour strings, MyGUI colour tags its texts are written with (std::strings made
/// at start by the initialisers at F163756 and after): 0x141f49248 "#afa68b", 0x141f49270
/// "#140806", 0x141f49298 "#492620", 0x141f492c0 "#59231a", 0x141f492e8 "#bf4834",
/// 0x141f49310 and 0x141f49338 "#a8b774", 0x141f49360 "#444444", 0x141f49388 "#777777",
/// 0x141f493b0 "#14ffdc".
pub(super) const fn tag(rgb: u32) -> Color {
    Color::srgb(((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0)
}
/// kenshi_colours.xml's "Alert" (#000000): the medical bars' arrows (F047087).
const ALERT: Color = Color::BLACK;
/// 0x141f49248, "#afa68b".
pub(super) const TAN: Color = tag(0xafa68b);
/// 0x141f49270, "#140806".
pub(super) const DARK_TEXT: Color = tag(0x140806);
/// 0x141f49298, "#492620".
pub(super) const DARK_RED: Color = tag(0x492620);
/// 0x141f492c0, "#59231a".
pub(super) const RED: Color = tag(0x59231a);
/// 0x141f49310, "#a8b774".
pub(super) const GREEN: Color = tag(0xa8b774);
/// 0x141f49360, "#444444".
pub(super) const GREY: Color = tag(0x444444);
/// A penalty: F047092 / F047093's red, #59231a.
pub(super) const PENALTY: Color = RED;
/// A bonus: F047092's green, #a8b774.
pub(super) const BONUS: Color = GREEN;
/// F039272's colour for a moderate load, #afa68b (Overloaded and Heavy in #59231a).
pub(super) const MODERATE: Color = TAN;
/// Kenshi_TextboxStandardText's "TextColour" (dark, on the bars).
const DARK: Color = Color::srgb(0.078, 0.031, 0.024);

/// A datapanel line (DataPanelLine_Text): its label on the left, its value on the right.
#[derive(Clone, PartialEq)]
pub(super) struct Row {
    pub label: String,
    pub value: String,
    pub colour: Color,
    /// The line's hover tip (DataPanelLine's tooltip), none when empty.
    pub tip: String,
    /// A button line (DataPanelLine_Button, F042415 / F042374); the value is its caption.
    pub button: Option<LineButton>,
}

/// A datapanel line's button: the click's action (its widget is named "Datapanel:" and the
/// action), its share of the line's width, its skin (Kenshi_Button2; a tick box's
/// Kenshi_TickButton2, F042417) and, for a tick box, whether it is held down (StateSelected).
#[derive(Clone, PartialEq)]
pub(super) struct LineButton {
    pub action: String,
    pub width: f32,
    pub skin: &'static str,
    pub checked: Option<bool>,
}

pub(super) fn row(label: impl Into<String>, value: impl Into<String>) -> Row {
    Row { label: label.into(), value: value.into(), colour: LIGHT, tip: String::new(), button: None }
}

/// A text's leading MyGUI colour code ("#rrggbb"), and the text after it.
pub(super) fn colour_code(s: &str) -> (Option<Color>, &str) {
    let b = s.as_bytes();
    if b.len() >= 7 && b[0] == b'#' && b[1..7].iter().all(u8::is_ascii_hexdigit) {
        let v = u32::from_str_radix(&s[1..7], 16).unwrap_or(0);
        return (Some(tag(v)), &s[7..]);
    }
    (None, s)
}

/// The marker on a datapanel's lines.
#[derive(Component)]
struct DataLines;

/// Fill a datapanel widget with lines (DatapanelGUI): one row each, label left and value right,
/// in `font`; `label_colour` for the labels; one line alone and `centre` puts its label in the
/// middle.
pub(super) struct SetRows {
    pub panel: Entity,
    pub rows: Vec<Row>,
    pub font: (Handle<Font>, f32),
    pub label_colour: Color,
    pub centre: bool,
}


impl Command for SetRows {
    type Out = ();
    fn apply(self, world: &mut World) {
        let old: Vec<Entity> = world
            .get::<Children>(self.panel)
            .map(|c| c.iter().filter(|e| world.get::<DataLines>(*e).is_some()).collect())
            .unwrap_or_default();
        for e in old {
            world.entity_mut(e).despawn();
        }
        let column = world
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: if self.centre { JustifyContent::Center } else { JustifyContent::FlexStart },
                    ..default()
                },
                DataLines,
            ))
            .id();
        let text = |s: String, c: Color| {
            (
                Text::new(s),
                TextFont { font: bevy::text::FontSource::Handle(self.font.0.clone()), font_size: bevy::text::FontSize::Px(self.font.1), ..default() },
                TextColor(c),
            )
        };
        let panel_w = world.get::<ComputedNode>(self.panel).map_or(0.0, |n| n.size().x * n.inverse_scale_factor());
        for r in self.rows {
            let line = world
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: if self.centre { JustifyContent::Center } else { JustifyContent::SpaceBetween },
                    ..default()
                })
                .id();
            if !r.tip.is_empty() {
                world.entity_mut(line).insert((Interaction::default(), super::inventory_ui::ItemTip(r.tip.clone())));
            }
            // (MyGUI's colour codes: a text's leading "#rrggbb" colours it)
            let (lc, label) = colour_code(&r.label);
            let (vc, value) = colour_code(&r.value);
            if let Some(LineButton { action, width: share, skin, checked }) = &r.button {
                // F042374: the line 1.2 lines high, the button from 0.99 - its share of the
                // datapanel's width to 0.99, its label hidden over 0.9 wide (F042415)
                let h = (self.font.1 * 1.2).floor();
                world.entity_mut(line).insert(Node { width: Val::Percent(100.0), height: Val::Px(h), flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() });
                if *share <= 0.9 {
                    let l = world.spawn(text(label.to_string(), lc.unwrap_or(self.label_colour))).id();
                    world.entity_mut(line).add_child(l);
                }
                let w = (panel_w * share).floor().max(1.0);
                let def = kenshi_gui::res::WidgetDef {
                    kind: "Button".into(),
                    skin: skin.to_string(),
                    pos: kenshi_gui::res::Pos::Px([0.0, 0.0, w, h]),
                    align: Default::default(),
                    name: format!("Datapanel:{action}"),
                    layer: None,
                    // (its colour code kept: kenshi_gui colours the caption by it)
                    props: vec![("Caption".into(), r.value.clone())],
                    user: Vec::new(),
                    children: Vec::new(),
                };
                let holder = world
                    .spawn(Node { position_type: PositionType::Absolute, left: Val::Percent((0.99 - share) * 100.0), top: Val::Px(0.0), width: Val::Px(w), height: Val::Px(h), ..default() })
                    .id();
                world.entity_mut(line).add_child(holder);
                let mut queue = bevy::ecs::world::CommandQueue::default();
                let built = {
                    let gui = world.resource::<kenshi_gui::GuiRes>().0.clone();
                    let assets = world.resource::<AssetServer>().clone();
                    let mut cmds = Commands::new(&mut queue, world);
                    kenshi_gui::ui::spawn(&mut cmds, &gui, &assets, &[def], Some(holder), [w, h])
                };
                queue.apply(world);
                if let Some(c) = checked {
                    for e in &built.roots {
                        world.entity_mut(*e).insert(kenshi_gui::ui::Checked(*c));
                    }
                }
                world.entity_mut(column).add_child(line);
                continue;
            }
            let l = world.spawn(text(label.to_string(), lc.unwrap_or(self.label_colour))).id();
            world.entity_mut(line).add_child(l);
            if !value.is_empty() {
                let v = world.spawn(text(value.to_string(), vc.unwrap_or(r.colour))).id();
                if self.centre {
                    // centred lines keep their value at the right edge (the hunger trend)
                    world.entity_mut(v).insert(Node { position_type: PositionType::Absolute, right: Val::Px(4.0), ..default() });
                }
                world.entity_mut(line).add_child(v);
            }
            world.entity_mut(column).add_child(line);
        }
        world.entity_mut(self.panel).add_child(column);
    }
}

impl Hud {
    /// The panel's root widgets (hidden while the build window is up).
    pub fn roots(&self) -> &[Entity] {
        &self.ui.roots
    }

    /// Show `rows` on the datapanel `name` (only when they changed).
    fn rows(&mut self, commands: &mut Commands, name: &str, rows: Vec<Row>, painted: bool, label_colour: Color, centre: bool) {
        let Some(panel) = self.ui.get(name) else { return };
        if self.rows.get(&panel) == Some(&rows) {
            return;
        }
        self.rows.insert(panel, rows.clone());
        let font = if painted { self.fonts.painted.clone() } else { self.fonts.medium.clone() };
        commands.queue(SetRows { panel, rows, font, label_colour, centre });
    }
}

/// A number as Kenshi's "{1,num}" formats it: thousands separated by commas.
pub(super) fn num(v: i64) -> String {
    let s = v.unsigned_abs().to_string();
    let mut out = String::new();
    for (k, ch) in s.chars().enumerate() {
        if k > 0 && (s.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if v < 0 { format!("-{out}") } else { out }
}

/// The area banner (MainBarGUI +0x1c0..+0x1e0): the region shown and the one before it, the
/// time left on screen, the time before another may show, and its alpha.
#[derive(Default)]
struct Banner {
    shown: Option<usize>,
    before: Option<usize>,
    left: f32,
    cooldown: f32,
    alpha: f32,
    visible: bool,
}

/// @fn F047028
///
/// The weapon indoors penalty (CharStats +0x28) as the equipment bonuses set it: the weapon
/// equipped's "indoors mod" (Weapon +0x250, F047085), 8 with no weapon at all. F047092 shows it
/// while the character is inside a building (Character::insideBuilding, F039242) and not where
/// it is open to the sky (F039279: +0xd5, (not ported) taken as never); F047029 adds it to the
/// buffed attack bonus there (a positive bonus first dropped to 0 under a penalty) unless the
/// building is flagged +0xc2. (not ported) The combat's attack bonus (`Fighter::attack_bonus`)
/// leaves it out, so the ATTACK total here does too.
fn indoors_penalty(sim: &kenshi_game::sim::Sim, p: &kenshi_game::sim::Person) -> i32 {
    p.inventory
        .equipped()
        .find(|i| i.kind == Some(kenshi_data::ItemType::Weapon))
        .map_or(8, |w| sim.data.get(&w.record).and_then(|r| r.ints.get("indoors mod").copied()).unwrap_or(0))
}

/// @fn F047081
///
/// A bonus as a tooltip's value and colour code (`tooltip_ui`): "+n" over 0 in #a8b774, "n" under
/// in #59231a, "0" in the text colour (F043426).
fn bonus_line(n: i32) -> String {
    let colour = match n {
        n if n > 0 => "g",
        n if n < 0 => "r",
        _ => "",
    };
    if n < 1 { format!("{n}\t{colour}") } else { format!("+{n}\t{colour}") }
}

/// The life bar parts of the medical panel, in the order Kenshi lists them.
const BARS: usize = 9;

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

/// The main panel's buttons' tooltips (F043076: the orders panel's, the sneak one its two
/// strings joined; F043077: the build, shortcut and floor buttons'), shown in its TooltipPanel
/// (`tooltip_ui`).
const TIPS: [(&str, &str); 19] = [
    (
        "OrdersBlockButton",
        "This will toggle the selected characters into defensive combat mode. When in defensive mode, a character will focus purely on staying alive and will gain 20 to his melee defense skill. However he will not attempt to make any attacks. This is handy for keeping your weaker characters alive when they are outclassed.",
    ),
    (
        "OrdersHoldButton",
        "HOLD POSITION means that the selected character won't run off to fight enemies, even to protect his allies.  He will hold his position and only attack any enemy that comes into close range.  He will still use a crossbow normally, but won't move around obstacles to get a shot.  Use this setting to tactically position fighters, or just to keep non-combatants away from the fighting.",
    ),
    (
        "OrdersPassiveButton",
        "In non-combat mode a character will never initiate combat, even if his allies are fighting or need help. He will only fight when directly attacked, and is less likely to be targeted by enemies.",
    ),
    ("OrdersChaseButton", "If this is disabled then the character won't do any of the jobs listed."),
    (
        "OrdersTauntButton",
        "The character will make himself offensive, noisy and conspicuous in an attempt to draw more of the enemy to attack him. Using this enables your experienced and well armoured characters to draw trouble away from your more vulnerable ones.",
    ),
    ("OrdersRangedButton", "If enabled then the character will prioritise ranged attacks, melee will be a last resort."),
    (
        "OrdersSneakButton",
        "Activates sneak mode so the character will try to go unnoticed.\nIf the character is unconscious, when he wakes up he will stay down on the ground and crawl to avoid detection.",
    ),
    (
        "OrdersMedicButton",
        "Pressing this button will order your selected characters to automatically heal everyone else in the squad, including themselves. They will need a medkit in their inventory.",
    ),
    (
        "OrdersRescueButton",
        "This will order your selected characters to pick up all your wounded allies. If you own any buildings nearby, they will take the wounded and put them in beds. Make sure to heal their wounds first.",
    ),
    (
        "OrdersProspectButton",
        "This will give you information on the richness of resources at your location. Stone, iron, water, fertility... It's important to check for these things before building an outpost.",
    ),
    (
        "BuildButton",
        "Opens the BUILD menu.  Here you can place buildings and start your own outpost anywhere in the world, but you should choose a safe area and check availability of resources using the PROSPECT button. You will also need building materials to finish construction.",
    ),
    ("ShortcutInventoryButton", "Open character inventory"),
    ("ShortcutStatsButton", "Open character status window"),
    ("ShortcutMapButton", "Open world map"),
    ("ShortcutTechButton", "Open research window"),
    ("ShortcutSquadButton", "Open squad management window"),
    ("ShortcutHelpButton", "Open help window"),
    ("FloorArrowDown", "Change the selected floor, use to move to a certain floor of a building, 0 is ground floor (also: Page Down key)"),
    ("FloorArrowUp", "Change the selected floor, use to move to a certain floor of a building, 0 is ground floor (also: Page Up key)"),
];

/// Spawn the panel once the world is ready and no editor is up.
pub fn open(mut commands: Commands, gui: Res<GuiRes>, assets: Res<AssetServer>, windows: Query<&Window>) {
    let size = window_size(&windows);
    let mut defs = gui.0.layout("Kenshi_MainPanel.layout");
    // the squad TabControl (Kenshi_SquadTabControl): its tab items are the player's squads, one
    // here; its first item is the squad's area, named, without the caption the layout gives it
    fn squad_tabs(d: &mut kenshi_gui::res::WidgetDef) {
        if d.name == "SquadPanel" {
            d.children.retain(|c| c.kind != "TabItem" || c.prop("Caption") == Some("Squad 1"));
            for c in &mut d.children {
                if c.kind == "TabItem" {
                    c.kind = "Widget".into();
                    c.name = "SquadTabItem".into();
                    c.props.retain(|(k, _)| k != "Caption");
                }
            }
        }
        for c in &mut d.children {
            squad_tabs(c);
        }
    }
    for d in &mut defs {
        squad_tabs(d);
    }
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(40));
    }
    // (TooltipPanel: the panel's tooltips show in its place, F043077's ToolTipFixed: tooltip_ui)
    for hidden in ["TownPanel", "TooltipPanel", "BountyPanel", "LoadingPanel"] {
        if let Some(e) = ui.get(hidden) {
            commands.queue(SetVisible(e, false));
        }
    }
    // the captions the game sets over the layout's (F043077: "Money")
    if let Some(e) = ui.get("MoneyLabelText") {
        commands.queue(SetCaption(e, "Money".to_string()));
    }
    for (name, text) in TIPS {
        if let Some(e) = ui.get(name) {
            commands.entity(e).insert(super::inventory_ui::ItemTip(text.to_string()));
        }
    }
    let fonts = Fonts::load(&gui.0, &assets);
    commands.insert_resource(Hud {
        ui,
        squad: Vec::new(),
        squad_names: Vec::new(),
        last: -1.0,
        stance_tested: false,
        banner: Banner::default(),
        rows: HashMap::new(),
        tips: HashMap::new(),
        fonts,
        tab: None,
        tabs_for: (Vec::new(), 0),
        active_squad: 0,
        seen_first: None,
        folded: std::env::var("KENSHI_HUD_FOLD").is_ok(),
        fold: 0.0,
        unfolded_top: None,
        jobs: Jobs::default(),
        hidden: false,
        portrait_click: None,
    });
}

/// One life bar's contents.
pub(super) struct Bar {
    pub label: String,
    pub value: String,
    /// The trend shown after its label (F047087's arrows, in the "Alert" colour).
    pub arrow: String,
    /// Its tooltip (on its LifeBar<n>Tooltip widget).
    pub tip: String,
    /// green, yellow, red, white (F042974's widths).
    pub widths: [f32; 4],
    pub robot: bool,
    pub crushed: bool,
}
/// The medical panel's bars (LifeBar1..9, MedicalDatapanel's lines): each bar's widths, value,
/// label and its tip; the bars past the last hidden.
fn draw_bars(commands: &mut Commands, hud: &mut Hud, ui: &Built, nodes: &mut Query<&mut Node>, bars: &[Bar]) {
    for k in 0..BARS {
        let bar = bars.get(k);
        for w in ["", "Value"] {
            if let Some(e) = ui.get(&format!("LifeBar{}{w}", k + 1)) {
                commands.queue(SetVisible(e, bar.is_some()));
            }
        }
        let Some(b) = bar else { continue };
        let [green, yellow, red, white] = b.widths;
        for (suffix, show, width) in [
            ("Red", true, red),
            ("Yellow", true, yellow),
            ("Green", true, green),
            ("White", true, white),
            ("Robot", b.robot, green),
            ("Crushed", b.crushed, 1.0),
        ] {
            let Some(e) = ui.get(&format!("LifeBar{}{suffix}", k + 1)) else { continue };
            commands.queue(SetVisible(e, show));
            if let Ok(mut n) = nodes.get_mut(e) {
                let full = hud.ui.sizes.get(&e).map_or(100.0, |s| s[0]);
                n.width = Val::Px(full * width.clamp(0.0, 1.0));
                // the skin's image is a child at the bar's full size: cut to the bar
                if n.overflow != Overflow::clip() {
                    n.overflow = Overflow::clip();
                }
            }
        }
        if let Some(e) = ui.get(&format!("LifeBar{}Value", k + 1)) {
            commands.queue(SetCaption(e, b.value.clone()));
        }
        // (the arrows in the "Alert" colour, kenshi_colours.xml's #000000)
        let label = vec![Row { label: b.label.clone(), value: b.arrow.clone(), colour: ALERT, tip: String::new(), button: None }];
        let name = format!("LifeBar{}Datapanel", k + 1);
        hud.rows(commands, &name, label, false, DARK, true);
        // its tip (F043001 / F043002 onto the bar's tooltip widget)
        if let Some(e) = ui.get(&format!("LifeBar{}Tooltip", k + 1))
            && hud.tips.get(&e) != Some(&b.tip)
        {
            // (it takes the mouse from its first tip on)
            if hud.tips.insert(e, b.tip.clone()).is_none() {
                commands.entity(e).insert(Interaction::default());
            }
            commands.entity(e).insert(super::inventory_ui::ItemTip(b.tip.clone()));
        }
    }
}


/// @fn F047087
///
/// A body part's bar's arrow and tooltip as MedicalDatapanel::populate makes them (the part's
/// name for {1}; health = flesh + treatment):
/// - destroyed (flesh + treatment - stun at or under -max health): "!!!", "Your {1} is totally
///   destroyed." and, for a part not vital, "Lucky it's not something you need to live.", else
///   "It's probably the reason you are dead.";
/// - health under 0: "This wound is getting worse. You need first aid." and "<<", but a robotic
///   part not vital "This limb is severely damaged and in need of repairs..." with no arrow; a
///   vital part above the KO point (F040402) "This is a dangerous wound and is getting worse.
///   Get some medical attention fast.  Death at {-max}" (unconscious, F029130: the coma's
///   warning in #59231a with the KO point, then "You will die at {2}" with its {2} unfilled, as
///   Kenshi formats it with one value), below it "This is a critical wound..." and "<<!!";
/// - health 0 or more: within 20 of the max and over 0.95 of it less the stun, "Your {1} is
///   okay." (treatment and stun 5 at most) or "is in the process of healing.", then over 5
///   wear damage its line in #59231a (40 or less "...can be fixed at a Skeleton Bed.", more
///   "The manual recommends..."); within 20 but not over, "needs bandaging, but it's not
///   urgent."; else a robotic part not vital "needs bandaging.", any other "<" and "needs
///   bandaging, it's slowly getting worse.";
/// then with health less the stun between 0 and 60 the part's effect (a leg "Your movement
/// speed...", an arm "Your combat and manual labour skills...", a vital part "Your stats...");
/// a leg when the character cannot kick (F047021: the legs collapsed, or a leg under 0) "You
/// won't be able to kick."; and a lost limb's (its slot state 1) tip is "This limb is gone
/// forever." alone (Kenshi assigns it over the rest). Robotic (F040400): the race a robot's
/// or the part a robotic limb.
fn part_tip(q: &kenshi_char::medical::BodyPart, name: &str, robot_race: bool, ko: f32, unconscious: bool, can_kick: bool) -> (String, String) {
    let max = q.max_health();
    let health = q.flesh + q.treatment;
    let robotic = robot_race || q.robotic;
    let mut arrow = "";
    let mut tip;
    if -max < health - q.stun {
        if 0.0 <= health {
            if max - q.flesh - q.treatment <= 20.0 {
                if max * 0.95 - q.stun <= health {
                    tip = if q.treatment + q.stun <= 5.0 { format!("Your {name} is okay.") } else { format!("Your {name} is in the process of healing.") };
                    let wear = q.max_loss;
                    if wear > 5.0 {
                        let line = if wear <= 40.0 {
                            format!("You have {} points of wear damage, this detracts from your max health and can be fixed at a Skeleton Bed.", wear as i32)
                        } else {
                            format!("You have {} points of wear damage. The manual recommends you find a Skeleton Bed for maintenance.", wear as i32)
                        };
                        tip = format!("{tip}\n#59231a{line}");
                    }
                } else {
                    tip = format!("Your {name} needs bandaging, but it's not urgent.");
                }
            } else if !q.vital && robotic {
                tip = format!("Your {name} needs bandaging.");
            } else {
                arrow = "<";
                tip = format!("Your {name} needs bandaging, it's slowly getting worse.");
            }
        } else {
            tip = "This wound is getting worse. You need first aid.".to_string();
            if !q.vital {
                if robotic {
                    tip = "This limb is severely damaged and in need of repairs. Luckily though skeleton limbs don't degenerate.".to_string();
                } else {
                    arrow = "<<";
                }
            } else if ko <= health {
                let death = -max as i32;
                tip = if !unconscious {
                    format!("This is a dangerous wound and is getting worse. Get some medical attention fast.  Death at {death}")
                } else {
                    format!(
                        "#59231aThis is a dangerous wound and is getting worse. If you can regain consciousness before it goes below {} then you could save yourself.\nIf it goes below that then you will have to wait for medical treatment. You will die at {{2}}",
                        num(ko as i64)
                    )
                };
                arrow = "<<";
            } else {
                tip = "This is a critical wound. If it doesn't get medical treatment you will die.".to_string();
                arrow = "<<!!";
            }
        }
    } else {
        arrow = "!!!";
        tip = if !q.vital {
            format!("Your {name} is totally destroyed. Lucky it's not something you need to live.")
        } else {
            format!("Your {name} is totally destroyed. It's probably the reason you are dead.")
        };
    }
    let left = health - q.stun;
    if 0.0 < left && left < 60.0 {
        let effect = if q.vital {
            Some("Your stats will be negatively affected.")
        } else {
            match q.kind {
                1 => Some("Your movement speed will be negatively affected."),
                2 => Some("Your combat and manual labour skills will be negatively affected."),
                _ => None,
            }
        };
        if let Some(e) = effect {
            tip = format!("{tip}\n{e}");
        }
    }
    if q.kind == 1 && !can_kick {
        tip = format!("{tip}\nYou won't be able to kick.");
    }
    if q.slot == 1 {
        tip = "This limb is gone forever.".to_string();
    }
    (arrow.to_string(), tip)
}

/// @fn F047086
///
/// The hunger rate's lines (a key and value each, the value its multiplier "{1,num=fix,p=2}x",
/// F047082: over 1 in #59231a, under in #a8b774): "Racial hunger rate:" (the race's), "Age:"
/// (an animal's growth, when not 1), "Encumbrance effect:" ((1 - load) x "encumbrance hunger
/// rate" + 1), "Resting/KO state:" (in bed or out cold: "bed hunger rate"), then "Idle:" (0.8)
/// while idle (the AI's current goal's +0x70 flag; (assumption) here no current task), else
/// "Current work:" (CharStats +0x178) when not 1 (kenshi_char::hunger::CURRENT_WORK_RATE: its
/// writer not traced, so never).
fn hunger_rate_lines(sim: &kenshi_game::sim::Sim, id: usize) -> Vec<String> {
    let p = &sim.folk[id];
    let f = &sim.arena.f[id];
    let [_, _, bed, enc, race_rate] = sim.hunger_c;
    let mult = |label: &str, v: f32| {
        let code = if v > 1.0 { "r" } else if v < 1.0 { "g" } else { "" };
        format!("{label}\t{v:.2}x\t{code}")
    };
    let mut lines = vec![mult("Racial hunger rate:", race_rate)];
    if f.body.maturity != 1.0 {
        lines.push(mult("Age:", f.body.size));
    }
    lines.push(mult("Encumbrance effect:", (1.0 - f.load) * enc + 1.0));
    if p.bed.is_some() || f.medical.knocked_out || f.medical.dead {
        lines.push(mult("Resting/KO state:", bed));
    }
    if p.ai.current.is_none() {
        lines.push(mult("Idle:", 0.8));
    } else if kenshi_char::hunger::CURRENT_WORK_RATE != 1.0 {
        lines.push(mult("Current work:", kenshi_char::hunger::CURRENT_WORK_RATE));
    }
    lines
}

/// @fn F047087
///
/// The hunger bar's tooltip: the hunger rate's lines (F047086), then how the character fares:
/// with nothing being digested, at 2 or less malnutrition (over 1: "You are suffering from
/// malnutrition, Your stats and strength will be affected, you need to get some food as soon
/// as possible. Stat penalty is {1}", the stat penalty F040433 activity 3 with the hunger alone
/// in #59231a "x"), at 1 or less starving ("When the red bar goes below {1} you will pass out"
/// at the KO fraction F040501, or passed out: alone, or with squad mates to feed them); over 2,
/// well fed; with food being digested, over 2 well fed, 1 or less recovering (or passed out
/// with food), else malnutrition with food. (stand-in) Passed out from starvation (F040414, a
/// roll each check) is hunger under 0.1, or out cold under the KO fraction.
fn hunger_tip(sim: &kenshi_game::sim::Sim, id: usize) -> String {
    let p = &sim.folk[id];
    let f = &sim.arena.f[id];
    let m = &f.medical;
    let h = m.hunger.level;
    let ko = f.ko_point();
    // F040501 at 1: (100 + the KO point) / 100
    let ko_fraction = (100.0 + ko) * 0.01;
    let passed_out = h < 0.1 || (m.knocked_out && h < ko_fraction);
    let penalty = || {
        let with = kenshi_char::fitness::Considering { hunger: true, parts: false, weariness: false, limbs: false, effort: false, mods: false };
        m.fitness(3, f.body.animal, ko, with, &kenshi_char::fitness::Mods::default())
    };
    let mut lines = hunger_rate_lines(sim, id);
    let state = if m.hunger.stomach <= 0.0 {
        if h <= 2.0 {
            if 1.0 < h {
                format!("You are suffering from malnutrition, Your stats and strength will be affected, you need to get some food as soon as possible. Stat penalty is #59231a{:.2}x", penalty())
            } else if !passed_out {
                format!(
                    "You are starving to death. Get some food fast. When the red bar goes below #59231a{}#140806 you will pass out. When it reaches zero, you die.",
                    (ko_fraction * 100.0) as i32
                )
            } else if p.platoon.is_none_or(|pl| sim.folk.iter().filter(|q| !q.dead && q.platoon == Some(pl)).count() < 2) {
                "You have passed out from starvation. You're all alone and going to die. Nobody cares.".to_string()
            } else {
                "You have passed out from starvation. One of your squad mates will have to feed you or you're going to die.".to_string()
            }
        } else {
            "You are well fed and currently have no problem.".to_string()
        }
    } else if 2.0 < h {
        "You are well fed and currently have no problem.".to_string()
    } else if h <= 1.0 {
        if !passed_out {
            "You are slowly recovering from starvation.".to_string()
        } else {
            "You have passed out from starvation. You have food though, so hopefully you will recover.".to_string()
        }
    } else {
        format!("You are suffering from malnutrition, but you have food so you are recovering. Stat penalty is #59231a{:.2}x", penalty())
    };
    lines.push(state);
    lines.join("\n")
}

/// (assumption, from the game's panel) the parts head first, then the torso (chest, stomach),
/// the arms and the legs, left before right.
fn part_order(q: &kenshi_char::medical::BodyPart, data: &kenshi_data::GameDataContainer) -> (i32, i32, String) {
    let rank = match q.kind {
        3 => 0,
        0 => 1,
        2 => 2,
        _ => 3,
    };
    let name = data.get(&q.id).map_or(q.id.clone(), |r| r.name.clone());
    let side = if name.starts_with("Left") { 0 } else { 1 };
    (rank, side, name)
}

/// @fn F047087
///
/// The status box's state and what hovering it tells (the game's title and description), in the
/// game's order: dead, the recovery coma, out cold, starving, bleeding, critical; then a blown
/// disguise ("Disguise Blown!", with its cooldown) or an escaped slave found out
/// ("Discovered!"), a disguise ("In Disguise": "You are currently disguised as {1}"), hunger,
/// wounds, else "Normal".
/// (assumption) The titles' colours (strings the game makes at start) are not read; the
/// unconscious states the rewrite has no flag for (playing dead, blood loss KO, crippled) are
/// not told apart.
fn status_line(sim: &kenshi_game::sim::Sim, id: usize) -> (&'static str, String) {
    let (p, f) = (&sim.folk[id], &sim.arena.f[id]);
    let m = &f.medical;
    let hurt = |k: f32| m.parts.iter().any(|q| q.flesh - q.stun < q.max_health() * k);
    if m.dead {
        return ("Dead", "Gone forever. RIP.".into());
    }
    if m.coma {
        return ("Recovery coma", "You've suffered too much damage. You won't be getting up again until all critical body parts have recovered above 0.".into());
    }
    if p.playing_dead {
        return ("Playing dead...", "This person has regained consciousness but is staying down, waiting for all the enemies to leave the area.".into());
    }
    if p.down {
        return ("Unconscious", "Knocked out cold.".into());
    }
    if m.hunger.starving() {
        return ("Starving", "You are starving to death. Get some food fast.".into());
    }
    if 0.0 < m.bleed_rate {
        return ("Bleeding", "Losing blood, will collapse if blood goes below 0.".into());
    }
    if m.parts.iter().any(|q| q.flesh - q.stun < 0.0) {
        return ("Critical", "Critical injuries will continue to degenerate without medical attention.".into());
    }
    // the weather or ground hurting it (kenshi_game::hazards; F040447..F040451's lines)
    if let Some(z) = &p.hazard {
        use kenshi_game::hazards::{WA_ACID, WA_BURNING, WA_GAS};
        let pct = (z.protection.clamp(0.0, 1.0) * 100.0).round();
        let gear = |what: &str| {
            if z.protection >= 1.0 {
                "Your gear is protecting you from being horribly burnt.".to_string()
            } else if z.protection > 0.0 {
                format!("{what}, your gear is providing you with {pct}% protection.")
            } else {
                format!("{what}.")
            }
        };
        return match (z.affect, z.ground) {
            (WA_ACID, true) if p.water == 3 => ("Acid!", "You are currently submersed in acid, get out of there as soon as you can.".into()),
            (WA_ACID, true) => ("Feet Burning!", "The ground here is acidic and burns your feet, find some footwear!".into()),
            (WA_ACID, false) => ("Acid Rain", gear("It is currently raining acid")),
            (WA_BURNING, _) => ("Burning", gear("You are burning")),
            (WA_GAS, _) => ("Poison Gas", gear("You are currently breathing in poison gas")),
            _ => ("Dust Storm", if z.protection > 0.0 { format!("You are in a dust storm, your gear is providing you with {pct}% protection.") } else { "You are in a dust storm, it affects your accuracy in combat.".into() }),
        };
    }
    // found out: an escaping slave or ex-slave, or one in some disguise, while its disguise is
    // blown (the state object +0x110), with the cooldown left (truncated)
    let state = sim.slave_state(id);
    if (state > kenshi_game::slavery::IS_SLAVE || sim.outfit(id).disguise > 0.0) && 0.0 < p.disguise_blown {
        let n = p.disguise_blown as i32;
        let cool = if n == 1 { "Cooldown 1 second".to_string() } else { format!("Cooldown {n} seconds") };
        if state > kenshi_game::slavery::IS_SLAVE {
            return ("Discovered!", format!("You've been discovered, somebody has noticed that you are an escaped slave.\n{cool}"));
        }
        return ("Disguise Blown!", format!("You've been discovered, somebody has seen through your disguise.\n{cool}"));
    }
    if let Some(d) = &p.disguised {
        let name = sim.data.get(d).map_or(d.clone(), |r| r.name.clone());
        return ("In Disguise", format!("You are currently disguised as {name}"));
    }
    if m.hunger.level < 1.0 {
        return ("Hungry", "You are hungry and suffering from malnutrition. Your stats and strength will be affected, you need to get some food as soon as possible.".into());
    }
    if hurt(0.7) {
        ("Wounded", String::new())
    } else if hurt(0.9) {
        ("Slightly Injured", String::new())
    } else {
        ("Normal", "You are kind of healthy for now.".into())
    }
}

/// @fn F039272
///
/// The state box's state (the first row) and what hovering it tells: a slave's, an escaped
/// slave's and an ex-slave's (by the slave state, F039148) with how much it looks like a slave
/// ("Looks like a slave:", the state object +0xf4 x 100), whether it has been found out
/// ("Situation:" "Seen!" while its disguise is blown, +0x110) and the hours left (F038568: 12
/// less the hours escaping, 96 less those as an ex-slave).
fn state_line(sim: &kenshi_game::sim::Sim, id: usize) -> (&'static str, String) {
    let p = &sim.folk[id];
    let m = &sim.arena.f[id].medical;
    // a faction's name (the player's as it was named, F035114)
    let faction = |f: &str| if f == kenshi_game::sim::PLAYER_FACTION { sim.player_faction_name() } else { sim.data.get(f).map_or(f.to_string(), |r| r.name.clone()) };
    let looks = || {
        let seen = if p.disguise_blown > 0.0 { "Seen!" } else { "Undiscovered" };
        format!("\nLooks like a slave: {}%\nSituation: {seen}", (sim.looks_like_slave(id) * 100.0) as i32)
    };
    let left = |s: &kenshi_game::slavery::Slavery, of: f64| {
        let n = (of - (sim.hours_gone() - s.since)) as i64;
        if n == 1 { "1 hour".to_string() } else { format!("{n} hours") }
    };
    if m.dead {
        return ("Dead", "This guy is dead and gone.".into());
    }
    if let Some(s) = p.slave.as_ref() {
        if s.state == kenshi_game::slavery::IS_SLAVE {
            return (
                "Slave",
                format!(
                    "A slave belonging to {}. Not considered a person with rights, a slave works for his master until death and nothing more. {}",
                    faction(&s.faction),
                    "Your character will have a permanent job assigned called [Obedient slave]. While this is active he will automatically work for his master. You can remove this job, but beware of the consequences..."
                ),
            );
        }
        {
            if s.state == kenshi_game::slavery::ESCAPING_SLAVE {
                let hours = left(s, 12.0);
                return (
                    "Escaped slave",
                    format!(
                        "A slave of {} that has been seen escaping. Needs to lay low for a day and avoid getting spotted. {}\nTime remaining: {hours}{}",
                        faction(&s.faction),
                        "Keep your [Looks Like A Slave] level as low as possible, wear clothing or disguises and find a plastic surgeon to restore your hair. Don't get spotted.",
                        looks()
                    ),
                );
            }
            return (
                "Ex-slave",
                format!(
                    "Was once a slave of {} but has now escaped. The heat has died down but he still needs to avoid getting found out. {}\nTime remaining: {}{}",
                    faction(&s.faction),
                    "Keep your [Looks Like A Slave] level as low as possible, wear clothing or disguises and find a plastic surgeon to restore your hair. You should be safe if you stay out of towns.",
                    left(s, 96.0),
                    looks()
                ),
            );
        }
    }
    if p.prison.is_some() {
        return ("Imprisoned", String::new());
    }
    if p.down {
        return ("Unconscious", "This guy is unconscious and probably needs medical attention and rest.".into());
    }
    if let Some(c) = &p.crime {
        // (assumption) the crime named from its enum (CrimeTheft: "theft")
        let what = format!("{:?}", c.kind).trim_start_matches("Crime").to_lowercase();
        return ("Committing Crime!", format!("This character is currently committing {what}, make sure nobody witnesses it."));
    }
    if p.escaped.is_some() {
        // (F039272 shows a fixed 99999 hrs: the status lasts until the character leaves town)
        return ("Escaped Prisoner", "This character broke out of prison, if he gets spotted by any guards he is in trouble. Leave town to clear this status. 99999 hrs".into());
    }
    if p.bed.is_some() {
        return ("Sleeping...", String::new());
    }
    ("Normal", String::new())
}

/// @fn F047093
///
/// The extended stats (MainBarGUI's "Stats 2" datapanel on ExtendedStatsText, mode 7: the
/// character's F039272 hands it to its stats, F047093), each line with its tooltip:
/// - Athletics XP: F047452, the load (F047034, CharStats +0x190, at least 0) x 1.5 while moving
///   faster than 0.8 x the run speed (F047018); its tip the encumbrance penalty (1 - load) and
///   the full speed bonus;
/// - Strength XP: out of combat the walking rate (F047455; its tip the encumbrance bonus, halved
///   with "Carrying somebody: 200%" while carrying), in combat the weapon's strain (CharStats
///   +0x184, "Weapon weight:"), unarmed (+0x1f0 = 5) 1 - load ("Encumbrance:");
/// - Toughness XP: F047453's multiplier: 1, 2 with the left arm crippled, +2 the right, +2 with
///   blood loss trauma, +2 while the worst part's health share (MedicalSystem +0xa4, F040427) is
///   under 15 (a share is at most 1: always, so never under 3x, as in Kenshi);
/// - KO point: getKOPoint (F040402);
/// - Run speed: CharStats +0x17c x 0.2237 in mph, with the difference from the run speed
///   (F047080);
/// - Hunger Rate: hungerRate (F047009) x 100.
/// (stand-ins: an arm is crippled with no health left, or lost; blood loss trauma (+0x163,
/// raised by the bleeding and damage of the frame) is bleeding now. The hunger rate's tooltip is
/// F047086's lines (`hunger_rate_lines`); the run speed's F047088's: "Athletics Skill:", "Max
/// possible speed:" (the run speed the athletics give, x 0.2237 mph), "Leg injury:" and
/// "Encumbrance:" ("-n%" in #59231a, when any), "Total speed:" (its key #afa68b). (not ported)
/// its "Armour/equipment:" (CharStats +0x18, 1 here) and "Robotics:" (the robotic legs' speed)
/// lines.)
fn extended_stats(sim: &kenshi_game::sim::Sim, id: usize) -> Vec<Row> {
    use kenshi_char::StatsEnumerated as S;
    let p = &sim.folk[id];
    let f = &sim.arena.f[id];
    let m = &f.medical;
    let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
    let strength = kenshi_char::encumbrance::strength_level(f.level(S::StatStrength), 1.0);
    let load = kenshi_char::encumbrance::load(kenshi_char::encumbrance::carried_weight(p.inventory.weight(), p.carrying.is_some(), &ec), 1.0, strength, &ec);
    let run = if f.run > 0.0 { f.run } else { sim.run };
    let tipped = |label: &str, value: String, lines: Vec<String>, about: &str| Row {
        label: label.into(),
        value,
        colour: LIGHT,
        tip: lines.into_iter().chain(std::iter::once(about.to_string())).collect::<Vec<_>>().join("\n"),
        button: None,
    };
    let mut rows = Vec::new();
    // Athletics XP (F047452)
    let fast = run * 0.8 < p.own_speed;
    let athletics = load.max(0.0) * if fast { 1.5 } else { 1.0 };
    // F047093: the penalty in #59231a, the bonus in #a8b774
    let mut lines = vec![format!("Encumbrance penalty:\t-{}%\tr", ((1.0 - load) * 100.0) as i32)];
    if fast {
        lines.push("Full speed bonus:\t+50%\tg".into());
    }
    rows.push(tipped("Athletics XP:", format!("{}%", (athletics * 100.0) as i32), lines, "This is the rate at which your athletics skill is increasing while running."));
    // Strength XP
    let carrying = p.carrying.is_some();
    let unarmed = f.weapon_weight <= 0.0;
    let strength_xp = if !f.active {
        kenshi_char::encumbrance::strength_xp_from_walking(load, carrying, &ec)
    } else if unarmed {
        1.0 - load
    } else {
        f.strain
    };
    let shown = (strength_xp * 100.0) as i32;
    let mut lines = Vec::new();
    if !f.active {
        lines.push(format!("Encumbrance bonus:\t{}%\t", if carrying { (shown as f32 * 0.5) as i32 } else { shown }));
        if carrying {
            lines.push("Carrying somebody:\t200%\t".into());
        }
    } else if unarmed {
        lines.push(format!("Encumbrance:\t{shown}%\t"));
    } else {
        lines.push(format!("Weapon weight:\t{shown}%\t"));
    }
    rows.push(tipped("Strength XP:", format!("{shown}%"), lines, "This is the rate at which your strength is increasing."));
    // Toughness XP (F047453)
    let crippled = |side_name: &str| {
        m.parts.iter().any(|q| q.kind == 2 && sim.data.get(&q.id).is_some_and(|r| r.name.contains(side_name)) && (q.slot == 1 || q.health() <= 0.0))
    };
    let (left, right) = (crippled("Left"), crippled("Right"));
    let trauma = 0.0 < m.bleed_rate;
    let worst = m.parts.iter().filter(|q| !q.robotic).map(|q| (q.treatment + q.flesh) / q.max_health().max(1e-3)).fold(1.0f32, f32::min);
    let worst = if 0.999 < worst { 1.0 } else { worst };
    let mut mult = if left { 2.0 } else { 1.0 };
    let mut lines = Vec::new();
    if left {
        lines.push("Crippled left arm:\t+1x\t".to_string());
    }
    if right {
        mult += 2.0;
        lines.push("Crippled right arm:\t+2x\t".into());
    }
    if trauma {
        mult += 2.0;
        lines.push("Blood loss trauma:\t+2x\t".into());
    }
    if worst < 15.0 {
        mult += 2.0;
    }
    if worst < 0.0 {
        lines.push("Critical wounds:\t+2x\t".into());
    }
    rows.push(tipped("Toughness XP:", format!("{}x", mult as i32), lines, "This is the rate at which your toughness increases when you get hurt."));
    // KO point (F040402)
    let ko = kenshi_char::medical::Medical::ko_point(&f.stats, &sim.arena.sh.mc) as i64;
    let about = format!(
        "When an injury to a body part goes below your KO point of {} then you are down for good. Your body will enter a coma and won't regain consciousness until all critical body parts have healed above 0.",
        num(ko)
    );
    rows.push(tipped("KO point:", num(ko), vec![], &about));
    // Run speed (F047080)
    let mph = (p.own_speed * 0.2237).round() as i32;
    let diff = ((p.own_speed - run) * 0.2237).round() as i32;
    let speed = match diff {
        d if d > 0 => format!("{mph} (+{d}) mph"),
        d if d < 0 => format!("{mph} (-{}) mph", -d),
        _ => format!("{mph} mph"),
    };
    // (F047093 writes the run speed in its red, 0x141f492c0, whenever it differs, faster too)
    // its tip (F047088): what makes it up
    let mut lines = vec![format!("Athletics Skill:\t{}\t", num(f.level(S::StatAthletics) as i64)), format!("Max possible speed:\t{} mph\t", num((run * 0.2237) as i64))];
    let legs = ((1.0 - f.legs) * 100.0) as i32;
    if legs > 0 {
        lines.push(format!("Leg injury:\t-{}%\tr", num(legs as i64)));
    }
    let heavy = ((1.0 - load) * 100.0) as i32;
    if heavy > 0 {
        lines.push(format!("Encumbrance:\t-{}%\tr", num(heavy as i64)));
    }
    lines.push(format!("Total speed:\t{} mph\tk", num((p.own_speed * 0.2237) as i64)));
    rows.push(Row { label: "Run speed:".into(), value: speed, colour: if diff != 0 { PENALTY } else { LIGHT }, tip: lines.join("\n"), button: None });
    // Hunger Rate (F047009)
    let [_, _, bed, enc, race_rate] = sim.hunger_c;
    let activity = if p.down || p.bed.is_some() {
        kenshi_char::hunger::Activity::Resting
    } else if load < 1.0 {
        kenshi_char::hunger::Activity::Encumbered { load }
    } else {
        kenshi_char::hunger::Activity::Normal
    };
    let rate = kenshi_char::hunger::hunger_rate(activity, race_rate, kenshi_char::hunger::CURRENT_WORK_RATE, bed, enc);
    // its tip the rate's lines (F047093 -> F047086)
    rows.push(Row { tip: hunger_rate_lines(sim, id).join("\n"), ..row("Hunger Rate:", format!("{}%", (rate * 100.0) as i32)) });
    rows
}

/// @fn F043077
///
/// The extending panel sliding (MainBarGUI's ExtendingPanel: the name, the state box and the
/// extended stats): out at its layout top, folded down until its bottom meets the information
/// panel's (F043077: the parent's height less its own, +0x1b4), the extended stats going under
/// the stats panel. (assumption: it slides over half a second, Kenshi's +0x1b8 rate 0.5)
pub fn extending_panel(mut hud: ResMut<Hud>, time: Res<Time>, mut nodes: Query<&mut Node>) {
    let (Some(panel), Some(parent)) = (hud.ui.get("ExtendingPanel"), hud.ui.get("InformationPanel")) else { return };
    let Ok([mut p, q]) = nodes.get_many_mut([panel, parent]) else { return };
    let (Val::Px(top), Val::Px(h), Val::Px(ph)) = (p.top, p.height, q.height) else { return };
    let out = *hud.unfolded_top.get_or_insert(top);
    let target = if hud.folded { 1.0 } else { 0.0 };
    let step = time.delta_secs() * 2.0;
    hud.fold = if hud.fold < target { (hud.fold + step).min(target) } else { (hud.fold - step).max(target) };
    let want = out + (ph - h - out) * hud.fold;
    if (top - want).abs() > 0.01 {
        p.top = Val::Px(want);
    }
}

/// A hover tip on a widget (the inventory's tooltip shows it): "title\ndescription", none
/// without a description.
fn tip(commands: &mut Commands, e: Option<Entity>, title: &str, text: &str) {
    let Some(e) = e else { return };
    if text.is_empty() {
        commands.entity(e).remove::<super::inventory_ui::ItemTip>();
    } else {
        commands.entity(e).insert((Interaction::default(), super::inventory_ui::ItemTip(format!("{title}\n{text}"))));
    }
}

/// "Cycle squads" (controls.cfg change_squad, Tab): the next squad with people shown and
/// selected ((assumption) the camera left where it is).
pub fn cycle_squads(act: super::bindings::Actions, mut hud: ResMut<Hud>, mut game: ResMut<Game>) {
    if !act.just("change_squad") || game.sim.player_squads.len() < 2 {
        return;
    }
    let n = game.sim.player_squads.len();
    for step in 1..=n {
        let k = (hud.active_squad + step) % n;
        let people: Vec<usize> = game.sim.squad_members(k).into_iter().filter(|&id| !game.sim.folk[id].dead).collect();
        if !people.is_empty() {
            hud.active_squad = k;
            game.selected = people;
            hud.seen_first = game.selected.first().copied();
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut hud: ResMut<Hud>,
    mut game: ResMut<Game>,
    mut clicks: MessageReader<GuiClick>,
    mut presses: ResMut<super::bindings::UiPresses>,
    mut nodes: Query<&mut Node>,
    time: Res<Time>,
    mut open: ResMut<super::OpenBuildings>,
    mut edits: MessageReader<GuiEdit>,
    boxes: Query<&EditText>,
    mut rig: ResMut<super::camera::CameraRig>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    // the outpost's name typed (TownPanel's OutpostNameEditBox)
    let name_box = hud.ui.get("OutpostNameEditBox");
    for ev in edits.read() {
        if Some(ev.widget) == name_box && !ev.text.trim().is_empty() {
            game.sim.outpost_name = ev.text.clone();
        }
    }
    game.sim.squads_ready();
    for c in clicks.read() {
        // a datapanel button of the building picked (building_ui)
        if let Some(action) = c.name.strip_prefix("Datapanel:") {
            let Game { selected_building, sim, selected, dismantle_request, .. } = &mut *game;
            if let Some(b) = selected_building.as_mut().and_then(|s| s.resolve(sim)) {
                super::building_ui::act(sim, selected, b, action, dismantle_request);
            }
            continue;
        }
        // a squad's tab: that squad shown, its people selected
        if c.name == "SquadTab"
            && let Some(k) = hud.tab.as_ref().and_then(|t| t.roots.iter().position(|&r| r == c.widget))
        {
            hud.active_squad = k;
            let people: Vec<usize> = game.sim.squad_members(k).into_iter().filter(|&id| !game.sim.folk[id].dead).collect();
            if !people.is_empty() {
                game.selected = people;
            }
            hud.seen_first = game.selected.first().copied();
            continue;
        }
        match c.name.as_str() {
            // the extended stats folded away or out again (ExtendedStatsButton, F043077's
            // delegate 0x140018b10)
            "ExtendedStatsButton" => {
                hud.folded = !hud.folded;
                if let Some(e) = hud.ui.get("ExtendedStatsButton") {
                    commands.entity(e).insert(kenshi_gui::ui::Checked(hud.folded));
                }
            }
            // the floor arrows: another floor of the opened building shown (0 - 3)
            "FloorArrowUp" | "FloorArrowDown" if open.open_any() => {
                let d = if c.name == "FloorArrowUp" { 1 } else { -1 };
                open.pick = Some((open.floor + d).clamp(0, 3));
                open.picked_at = open.own;
            }
            "TimeSpeedButton1" => game.paused = true,
            "TimeSpeedButton2" | "TimeSpeedButton3" | "TimeSpeedButton4" => {
                let k = c.name.chars().last().and_then(|d| d.to_digit(10)).unwrap_or(2) as usize - 2;
                game.speed = SPEEDS[k.min(SPEEDS.len() - 1)];
                game.paused = false;
            }
            "ShortcutInventoryButton" => presses.next.push("toggle_inventory".into()),
            "ShortcutStatsButton" => presses.next.push("toggle_stats".into()),
            "OpenToPublicCheckBox" => game.sim.outpost_public = !game.sim.outpost_public,
            "OrdersProspectButton" => {
                let sel = game.selected.clone();
                game.sim.order_prospect(&sel);
            }
            // the Medic and Rescue orders (kenshi_game::routines)
            "OrdersMedicButton" => {
                let sel = game.selected.clone();
                game.sim.order_medic(&sel);
            }
            "OrdersRescueButton" => {
                let sel = game.selected.clone();
                game.sim.order_rescue(&sel);
            }
            "OrdersSneakButton" => {
                let sel = game.selected.clone();
                game.sim.toggle_sneak(&sel);
            }
            // the JOBS button (named "OrdersChaseButton" in the layout): the job list on or off
            "OrdersChaseButton" => {
                let sel = game.selected.clone();
                game.sim.toggle_jobs(&sel);
            }
            // the stance orders (kenshi_char::stance)
            "OrdersBlockButton" | "OrdersHoldButton" | "OrdersPassiveButton" | "OrdersTauntButton" | "OrdersRangedButton" => {
                use kenshi_game::orders::StanceOrder::*;
                let which = match c.name.as_str() {
                    "OrdersBlockButton" => Block,
                    "OrdersHoldButton" => Hold,
                    "OrdersPassiveButton" => Passive,
                    "OrdersTauntButton" => Taunt,
                    _ => Ranged,
                };
                let sel = game.selected.clone();
                game.sim.toggle_stance(&sel, which);
            }
            _ => {
                if let Some(&(_, _, id, _)) = hud.squad.iter().find(|(_, b, _, _)| *b == c.widget) {
                    // F033287 -> F045566: alone selected; with shift added, or taken out if
                    // selected with others
                    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
                    if !shift {
                        game.selected = vec![id];
                    } else if !game.selected.contains(&id) {
                        game.selected.push(id);
                    } else if game.selected.len() >= 2 {
                        game.selected.retain(|&k| k != id);
                    }
                    // F033288: a portrait double-clicked (MyGUI's 250 ms) has the camera
                    // follow them (F045568 -> F045566 -> F045539)
                    let now = time.elapsed_secs();
                    if hud.portrait_click.is_some_and(|(who, t)| who == id && now - t <= 0.25) {
                        rig.follow(id);
                    }
                    hud.portrait_click = Some((id, now));
                }
            }
        }
    }
    // the texts, a few times a second
    let now = time.elapsed_secs();
    if now - hud.last < 0.2 {
        return;
    }
    hud.last = now;
    let ui = hud.ui.clone();
    // (testing) KENSHI_STANCE=<Block|Hold|Passive|Taunt|Ranged>[,...]: those orders given to the
    // selection once, at the start
    if let Ok(v) = std::env::var("KENSHI_STANCE")
        && now > 3.0
        && !hud.stance_tested
    {
        hud.stance_tested = true;
        use kenshi_game::orders::StanceOrder::*;
        let sel = game.selected.clone();
        for w in v.split(',') {
            let which = match w {
                "Block" => Block,
                "Hold" => Hold,
                "Passive" => Passive,
                "Taunt" => Taunt,
                _ => Ranged,
            };
            game.sim.toggle_stance(&sel, which);
        }
    }
    // the stance buttons held down as the first selected has them
    if let Some(&first) = game.selected.first() {
        use kenshi_game::orders::StanceOrder::*;
        for (name, which) in [("OrdersBlockButton", Block), ("OrdersHoldButton", Hold), ("OrdersPassiveButton", Passive), ("OrdersTauntButton", Taunt), ("OrdersRangedButton", Ranged)] {
            if let Some(e) = ui.get(name) {
                commands.entity(e).insert(kenshi_gui::ui::Checked(game.sim.has_stance(first, which)));
            }
        }
    }
    // the JOBS button as the first selected has it (its job list: `jobs_panel`)
    if let Some(&first) = game.selected.first()
        && let Some(p) = game.sim.folk.get(first)
    {
        if let Some(e) = ui.get("OrdersChaseButton") {
            commands.entity(e).insert(kenshi_gui::ui::Checked(p.jobs_on));
        }
        // SNEAK held down while the first selected sneaks
        if let Some(e) = ui.get("OrdersSneakButton") {
            commands.entity(e).insert(kenshi_gui::ui::Checked(p.sneaking));
        }
    }
    if let Some(p) = ui.get("PausedPanel") {
        commands.queue(SetVisible(p, game.paused));
    }
    let hour = game.sim.hour();
    let day = 1 + (game.sim.hours_gone() + game.sim.start_hour) as i64 / 24;
    // F043067: "Day: {1,num}", the time, "c.{1,num}", "Floor {1,num}"
    if let Some(e) = ui.get("DayText") {
        commands.queue(SetCaption(e, format!("Day: {}", num(day))));
    }
    if let Some(e) = ui.get("TimeText") {
        commands.queue(SetCaption(e, format!("{:02}:{:02}", hour as u32, ((hour.fract()) * 60.0) as u32)));
    }
    if let Some(e) = ui.get("MoneyAmountText") {
        commands.queue(SetCaption(e, format!("c.{}", num(game.sim.money as i64))));
    }
    // the floor shown (the opened building's: the selection's, or the arrows' pick; 0 outdoors)
    if let Some(e) = ui.get("FloorText") {
        let f = if open.open_any() { open.floor } else { 0 };
        commands.queue(SetCaption(e, format!("Floor {f}")));
    }
    town_panel(&mut commands, &ui, &game, &boxes);
    // a building picked (building_ui): its view in the main panel's datapanels
    let building = {
        let Game { selected_building, sim, .. } = &mut *game;
        selected_building.as_mut().and_then(|s| s.resolve(sim))
    };
    if building.is_none() {
        game.selected_building = None;
    }
    if let Some(b) = building {
        let v = super::building_ui::view(&game.sim, b);
        if let Some(e) = ui.get("HealthText") {
            commands.queue(SetCaption(e, v.status.0.clone()));
        }
        tip(&mut commands, ui.get("StatusPanel"), &v.status.0, &v.status.1);
        if let Some(panel) = ui.get("BountyPanel") {
            commands.queue(SetVisible(panel, false));
        }
        draw_bars(&mut commands, &mut hud, &ui, &mut nodes, &v.bars);
        hud.rows(&mut commands, "NameText", v.name, false, LIGHT, false);
        hud.rows(&mut commands, "StateText", v.info, false, LIGHT, false);
        hud.rows(&mut commands, "StatsText", v.stats, true, LIGHT, false);
        hud.rows(&mut commands, "ExtendedStatsText", v.extended, false, LIGHT, false);
    }
    // the selected character
    if building.is_some() {
    } else if let Some(&id) = game.selected.first() {
        let sim = &game.sim;
        let p = &sim.folk[id];
        let f = &sim.arena.f[id];
        let m = &f.medical;
        let name = sim.display_name(id);
        // (the player's faction by the name it was given, F035114)
        let faction = match p.faction.as_deref() {
            Some(kenshi_game::sim::PLAYER_FACTION) => sim.player_faction_name(),
            f => f.and_then(|s| sim.data.get(s)).map_or(String::new(), |r| r.name.clone()),
        };
        // the name, and its faction faded at the right
        let name_row = vec![Row { label: name, value: faction, colour: FADED, tip: String::new(), button: None }];
        // the medical state (F047087's status list), on the status box
        let (status, about) = status_line(sim, id);
        if let Some(e) = ui.get("HealthText") {
            commands.queue(SetCaption(e, status.to_string()));
        }
        tip(&mut commands, ui.get("StatusPanel"), status, &about);
        // the character's state, goal and encumbrance (F039272)
        // F039272's slave, crime and prison states first
        let (state, about) = state_line(sim, id);
        tip(&mut commands, ui.get("StateText"), state, &about);
        let goal = p
            .ai
            .current
            .as_ref()
            .and_then(|t| kenshi_ai::task_def::def(t.task))
            .map_or("Aimless", |d| d.name);
        let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
        let capacity = ec.carry_weight_mult * f.level(kenshi_char::StatsEnumerated::StatStrength) + ec.encumbrance_base;
        let enc = (p.inventory.weight() / capacity.max(1.0) * 100.0).round() as i32;
        let mut info = vec![row("State:", state), row("Goal:", goal), row("Encumbrance:", format!("{enc}%"))];
        // the current skill (F039272, its "Current Skill" line): in a fight the wielded weapon's
        // category's (CharStats +0x3d8: Heavy, Katana, Hacker, Sabre, Martial Arts unarmed,
        // Polearm; blunt weapons none), else sneaking Stealth, else moving over 9 Athletics
        // (CharMovement +0xb8). (not ported: a job's own skill, CharStats +0x198, and the
        // crossbow's lines in ranged mode)
        {
            use kenshi_char::StatsEnumerated as S;
            use kenshi_combat::WeaponCategory as W;
            let skill = if f.active && !f.shooting {
                match f.wielded.category {
                    W::SkillHeavy => Some(("Heavy:", S::StatHeavyweapons)),
                    W::SkillKatanas => Some(("Katana:", S::StatKatanas)),
                    W::SkillHackers => Some(("Hacker:", S::StatHackers)),
                    W::SkillSabres => Some(("Sabre:", S::StatSabres)),
                    W::SkillUnarmed => Some(("Martial Arts:", S::StatMartialarts)),
                    W::AttackPolearms => Some(("Polearm:", S::StatPolearms)),
                    _ => None,
                }
            } else if p.sneaking {
                Some(("Stealth:", S::StatStealth))
            } else if p.speed > 9.0 {
                Some(("Athletics:", S::StatAthletics))
            } else {
                None
            };
            if let Some((label, stat)) = skill {
                info.push(row(label, (f.level(stat) as i32).to_string()));
            }
        }
        // F039271: "Imprisoned, sentence time remaining:"
        if let Some(q) = p.prison.as_ref().filter(|q| !q.slave) {
            let left = (q.until - sim.hours_gone()).max(0.0);
            info.push(row("Sentence remaining:", if left > 99999.0 { "Life".to_string() } else { format!("{left:.0} hrs") }));
        }
        // the bounty panel (BountyPanel, F039261 "Bounty:"): the bounties of the selected
        if let (Some(panel), Some(text)) = (ui.get("BountyPanel"), ui.get("BountyText")) {
            let total: i32 = p.bounties.iter().map(|b| b.amount).sum();
            commands.queue(SetVisible(panel, total > 0));
            let factions: Vec<String> = p.bounties.iter().filter_map(|b| sim.data.get(&b.faction).map(|r| r.name.clone())).collect();
            hud.rows(&mut commands, "BountyText", vec![row("Bounty:", format!("c.{} ({})", num(total as i64), factions.join(", ")))], false, LIGHT, false);
            let _ = text;
        }
        // the combat stats (F047092): the first by the stats' mode (F047026: no weapon, or a
        // martial arts one, Martial Arts; a crossbow in the ranged stance, Crossbows; else
        // Attack and Defense), then Strength, Dexterity, Toughness and (not with the crossbow)
        // Dodge; a bonus "(+N)" in green, a penalty "(N)" in red
        use kenshi_char::StatsEnumerated as S;
        let with = |label: &str, total: f32, bonus: i32| {
            let n = total as i32;
            match bonus {
                0 => row(label, n.to_string()),
                b if b > 0 => Row { label: label.into(), value: format!("{n} (+{b})"), colour: BONUS, tip: String::new(), button: None },
                b => Row { label: label.into(), value: format!("{n} ({b})"), colour: PENALTY, tip: String::new(), button: None },
            }
        };
        let mode = if f.wielded.category == kenshi_combat::WeaponCategory::SkillUnarmed {
            1
        } else if p.stance.ranged && kenshi_combat::arena::ranged_usable(f) {
            2
        } else {
            0
        };
        let mut stats = Vec::new();
        match mode {
            1 => {
                let total = f.attack_level();
                stats.push(with("MARTIAL ARTS:", total, (total - f.level(S::StatMartialarts)).round() as i32));
            }
            2 => stats.push(row("CROSSBOWS:", (f.ranged_level(S::StatCrossbows) as i32).to_string())),
            _ => {
                // the attack skill (F047043: the attack bonus + melee attack, the weapon's own
                // skill not in it) and the buffed melee defence (F047044: the defence bonus +
                // melee defence, +20 in the block stance, "Defensive combat mode")
                // their tooltips (F047092): the equipment's bonus, the weapon's indoors penalty
                // (`indoors_penalty`; "-" outside), the weather's ("-" with none), the base hit
                // chance; the block stance's +20 in green and what defense is
                let dust = if f.dust > 0.0 { (f.dust * 10.0) as i32 } else { 0 };
                let indoors = if p.inside.is_some() { bonus_line(indoors_penalty(&game.sim, p)) } else { "-\t".into() };
                let mut attack = with("ATTACK:", f.attack_level(), f.attack_bonus());
                attack.tip = [
                    format!("Total equipment bonuses:\t{}", bonus_line(f.gear.attack + f.weapon_mods.0)),
                    format!("Weapon indoors penalty:\t{indoors}"),
                    format!("Weather penalty:\t{}", if dust != 0 { bonus_line(-dust) } else { "-\t".into() }),
                    format!("Base hit chance:\t{}%\t", f.attack_level() as i32),
                ]
                .join("\n");
                stats.push(attack);
                let block = if p.stance.block { 20 } else { 0 };
                let mut defense = with("DEFENSE:", f.melee_defence(true), f.defence_bonus() + block);
                let mut lines = vec![format!("Total equipment bonuses:\t{}", bonus_line(f.defence_bonus()))];
                if p.stance.block {
                    lines.push("Defensive combat mode:\t+20\tg".into());
                }
                lines.push(format!("Weapon indoors penalty:\t{indoors}"));
                lines.push("Your defense skill is your chance of blocking enemy attacks.".into());
                defense.tip = lines.join("\n");
                stats.push(defense);
            }
        }
        stats.push(row("STRENGTH:", (f.level(S::StatStrength) as i32).to_string()));
        stats.push(row("DEXTERITY:", (f.level(S::StatDexterity) as i32).to_string()));
        stats.push(row("TOUGHNESS:", (f.level(S::StatToughness) as i32).to_string()));
        if mode != 2 {
            // F047048 buffed against base: the armour's dodge mult
            let base = f.level(S::StatDodge);
            let buffed = base * f.gear.dodge;
            stats.push(with("DODGE:", buffed, (buffed - base + 0.5).floor() as i32));
        }
        // the bars (F047087): blood, the body parts head to legs, hunger
        let mut bars: Vec<Bar> = Vec::new();
        // (a robot's is its oil); its tip the race's bleed rate, over 1 in #59231a, under in
        // #a8b774
        let bleed = f.body.bleed;
        let bleed_tag = if bleed > 1.0 { "#59231a" } else if bleed < 1.0 { "#a8b774" } else { "" };
        bars.push(Bar {
            label: if f.body.robot { "Oil" } else { "Blood" }.into(),
            value: format!("{}", (m.blood + 0.5) as i32),
            arrow: String::new(),
            tip: format!("{bleed_tag}Racial bleed rate multiplier: x{bleed:.2}"),
            widths: [(m.blood / m.max_blood.max(1.0)).max(0.0), 0.0, 0.0, 0.0],
            robot: false,
            crushed: false,
        });
        let ko = f.ko_point();
        let unconscious = m.knocked_out || m.dead;
        let can_kick = !m.legs_collapsed(f.body.animal, ko) && m.parts.iter().filter(|q| q.kind == 1).all(|q| q.health() >= 0.0);
        let mut order: Vec<usize> = (0..m.parts.len()).collect();
        order.sort_by_key(|&k| part_order(&m.parts[k], &sim.data));
        for k in order {
            let q = &m.parts[k];
            let max = q.max_health().max(1e-3);
            let green = ((q.flesh - q.stun) / max).max(0.0);
            let yellow = ((q.flesh + q.treatment - q.stun) / max).max(0.0);
            let red = ((max - q.stun) / max).max(0.0);
            let white = if q.slot == 1 { 0.0 } else { (q.bonus * 0.01 + green).clamp(0.0, 1.0) };
            let name = sim.data.get(&q.id).map_or(q.id.clone(), |r| r.name.clone());
            let (arrow, part_tip) = part_tip(q, &name, f.body.robot, ko, unconscious, can_kick);
            bars.push(Bar {
                label: name,
                value: format!("{}", (q.flesh - q.stun + 0.5).floor() as i32),
                arrow,
                tip: part_tip,
                widths: [green, yellow, red, white],
                robot: q.robotic || q.slot == 2,
                crushed: q.slot == 1 || q.slot == 3,
            });
        }
        // hunger: x100; the arrow: digesting ">", else "<" (none in bed), "<!" under 2 (its
        // "<<!!" under 1 is never reached: Kenshi tests under 2 first); a robot's empty, its tip
        // "Skeletons don't need to eat."
        let h = m.hunger.level;
        let arrow = if 0.0 < m.hunger.stomach {
            ">"
        } else if h < 2.0 {
            "<!"
        } else if p.bed.is_some() {
            ""
        } else {
            "<"
        };
        bars.push(if f.body.robot {
            Bar { label: "Hunger".into(), value: String::new(), arrow: String::new(), tip: "Skeletons don't need to eat.".into(), widths: [0.0; 4], robot: false, crushed: false }
        } else {
            Bar {
                label: "Hunger".into(),
                value: format!("{}", (h * 100.0) as i32),
                arrow: arrow.into(),
                tip: hunger_tip(sim, id),
                widths: [(h - 2.0).max(0.0), (h - 1.0).max(0.0), h.max(0.0), 0.0],
                robot: false,
                crushed: false,
            }
        });
        draw_bars(&mut commands, &mut hud, &ui, &mut nodes, &bars);
        hud.rows(&mut commands, "NameText", name_row, false, LIGHT, false);
        // the state box over the stats (StateText), the combat stats below (StatsText)
        hud.rows(&mut commands, "StateText", info, false, LIGHT, false);
        hud.rows(&mut commands, "StatsText", stats, true, LIGHT, false);
        // the extended stats under the state box (ExtendedStatsText, DatapanelGUI mode 7)
        hud.rows(&mut commands, "ExtendedStatsText", extended_stats(sim, id), false, LIGHT, false);
    }
    let _ = &mut game;
}

/// @fn F043058
///
/// The town panel (TownPanel, F043042): shown while the selected character is in the player's
/// outpost (F043067; `Sim::in_outpost`): its name to edit, open to the public, the research
/// ("None", or the first queued with how far it is), the power (drawn of supplied, "{1,num}/{2,num}")
/// and the batteries' charge of their capacity, "<< " before it while they drain, " >>" after it
/// while they charge.
fn town_panel(commands: &mut Commands, ui: &Built, game: &Game, boxes: &Query<&EditText>) {
    let sim = &game.sim;
    let show = game.selected.first().is_some_and(|&id| sim.in_outpost(sim.folk[id].mover.position_units()));
    let Some(panel) = ui.get("TownPanel") else { return };
    commands.queue(SetVisible(panel, show));
    if !show {
        return;
    }
    if let Some(e) = ui.get("OutpostNameEditBox")
        && !boxes.get(e).is_ok_and(|t| t.focused)
    {
        commands.queue(SetCaption(e, sim.outpost_name.clone()));
    }
    if let Some(e) = ui.get("OpenToPublicCheckBox") {
        commands.entity(e).insert(kenshi_gui::ui::Checked(sim.outpost_public));
    }
    let set = |commands: &mut Commands, w: &str, t: String| {
        if let Some(e) = ui.get(w) {
            commands.queue(SetCaption(e, t));
        }
    };
    let bar = |commands: &mut Commands, w: &str, frac: f32| {
        if let Some(e) = ui.get(w) {
            commands.queue(SetProgress(e, frac));
        }
    };
    let first = sim.research_queue.first().and_then(|r| sim.data.get(r));
    set(commands, "ResearchProgressBarCaption", first.map_or("None".to_string(), |r| r.name.clone()));
    bar(commands, "ResearchProgressBar", first.map_or(0.0, |r| sim.research_progress / kenshi_game::production::research_time(r).max(0.01)));
    let (supply, demand) = (sim.power_supply, sim.power_demand);
    bar(commands, "PowerProgressBar", if supply <= 0.0 { if demand > 0.0 { 1.0 } else { 0.0 } } else { demand / supply });
    set(commands, "PowerProgressBarCaption", format!("{}/{}", num(demand as i64), num(supply as i64)));
    let mut charge = format!("{}/{}", num(sim.battery as i64), num(sim.battery_capacity as i64));
    if supply > demand && sim.battery < sim.battery_capacity {
        charge.push_str(" >>");
    } else if supply < demand && sim.battery > 0.0 {
        charge.insert_str(0, "<< ");
    }
    set(commands, "BatteryProgressBarCaption", charge);
    bar(commands, "BatteryProgressBar", if sim.battery_capacity > 0.0 { sim.battery / sim.battery_capacity } else { 0.0 });
}

/// The squad panel: the squad's tab (Kenshi_SquadTabButton, ButtonWidth 200, the squad's name)
/// and a portrait (Kenshi_PortraitCharacter.layout) per member in the squad's area, its face
/// drawn live (`portraits`), its name over it; clicking one selects it.
/// (assumption) The player's squad is named "<faction>_0"; a portrait is 92 units square with 4
/// between, from the area's corner, as the game's panel looks.
#[allow(clippy::too_many_arguments)]
pub fn squad_panel(
    mut commands: Commands,
    mut hud: ResMut<Hud>,
    game: Res<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    portraits: Res<super::portraits::Portraits>,
    mut images: Query<&mut ImageNode>,
) {
    let scale = kenshi_gui::res::ui_scale();
    // the squad shown follows the selection: the first selected one's squad
    let first = game.selected.first().copied();
    if first != hud.seen_first {
        hud.seen_first = first;
        if let Some(k) = first.and_then(|id| game.sim.player_squad_index(id)) {
            hud.active_squad = k;
        }
    }
    hud.active_squad = hud.active_squad.min(game.sim.player_squads.len().saturating_sub(1));
    // the tabs, one per squad (its name), the shown one selected; made again on a change
    let names: Vec<String> = game.sim.player_squads.iter().map(|s| s.name.to_uppercase()).collect();
    let want = (names.clone(), hud.active_squad);
    if hud.tabs_for != want
        && !names.is_empty()
        && let Some(place) = hud.ui.get("SquadPanel")
    {
        if let Some(old) = hud.tab.take() {
            for r in old.roots {
                commands.entity(r).despawn();
            }
        }
        // on the panel's header (the template's HeaderPlace: 20 in, 37 high of 716 x 220); (as
        // many as fit) side by side at the tab control's ButtonWidth 200
        let size = hud.ui.sizes.get(&place).copied().unwrap_or([600.0, 200.0]);
        let w = (200.0 * scale).min((size[0] - 40.0 * scale) / names.len() as f32);
        let defs: Vec<kenshi_gui::res::WidgetDef> = names
            .iter()
            .enumerate()
            .map(|(k, n)| kenshi_gui::res::WidgetDef {
                kind: "Button".into(),
                skin: "Kenshi_SquadTabButton".into(),
                pos: kenshi_gui::res::Pos::Real([(20.0 * scale + k as f32 * w) / size[0], 0.0, w / size[0], 37.0 / 220.0]),
                align: Default::default(),
                name: "SquadTab".into(),
                layer: None,
                props: vec![("Caption".into(), n.clone()), ("StateSelected".into(), (k == hud.active_squad).to_string())],
                user: Vec::new(),
                children: Vec::new(),
            })
            .collect();
        hud.tab = Some(kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, Some(place), size));
        hud.tabs_for = want;
    }
    let squad: Vec<usize> = if game.sim.player_squads.is_empty() {
        game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect()
    } else {
        game.sim.squad_members(hud.active_squad)
    };
    let made: Vec<usize> = hud.squad.iter().map(|s| s.2).collect();
    if squad != made {
        hud.squad_names.clear();
        for (roots, _, _, _) in hud.squad.drain(..) {
            for r in roots {
                commands.entity(r).despawn();
            }
        }
        let Some(area) = hud.ui.get("SquadTabItem") else { return };
        let area_size = hud.ui.sizes.get(&area).copied().unwrap_or([600.0, 160.0]);
        let side = 136.0 * scale;
        for (k, &id) in squad.iter().enumerate() {
            let mut defs = gui.0.layout("Kenshi_PortraitCharacter.layout");
            if let Some(d) = defs.first_mut() {
                let x = 4.0 * scale + k as f32 * (side + 4.0 * scale);
                d.pos = kenshi_gui::res::Pos::Real([x / area_size[0], 4.0 * scale / area_size[1], side / area_size[0], side / area_size[1]]);
                // the name over the portrait is a label, not a box to type in: it covers the
                // whole portrait, so a click on it took the keyboard (renaming, the camera keys
                // dead) instead of selecting
                fn read_only(d: &mut kenshi_gui::WidgetDef) {
                    if d.name == "NameText" {
                        d.props.push(("ReadOnly".to_string(), "true".to_string()));
                    }
                    d.children.iter_mut().for_each(read_only);
                }
                read_only(d);
                // the layout's parts are in its design pixels (136 square): at the screen's
                // scale, as the root is
                fn scaled(d: &mut kenshi_gui::WidgetDef, k: f32) {
                    if let kenshi_gui::res::Pos::Px(p) = &mut d.pos {
                        p.iter_mut().for_each(|v| *v *= k);
                    }
                    d.children.iter_mut().for_each(|c| scaled(c, k));
                }
                d.children.iter_mut().for_each(|c| scaled(c, scale));
            }
            let b = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, Some(area), area_size);
            let name = game.sim.display_name(id);
            if let Some(e) = b.get("NameText") {
                commands.queue(SetCaption(e, name.clone()));
            }
            hud.squad_names.push((b.get("NameText"), name));
            let button = b.get("BorderPanel").unwrap_or(b.roots[0]);
            // (assumption) the status overlays (Kenshi_PortraitImage's BackOverlay and
            // FrontOverlay groups) only show for a status the code sets: hidden here
            for w in ["BackImage", "FrontImage"] {
                if let Some(e) = b.get(w) {
                    commands.queue(SetVisible(e, false));
                }
            }
            hud.squad.push((b.roots.clone(), button, id, b.get("PortraitImage")));
        }
    }
    // the names, when one changed
    for k in 0..hud.squad.len().min(hud.squad_names.len()) {
        let name = game.sim.display_name(hud.squad[k].2);
        if hud.squad_names[k].1 != name {
            if let Some(e) = hud.squad_names[k].0 {
                commands.queue(SetCaption(e, name.clone()));
            }
            hud.squad_names[k].1 = name;
        }
    }
    // the faces
    for (_, _, id, img) in &hud.squad {
        let (Some(img), Some((_, image))) = (img, portraits.by.get(id)) else { continue };
        match images.get_mut(*img) {
            Ok(mut i) => {
                if i.image != *image {
                    i.image = image.clone();
                }
            }
            Err(_) => {
                let e = super::portraits::EXPOSURE;
                commands.entity(*img).insert(ImageNode { image: image.clone(), color: Color::linear_rgb(e, e, e), ..default() });
            }
        }
    }
}

/// The area banner (MainBarGUI update, F043067): the region under the camera (BIOME_GROUP), new
/// and not shown in the last 30 s (480 s for the one just left), named on the banner for 5 s,
/// fading in and out ((assumption) at 1 a second); "NONE" and 18003-gamedata.base are never
/// named. (Its "New_Biome" sound: sound::regions.)
pub fn banner(mut commands: Commands, mut hud: ResMut<Hud>, game: Res<Game>, weather: Option<Res<super::WeatherState>>, time: Res<Time>) {
    let dt = time.delta_secs();
    let (Some(panel), Some(text)) = (hud.ui.get("BiomeTextPanel"), hud.ui.get("BiomeTextBox")) else { return };
    let Some(region) = weather.as_ref().and_then(|w| w.current) else { return };
    let b = &mut hud.banner;
    b.cooldown -= dt;
    b.left -= dt;
    if Some(region) == b.shown || b.cooldown > 0.0 {
        if b.visible {
            if b.left <= 0.0 {
                b.alpha -= dt;
                if b.alpha < 0.0 {
                    b.visible = false;
                    commands.queue(SetVisible(panel, false));
                }
            } else if b.alpha < 1.0 {
                b.alpha = (b.alpha + dt).min(1.0);
            }
            commands.queue(SetAlpha(panel, b.alpha.max(0.0)));
        }
        return;
    }
    let sid = weather.as_ref().map(|w| w.regions[region].biome_group.clone()).unwrap_or_default();
    let name = game.sim.data.get(&sid).map_or(String::new(), |r| r.name.clone());
    if std::env::var("KENSHI_BANNER_DEBUG").is_ok() {
        eprintln!("banner: region {region} {sid} '{name}'");
    }
    if !name.starts_with("NONE") && sid != "18003-gamedata.base" && !name.is_empty() {
        commands.queue(SetCaption(text, name));
        commands.queue(SetVisible(panel, true));
        b.visible = true;
        b.alpha = 0.0;
        commands.queue(SetAlpha(panel, 0.0));
        b.left = 5.0;
    }
    b.cooldown = if Some(region) == b.before { 480.0 } else { 30.0 };
    b.before = b.shown;
    b.shown = Some(region);
}


/// The main panel's keys (F031456's actions, as its buttons do): the stances (toggle_block,
/// toggle_hold, toggle_passive, toggle_ranged, toggle_taunt), JOBS (toggle_jobs), the Medic,
/// Rescue and Prospect orders, the floor arrows (floor_up, floor_down), the next and previous
/// character of the squad shown (character_next, character_prev) and the panel itself hidden
/// or shown (toggle_bar).
pub fn hotkeys(act: super::bindings::Actions, mut hud: ResMut<Hud>, mut game: ResMut<Game>, mut open: ResMut<super::OpenBuildings>) {
    if act.is_typing() {
        return;
    }
    use kenshi_game::orders::StanceOrder::*;
    let sel = game.selected.clone();
    for (action, which) in [("toggle_block", Block), ("toggle_hold", Hold), ("toggle_passive", Passive), ("toggle_ranged", Ranged), ("toggle_taunt", Taunt)] {
        if act.just(action) {
            game.sim.toggle_stance(&sel, which);
        }
    }
    if act.just("toggle_jobs") {
        game.sim.toggle_jobs(&sel);
    }
    if act.just("medic") {
        game.sim.order_medic(&sel);
    }
    if act.just("rescue") {
        game.sim.order_rescue(&sel);
    }
    if act.just("prospect") {
        game.sim.order_prospect(&sel);
    }
    for (action, d) in [("floor_up", 1), ("floor_down", -1)] {
        if act.just(action) && open.open_any() {
            open.pick = Some((open.floor + d).clamp(0, 3));
            open.picked_at = open.own;
        }
    }
    for (action, d) in [("character_next", 1i64), ("character_prev", -1)] {
        if act.just(action) {
            let squad: Vec<usize> = game.sim.squad_members(hud.active_squad).into_iter().filter(|&id| !game.sim.folk[id].dead).collect();
            if !squad.is_empty() {
                let at = game.selected.first().and_then(|f| squad.iter().position(|id| id == f)).map_or(0, |k| (k as i64 + d).rem_euclid(squad.len() as i64) as usize);
                game.selected = vec![squad[at]];
            }
        }
    }
    if act.just("toggle_bar") {
        hud.hidden = !hud.hidden;
    }
}

/// The main panel hidden while the character editor is open over the game (the plastic
/// surgeon's), as the editor at a new game's start has none.
pub fn hide_under_editor(hud: Option<Res<Hud>>, ed: Option<Res<super::editor::Editor>>, mut vis: Query<&mut Visibility>) {
    let Some(hud) = hud else { return };
    // (and while "Toggle main gui" hides it)
    let want = if ed.is_some() || hud.hidden { Visibility::Hidden } else { Visibility::Inherited };
    let roots = hud.ui.roots.iter().chain(hud.squad.iter().flat_map(|s| s.0.iter())).chain(hud.tab.iter().flat_map(|t| t.roots.iter()));
    for &e in roots {
        if let Ok(mut v) = vis.get_mut(e)
            && *v != want
        {
            *v = want;
        }
    }
}

/// The AI_TASKs whose jobs are drawn at full alpha with JOBS off (F036879: Attacking enemies,
/// Staying close, Bodyguard, Robotics, Medic, Splint Rigging); the rest are faded then.
const JOBS_ALWAYS: [u16; 6] = [16, 31, 45, 57, 58, 250];
/// The job rows' text colour: the game's colour string 0x141f49248, "#afa68b" (F043041).
const JOB_TEXT: Color = Color::srgb(0xaf as f32 / 255.0, 0xa6 as f32 / 255.0, 0x8b as f32 / 255.0);
/// The Obedient Slave job (F030705).
const OBEDIENT_SLAVE: u16 = 187;

/// @fn F043047
///
/// The jobs panel (JobsPanel, an ItemBox of Kenshi_OrdersPanelOrder rows, OrderCellView
/// F043070): the first selected character's jobs, a row each, "{n}: " and the job's name with,
/// for a job of a target-taking kind (its definition's +0x08), its target's (F043041), in the
/// game's colour #afa68b; at full alpha while JOBS is on or for the jobs always done
/// (F036879), else at half (F043026). The info text (JobsPanelInfo) shows only with no jobs.
/// A row's X removes its job (F043053 -> F036800); a row dragged onto another moves its job
/// there, or dropped on the panel below the rows to the end (F043049 -> F036810). Rows are 24
/// high and the panel's width less 4, at the UI scale (F043076).
/// (stand-in) The slave's Obedient Slave job is a flag here (`Slave::obedient`): its row is
/// first and stays first. (assumption) The list scrolls by whole rows (a row a wheel notch), its
/// scroll bar (the ItemBox's VScroll, 18 wide: Kenshi_ScrollBarV's 17 x 1.1, F043076) shown
/// while the rows do not fit.
#[allow(clippy::too_many_arguments)]
pub fn jobs_panel(
    mut commands: Commands,
    mut hud: ResMut<Hud>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut clicks: MessageReader<GuiClick>,
    mouse: Res<ButtonInput<MouseButton>>,
    (mut wheel, mut scrolled): (MessageReader<bevy::input::mouse::MouseWheel>, MessageReader<kenshi_gui::GuiScroll>),
    windows: Query<&Window>,
    rows: Query<(&JobRow, &Interaction, &bevy::ui::RelativeCursorPosition)>,
    over: Query<&bevy::ui::RelativeCursorPosition, Without<JobRow>>,
    (mut nodes, parts): (Query<&mut Node>, Query<&kenshi_gui::Parts>),
) {
    let Some(item_box) = hud.ui.get("JobsPanel") else { return };
    // the rows go in the item box's client area; its scroll bar beside
    let part = |n: &str| parts.get(item_box).ok().and_then(|p| p.0.get(n).copied());
    let panel = part("Client").unwrap_or(item_box);
    let bar = part("VScroll");
    let first = game.selected.first().copied().filter(|&id| game.sim.folk.get(id).is_some_and(|p| p.player));
    // (testing) KENSHI_JOBS=<AI_TASK>,...: those jobs given to the first selected once, on the
    // nearest other person
    if let (Ok(v), Some(id)) = (std::env::var("KENSHI_JOBS"), first)
        && !hud.jobs.tested
    {
        hud.jobs.tested = true;
        let at = game.sim.folk[id].mover.position_units();
        let near = game.sim.folk.iter().filter(|p| p.id != id).min_by(|a, b| {
            let d = |p: &kenshi_game::sim::Person| {
                let q = p.mover.position_units();
                (q[0] - at[0]).hypot(q[2] - at[2])
            };
            d(a).total_cmp(&d(b))
        });
        let target = near.map_or(id, |p| p.id);
        for task in v.split(',').filter_map(|t| t.trim().parse::<u16>().ok()) {
            let label = game.sim.job_label(task, target);
            game.sim.folk[id].jobs.push(kenshi_game::orders::Job { task, target, label });
        }
        eprintln!("jobs test: {:?}", game.sim.folk[id].jobs.iter().map(|j| &j.label).collect::<Vec<_>>());
    }
    // the panel clips its rows and tells where the mouse is over it
    if over.get(panel).is_err() {
        commands.entity(panel).insert(bevy::ui::RelativeCursorPosition::default());
        if let Ok(mut n) = nodes.get_mut(panel) {
            n.overflow = Overflow::clip();
        }
    }
    // the X of a row: its job removed (F043053)
    let removed: Vec<usize> = clicks
        .read()
        .filter(|c| c.name == "RemoveButton")
        .filter_map(|c| hud.jobs.rows.iter().position(|(_, x)| *x == c.widget))
        .collect();
    if let Some(id) = first {
        for k in removed {
            let at = hud.jobs.scroll + k;
            remove_job(&mut game.sim.folk[id], at);
        }
    }
    let scale = kenshi_gui::res::ui_scale();
    let size = hud.ui.sizes.get(&panel).or_else(|| hud.ui.sizes.get(&item_box)).copied().unwrap_or([300.0, 120.0]);
    let h = (24.0 * scale).floor().max(1.0);
    let fit = ((size[1] / h).floor() as usize).max(1);
    // the job list as the rows show it: its text, and whether at full alpha
    let (tasks, list): (Vec<u16>, Vec<(String, bool)>) = first.map_or_else(Default::default, |id| {
        let p = &game.sim.folk[id];
        let obedient = p.slave.as_ref().is_some_and(|s| s.obedient);
        let jobs = obedient.then(|| (OBEDIENT_SLAVE, "Obedient Slave".to_string())).into_iter().chain(p.jobs.iter().map(|j| {
            let text = match kenshi_ai::task_def::def(j.task) {
                Some(d) if d.f08 == 0 => d.name.to_string(),
                _ => j.label.clone(),
            };
            (j.task, text)
        }));
        jobs.enumerate().map(|(k, (task, text))| (task, (format!("{}: {text}", k + 1), p.jobs_on || JOBS_ALWAYS.contains(&task)))).unzip()
    });
    // the wheel over the panel scrolls it
    let hovered = over.get(panel).is_ok_and(|r| r.cursor_over());
    let notches: f32 = wheel.read().map(|w| w.y.signum()).sum();
    let max = list.len().saturating_sub(fit);
    if hovered && notches != 0.0 {
        hud.jobs.scroll = (hud.jobs.scroll as i64 - notches as i64).clamp(0, max as i64) as usize;
    }
    // its scroll bar moved
    for m in scrolled.read() {
        if Some(m.widget) == bar {
            hud.jobs.scroll = (m.pos.round() as usize).min(max);
        }
    }
    hud.jobs.scroll = hud.jobs.scroll.min(list.len().saturating_sub(fit));
    // a row dragged and dropped (F043049): onto another row, or the panel below the rows (the
    // end); anywhere else nothing
    let cursor = windows.iter().next().and_then(|w| w.cursor_position());
    if mouse.just_pressed(MouseButton::Left)
        && let Some((r, _, _)) = rows.iter().find(|(_, i, _)| **i == Interaction::Pressed)
        && let Some(c) = cursor
    {
        hud.jobs.drag = Some((r.0, c));
    }
    if let Some((from, at)) = hud.jobs.drag {
        if !mouse.pressed(MouseButton::Left) {
            hud.jobs.drag = None;
            let moved = cursor.is_some_and(|c| c.distance(at) >= 1.0);
            let onto = rows.iter().find(|(r, _, rel)| r.0 != from && rel.cursor_over()).map(|(r, _, _)| Some(r.0));
            let to = onto.or_else(|| hovered.then_some(None));
            if moved
                && let (Some(to), Some(id)) = (to, first)
            {
                move_job(&mut game.sim.folk[id], from, to);
            }
            // the rows made again where they belong
            hud.jobs.shown = None;
        } else if let (Some(c), Some(&(e, _))) = (cursor, from.checked_sub(hud.jobs.scroll).and_then(|k| hud.jobs.rows.get(k)))
            && let Ok(mut n) = nodes.get_mut(e)
        {
            // the dragged row follows the mouse
            n.top = Val::Px((from - hud.jobs.scroll) as f32 * h + c.y - at.y);
        }
    }
    let want = first.map(|id| (id, list.clone(), hud.jobs.scroll));
    if want == hud.jobs.shown {
        return;
    }
    hud.jobs.shown = want;
    hud.jobs.tasks = tasks;
    for (e, _) in hud.jobs.rows.drain(..) {
        commands.entity(e).try_despawn();
    }
    // the info text with no jobs (F043047)
    if let Some(info) = hud.ui.get("JobsPanelInfo") {
        commands.queue(SetVisible(info, list.is_empty()));
    }
    // the scroll bar while the rows do not fit, the rows narrower by it (F043047)
    let over_full = list.len() > fit;
    let w = size[0] - 4.0 * scale - if over_full { (17.0f32 * 1.1).floor() } else { 0.0 };
    if let Some(b) = bar {
        commands.entity(b).insert(if over_full { Visibility::Inherited } else { Visibility::Hidden });
        let (range, pos) = (max as f32, hud.jobs.scroll as f32);
        commands.queue(move |world: &mut World| {
            if let Some(mut s) = world.get_mut::<kenshi_gui::Scroll>(b) {
                s.range = range;
            }
        });
        commands.queue(kenshi_gui::SetScroll(b, pos));
    }
    let mut defs = gui.0.layout("Kenshi_OrdersPanelOrder.layout");
    let design = defs.first().map_or([300.0, 20.0], |d| match d.pos {
        kenshi_gui::res::Pos::Px(p) => [p[2], p[3]],
        kenshi_gui::res::Pos::Real(r) => [r[2] * size[0], r[3] * size[1]],
    });
    // the cell resized to the row (F043026: its setSize), its parts by their align
    if let Some(d) = defs.first_mut() {
        d.pos = kenshi_gui::res::Pos::Px([0.0, 0.0, design[0], design[1]]);
        d.align = kenshi_gui::res::Align::parse("Stretch");
    }
    for (k, (text, full)) in list.iter().enumerate().skip(hud.jobs.scroll).take(fit) {
        let row = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px((k - hud.jobs.scroll) as f32 * h),
                    width: Val::Px(w),
                    height: Val::Px(h),
                    ..default()
                },
                JobRow(k),
                Interaction::default(),
                bevy::ui::RelativeCursorPosition::default(),
            ))
            .id();
        commands.entity(panel).add_child(row);
        let cell = kenshi_gui::ui::spawn_design(&mut commands, &gui.0, &assets, &defs, Some(row), design, [w, h]);
        if let Some(t) = cell.get("OrderText") {
            commands.queue(SetCaption(t, text.clone()));
            commands.queue(move |world: &mut World| {
                if let Some(c) = world.get::<kenshi_gui::Caption>(t).map(|c| c.0)
                    && let Some(mut col) = world.get_mut::<TextColor>(c)
                {
                    col.0 = JOB_TEXT;
                }
            });
        }
        commands.queue(SetAlpha(row, if *full { 1.0 } else { 0.5 }));
        let x = cell.get("RemoveButton").unwrap_or(row);
        hud.jobs.rows.push((row, x));
    }
}

/// F036800 on the jobs panel's list (the Obedient Slave flag first when set): job `k` gone.
fn remove_job(p: &mut kenshi_game::sim::Person, k: usize) {
    let obedient = p.slave.as_ref().is_some_and(|s| s.obedient);
    if obedient && k == 0 {
        // F039272: "You can remove this job, but beware of the consequences..."
        if let Some(s) = p.slave.as_mut() {
            s.obedient = false;
        }
        return;
    }
    let k = k - obedient as usize;
    if k < p.jobs.len() {
        p.jobs.remove(k);
    }
}

/// F036810 on the jobs panel's list: job `from` taken out and put in at `to` (of the list
/// without it), or at the end (None). (stand-in) The Obedient Slave flag's row stays first.
fn move_job(p: &mut kenshi_game::sim::Person, from: usize, to: Option<usize>) {
    let skip = p.slave.as_ref().is_some_and(|s| s.obedient) as usize;
    if from < skip || from - skip >= p.jobs.len() {
        return;
    }
    let job = p.jobs.remove(from - skip);
    match to {
        Some(t) => {
            let t = t.saturating_sub(skip).min(p.jobs.len());
            p.jobs.insert(t, job);
        }
        None => p.jobs.push(job),
    }
}

#[cfg(test)]
mod tests {
    use super::part_tip;
    use kenshi_char::medical::BodyPart;

    fn part(vital: bool, kind: i32) -> BodyPart {
        let mut q = BodyPart { vital, kind, max_a: 100.0, max_b: 1.0, max_c: 1.0, ..Default::default() };
        q.flesh = 100.0;
        q
    }

    #[test]
    fn part_tips_follow_the_health() {
        let q = part(true, 3);
        assert_eq!(part_tip(&q, "Head", false, -40.0, false, true), (String::new(), "Your Head is okay.".to_string()));
        let mut q = part(false, 2);
        q.flesh = 50.0;
        let (arrow, tip) = part_tip(&q, "Left Arm", false, -40.0, false, true);
        assert_eq!(arrow, "<");
        assert_eq!(tip, "Your Left Arm needs bandaging, it's slowly getting worse.\nYour combat and manual labour skills will be negatively affected.");
        q.flesh = -150.0;
        assert_eq!(part_tip(&q, "Left Arm", false, -40.0, false, true).0, "!!!");
        q.slot = 1;
        assert_eq!(part_tip(&q, "Left Arm", false, -40.0, false, true).1, "This limb is gone forever.");
        let mut q = part(true, 0);
        q.flesh = -60.0;
        assert_eq!(part_tip(&q, "Chest", false, -40.0, false, true), ("<<!!".to_string(), "This is a critical wound. If it doesn't get medical treatment you will die.".to_string()));
        q.flesh = -20.0;
        assert_eq!(part_tip(&q, "Chest", false, -40.0, false, true).1, "This is a dangerous wound and is getting worse. Get some medical attention fast.  Death at -100");
    }
}
