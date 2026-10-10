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
    tab: Option<Built>,
    last: f32,
    banner: Banner,
    /// What each datapanel shows now (rebuilt when it changes).
    rows: HashMap<Entity, Vec<Row>>,
    fonts: Fonts,
}

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
const FADED: Color = Color::srgb(0.6, 0.56, 0.5);
/// A penalty.
const PENALTY: Color = Color::srgb(0.78, 0.27, 0.22);
/// Kenshi_TextboxStandardText's "TextColour" (dark, on the bars).
const DARK: Color = Color::srgb(0.078, 0.031, 0.024);

/// A datapanel line (DataPanelLine_Text): its label on the left, its value on the right.
#[derive(Clone, PartialEq)]
pub(super) struct Row {
    pub label: String,
    pub value: String,
    pub colour: Color,
}

pub(super) fn row(label: impl Into<String>, value: impl Into<String>) -> Row {
    Row { label: label.into(), value: value.into(), colour: LIGHT }
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
        for r in self.rows {
            let line = world
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: if self.centre { JustifyContent::Center } else { JustifyContent::SpaceBetween },
                    ..default()
                })
                .id();
            let l = world.spawn(text(r.label, self.label_colour)).id();
            world.entity_mut(line).add_child(l);
            if !r.value.is_empty() {
                let v = world.spawn(text(r.value, r.colour)).id();
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

/// The life bar parts of the medical panel, in the order Kenshi lists them.
const BARS: usize = 9;

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

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
    // (TooltipPanel: an empty frame no code of the game's fills)
    for hidden in ["TownPanel", "TooltipPanel", "BountyPanel", "LoadingPanel"] {
        if let Some(e) = ui.get(hidden) {
            commands.queue(SetVisible(e, false));
        }
    }
    // the captions the game sets over the layout's (F043077: "Money")
    if let Some(e) = ui.get("MoneyLabelText") {
        commands.queue(SetCaption(e, "Money".to_string()));
    }
    let fonts = Fonts::load(&gui.0, &assets);
    commands.insert_resource(Hud { ui, squad: Vec::new(), last: -1.0, banner: Banner::default(), rows: HashMap::new(), fonts, tab: None });
}

/// One life bar's contents.
struct Bar {
    label: String,
    value: String,
    /// The hunger trend shown after its label.
    arrow: String,
    /// green, yellow, red, white (F042974's widths).
    widths: [f32; 4],
    robot: bool,
    crushed: bool,
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
    if 0.0 < p.disguise_blown {
        let n = p.disguise_blown.ceil() as i32;
        let cool = if n == 1 { "Cooldown 1 second".to_string() } else { format!("Cooldown {n} seconds") };
        return ("Disguise Blown!", format!("You've been discovered, somebody has seen through your disguise.\n{cool}"));
    }
    if let Some(s) = p.slave.as_ref().filter(|s| !s.obedient && p.prison.is_none())
        && 0 < sim.bounty_with(id, &s.faction)
    {
        return ("Discovered!", "You've been discovered, somebody has noticed that you are an escaped slave.".into());
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
/// slave's and an ex-slave's with how much it looks like a slave ("Looks like a slave:",
/// `Sim::looks_like_slave`), whether it has been seen ("Situation:") and an escapee's time left.
/// (assumption) The slave lines come from the game's strings in their order (the function is too
/// large to decompile); "Seen!" is a bounty with its masters.
fn state_line(sim: &kenshi_game::sim::Sim, id: usize) -> (&'static str, String) {
    let p = &sim.folk[id];
    let m = &sim.arena.f[id].medical;
    let faction = |f: &str| sim.data.get(f).map_or(f.to_string(), |r| r.name.clone());
    let looks = |s: &kenshi_game::slavery::Slavery| {
        let seen = if 0 < sim.bounty_with(id, &s.faction) { "Seen!" } else { "Undiscovered" };
        format!("\nLooks like a slave: {}%\nSituation: {seen}", (sim.looks_like_slave(id) * 100.0).round() as i32)
    };
    if m.dead {
        return ("Dead", "This guy is dead and gone.".into());
    }
    if let Some(s) = p.slave.as_ref() {
        if p.prison.as_ref().is_some_and(|q| q.slave) || s.obedient {
            return (
                "Slave",
                format!(
                    "A slave belonging to {}. Not considered a person with rights, a slave works for his master until death and nothing more. {}",
                    faction(&s.faction),
                    "Your character will have a permanent job assigned called [Obedient slave]. While this is active he will automatically work for his master. You can remove this job, but beware of the consequences..."
                ),
            );
        }
        if p.prison.is_none() {
            if s.fled || p.escaped.is_some() {
                let left = p.escaped.as_ref().map_or(0.0, |e| (e.1 - sim.hours_gone()).max(0.0)).ceil() as i64;
                let hours = if left == 1 { "1 hour".to_string() } else { format!("{left} hours") };
                return (
                    "Escaped slave",
                    format!(
                        "A slave of {} that has been seen escaping. Needs to lay low for a day and avoid getting spotted. {}\nTime remaining: {hours}{}",
                        faction(&s.faction),
                        "Keep your [Looks Like A Slave] level as low as possible, wear clothing or disguises and find a plastic surgeon to restore your hair. Don't get spotted.",
                        looks(s)
                    ),
                );
            }
            return (
                "Ex-slave",
                format!(
                    "Was once a slave of {} but has now escaped. The heat has died down but he still needs to avoid getting found out. {}{}",
                    faction(&s.faction),
                    "Keep your [Looks Like A Slave] level as low as possible, wear clothing or disguises and find a plastic surgeon to restore your hair. You should be safe if you stay out of towns.",
                    looks(s)
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
    if let Some((_, until)) = &p.escaped {
        let left = (until - sim.hours_gone()).max(0.0).ceil() as i64;
        return ("Escaped Prisoner", format!("This character broke out of prison, if he gets spotted by any guards he is in trouble. Leave town to clear this status. Expires in {left} hours"));
    }
    if p.bed.is_some() {
        return ("Sleeping...", String::new());
    }
    ("Normal", String::new())
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

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut hud: ResMut<Hud>,
    mut game: ResMut<Game>,
    mut clicks: MessageReader<GuiClick>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    binds: Res<super::bindings::Bindings>,
    mut nodes: Query<&mut Node>,
    time: Res<Time>,
    mut open: ResMut<super::OpenBuildings>,
    mut edits: MessageReader<GuiEdit>,
    boxes: Query<&EditText>,
) {
    // the outpost's name typed (TownPanel's OutpostNameEditBox)
    let name_box = hud.ui.get("OutpostNameEditBox");
    for ev in edits.read() {
        if Some(ev.widget) == name_box && !ev.text.trim().is_empty() {
            game.sim.outpost_name = ev.text.clone();
        }
    }
    for c in clicks.read() {
        match c.name.as_str() {
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
            "ShortcutInventoryButton" => keys.press(binds.key("toggle_inventory").unwrap_or(KeyCode::KeyI)),
            "ShortcutStatsButton" => keys.press(binds.key("toggle_stats").unwrap_or(KeyCode::KeyC)),
            "OpenToPublicCheckBox" => game.sim.outpost_public = !game.sim.outpost_public,
            "OrdersProspectButton" => {
                let sel = game.selected.clone();
                game.sim.order_prospect(&sel);
            }
            "OrdersSneakButton" => {
                let sel = game.selected.clone();
                game.sim.toggle_sneak(&sel);
            }
            _ => {
                if let Some(&(_, _, id, _)) = hud.squad.iter().find(|(_, b, _, _)| *b == c.widget) {
                    game.selected = vec![id];
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
    // the selected character
    if let Some(&id) = game.selected.first() {
        let sim = &game.sim;
        let p = &sim.folk[id];
        let f = &sim.arena.f[id];
        let m = &f.medical;
        let name = sim.display_name(id);
        let faction = p.faction.as_deref().and_then(|s| sim.data.get(s)).map_or(String::new(), |r| r.name.clone());
        // the name, and its faction faded at the right
        let name_row = vec![Row { label: name, value: faction, colour: FADED }];
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
        // the combat stats, each with its penalty in red brackets
        use kenshi_char::StatsEnumerated as S;
        let stat = |label: &str, base: f32, now: f32| {
            // a penalty in brackets, in red
            let (b, n) = (base.floor() as i32, now.floor() as i32);
            if b <= n {
                row(label, b.to_string())
            } else {
                Row { label: label.into(), value: format!("{n} ({})", n - b), colour: PENALTY }
            }
        };
        let stats = vec![
            stat("ATTACK:", f.level(S::StatMeleeAttack), f.attack_level()),
            stat("DEFENSE:", f.level(S::StatMeleeDefence), f.level(S::StatMeleeDefence)),
            stat("STRENGTH:", f.level(S::StatStrength), f.level(S::StatStrength)),
            stat("DEXTERITY:", f.level(S::StatDexterity), f.level(S::StatDexterity)),
            stat("TOUGHNESS:", f.level(S::StatToughness), f.level(S::StatToughness)),
            stat("DODGE:", f.level(S::StatDodge), f.level(S::StatDodge)),
        ];
        // the bars (F047087): blood, the body parts head to legs, hunger
        let mut bars: Vec<Bar> = Vec::new();
        bars.push(Bar {
            label: "Blood".into(),
            value: format!("{}", (m.blood + 0.5) as i32),
            arrow: String::new(),
            widths: [(m.blood / m.max_blood.max(1.0)).max(0.0), 0.0, 0.0, 0.0],
            robot: false,
            crushed: false,
        });
        let mut order: Vec<usize> = (0..m.parts.len()).collect();
        order.sort_by_key(|&k| part_order(&m.parts[k], &sim.data));
        for k in order {
            let q = &m.parts[k];
            let max = q.max_health().max(1e-3);
            let green = ((q.flesh - q.stun) / max).max(0.0);
            let yellow = ((q.flesh + q.treatment - q.stun) / max).max(0.0);
            let red = ((max - q.stun) / max).max(0.0);
            let white = if q.slot == 1 { 0.0 } else { (q.bonus * 0.01 + green).clamp(0.0, 1.0) };
            bars.push(Bar {
                label: sim.data.get(&q.id).map_or(q.id.clone(), |r| r.name.clone()),
                value: format!("{}", (q.flesh - q.stun + 0.5).floor() as i32),
                arrow: String::new(),
                widths: [green, yellow, red, white],
                robot: q.robotic || q.slot == 2,
                crushed: q.slot == 1 || q.slot == 3,
            });
        }
        // hunger: x100; the arrow: digesting ">", else "<", "<!" under 2, "<<!!" under 1
        let h = m.hunger.level;
        let arrow = if 0.0 < m.hunger.stomach {
            ">"
        } else if h < 1.0 {
            "<<!!"
        } else if h < 2.0 {
            "<!"
        } else {
            "<"
        };
        bars.push(Bar {
            label: "Hunger".into(),
            value: format!("{}", (h * 100.0) as i32),
            arrow: arrow.into(),
            widths: [(h - 2.0).max(0.0), (h - 1.0).max(0.0), h.max(0.0), 0.0],
            robot: false,
            crushed: false,
        });
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
            let label = vec![Row { label: b.label.clone(), value: b.arrow.clone(), colour: DARK }];
            let name = format!("LifeBar{}Datapanel", k + 1);
            hud.rows(&mut commands, &name, label, false, DARK, true);
        }
        hud.rows(&mut commands, "NameText", name_row, false, LIGHT, false);
        // the state box over the stats (StateText), the combat stats below (StatsText)
        hud.rows(&mut commands, "StateText", info, false, LIGHT, false);
        hud.rows(&mut commands, "StatsText", stats, true, LIGHT, false);
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
    // the tab, once
    if hud.tab.is_none()
        && let Some(place) = hud.ui.get("SquadPanel")
    {
        // on the panel's header (the template's HeaderPlace: 20 in, 37 high of 716 x 220)
        let size = hud.ui.sizes.get(&place).copied().unwrap_or([600.0, 200.0]);
        let faction = game.sim.folk.iter().find(|p| p.player).and_then(|p| p.faction.as_deref()).and_then(|f| game.sim.data.get(f)).map_or("Squad".to_string(), |r| r.name.clone());
        let def = kenshi_gui::res::WidgetDef {
            kind: "Button".into(),
            skin: "Kenshi_SquadTabButton".into(),
            pos: kenshi_gui::res::Pos::Real([20.0 / 716.0, 0.0, 200.0 * scale / size[0], 37.0 / 220.0]),
            align: Default::default(),
            name: "SquadTab".into(),
            layer: None,
            props: vec![("Caption".into(), format!("{faction}_0").to_uppercase()), ("StateSelected".into(), "true".into())],
            user: Vec::new(),
            children: Vec::new(),
        };
        hud.tab = Some(kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &[def], Some(place), size));
    }
    let squad: Vec<usize> = game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let made: Vec<usize> = hud.squad.iter().map(|s| s.2).collect();
    if squad != made {
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
            if let Some(e) = b.get("NameText") {
                commands.queue(SetCaption(e, game.sim.display_name(id)));
            }
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

