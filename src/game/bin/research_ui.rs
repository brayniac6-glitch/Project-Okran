//! The research window (the overview window's RESEARCH tab, Kenshi_OverviewWindow.layout;
//! "toggle_research": T, controls.cfg, or the main panel's TCH button): the research types, the
//! type's blueprints (right click: learn, F035133), the queue (its first being researched), and
//! the chosen research's description and what it gives (F035131). The window's caption is
//! "RESEARCH - LEVEL n (CURRENT RATE: r x)" (F035128); the bench message is F035131/F035132's.
//! Its CRAFTING tab: the crafting benches built ("STATIONS"), the chosen one's items ("ITEMS":
//! a click queues one), its queue ("QUEUE": the first being made; right click takes one off),
//! "Repeat Queue", and the item's description and ingredients.
//! Its MAP tab ("toggle_map": M, the MAP button; F035108): the world map (GUI_Map.dds) zoomed
//! with the + / - buttons or the wheel and dragged, the towns' markers (Kenshi_MapMarkers: Town,
//! Outpost, Village, Camp, Ruin, Nest...) with the towns' names, the player's characters' (the
//! selected "MarkerPlayerSelected", F035108), "O" centring on the selected. Its FACTION tab
//! (F035125, F035113): the player's faction's name, each faction's relation with it as a
//! Kenshi_FactionRelationsLine (its name, its value, a bar right for friends and left for
//! foes), and the faction's numbers. Its SQUADS tab (the SQD button; F035124): each squad as a
//! Kenshi_SquadBox (its name, its size of 20) with its people's portraits
//! (Kenshi_SquadsPanel_PortraitCharacter), and the faction's size of 256.
//! Its DIALOGUE tab: the message log; its AI tab (F035130): the AI options' tick boxes and the
//! follow formation, the hovered one's tooltip below. The queues are Kenshi_ItemListItem rows
//! (`item_list_rows`: name, hours left, progress bar, X). (stand-in) The tabs are switched by
//! buttons over them; the repeat button's state is in its caption; the map's way round as the
//! biome map's (rows against z).
use super::Game;
use super::build_ui::{ListRow, list_rows};
use super::hud_ui;
use bevy::prelude::*;
use kenshi_data::ItemType;
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption, SetVisible};

/// The list rows' tags.
const TYPES: u8 = 10;
const BLUEPRINTS: u8 = 11;
const QUEUE: u8 = 12;
const STATIONS: u8 = 13;
const ITEMS: u8 = 14;
const CRAFT_QUEUE: u8 = 15;

#[derive(Resource, Default)]
pub struct ResearchWindow {
    pub(super) open: Option<Open>,
}

impl ResearchWindow {
    /// The overview window's tab shown (0 research, 1 crafting, 2 map, 3 faction, 4 squads,
    /// 5 dialogue, 6 AI), if
    /// it is open.
    pub fn tab(&self) -> Option<u8> {
        self.open.as_ref().map(|o| o.tab)
    }

    /// @fn F049757
    ///
    /// The window's widgets a tutorial pointer case frames: 31 its CRAFTING tab's button (the
    /// tab control's tab 3) while it is open, 32 the crafting tab's items (its +0x140 list,
    /// (assumption) QueueItemsPanel's place) while that tab is shown.
    pub fn pointer_target(&self, case: u8) -> Option<Entity> {
        let o = self.open.as_ref()?;
        match case {
            31 => o.tabs.as_ref()?.get("OverviewTabCrafting"),
            32 if o.tab == 1 => o.ui.get("QueueItemsPanel"),
            _ => None,
        }
    }
}

/// The faction tab's relations panel (a Kenshi_ScrollView): its client area, the canvas's width
/// (F035125), the view's height, its scroll bar.
///
/// MyGUI's ScrollViewBase::updateScrollSize hides a scroll bar its canvas does not need and
/// widens the client by it (narrows it again as the bar shows): the empty canvas at the panel's
/// start leaves both bars hidden, the client the skin's widened by the vertical bar's width and
/// heightened by the horizontal's. F035125's canvas width, that client's width less the
/// vertical bar's, is then the skin's own client width; the view is the skin's client
/// heightened by the horizontal bar (never shown: the canvas is no wider than the view).
#[derive(Clone, Copy)]
pub(super) struct RelationsCanvas {
    client: Entity,
    width: f32,
    view_h: f32,
    bar: Option<Entity>,
}

pub(super) struct Open {
    ui: Built,
    types: Vec<String>,
    kind: usize,
    /// The type's research (ids), and the one chosen.
    list: Vec<String>,
    chosen: Option<String>,
    rows: Vec<Entity>,
    fonts: ((Handle<Font>, f32), (Handle<Font>, f32)),
    /// What was last shown (queue, progress %, done count, message), to redraw on a change.
    shown: (Vec<String>, u32, usize, Option<String>, i32, u32),
    /// The tab shown: 0 research, 1 crafting, 2 map, 3 faction, 4 squads, 5 dialogue, 6 AI; the
    /// tab buttons.
    tab: u8,
    /// The map: zoom (1: the whole world fits), its centre (world x, z), a drag's last cursor
    /// point, the map image and its markers, when last drawn.
    map_zoom: f32,
    /// The map view's laid-out size and top-left (logical pixels).
    map_rect: Option<(Vec2, Vec2)>,
    map_centre: [f32; 2],
    map_drag: Option<Vec2>,
    map_nodes: Vec<Entity>,
    map_drawn: f32,
    /// The faction tab's lines scrolled past; the relations panel's canvas (its client area
    /// and width, F035125) once its parts are laid; its lines' column.
    faction_scroll: usize,
    relations_canvas: Option<RelationsCanvas>,
    faction_column: Option<Entity>,
    /// The relations panel scrolled down (pixels) and how far it can be (its canvas's height
    /// less its view's, MyGUI's ScrollView mVRange).
    relations_offset: f32,
    relations_range: f32,
    /// The name last put in the faction's name box.
    name_shown: Option<String>,
    /// The screen's size (logical pixels) as the window opened.
    screen: [f32; 2],
    /// The dialogue tab: the log's length when drawn. The AI tab: its tooltip's text.
    log_len: usize,
    ai_tip: Option<Entity>,
    tabs: Option<Built>,
    /// The crafting tab: the stations (building indices), the one chosen, its items, the item
    /// chosen, and what was last shown (queue, progress %, repeat).
    stations: Vec<usize>,
    station: usize,
    items: Vec<String>,
    item: Option<String>,
    craft_shown: (Vec<String>, u32, bool, usize),
    /// The squads tab: each squad's name and people as drawn (and how many faces they had), the
    /// boxes shown (squad, box, name box, DELETE), the portraits (widget, person), the squads
    /// scrolled past, a portrait being dragged (person, its picture).
    squads_shown: Vec<(String, Vec<usize>)>,
    squads_shown_portraits: usize,
    squad_boxes: Vec<(usize, Entity, Option<Entity>, Option<Entity>)>,
    squad_portraits: Vec<(Entity, usize)>,
    squad_scroll: usize,
    squad_drag: Option<(usize, Entity)>,
    squad_test_frames: u32,
    /// The queues' rows' X buttons: (button, list, place); a queue row being dragged (list,
    /// place).
    queue_x: Vec<(Entity, u8, usize)>,
    queue_drag: Option<(u8, usize)>,
}

/// The research types: the RESEARCH records' "category", in name order.
fn types(game: &Game) -> Vec<String> {
    let mut out: Vec<String> = game.sim.data.of_type(ItemType::Research).filter_map(|r| r.strings.get("category").cloned()).filter(|c| !c.is_empty()).collect();
    out.sort();
    out.dedup();
    out
}

fn redraw(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer) {
    for r in o.rows.drain(..) {
        commands.entity(r).despawn();
    }
    o.queue_x.clear();
    for (name, on) in [("ResearchTab", o.tab == 0), ("CraftTab", o.tab == 1), ("MapTab", o.tab == 2), ("FactionTab", o.tab == 3), ("SquadsTab", o.tab == 4), ("MessagesTab", o.tab == 5), ("AITab", o.tab == 6)] {
        if let Some(e) = o.ui.get(name) {
            commands.queue(SetVisible(e, on));
        }
    }
    if let (Some(cap), Some(name)) = (o.ui.get("Root.Caption"), ["", "", "MAP", "FACTION", "SQUADS", "DIALOGUE", "AI"].get(o.tab as usize).filter(|n| !n.is_empty())) {
        commands.queue(SetCaption(cap, name.to_string()));
    }
    match o.tab {
        1 => {
            redraw_crafting(commands, o, game, gui, assets);
            return;
        }
        3 => {
            redraw_faction(commands, o, game, gui, assets);
            return;
        }
        5 => {
            redraw_dialogue(commands, o, game);
            return;
        }
        // the AI tab's rows (they need the gui) drawn again next frame
        6 => {
            o.map_drawn = -1.0;
            return;
        }
        2 | 4 => return,
        _ => {}
    }
    let sim = &game.sim;
    let kind = o.types.get(o.kind).cloned().unwrap_or_default();
    // the type's research not yet done: those that can be learnt first
    let mut list: Vec<&kenshi_data::GameData> = sim
        .data
        .of_type(ItemType::Research)
        .filter(|r| r.strings.get("category") == Some(&kind) && !sim.research_done.contains(&r.string_id) && !r.name.starts_with('_'))
        .collect();
    list.sort_by(|a, b| (!sim.research_available(a), &a.name).cmp(&(!sim.research_available(b), &b.name)));
    o.list = list.iter().map(|r| r.string_id.clone()).collect();
    let font = o.fonts.0.clone();
    if let Some(l) = o.ui.get("ResearchTypeList") {
        let rows: Vec<(String, bool)> = o.types.iter().map(|t| (t.clone(), true)).collect();
        o.rows.push(list_rows(commands, l, TYPES, &rows, Some(o.kind), &font, (10.0, 17.0, 74.0, 65.0)));
    }
    if let Some(l) = o.ui.get("BlueprintsAvailablePanel") {
        let rows: Vec<(String, bool)> = list.iter().map(|r| (r.name.clone(), sim.research_available(r))).collect();
        let chosen = o.chosen.as_ref().and_then(|c| o.list.iter().position(|s| s == c));
        o.rows.push(list_rows(commands, l, BLUEPRINTS, &rows, chosen, &font, (4.0, 2.0, 92.0, 96.0)));
    }
    if let Some(l) = o.ui.get("ResearchQueueList") {
        // F035825: its name, the hours left (F046251), how far along
        let rows: Vec<(String, String, f32)> = sim
            .research_queue
            .iter()
            .enumerate()
            .filter_map(|(k, id)| {
                let r = sim.data.get(id)?;
                let time = kenshi_game::production::research_time(r).max(0.01);
                let done = if k == 0 { (sim.research_progress / time).clamp(0.0, 1.0) } else { 0.0 };
                Some((r.name.clone(), research_hours(time, sim.research_rate, done), done))
            })
            .collect();
        let list_w = o.ui.sizes.get(&l).map_or(200.0, |s| s[0]);
        let fs2 = kenshi_gui::res::font_size2();
        let (column, xs) = item_list_rows(commands, gui, assets, l, list_w, fs2, QUEUE, &rows);
        o.rows.push(column);
        o.queue_x.extend(xs);
    }
    // the bench message: no bench, or why research waits
    if let Some(e) = o.ui.get("ResearchBenchMessage") {
        let msg = if sim.bench_level() == 0 { Some("Needs research bench level 1".to_string()) } else { sim.research_message.clone() };
        commands.queue(SetVisible(e, msg.is_some()));
        commands.queue(SetCaption(e, msg.unwrap_or_default()));
    }
    // F035128: the caption
    if let Some(root) = o.ui.get("Root.Caption") {
        commands.queue(SetCaption(root, format!("RESEARCH - LEVEL {} (CURRENT RATE: {:.2}x)", sim.bench_level(), sim.research_rate)));
    }
    describe(commands, o, game);
}

/// @fn F046251
///
/// A queued research's hours left: its time over the rate (when there is one), less the share
/// done, rounded up, at least 1, "<" before it under an hour; "{1,num} hr" for one, else
/// "{1,num} hrs".
fn research_hours(time: f32, rate: f32, done: f32) -> String {
    let mut h = time;
    if rate > 1e-6 {
        h /= rate;
    }
    let h = h * (1.0 - done);
    let n = (h.ceil() as i64).max(1);
    let less = if h < 1.0 { "<" } else { "" };
    if n == 1 { format!("{less}{n} hr") } else { format!("{less}{n} hrs") }
}

/// The research and crafting queues' rows (ReorderableListItem cells, Kenshi_ItemListItem.layout,
/// F035994): a row (font size2 + 12 high, the list's width; F035579) a queued thing, its name,
/// its value on the right (the research's hours left; none for crafting, F030012), its progress
/// bar as wide as its share done of the value's width (F035825), and its X (RemoveButton)
/// taking it off; a row dragged onto another moves it there (`move_queued`). Returns the rows'
/// column and the X buttons with their list and place.
#[allow(clippy::too_many_arguments)]
fn item_list_rows(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, list: Entity, list_w: f32, font_size2: f32, tag: u8, rows: &[(String, String, f32)]) -> (Entity, Vec<(Entity, u8, usize)>) {
    let scale = kenshi_gui::res::ui_scale();
    let h = ((font_size2 + 12.0) * scale).floor().max(8.0);
    let w = list_w * 0.96;
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(2.0),
                top: Val::Percent(2.0),
                width: Val::Percent(96.0),
                height: Val::Percent(96.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            ZIndex(2),
        ))
        .id();
    commands.entity(list).add_child(column);
    let defs = gui.0.layout("Kenshi_ItemListItem.layout");
    let mut xs = Vec::new();
    for (k, (name, value, done)) in rows.iter().enumerate() {
        let row = commands
            .spawn((Node { width: Val::Percent(100.0), height: Val::Px(h), flex_shrink: 0.0, ..default() }, ListRow(tag, k), Interaction::default(), bevy::ui::RelativeCursorPosition::default()))
            .id();
        commands.entity(column).add_child(row);
        let cell = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, Some(row), [w, h]);
        if let Some(t) = cell.get("ItemName") {
            commands.queue(SetCaption(t, name.clone()));
        }
        if let Some(t) = cell.get("ItemValue") {
            commands.queue(SetCaption(t, value.clone()));
        }
        if let Some(p) = cell.get("Progress") {
            let share = *done;
            commands.queue(move |w: &mut World| {
                if let Some(mut n) = w.get_mut::<Node>(p) {
                    n.width = Val::Percent(86.0 * share);
                    n.overflow = Overflow::clip();
                }
            });
        }
        if let Some(x) = cell.get("RemoveButton") {
            xs.push((x, tag, k));
        }
    }
    (column, xs)
}

/// @fn F036005
///
/// A queue's drop (ReorderableList's eventDropResult): the thing dragged taken out and put in
/// before the row dropped on, at the front for the first, at the end past the last (both rows
/// within the queue). (stand-in) The research under way keeps its progress only while it stays
/// first (the rewrite keeps the first's progress alone).
fn move_queued<T>(q: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from >= q.len() || to >= q.len() || from == to {
        return false;
    }
    let it = q.remove(from);
    if to < q.len() {
        q.insert(to, it);
    } else {
        q.push(it);
    }
    true
}

/// The crafting tab: the stations, the chosen one's items and queue, the repeat button, the
/// item's description.
fn redraw_crafting(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer) {
    let sim = &game.sim;
    o.stations = sim.crafting_benches();
    if o.station >= o.stations.len() {
        o.station = 0;
    }
    let font = o.fonts.0.clone();
    let name_of = |id: &str| sim.data.get(id).map_or(id.to_string(), |r| r.name.clone());
    if let Some(l) = o.ui.get("CraftingStationsList") {
        let rows: Vec<(String, bool)> = o.stations.iter().map(|&b| (name_of(&sim.buildings[b].record), true)).collect();
        o.rows.push(list_rows(commands, l, STATIONS, &rows, (!rows.is_empty()).then_some(o.station), &font, (10.0, 17.0, 74.0, 65.0)));
    }
    let station = o.stations.get(o.station).copied();
    o.items = station.map_or(Vec::new(), |b| sim.crafts(b));
    if let Some(l) = o.ui.get("QueueItemsPanel") {
        let rows: Vec<(String, bool)> = o.items.iter().map(|i| (name_of(i), true)).collect();
        let chosen = o.item.as_ref().and_then(|c| o.items.iter().position(|i| i == c));
        o.rows.push(list_rows(commands, l, ITEMS, &rows, chosen, &font, (4.0, 2.0, 92.0, 96.0)));
    }
    let pb = station.map(|b| &sim.buildings[b]);
    if let Some(l) = o.ui.get("CraftQueue") {
        // (not ported) the crafting cell's value text
        let rows: Vec<(String, String, f32)> = pb.map_or(Vec::new(), |pb| {
            pb.craft_queue
                .iter()
                .enumerate()
                .map(|(k, i)| (name_of(i), String::new(), if k == 0 { pb.made.first().copied().unwrap_or(0.0).clamp(0.0, 1.0) } else { 0.0 }))
                .collect()
        });
        let list_w = o.ui.sizes.get(&l).map_or(200.0, |s| s[0]);
        let fs2 = kenshi_gui::res::font_size2();
        let (column, xs) = item_list_rows(commands, gui, assets, l, list_w, fs2, CRAFT_QUEUE, &rows);
        o.rows.push(column);
        o.queue_x.extend(xs);
    }
    if let Some(e) = o.ui.get("QueueRepeat") {
        let on = pb.is_some_and(|pb| pb.craft_repeat);
        commands.queue(SetCaption(e, format!("Repeat Queue{}", if on { " (on)" } else { "" })));
    }
    if let Some(root) = o.ui.get("Root.Caption") {
        commands.queue(SetCaption(root, "CRAFTING".to_string()));
    }
    // the item: its name and description, then its station's ingredients for it
    let (Some(p1), Some(p2)) = (o.ui.get("QueueDescriptionPanel1"), o.ui.get("QueueDescriptionPanel2")) else { return };
    let mut left = Vec::new();
    let mut right = Vec::new();
    if let Some(r) = o.item.as_ref().and_then(|i| sim.data.get(i)) {
        left.push(hud_ui::row(r.name.to_uppercase(), ""));
        let text = r.strings.get("description").or(r.strings.get("Description")).cloned().unwrap_or_default();
        let mut line = String::new();
        for w in text.split_whitespace() {
            if line.len() + w.len() > 50 {
                left.push(hud_ui::row(std::mem::take(&mut line), ""));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(w);
        }
        if !line.is_empty() {
            left.push(hud_ui::row(line, ""));
        }
        if let Some(b) = station
            && let Some(rec) = sim.data.get(&sim.buildings[b].record)
            && let Some((f, _)) = kenshi_game::production::function_of(&sim.data, rec)
        {
            for (k, (s, v)) in f.refs("consumes").iter().enumerate() {
                let n = v[1].max(0) as f32 * 0.01;
                right.push(hud_ui::row(if k == 0 { "Ingredients:" } else { "" }, format!("{n} x {}", name_of(s))));
            }
        }
        right.push(hud_ui::row("Value:", format!("c.{}", r.ints.get("value").copied().unwrap_or(0))));
    }
    commands.queue(hud_ui::SetRows { panel: p1, rows: left, font: o.fonts.0.clone(), label_colour: hud_ui::LIGHT, centre: false });
    commands.queue(hud_ui::SetRows { panel: p2, rows: right, font: o.fonts.0.clone(), label_colour: hud_ui::LIGHT, centre: false });
}

/// @fn F048704
///
/// A town marker's tooltip (tooltip_type 4, its tooltip_data the town, F035094): its name
/// (once explored, else its unexplored name) in #492620, "Faction:" in #492620 and the faction
/// in #afa68b, "Relations:" and the player's faction's relation with it ("{1,num}") in
/// #afa68b, its population manager's lines (`population_lines`), then for a town "Residents:"
/// in #492620 over "-" and each resident faction in #afa68b (+0xf0, the factions of the squads
/// living there; Kenshi's set is in pointer order, (stand-in) here in the order first met).
/// (not ported) "Hostile Occupiers:" in #59231a over the town's occupiers (+0x120): the
/// rewrite has no occupied towns.
fn town_tip(sim: &kenshi_game::sim::Sim, ti: usize, t: &kenshi_world::town::Town, shown: &str) -> String {
    let mut lines = vec![format!("{shown}\t\tt")];
    if let Some(f) = t.faction.as_ref().and_then(|f| sim.data.get(f)) {
        lines.push(format!("Faction:\t{}\th", f.name));
        let mine = sim.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
        let rel = mine.as_ref().and_then(|m| sim.relations.get(m).cloned()).or_else(|| mine.as_ref().and_then(|m| sim.data.get(m)).map(|r| kenshi_ai::faction::FactionRelations::from_data(r, &sim.data)));
        let theirs = mine.as_ref().and_then(|m| f.refs("relations").iter().find(|(o, _)| o == m).map(|(_, v)| v[0] as f32));
        let ours = rel.as_ref().and_then(|r| r.relations.get(&f.string_id).map(|x| x.relation));
        let default = f.ints.get("default relation").copied().unwrap_or(0) as f32;
        let r = ours.or(theirs).unwrap_or(default);
        lines.push(format!("Relations:\t{}\tc", hud_ui::num(r as i64)));
    }
    lines.extend(population_lines(sim, ti));
    lines.push("Residents:\t\tt".into());
    let mut factions: Vec<&str> = Vec::new();
    for p in sim.folk.iter().filter(|p| !p.dead && p.home.and_then(|h| sim.placed_town.get(h).copied().flatten()) == Some(ti)) {
        if let Some(name) = p.faction.as_ref().and_then(|f| sim.data.get(f)).map(|f| f.name.as_str())
            && !factions.contains(&name)
        {
            factions.push(name);
        }
    }
    lines.extend(factions.iter().map(|f| format!("-{f}\t\tc")));
    lines.join("\n")
}

/// @fn F048144
///
/// A town's population manager's lines for its tooltip, each "#afa68b" key and value:
/// "Residents max:" its residents' current and desired most ("{1,num}/{2,num}", list 1's +0xc
/// and +8), "Actual Num Residents:" how many live ("{1,num}"), both "???/???" and "???" until
/// the manager has first spawned (+9); "Roaming currentMax/desiredMax:" (list 2's +0xc and +8),
/// "Roaming Population:" and "Num Roaming Squads:" (list 2's +0x58); a dead manager (+8)
/// shows "Dead" in #59231a in the first three. (stand-in) The rewrite has no population
/// manager: the residents' maxes are both the people homed in the town (dead or alive), its
/// manager spawned once any are; the roaming maxes both the town faction's "roaming
/// population" (the budget `spawn_homeless` keeps to); the roaming population and squads those
/// out from the town (`Population::homes`). (not ported) A dead manager.
fn population_lines(sim: &kenshi_game::sim::Sim, ti: usize) -> Vec<String> {
    let homed: Vec<&kenshi_game::sim::Person> = sim.folk.iter().filter(|p| p.home.and_then(|h| sim.placed_town.get(h).copied().flatten()) == Some(ti)).collect();
    let mut lines = Vec::new();
    if homed.is_empty() {
        lines.push("Residents max:\t???/???\tc".to_string());
        lines.push("Actual Num Residents:\t???\tc".to_string());
    } else {
        lines.push(format!("Residents max:\t{}/{}\tc", hud_ui::num(homed.len() as i64), hud_ui::num(homed.len() as i64)));
        lines.push(format!("Actual Num Residents:\t{}\tc", hud_ui::num(homed.iter().filter(|p| !p.dead).count() as i64)));
    }
    let budget = sim.towns[ti].faction.as_ref().and_then(|f| sim.data.get(f)).and_then(|f| f.ints.get("roaming population").copied()).unwrap_or(0) as i64;
    lines.push(format!("Roaming currentMax/desiredMax:\t{}/{}\tc", hud_ui::num(budget), hud_ui::num(budget)));
    let (mut people, mut squads) = (0, 0);
    if let Some(pop) = sim.population.as_ref() {
        for (members, _, _) in pop.squads.iter().filter(|(m, _, _)| m.first().is_some_and(|l| pop.homes.get(l) == Some(&ti))) {
            squads += 1;
            people += members.iter().filter(|&&m| sim.folk.get(m).is_some_and(|p| !p.dead)).count();
        }
    }
    lines.push(format!("Roaming Population:\t{}\tc", hud_ui::num(people as i64)));
    lines.push(format!("Num Roaming Squads:\t{}\tc", hud_ui::num(squads)));
    lines
}

/// The map tab (F035108): the world map at its zoom round its centre, the towns' markers and
/// names, the player's characters'.
fn redraw_map(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer) {
    for e in o.map_nodes.drain(..) {
        commands.entity(e).despawn();
    }
    let Some(view) = o.ui.get("MapScrollView") else { return };
    let size = o.map_rect.map_or_else(|| o.ui.sizes.get(&view).copied().unwrap_or([600.0, 600.0]), |(s, _)| [s.x, s.y]);
    // the map covers the view (its longer side) at the least zoom, and is kept over it
    let side = size[0].max(size[1]) * o.map_zoom;
    let ws = kenshi_world::WORLD_SIZE;
    for (k, extent) in [(0, size[0]), (1, size[1])] {
        let half = (extent * 0.5 / side * ws).min(ws * 0.5);
        o.map_centre[k] = o.map_centre[k].clamp(-ws * 0.5 + half, ws * 0.5 - half);
    }
    // the layout's own MapImage (the scroll view's canvas) is not used: this map is drawn instead
    if let Some(e) = o.ui.get("MapImage") {
        commands.queue(SetVisible(e, false));
    }
    // world (x, z) to the view's pixels: the map's rows run against z (as the biome map's)
    let to_px = |x: f32, z: f32| {
        let u = (x - o.map_centre[0]) / ws * side + size[0] * 0.5;
        let v = -(z - o.map_centre[1]) / ws * side + size[1] * 0.5;
        Vec2::new(u, v)
    };
    let clip = commands
        .spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), overflow: Overflow::clip(), ..default() }, ZIndex(3)))
        .id();
    commands.entity(view).add_child(clip);
    o.map_nodes.push(clip);
    let corner = to_px(-ws * 0.5, ws * 0.5);
    let map = game.res.find("GUI_Map.dds").map(|f| assets.load::<Image>(f));
    let img = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(corner.x), top: Val::Px(corner.y), width: Val::Px(side), height: Val::Px(side), ..default() },
            ImageNode { image: map.unwrap_or_default(), ..default() },
        ))
        .id();
    commands.entity(clip).add_child(img);
    let ui_png = gui.0.file("Kenshi_UI.png").map(|f| assets.load::<Image>(f.to_string()));
    let marker = |commands: &mut Commands, at: Vec2, rect: [f32; 4], tint: Color| -> Entity {
        let e = commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(at.x - rect[2] * 0.25), top: Val::Px(at.y - rect[3] * 0.25), width: Val::Px(rect[2] * 0.5), height: Val::Px(rect[3] * 0.5), ..default() },
                ImageNode { image: ui_png.clone().unwrap_or_default(), rect: Some(Rect::new(rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3])), color: tint, ..default() },
            ))
            .id();
        commands.entity(clip).add_child(e);
        e
    };
    // the towns found (F048883): Kenshi_MapMarkers' group by the town's type
    let mut names: Vec<Rect> = Vec::new();
    // (testing) KENSHI_MAP_ALL: every town shown
    let all = std::env::var("KENSHI_MAP_ALL").is_ok();
    let mut found: Vec<_> = game.sim.towns.iter().enumerate().filter(|(_, t)| all || game.sim.world.discovered.contains(&t.id)).collect();
    found.sort_by_key(|(_, t)| !(t.kind == 1 || t.kind == 2));
    for (ti, t) in found {
        let at = to_px(t.position[0], t.position[2]);
        if at.x < -50.0 || at.y < -50.0 || at.x > size[0] + 50.0 || at.y > size[1] + 50.0 {
            continue;
        }
        let rect = match t.kind {
            2 => [835.0, 9.0, 105.0, 79.0],
            1 => [950.0, 6.0, 48.0, 108.0],
            4 => [1245.0, 23.0, 22.0, 34.0],
            8 => [1202.0, 25.0, 32.0, 32.0],
            9 => [1158.0, 25.0, 32.0, 32.0],
            _ => [1083.0, 25.0, 32.0, 32.0],
        };
        // (KENSHI_MAP_ALL: taken as been at too)
        let tint = town_marker_colour(&game.sim, t, all || game.sim.world.explored.contains(&t.id));
        let m = marker(commands, at, rect, tint);
        // its name: until the player has been at it (F048883's explored), its "unexplored name"
        // when it has one (F048888)
        let shown = if game.sim.world.explored.contains(&t.id) {
            t.name.clone()
        } else {
            game.sim.data.get(&t.record).and_then(|r| r.strings.get("unexplored name")).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| t.name.clone())
        };
        // its tooltip only once the player has been at it (F035106: setNeedToolTip by TownBase
        // +0x181; KENSHI_MAP_ALL: all)
        commands.entity(m).insert(Interaction::default());
        if all || game.sim.world.explored.contains(&t.id) {
            commands.entity(m).insert(super::inventory_ui::ItemTip(town_tip(&game.sim, ti, t, &shown)));
        }
        // a name over another's is left out (the bigger places drawn first keep theirs)
        let est = Rect::new(at.x + 10.0, at.y - 8.0, at.x + 10.0 + shown.len() as f32 * o.fonts.0.1 * 0.55, at.y - 8.0 + o.fonts.0.1 * 1.2);
        let free = names.iter().all(|r| r.intersect(est).is_empty());
        if free && (t.kind == 1 || t.kind == 2 || o.map_zoom >= 3.0) {
            names.push(est);
            let name = commands
                .spawn((
                    Text::new(shown),
                    TextFont { font: bevy::text::FontSource::Handle(o.fonts.0.0.clone()), font_size: bevy::text::FontSize::Px(o.fonts.0.1), ..default() },
                    TextColor(hud_ui::LIGHT),
                    TextShadow::default(),
                    Node { position_type: PositionType::Absolute, left: Val::Px(at.x + 10.0), top: Val::Px(at.y - 8.0), ..default() },
                ))
                .id();
            commands.entity(clip).add_child(name);
        }
    }
    // the player's characters in kenshi_colours' "MarkerPlayer" (#8dcdff), the selected in
    // "MarkerPlayerSelected" (not in the xml: the game's own (0.652, 0.903, 1), F163236; F035101)
    for p in game.sim.folk.iter().filter(|p| p.player && !p.dead) {
        let q = p.mover.position_units();
        let tint = if game.selected.contains(&p.id) { MARKER_PLAYER_SELECTED } else { MARKER_PLAYER };
        marker(commands, to_px(q[0], q[2]), [1305.0, 27.0, 32.0, 32.0], tint);
    }
    // the camera (the layout's CameraMarker: Kenshi_MapMarkers' Camera, a RotatingSkin turned with
    // the camera; 0.0078125 of the 4096 canvas, so 32 of the map's pixels) where it looks
    let cam = game.focus + game.zone.origin;
    let at = to_px(cam.x, cam.z);
    let w = (side * 0.0078125).max(12.0);
    let e = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(at.x - w * 0.5), top: Val::Px(at.y - w * 0.5), width: Val::Px(w), height: Val::Px(w), ..default() },
            ImageNode { image: ui_png.clone().unwrap_or_default(), rect: Some(Rect::new(1278.0, 29.0, 1310.0, 61.0)), ..default() },
            UiTransform::from_rotation(Rot2::radians(-game.yaw)),
        ))
        .id();
    commands.entity(clip).add_child(e);
}

/// kenshi_colours.xml's map marker colours (F035108 reads them): "MarkerAlly", "MarkerNeutral",
/// "MarkerEnemy", "MarkerPlayer"; "MarkerPlayerSelected" is not in the xml, so the game's own.
const MARKER_ALLY: Color = Color::srgb(0x5c as f32 / 255.0, 0xb4 as f32 / 255.0, 0x73 as f32 / 255.0);
const MARKER_NEUTRAL: Color = Color::srgb(0xdf as f32 / 255.0, 0xdf as f32 / 255.0, 0xdf as f32 / 255.0);
const MARKER_ENEMY: Color = Color::srgb(0xe6 as f32 / 255.0, 0x51 as f32 / 255.0, 0x39 as f32 / 255.0);
const MARKER_PLAYER: Color = Color::srgb(0x8d as f32 / 255.0, 0xcd as f32 / 255.0, 1.0);
const MARKER_PLAYER_SELECTED: Color = Color::srgb(0.652, 0.903, 1.0);

/// @fn F035106
/// @fn F035101
///
/// A town marker's colour: a town not yet been at (TownBase +0x181, F048883's 400 units) in
/// "MarkerNeutral"; else by its faction (F035101): the player's own "MarkerPlayer"; one that
/// takes the player's faction for an ally (F041495) "MarkerAlly"; one that takes it for no enemy
/// (F041496) with a default relation of at least -30 and not "not real" "MarkerNeutral"; else
/// "MarkerEnemy". (assumption) A town with no faction is neutral.
fn town_marker_colour(sim: &kenshi_game::sim::Sim, t: &kenshi_world::town::Town, explored: bool) -> Color {
    let Some(f) = t.faction.as_ref().filter(|_| explored) else { return MARKER_NEUTRAL };
    let mine = sim.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone()).unwrap_or_else(|| kenshi_game::sim::PLAYER_FACTION.to_string());
    if *f == mine {
        return MARKER_PLAYER;
    }
    let Some(rel) = sim.relations.get(f).cloned().or_else(|| sim.data.get(f).map(|r| kenshi_ai::faction::FactionRelations::from_data(r, &sim.data))) else { return MARKER_NEUTRAL };
    if rel.is_ally(Some(&mine)) {
        MARKER_ALLY
    } else if !rel.is_enemy(Some(&mine)) && rel.default_relation >= -30.0 && !sim.faction_not_real(f) {
        MARKER_NEUTRAL
    } else {
        MARKER_ENEMY
    }
}

/// The faction tab (F035125, F035113): the player's faction's name and numbers, every faction's
/// relation with it.
/// @fn F035167
///
/// The dialogue tab (MessagesTab): the message log (`Game::log`, what was said and the game's
/// messages) in its text box, the newest at the bottom, the wheel scrolling back.
/// The text black (F035167's "#000000").
/// (assumption) Kenshi's box scrolls itself; here the lines that fit are shown from the scroll.
fn redraw_dialogue(commands: &mut Commands, o: &mut Open, game: &Game) {
    o.log_len = game.log.len();
    let Some(panel) = o.ui.get("MessagesTextBox") else { return };
    let height = o.ui.sizes.get(&panel).map_or(400.0, |s| s[1]);
    let (font, px) = o.fonts.0.clone();
    let fit = ((height / (px * 1.25)) as usize).max(1);
    o.faction_scroll = o.faction_scroll.min(game.log.len().saturating_sub(fit));
    let end = game.log.len() - o.faction_scroll;
    let start = end.saturating_sub(fit);
    let text = if game.log.is_empty() { String::new() } else { game.log[start..end].join("\n") };
    let e = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(8.0), right: Val::Px(8.0), bottom: Val::Px(6.0), max_height: Val::Percent(100.0), overflow: Overflow::clip(), ..default() },
            Text::new(text),
            TextFont { font: bevy::text::FontSource::Handle(font), font_size: bevy::text::FontSize::Px(px), ..default() },
            TextColor(Color::BLACK),
            ZIndex(3),
        ))
        .id();
    commands.entity(panel).add_child(e);
    o.rows.push(e);
}

/// An AI option's row (its index in `AI_OPTIONS`).
#[derive(Component)]
pub(super) struct AiRow(usize);

/// The AI tab's options in the game's order (F035130): name, which flag, tooltip; the last row
/// is the follow formation.
const AI_OPTIONS: [(&str, u8, &str); 10] = [
    ("Ditch items", 0, "If they have nothing else to do, characters will automatically put any excess materials they are carrying into storage.  This can help to un-block the base AI when things grind to a halt from over-filled inventories."),
    ("Sleep when injured", 1, "Characters will automatically go to bed if they are injured and have nothing else to do"),
    ("Sit when idle", 2, "Idle characters at home will sit down on chairs and benches, if you have any"),
    ("Heal Allies", 3, "Characters with the medic job will also heal characters of allied factions"),
    ("Help Allies", 4, "Characters will help out characters from other allied factions if they are under attack"),
    ("Rescue Allies", 5, "Characters with the rescue job will also rescue allied characters from other factions"),
    ("Share Food", 6, "Characters in a squad will automatically share any of their food that is in a backpack (if in range)."),
    ("Feed Animals", 7, "If food is running low, you can elect not to feed your animals."),
    ("Shoot first, ask later", 8, "Your turret gunners will shoot at any potentially dangerous but neutral targets, such as wild animals or bandits, without waiting for them to attack the base first."),
    ("Follow Formation", 9, "Formation used when squad members are following a leader"),
];

/// The AI option flag of row `k`.
fn ai_flag(a: &mut kenshi_game::sim::AiOptions, k: usize) -> Option<&mut bool> {
    Some(match AI_OPTIONS.get(k)?.1 {
        0 => &mut a.ditch_items,
        1 => &mut a.sleep_injured,
        2 => &mut a.sit_idle,
        3 => &mut a.heal_allies,
        4 => &mut a.help_allies,
        5 => &mut a.rescue_allies,
        6 => &mut a.share_food,
        7 => &mut a.feed_animals,
        8 => &mut a.shoot_first,
        _ => return None,
    })
}

/// @fn F035130
///
/// The AI tab: a row per option in AIOptions (its name, a tick box; the formation a button
/// naming it: Random, Military, Caravan), the hovered one's tooltip in AIToolTip below.
/// (assumption) The rows are laid out as the options window's (`options_ui`), the game building
/// both with its options panel.
fn redraw_ai(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer) {
    for r in o.rows.drain(..) {
        commands.entity(r).despawn();
    }
    let Some(panel) = o.ui.get("AIOptions") else { return };
    let width = o.ui.sizes.get(&panel).map_or(600.0, |s| s[0]);
    let scale = kenshi_gui::res::ui_scale();
    let row_h = 40.0 * scale;
    let mut opts = game.sim.ai_options.clone();
    let text = |name: &str, caption: &str, pos: [f32; 4]| kenshi_gui::res::WidgetDef {
        kind: "TextBox".into(),
        skin: "Kenshi_TextboxStandardText".into(),
        pos: kenshi_gui::res::Pos::Px(pos),
        align: Default::default(),
        name: name.into(),
        layer: None,
        props: vec![
            ("Caption".into(), caption.into()),
            ("TextAlign".into(), "Left VCenter".into()),
            ("FontName".into(), "Kenshi_StandardFont_Large".into()),
            ("TextColour".into(), "0.85 0.8 0.7".into()),
        ],
        user: Vec::new(),
        children: Vec::new(),
    };
    let button = |name: &str, skin: &str, caption: &str, pos: [f32; 4]| kenshi_gui::res::WidgetDef {
        kind: "Button".into(),
        skin: skin.into(),
        pos: kenshi_gui::res::Pos::Px(pos),
        align: Default::default(),
        name: name.into(),
        layer: None,
        props: if caption.is_empty() { Vec::new() } else { vec![("Caption".into(), caption.into())] },
        user: Vec::new(),
        children: Vec::new(),
    };
    for (k, (label, flag, _)) in AI_OPTIONS.iter().enumerate() {
        let holder = commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(k as f32 * row_h), width: Val::Px(width), height: Val::Px(row_h), ..default() },
                Interaction::default(),
                AiRow(k),
                ZIndex(3),
            ))
            .id();
        commands.entity(panel).add_child(holder);
        o.rows.push(holder);
        let cx = width * 0.5;
        let control = if *flag == 9 {
            let name = match opts.formation {
                1 => "Caravan",
                2 => "Military",
                _ => "Random",
            };
            button("AIFormation", "Kenshi_Button1", name, [cx, 4.0 * scale, width * 0.3, row_h - 8.0 * scale])
        } else {
            button(&format!("AITick{k}"), "Kenshi_TickBoxSkin", "", [cx, (row_h - 33.0 * scale) * 0.5, 35.0 * scale, 33.0 * scale])
        };
        let built = kenshi_gui::ui::spawn(commands, &gui.0, assets, &[text("Label", label, [10.0 * scale, 0.0, width * 0.48, row_h]), control], Some(holder), [width, row_h]);
        if let Some(v) = ai_flag(&mut opts, k)
            && let Some(e) = built.get(&format!("AITick{k}"))
        {
            commands.entity(e).insert(kenshi_gui::ui::Checked(*v));
        }
    }
    // the tooltip's text
    if let Some(tip) = o.ui.get("AIToolTip") {
        let (font, px) = o.fonts.0.clone();
        let t = commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(10.0), right: Val::Px(10.0), top: Val::Px(8.0), ..default() },
                Text::new(String::new()),
                TextFont { font: bevy::text::FontSource::Handle(font), font_size: bevy::text::FontSize::Px(px), ..default() },
                TextColor(Color::BLACK),
                ZIndex(3),
            ))
            .id();
        commands.entity(t).insert(kenshi_gui::Caption(t));
        commands.entity(tip).add_child(t);
        o.rows.push(t);
        o.ai_tip = Some(t);
    }
}

fn redraw_faction(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer) {
    let sim = &game.sim;
    // the name box shows the faction's name (not again while it is the same: what is being
    // typed stays)
    let name = sim.player_faction_name();
    if let Some(e) = o.ui.get("FactionNameText")
        && o.name_shown.as_ref() != Some(&name)
    {
        commands.queue(SetCaption(e, name.clone()));
        o.name_shown = Some(name);
    }
    if let Some(e) = o.ui.get("lbFactionName") {
        commands.queue(SetCaption(e, "FACTION NAME".to_string()));
    }
    if let Some(p) = o.ui.get("FactionInfoPanel") {
        let members = sim.folk.iter().filter(|p| p.player && !p.dead).count();
        let mut rows = vec![
            hud_ui::row("Members:", format!("{members}")),
            hud_ui::row("Money:", format!("c.{}", sim.money)),
            hud_ui::row("Buildings:", format!("{}", sim.buildings.iter().filter(|b| b.done).count())),
            hud_ui::row("Bounties:", format!("c.{}", sim.folk.iter().filter(|p| p.player).flat_map(|p| p.bounties.iter()).map(|b| b.amount).sum::<i32>())),
        ];
        // F035126: the active contracts, then the upcoming events, each under its heading (in
        // the game's colour 0x141f49360, #444444). (stand-in) A contract is
        // its leader and the hours it has left; an event is a campaign a dialogue set coming,
        // with the hours to it
        let heading = |s: &str| hud_ui::Row { label: s.into(), value: String::new(), colour: hud_ui::GREY, tip: String::new(), button: None };
        rows.push(heading("---Active Contracts---"));
        let now = sim.hours_gone();
        for c in &sim.contracts {
            rows.push(hud_ui::row(format!("{}:", sim.display_name(c.leader)), format!("{:.0} hrs", (c.until - now).max(0.0))));
        }
        rows.push(heading("---Upcoming Events---"));
        for (rec, at) in &sim.world.triggered {
            let name = sim.data.get(rec).map_or(rec.clone(), |r| r.name.clone());
            rows.push(hud_ui::row(format!("{name}:"), format!("{:.0} hrs", (at - now).max(0.0))));
        }
        commands.queue(hud_ui::SetRows { panel: p, rows, font: o.fonts.0.clone(), label_colour: hud_ui::LIGHT, centre: false });
    }
    faction_lines(commands, o, game, gui, assets);
}

/// @fn F035139
///
/// The faction tab's relations lines, made again every 2 seconds while it shows: a
/// Kenshi_FactionRelationsLine (F035113) for each faction the player's faction has a relation
/// with (F041500: all but itself), that it knows (F045530), not "not real" (Faction +0x1d0) and
/// not the record GENERATION_TEMPLATE_FACTION (its GameData +0x58, the string id); in their
/// names' order (F035702, as strings compare),
/// one under another 3 apart (at the UI scale), at the layout's height (0.0222222 of the
/// screen) and the canvas's width (F035125, `RelationsCanvas`); each one's relation the other faction's toward the player's (F041507). The canvas
/// as high as the lines (F035139's setCanvasSize); the scroll view (MyGUI's ScrollView) shows
/// its scroll bar while the canvas is higher than the view, and moves it 50 pixels a wheel
/// notch (ScrollView::notifyMouseWheel), within 0 .. its range.
fn faction_lines(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer) {
    // the last lines gone (unless a redraw took them already)
    if let Some(c) = o.faction_column.take()
        && let Some(k) = o.rows.iter().position(|r| *r == c)
    {
        o.rows.remove(k);
        commands.entity(c).despawn();
    }
    let Some(panel) = o.ui.get("RelationsPanel") else { return };
    let sim = &game.sim;
    let mine = sim.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
    let from_data = |f: &str| sim.data.get(f).map(|r| kenshi_ai::faction::FactionRelations::from_data(r, &sim.data));
    let rel = mine.as_ref().and_then(|f| sim.relations.get(f).cloned().or_else(|| from_data(f)));
    let known = &sim.world.known_factions;
    let mut list: Vec<(String, f32)> = sim
        .data
        .of_type(ItemType::Faction)
        .filter(|f| known.contains(&f.string_id) && rel.as_ref().is_none_or(|r| r.relations.contains_key(&f.string_id)))
        .filter(|f| Some(&f.string_id) != mine.as_ref() && f.string_id != "GENERATION_TEMPLATE_FACTION" && !f.bools.get("not real").copied().unwrap_or(false))
        .map(|f| {
            // F041507: the other faction's own entry for the player's
            let toward = |r: &kenshi_ai::faction::FactionRelations| mine.as_deref().map_or(r.default_relation, |m| r.relation(m));
            let r = sim.relations.get(&f.string_id).map(toward).or_else(|| from_data(&f.string_id).as_ref().map(toward)).unwrap_or(0.0);
            (f.name.clone(), r)
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    let scale = kenshi_gui::res::ui_scale();
    // the canvas: the scroll view's client area (F035125, `RelationsCanvas`)
    let canvas = o.relations_canvas.unwrap_or_else(|| {
        let size = o.ui.sizes.get(&panel).copied().unwrap_or([400.0, 400.0]);
        RelationsCanvas { client: panel, width: size[0], view_h: size[1], bar: None }
    });
    let (parent, w) = (canvas.client, canvas.width);
    // (MyGUI's coordinates are whole pixels: the layout's height cut down)
    let h = (o.screen[1] * 0.0222222).trunc();
    let mut tops = Vec::with_capacity(list.len());
    let mut y = 0.0f32;
    for _ in &list {
        tops.push(y);
        y = (3.0 * scale + h + y).trunc();
    }
    // the canvas's height (F035139) and the view's range over it (ScrollViewBase)
    o.relations_range = (y - canvas.view_h).max(0.0);
    o.relations_offset = o.relations_offset.clamp(0.0, o.relations_range);
    let column = commands
        .spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(-o.relations_offset), width: Val::Px(w), height: Val::Px(y), ..default() }, ZIndex(3)))
        .id();
    commands.entity(parent).add_child(column);
    // the view clips its canvas; its scroll bar shows while the canvas does not fit
    commands.queue(move |world: &mut World| {
        if let Some(mut n) = world.get_mut::<Node>(parent) {
            n.overflow = Overflow::clip();
        }
    });
    // the vertical bar (ScrollViewBase::updateScrollSize): its range the canvas's height less
    // the view's, its arrows 16 pixels (ScrollView::getVScrollPage), its thumb the view's
    // share of the canvas
    if let Some(b) = canvas.bar {
        commands.entity(b).insert(if o.relations_range > 0.0 { Visibility::Inherited } else { Visibility::Hidden });
        let (range, pos) = (o.relations_range, o.relations_offset);
        commands.queue(move |world: &mut World| {
            if let Some(mut s) = world.get_mut::<kenshi_gui::Scroll>(b) {
                s.range = range;
                s.page = 16.0;
            }
        });
        commands.queue(kenshi_gui::SetScroll(b, pos));
        if y > 0.0 {
            commands.queue(kenshi_gui::SetScrollTrack(b, canvas.view_h / y));
        }
    }
    o.rows.push(column);
    o.faction_column = Some(column);
    // (the layout's root laid over the whole line, its parts by their align)
    let mut defs = gui.0.layout("Kenshi_FactionRelationsLine.layout");
    if let Some(d) = defs.first_mut() {
        d.pos = kenshi_gui::res::Pos::Px([0.0, 0.0, 500.0, 24.0]);
        d.align = kenshi_gui::res::Align::parse("Stretch");
    }
    for ((name, r), top) in list.into_iter().zip(tops) {
        let holder = commands.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(top), width: Val::Px(w), height: Val::Px(h), ..default() }).id();
        commands.entity(column).add_child(holder);
        let line = kenshi_gui::ui::spawn_design(commands, &gui.0, assets, &defs, Some(holder), [500.0, 24.0], [w, h]);
        relation_line(commands, &line, &name, r);
    }
}

/// The relations panel's canvas moved to the scroll view's offset (its top at minus it).
fn relations_scrolled(commands: &mut Commands, o: &Open) {
    let (Some(c), top) = (o.faction_column, -o.relations_offset) else { return };
    commands.queue(move |world: &mut World| {
        if let Some(mut n) = world.get_mut::<Node>(c) {
            n.top = Val::Px(top);
        }
    });
    if let Some(b) = o.relations_canvas.and_then(|c| c.bar) {
        commands.queue(kenshi_gui::SetScroll(b, o.relations_offset));
    }
}

/// @fn F035138
///
/// A faction's relations line: its name (FactionName), its relation in FactionValue
/// ("{1,num}", over 60 in #a8b774, under -30 in #bf4834, else #afa68b), and its bar: a friend's
/// RightBar from the middle rightward its share (relation / 100) of the bar's width, a foe's
/// LeftBar leftward from the middle; the other none.
fn relation_line(commands: &mut Commands, line: &Built, name: &str, r: f32) {
    if let Some(e) = line.get("FactionName") {
        commands.queue(SetCaption(e, name.to_string()));
    }
    let colour = if r < -30.0 {
        hud_ui::tag(0xbf4834)
    } else if r > 60.0 {
        hud_ui::GREEN
    } else {
        hud_ui::TAN
    };
    if let Some(e) = line.get("FactionValue") {
        commands.queue(SetCaption(e, hud_ui::num(r as i64)));
        commands.queue(move |world: &mut World| {
            if let Some(c) = world.get::<kenshi_gui::Caption>(e).map(|c| c.0)
                && let Some(mut t) = world.get_mut::<TextColor>(c)
            {
                t.0 = colour;
            }
        });
    }
    // the layout's bars: LeftBar 0.01 .. 0.5, RightBar 0.5 .. 0.99 of the line
    let bars = [(line.get("LeftBar"), 0.01f32, 0.49f32, r < 0.0), (line.get("RightBar"), 0.5, 0.49, r >= 0.0)];
    for (e, x, full, this) in bars {
        let Some(e) = e else { continue };
        let width = if this { full * r.abs() * 0.01 } else { 0.0 };
        // the foe's bar grows from the middle leftward
        let left = if x < 0.5 { x + full - width } else { x };
        commands.queue(move |world: &mut World| {
            if let Some(mut n) = world.get_mut::<Node>(e) {
                n.left = Val::Percent(left * 100.0);
                n.width = Val::Percent(width * 100.0);
                // (the skin's image is a child at the bar's full size: cut to the bar)
                n.overflow = Overflow::clip();
            }
        });
    }
}

/// The squads tab (F035124, F035170): each of the player's squads a Kenshi_SquadBox (its name to
/// type over, its size of "max squad size", DELETE, its people's portraits), three to a view and
/// the wheel for more; the faction's size of 256.
fn redraw_squads(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer, portraits: &super::portraits::Portraits) {
    for e in o.map_nodes.drain(..) {
        commands.entity(e).despawn();
    }
    o.squad_boxes.clear();
    o.squad_portraits.clear();
    let sim = &game.sim;
    let members = sim.folk.iter().filter(|p| p.player && !p.dead && p.race.is_none()).count();
    if let Some(e) = o.ui.get("FactionSizeText") {
        commands.queue(SetCaption(e, format!("{members}/256")));
    }
    o.squads_shown = (0..sim.player_squads.len()).map(|k| (sim.player_squads[k].name.clone(), sim.squad_members(k))).collect();
    o.squads_shown_portraits = o.squads_shown.iter().flat_map(|(_, m)| m.iter()).filter(|id| portraits.by.contains_key(id)).count();
    let Some(panel) = o.ui.get("SquadsPanel") else { return };
    let size = o.ui.sizes.get(&panel).copied().unwrap_or([800.0, 600.0]);
    let shown = 3usize;
    o.squad_scroll = o.squad_scroll.min(o.squads_shown.len().saturating_sub(shown));
    let max = sim.max_squad_size();
    for k in o.squad_scroll..(o.squad_scroll + shown).min(o.squads_shown.len()) {
        let (name, people) = o.squads_shown[k].clone();
        let mut defs = gui.0.layout("Kenshi_SquadBox.layout");
        if let Some(d) = defs.first_mut() {
            d.pos = kenshi_gui::res::Pos::Px([0.0, (k - o.squad_scroll) as f32 * size[1] * 0.3, size[0], size[1] * 0.3]);
        }
        let b = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, Some(panel), size);
        o.map_nodes.extend(b.roots.iter().copied());
        if let Some(e) = b.get("SquadNameText") {
            commands.queue(SetCaption(e, name.to_uppercase()));
        }
        if let Some(e) = b.get("SquadSizeText") {
            commands.queue(SetCaption(e, format!("{} / {max}", people.len())));
        }
        o.squad_boxes.push((k, b.roots[0], b.get("SquadNameText"), b.get("SquadRemoveButton")));
        let Some(area) = b.get("PortraitsPanel") else { continue };
        let asize = b.sizes.get(&area).copied().unwrap_or([600.0, 100.0]);
        let side = asize[1] * 0.9;
        for (j, &id) in people.iter().enumerate() {
            let mut defs = gui.0.layout("Kenshi_SquadsPanel_PortraitCharacter.layout");
            if let Some(d) = defs.first_mut() {
                d.pos = kenshi_gui::res::Pos::Px([4.0 + j as f32 * (side + 6.0), asize[1] * 0.05, side, side]);
            }
            let pb = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, Some(area), asize);
            if let Some(e) = pb.get("NameText") {
                commands.queue(SetCaption(e, sim.display_name(id)));
            }
            if let (Some(e), Some((_, image))) = (pb.get("PortraitImage"), portraits.by.get(&id)) {
                commands.entity(e).insert(ImageNode { image: image.clone(), color: Color::linear_rgb(super::portraits::EXPOSURE, super::portraits::EXPOSURE, super::portraits::EXPOSURE), ..default() });
            }
            o.squad_portraits.push((pb.roots[0], id));
        }
    }
}

/// A laid-out widget's rectangle (logical pixels: top left, size).
fn rect_of(laid: &Query<(&ComputedNode, &UiGlobalTransform)>, e: Entity) -> Option<(Vec2, Vec2)> {
    let (n, t) = laid.get(e).ok()?;
    let k = n.inverse_scale_factor();
    let size = n.size() * k;
    Some((t.translation * k - size * 0.5, size))
}

fn inside(r: Option<(Vec2, Vec2)>, p: Vec2) -> bool {
    r.is_some_and(|(at, size)| p.x >= at.x && p.y >= at.y && p.x <= at.x + size.x && p.y <= at.y + size.y)
}

/// The squads tab's doings (F035111 NEW SQUAD, F035112 DELETE, F035090 a name typed, F035150 a
/// portrait dragged to a squad or to DRAG HERE TO DISMISS, the wheel); whether to draw it again.
#[allow(clippy::too_many_arguments)]
fn squads_tab(
    commands: &mut Commands,
    o: &mut Open,
    game: &mut Game,
    clicks: &[GuiClick],
    edits: &[kenshi_gui::GuiEdit],
    (press, release): (bool, bool),
    cursor: Option<Vec2>,
    wheel: f32,
    laid: &Query<(&ComputedNode, &UiGlobalTransform)>,
    portraits: &super::portraits::Portraits,
) -> bool {
    use super::message_box::{Ask, Question};
    let mut again = false;
    game.sim.squads_ready();
    let note = |game: &mut Game, text: &str| game.ask = Some(Ask { title: String::new(), text: text.to_string(), question: Question::Note });
    for c in clicks {
        if c.name == "NewSquadButton" {
            match game.sim.new_squad() {
                Ok(k) => {
                    // the new one in view
                    o.squad_scroll = k.saturating_sub(2);
                    again = true;
                }
                Err(m) => note(game, m),
            }
        } else if c.name == "SquadRemoveButton"
            && let Some(&(k, ..)) = o.squad_boxes.iter().find(|b| b.3 == Some(c.widget))
        {
            match game.sim.remove_squad(k) {
                Ok(()) => again = true,
                Err(m) => note(game, m),
            }
        }
    }
    // a squad's name typed (kept as it is shown, not drawn again under the typing)
    for e in edits {
        if let Some(&(k, ..)) = o.squad_boxes.iter().find(|b| b.2 == Some(e.widget))
            && let Some(s) = game.sim.player_squads.get_mut(k)
            && !e.text.trim().is_empty()
        {
            s.name = e.text.clone();
            if let Some(sh) = o.squads_shown.get_mut(k) {
                sh.0 = e.text.clone();
            }
        }
    }
    if wheel != 0.0 {
        let top = o.squads_shown.len().saturating_sub(3);
        let to = if wheel > 0.0 { o.squad_scroll.saturating_sub(1) } else { (o.squad_scroll + 1).min(top) };
        if to != o.squad_scroll {
            o.squad_scroll = to;
            again = true;
        }
    }
    // a portrait taken up: it follows the cursor (its face, small) until let go
    if let Some(p) = cursor {
        if press
            && o.squad_drag.is_none()
            && let Some(&(_, id)) = o.squad_portraits.iter().find(|(e, _)| inside(rect_of(laid, *e), p))
        {
            let mut node = commands.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(p.x - 32.0), top: Val::Px(p.y - 32.0), width: Val::Px(64.0), height: Val::Px(64.0), ..default() },
                GlobalZIndex(90),
                bevy::ui::FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
            if let Some((_, image)) = portraits.by.get(&id) {
                node.insert(ImageNode { image: image.clone(), color: Color::linear_rgba(1.0, 1.0, 1.0, 0.8), ..default() });
            } else {
                node.insert(BackgroundColor(Color::srgba(0.6, 0.55, 0.4, 0.6)));
            }
            o.squad_drag = Some((id, node.id()));
        }
        if let Some((_, ghost)) = o.squad_drag {
            commands.entity(ghost).insert(Node { position_type: PositionType::Absolute, left: Val::Px(p.x - 32.0), top: Val::Px(p.y - 32.0), width: Val::Px(64.0), height: Val::Px(64.0), ..default() });
        }
    }
    if release
        && let Some((id, ghost)) = o.squad_drag.take()
    {
        commands.entity(ghost).despawn();
        let Some(p) = cursor else { return again };
        if inside(o.ui.get("DismissCharPanel").and_then(|e| rect_of(laid, e)), p) {
            // F035150: asked first; F035103 when answered yes
            match game.sim.may_dismiss(id) {
                Err(m) => game.ask = Some(Ask { title: "Dismiss character".into(), text: m.to_string(), question: Question::Note }),
                Ok(()) => {
                    let text = format!("Dismiss character '{}'?", game.sim.display_name(id));
                    game.ask = Some(Ask { title: "Dismiss character".into(), text, question: Question::Dismiss(id) });
                }
            }
        } else if let Some(&(k, ..)) = o.squad_boxes.iter().find(|b| inside(rect_of(laid, b.1), p))
            && game.sim.move_to_squad(id, k)
        {
            again = true;
        }
    }
    // drawn again when the squads or their people changed (not their names: those are typed)
    let now: Vec<Vec<usize>> = (0..game.sim.player_squads.len()).map(|k| game.sim.squad_members(k)).collect();
    let had: Vec<Vec<usize>> = o.squads_shown.iter().map(|(_, m)| m.clone()).collect();
    let faces = now.iter().flatten().filter(|id| portraits.by.contains_key(id)).count();
    if now != had || faces != o.squads_shown_portraits {
        again = true;
    }
    again && o.squad_drag.is_none()
}

/// @fn F035131
///
/// The chosen research's description: its name and text, then what it gives ("New Building:",
/// "New Items:", ...), its "Requirements:", "Cost:" and the bench level it needs.
fn describe(commands: &mut Commands, o: &mut Open, game: &Game) {
    let sim = &game.sim;
    let r = o.chosen.as_ref().and_then(|c| sim.data.get(c));
    let (Some(p1), Some(p2)) = (o.ui.get("ResearchDescriptionPanel1"), o.ui.get("ResearchDescriptionPanel2")) else { return };
    let mut left = Vec::new();
    let mut right = Vec::new();
    if let Some(r) = r {
        left.push(hud_ui::row(r.name.to_uppercase(), ""));
        // the description, a line a row (word wrapped by eye: 60 characters)
        let text = r.strings.get("description").cloned().unwrap_or_default();
        let mut line = String::new();
        for w in text.split_whitespace() {
            if line.len() + w.len() > 60 {
                left.push(hud_ui::row(std::mem::take(&mut line), ""));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(w);
        }
        if !line.is_empty() {
            left.push(hud_ui::row(line, ""));
        }
        let names = |cat: &str| -> Vec<String> { r.refs(cat).iter().filter_map(|(s, _)| sim.data.get(s)).map(|x| x.name.clone()).collect() };
        for (cat, label) in [
            ("enable buildings", "New Building:"),
            ("enable item", "New Items:"),
            ("enable weapon type", "New Weapon Class:"),
            ("enable weapon model", "New Weapons Grade:"),
            ("enable armour", "New Armour Type:"),
            ("enable crossbow", "New Crossbows:"),
            ("enable backpack", "New Backpacks:"),
            ("enable robotics", "New Robot Limbs:"),
            ("requirements", "Requirements:"),
        ] {
            let n = names(cat);
            for (k, name) in n.iter().enumerate() {
                right.push(hud_ui::row(if k == 0 { label } else { "" }, name.clone()));
            }
        }
        let cost: Vec<String> = r.refs("cost").iter().filter_map(|(s, v)| sim.data.get(s).map(|x| format!("{} x {}", v[0].max(1), x.name))).collect();
        let money = r.ints.get("money").copied().unwrap_or(0);
        for (k, c) in cost.iter().chain((money > 0).then(|| format!("c.{money}")).iter()).enumerate() {
            right.push(hud_ui::row(if k == 0 { "Cost:" } else { "" }, c.clone()));
        }
        let level = r.ints.get("level").copied().unwrap_or(1);
        if sim.bench_level() < level {
            right.push(hud_ui::row(format!("Needs research bench level {level}"), ""));
        }
        if !sim.research_available(r) {
            right.push(hud_ui::row("Cannot learn this yet", ""));
        }
    }
    commands.queue(hud_ui::SetRows { panel: p1, rows: left, font: o.fonts.0.clone(), label_colour: hud_ui::LIGHT, centre: false });
    commands.queue(hud_ui::SetRows { panel: p2, rows: right, font: o.fonts.0.clone(), label_colour: hud_ui::LIGHT, centre: false });
}

/// T or the TCH button: open or close the window; its lists' clicks.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    act: super::bindings::Actions,
    mouse: Res<ButtonInput<MouseButton>>,
    mut clicks: MessageReader<GuiClick>,
    mut win: ResMut<ResearchWindow>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    rows: Query<(&Interaction, &ListRow)>,
    (mut wheel, time, portraits, laid): (MessageReader<bevy::input::mouse::MouseWheel>, Res<Time>, Res<super::portraits::Portraits>, Query<(&ComputedNode, &UiGlobalTransform)>),
    (ai_rows, queue_over, parts, mut scrolled): (Query<(&Interaction, &AiRow)>, Query<(&ListRow, &bevy::ui::RelativeCursorPosition)>, Query<&kenshi_gui::Parts>, MessageReader<kenshi_gui::GuiScroll>),
    (mut edits, mut edits_done): (MessageReader<kenshi_gui::GuiEdit>, MessageReader<kenshi_gui::GuiEditDone>),
) {
    let squad_edits: Vec<kenshi_gui::GuiEdit> = edits.read().cloned().collect();
    let mut squad_clicks: Vec<GuiClick> = Vec::new();
    // the tab asked for (controls.cfg: T research, Y crafting, M map); the main panel's TCH, MAP and SQD
    let mut asked: Option<u8> = None;
    for (a, t) in [("toggle_research", 0u8), ("toggle_crafting", 1), ("toggle_map", 2), ("toggle_faction", 3)] {
        if act.just(a) {
            asked = Some(t);
        }
    }
    let mut toggled = false;
    let mut close = false;
    let mut want_tab = 0u8;
    let mut station_request = None;
    // a crafting bench's menu: the window on its crafting tab
    if let Some(b) = game.crafting_request.take() {
        if let Some(o) = win.open.as_mut() {
            o.tab = 1;
            o.station = game.sim.crafting_benches().iter().position(|&s| s == b).unwrap_or(0);
            redraw(&mut commands, o, &game, &gui, &assets);
        } else {
            toggled = true;
            want_tab = 1;
            station_request = Some(b);
        }
    }
    let mut switch = None;
    let mut repeat = false;
    let mut map_zoom = 1.0f32;
    let mut map_centre = false;
    let mut ai_changed = false;
    let mut removes: Vec<Entity> = Vec::new();
    for c in clicks.read() {
        if c.name == "NewSquadButton" || c.name == "SquadRemoveButton" {
            squad_clicks.push(c.clone());
        }
        if c.name == "RemoveButton" {
            removes.push(c.widget);
        }
        match c.name.as_str() {
            "ShortcutTechButton" | "ResearchButton" => asked = Some(0),
            "ShortcutMapButton" => asked = Some(2),
            "ShortcutSquadButton" => asked = Some(4),
            "OverviewTabResearch" => switch = Some(0),
            "OverviewTabCrafting" => switch = Some(1),
            "OverviewTabMap" => switch = Some(2),
            "OverviewTabFaction" => switch = Some(3),
            "OverviewTabSquads" => switch = Some(4),
            "OverviewTabDialogue" => switch = Some(5),
            "OverviewTabAI" => switch = Some(6),
            "AIFormation" => {
                let f = &mut game.sim.ai_options.formation;
                // the list's order: Random, Military, Caravan
                *f = match *f {
                    0 => 2,
                    2 => 1,
                    _ => 0,
                };
                ai_changed = true;
            }
            n if n.starts_with("AITick") => {
                if let Some(v) = n[6..].parse::<usize>().ok().and_then(|k| ai_flag(&mut game.sim.ai_options, k)) {
                    *v = !*v;
                    ai_changed = true;
                }
            }
            "MapZoomInButton" => map_zoom = 1.5,
            "MapZoomOutButton" => map_zoom = 1.0 / 1.5,
            "MapCenterButton" => map_centre = true,
            "QueueRepeat" => repeat = true,
            // the window's header buttons (close, pin)
            "Button" if win.open.as_ref().is_some_and(|o| o.ui.named.iter().any(|(_, e)| *e == c.widget)) => close = true,
            _ => {}
        }
    }
    // (testing) KENSHI_OVERVIEW_TAB=n: that tab once the window is open
    if win.open.is_some()
        && let Some(t) = std::env::var("KENSHI_OVERVIEW_TAB").ok().and_then(|v| v.parse::<u8>().ok())
    {
        switch = switch.or(Some(t));
    }
    // the tab asked for: closed when it is the one shown, else shown
    if let Some(t) = asked {
        match win.open.as_ref() {
            Some(o) if o.tab == t => toggled = true,
            Some(_) => switch = Some(t),
            None => {
                toggled = true;
                want_tab = t;
            }
        }
    }
    if win.open.is_some() && (toggled || close || keys.just_pressed(KeyCode::Escape)) {
        let o = win.open.take().unwrap();
        for r in o.ui.roots.iter().chain(o.rows.iter()) {
            commands.entity(*r).despawn();
        }
        if let Some(t) = &o.tabs {
            for r in &t.roots {
                commands.entity(*r).despawn();
            }
        }
        return;
    }
    if toggled {
        let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
        let mut defs = gui.0.layout("Kenshi_OverviewWindow.layout");
        // the research and crafting tabs: the tab control's other items go, these become widgets
        fn research_only(d: &mut kenshi_gui::res::WidgetDef) {
            if d.name == "TabsMain" {
                // the AI tab has no name of its own
                for c in &mut d.children {
                    if c.name.is_empty() && c.children.iter().any(|k| k.name == "AIOptions") {
                        c.name = "AITab".into();
                    }
                }
                d.children.retain(|c| ["ResearchTab", "CraftTab", "MapTab", "FactionTab", "SquadsTab", "MessagesTab", "AITab"].contains(&c.name.as_str()));
                for c in &mut d.children {
                    c.kind = "Widget".into();
                    c.props.retain(|(k, _)| k != "Caption" && k != "Visible");
                }
                d.kind = "Widget".into();
            }
            for c in &mut d.children {
                research_only(c);
            }
        }
        for d in &mut defs {
            research_only(d);
            if let kenshi_gui::res::Pos::Real(r) = d.pos {
                d.pos = kenshi_gui::res::Pos::Real([(1.0 - r[2]) * 0.5, 0.08, r[2], r[3]]);
            }
            // a caption to set (F035128's, see `redraw`)
            d.props.retain(|(k, _)| k != "Caption");
            d.props.push(("Caption".into(), "RESEARCH".into()));
        }
        let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
        for r in &ui.roots {
            commands.entity(*r).insert(GlobalZIndex(60));
        }
        let font = |name: &str| gui.0.fonts.get(name).map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
        let mut o = Open {
            ui,
            types: types(&game),
            kind: 0,
            list: Vec::new(),
            chosen: None,
            rows: Vec::new(),
            fonts: (font("Kenshi_StandardFont_Medium"), font("Kenshi_PaintedTextFont_Small")),
            shown: Default::default(),
            tab: want_tab,
            tabs: None,
            stations: Vec::new(),
            station: station_request.and_then(|b| game.sim.crafting_benches().iter().position(|&s| s == b)).unwrap_or(0),
            items: Vec::new(),
            item: None,
            craft_shown: Default::default(),
            map_zoom: 1.0,
            map_rect: None,
            map_centre: [0.0, 0.0],
            map_drag: None,
            map_nodes: Vec::new(),
            map_drawn: -1.0,
            faction_scroll: 0,
            relations_canvas: None,
            faction_column: None,
            relations_offset: 0.0,
            relations_range: 0.0,
            name_shown: None,
            screen: size,
            log_len: usize::MAX,
            ai_tip: None,
            squads_shown: Vec::new(),
            squads_shown_portraits: 0,
            squad_boxes: Vec::new(),
            squad_portraits: Vec::new(),
            squad_scroll: 0,
            squad_drag: None,
            squad_test_frames: 0,
            queue_x: Vec::new(),
            queue_drag: None,
        };
        // the two tab buttons (Kenshi_TabControl's ButtonWidth 140), over the tab area
        if let Some(place) = o.ui.get("TabsMain") {
            let scale = kenshi_gui::res::ui_scale();
            let size = o.ui.sizes.get(&place).copied().unwrap_or([800.0, 600.0]);
            // (assumption) seven at the tab control's ButtonWidth (140) overrun the window: as
            // many as fit side by side
            let w = (140.0 * scale).min(size[0] / 7.0 - 4.0 * scale);
            let button = |name: &str, caption: &str, k: f32| kenshi_gui::res::WidgetDef {
                kind: "Button".into(),
                skin: "Kenshi_Button1".into(),
                pos: kenshi_gui::res::Pos::Px([k * (w + 4.0 * scale), 0.0, w, size[1] * 0.045]),
                align: Default::default(),
                name: name.into(),
                layer: None,
                props: vec![("Caption".into(), caption.into())],
                user: Vec::new(),
                children: Vec::new(),
            };
            o.tabs = Some(kenshi_gui::ui::spawn(
                &mut commands,
                &gui.0,
                &assets,
                &[
                    button("OverviewTabMap", "MAP", 0.0),
                    button("OverviewTabFaction", "FACTION", 1.0),
                    button("OverviewTabResearch", "RESEARCH", 2.0),
                    button("OverviewTabCrafting", "CRAFTING", 3.0),
                    button("OverviewTabSquads", "SQUADS", 4.0),
                    button("OverviewTabDialogue", "DIALOGUE", 5.0),
                    button("OverviewTabAI", "AI", 6.0),
                ],
                Some(place),
                size,
            ));
        }
        redraw(&mut commands, &mut o, &game, &gui, &assets);
        win.open = Some(o);
        return;
    }
    let Some(o) = win.open.as_mut() else { return };
    // the map: zoom, centre, drag with the left button, the wheel; drawn a few times a second
    if o.tab == 2 {
        if map_zoom != 1.0 {
            o.map_zoom = (o.map_zoom * map_zoom).clamp(1.0, 32.0);
            o.map_drawn = -1.0;
        }
        for w in wheel.read() {
            o.map_zoom = (o.map_zoom * if w.y > 0.0 { 1.25 } else { 0.8 }).clamp(1.0, 32.0);
            o.map_drawn = -1.0;
        }
        if map_centre && let Some(&me) = game.selected.first() {
            let p = game.sim.folk[me].mover.position_units();
            o.map_centre = [p[0], p[2]];
            o.map_drawn = -1.0;
        }
        // the view as laid out (the layout's own numbers are its design size, not what is shown)
        // (measured on its tab, MapTab: the scroll view itself grows to its 4096 canvas)
        if let Some(tab) = o.ui.get("MapTab")
            && let Ok((n, t)) = laid.get(tab)
        {
            let k = n.inverse_scale_factor();
            let tab_size = n.size() * k;
            let size = tab_size * Vec2::new(0.995495, 0.944109);
            let top_left = t.translation * k - tab_size * 0.5 + Vec2::new(0.0, tab_size.y * 0.00302115);
            if size.x > 1.0 && o.map_rect.is_none_or(|(s, p)| (s - size).length() > 0.5 || (p - top_left).length() > 0.5) {
                o.map_rect = Some((size, top_left));
                o.map_drawn = -1.0;
            }
        }
        let cursor = windows.iter().next().and_then(|w| w.cursor_position());
        if let (Some((sz, at)), Some(c)) = (o.map_rect, cursor) {
            let inside = c.x >= at.x && c.y >= at.y && c.x <= at.x + sz.x && c.y <= at.y + sz.y;
            if mouse.pressed(MouseButton::Left) && let Some(last) = o.map_drag {
                let d = c - last;
                let per_px = kenshi_world::WORLD_SIZE / (sz.x.max(sz.y) * o.map_zoom);
                o.map_centre[0] -= d.x * per_px;
                o.map_centre[1] += d.y * per_px;
                o.map_drawn = -1.0;
            }
            // a drag starts only on the map itself
            o.map_drag = mouse.pressed(MouseButton::Left).then_some(c).filter(|_| o.map_drag.is_some() || (mouse.just_pressed(MouseButton::Left) && inside));
        }
        let now = time.elapsed_secs();
        if now - o.map_drawn > 0.25 {
            o.map_drawn = now;
            redraw_map(&mut commands, o, &game, &gui, &assets);
        }
    }
    // the relations panel's canvas (F035125, `RelationsCanvas`), known once the panel's parts
    // are made
    if o.relations_canvas.is_none()
        && let Some(panel) = o.ui.get("RelationsPanel")
        && let Ok(p) = parts.get(panel)
        && let Some(&client) = p.0.get("Client")
    {
        let size = o.ui.sizes.get(&client).copied().unwrap_or([0.0, 0.0]);
        let bar = p.0.get("VScroll").copied();
        let hbar_h = p.0.get("HScroll").and_then(|b| o.ui.sizes.get(b)).map_or(0.0, |s| s[1]);
        let view_h = size[1] + hbar_h;
        // the client over the hidden horizontal bar's place (ScrollViewBase)
        commands.queue(move |world: &mut World| {
            if let Some(mut n) = world.get_mut::<Node>(client) {
                n.height = Val::Px(view_h);
            }
        });
        o.relations_canvas = Some(RelationsCanvas { client, width: size[0].max(1.0), view_h, bar });
        if o.tab == 3 {
            o.map_drawn = -1.0;
        }
    }
    // the faction tab: its lines made again every 2 seconds (F035139); the wheel and the
    // scroll bar move its scroll view
    if o.tab == 3 {
        let mut moved = false;
        // (MyGUI's wheel goes to the widget under the mouse)
        let cursor = windows.iter().next().and_then(|w| w.cursor_position());
        let over = o.ui.get("RelationsPanel").is_some_and(|p| cursor.is_some_and(|c| inside(rect_of(&laid, p), c)));
        for w in wheel.read() {
            if over && o.relations_range > 0.0 {
                o.relations_offset = (o.relations_offset + if w.y < 0.0 { 50.0 } else { -50.0 }).clamp(0.0, o.relations_range);
                moved = true;
            }
        }
        let bar = o.relations_canvas.and_then(|c| c.bar);
        for m in scrolled.read() {
            if Some(m.widget) == bar {
                o.relations_offset = m.pos.round().clamp(0.0, o.relations_range);
                moved = true;
            }
        }
        // (testing) KENSHI_FACTION_WHEEL=n: n wheel notches down over the panel once it can scroll
        if let Some(n) = std::env::var("KENSHI_FACTION_WHEEL").ok().and_then(|v| v.parse::<i32>().ok())
            && o.relations_range > 0.0
            && o.relations_offset == 0.0
            && !o.ui.named.is_empty()
        {
            o.relations_offset = (50.0 * n as f32).clamp(0.0, o.relations_range);
            moved = true;
        }
        if moved {
            relations_scrolled(&mut commands, o);
        }
        let now = time.elapsed_secs();
        if o.map_drawn < 0.0 || now - o.map_drawn >= 2.0 {
            o.map_drawn = now;
            faction_lines(&mut commands, o, &game, &gui, &assets);
        }
    }
    // the dialogue tab: drawn again as the log grows, scrolled back by the wheel
    if o.tab == 5 {
        let mut moved = false;
        for w in wheel.read() {
            o.faction_scroll = if w.y > 0.0 { o.faction_scroll + 3 } else { o.faction_scroll.saturating_sub(3) };
            moved = true;
        }
        if moved || o.log_len != game.log.len() {
            redraw(&mut commands, o, &game, &gui, &assets);
        }
    }
    // the AI tab: drawn on showing and on a change; the hovered option's tooltip below
    if o.tab == 6 {
        if ai_changed || o.map_drawn < 0.0 {
            o.map_drawn = time.elapsed_secs();
            redraw_ai(&mut commands, o, &game, &gui, &assets);
        }
        if let Some(t) = o.ai_tip {
            let hovered = ai_rows.iter().find(|(i, _)| **i != Interaction::None).map(|(_, r)| r.0);
            let text = hovered.and_then(|k| AI_OPTIONS.get(k)).map_or("", |a| a.2);
            commands.queue(SetCaption(t, text.to_string()));
        }
    }
    if o.tab == 4 {
        let mut cursor = windows.iter().next().and_then(|w| w.cursor_position());
        let mut buttons = (mouse.just_pressed(MouseButton::Left), mouse.just_released(MouseButton::Left));
        // (testing) KENSHI_SQUAD_DRAG="frame@portrait>squad" (or ">dismiss"): that many frames
        // into the tab, the portrait shown at that place taken up at its middle and, 10 frames
        // later, let go over that squad's box (or DRAG HERE TO DISMISS)
        if let Ok(v) = std::env::var("KENSHI_SQUAD_DRAG")
            && let Some((at, rest)) = v.split_once('@')
            && let Some((from, to)) = rest.split_once('>')
            && let (Ok(at), Ok(from)) = (at.parse::<u32>(), from.parse::<usize>())
        {
            o.squad_test_frames += 1;
            let mid = |e: Entity| rect_of(&laid, e).map(|(p, sz)| p + sz * 0.5);
            if o.squad_test_frames == at {
                cursor = o.squad_portraits.get(from).and_then(|&(e, _)| mid(e));
                buttons = (true, false);
                eprintln!("squad drag: take portrait {from} at {cursor:?}");
            } else if o.squad_test_frames == at + 10 {
                cursor = if to == "dismiss" { o.ui.get("DismissCharPanel").and_then(mid) } else { to.parse::<usize>().ok().and_then(|k| o.squad_boxes.iter().find(|b| b.0 == k)).and_then(|b| mid(b.1)) };
                buttons = (false, true);
                eprintln!("squad drag: let go over {to} at {cursor:?}");
            }
        }
        let notches: f32 = wheel.read().map(|w| w.y.signum()).sum();
        let first = o.map_drawn < 0.0 || o.squads_shown.is_empty();
        if squads_tab(&mut commands, o, &mut game, &squad_clicks, &squad_edits, buttons, cursor, notches, &laid, &portraits) || first {
            o.map_drawn = time.elapsed_secs();
            redraw_squads(&mut commands, o, &game, &gui, &assets, &portraits);
        }
    } else if let Some((_, ghost)) = o.squad_drag.take() {
        commands.entity(ghost).despawn();
    }
    // @fn F035114
    //
    // The faction's name as typed, taken on Enter or as the box lets the keyboard go (F035125's
    // eventEditSelectAccept, F035115 on eventKeyLostFocus): a name another faction has (F030301)
    // is refused, "This faction name already exists" (title "Faction Name"), the box back to
    // the name; any other new one is the faction's (GameWorld +8, the faction's +0x1a8).
    // (stand-in) An empty name shows as the record's ("Nameless": Sim::player_faction_name).
    for e in edits_done.read() {
        if o.ui.get("FactionNameText") != Some(e.widget) {
            continue;
        }
        let now = game.sim.player_faction_name();
        if e.text == now {
            continue;
        }
        let taken = game.sim.data.of_type(ItemType::Faction).any(|f| f.string_id != kenshi_game::sim::PLAYER_FACTION && f.name == e.text);
        if taken {
            game.ask = Some(super::message_box::Ask { title: "Faction Name".into(), text: "This faction name already exists".into(), question: super::message_box::Question::Note });
            commands.queue(SetCaption(e.widget, now));
        } else {
            game.sim.faction_name = e.text.clone();
            let name = game.sim.player_faction_name();
            commands.queue(SetCaption(e.widget, name.clone()));
            o.name_shown = Some(name);
        }
    }
    // the lists: left click chooses, right click learns (a blueprint) or takes off (the queue)
    let mut changed = false;
    // a queue row's X: that one taken off (F035994's RemoveButton)
    for w in removes {
        let Some(&(_, tag, k)) = o.queue_x.iter().find(|(x, ..)| *x == w) else { continue };
        if tag == QUEUE {
            game.sim.unqueue_research(k);
        } else if let Some(&b) = o.stations.get(o.station) {
            let pb = &mut game.sim.buildings[b];
            if k < pb.craft_queue.len() {
                pb.craft_queue.remove(k);
                if k == 0 {
                    pb.made.clear();
                }
            }
        }
        changed = true;
    }
    // a queue row dragged onto another (F036005)
    if mouse.just_pressed(MouseButton::Left)
        && let Some((_, row)) = rows.iter().find(|(i, r)| **i == Interaction::Pressed && (r.0 == QUEUE || r.0 == CRAFT_QUEUE))
    {
        o.queue_drag = Some((row.0, row.1));
    }
    if let Some((tag, from)) = o.queue_drag
        && !mouse.pressed(MouseButton::Left)
    {
        o.queue_drag = None;
        if let Some((r, _)) = queue_over.iter().find(|(r, c)| r.0 == tag && c.cursor_over()) {
            let to = r.1;
            if tag == QUEUE {
                let first = game.sim.research_queue.first().cloned();
                if move_queued(&mut game.sim.research_queue, from, to) {
                    if game.sim.research_queue.first() != first.as_ref() {
                        game.sim.research_progress = 0.0;
                        game.sim.research_paid = false;
                    }
                    changed = true;
                }
            } else if let Some(&b) = o.stations.get(o.station) {
                let pb = &mut game.sim.buildings[b];
                let first = pb.craft_queue.first().cloned();
                if move_queued(&mut pb.craft_queue, from, to) {
                    if pb.craft_queue.first() != first.as_ref() {
                        pb.made.clear();
                    }
                    changed = true;
                }
            }
        }
    }
    for (i, row) in &rows {
        let left = *i == Interaction::Pressed && mouse.just_pressed(MouseButton::Left);
        let right = *i == Interaction::Hovered && mouse.just_pressed(MouseButton::Right);
        if !(left || right) {
            continue;
        }
        match (row.0, right) {
            (TYPES, false) if o.kind != row.1 => {
                o.kind = row.1;
                o.chosen = None;
                changed = true;
            }
            (BLUEPRINTS, false) => {
                o.chosen = o.list.get(row.1).cloned();
                changed = true;
            }
            (BLUEPRINTS, true) => {
                if let Some(id) = o.list.get(row.1).cloned() {
                    o.chosen = Some(id.clone());
                    if let Err(e) = game.sim.queue_research(&id) {
                        game.sim.research_message = Some(e);
                    }
                    changed = true;
                }
            }
            (QUEUE, true) => {
                game.sim.unqueue_research(row.1);
                changed = true;
            }
            (STATIONS, false) => {
                o.station = row.1;
                o.item = None;
                changed = true;
            }
            (ITEMS, false) => {
                if let (Some(&b), Some(item)) = (o.stations.get(o.station), o.items.get(row.1).cloned()) {
                    o.item = Some(item.clone());
                    game.sim.buildings[b].craft_queue.push(item);
                    changed = true;
                }
            }
            (ITEMS, true) => {
                o.item = o.items.get(row.1).cloned();
                changed = true;
            }
            (CRAFT_QUEUE, true) => {
                if let Some(&b) = o.stations.get(o.station) {
                    let pb = &mut game.sim.buildings[b];
                    if row.1 < pb.craft_queue.len() {
                        pb.craft_queue.remove(row.1);
                        if row.1 == 0 {
                            pb.made.clear();
                        }
                    }
                    changed = true;
                }
            }
            _ => {}
        }
    }
    // (testing) KENSHI_OVERVIEW_TAB=n: that tab once open
    if let Some(t) = std::env::var("KENSHI_OVERVIEW_TAB").ok().and_then(|v| v.parse::<u8>().ok())
        && o.map_zoom == 1.0
        && o.tab == 2
        && t != 2
    {
        switch = Some(t);
    }
    if let Some(t) = switch
        && t != o.tab
    {
        o.tab = t;
        o.map_drawn = -1.0;
        changed = true;
    }
    if repeat && let Some(&b) = o.stations.get(o.station) {
        let pb = &mut game.sim.buildings[b];
        pb.craft_repeat = !pb.craft_repeat;
        changed = true;
    }
    // the state shown, redrawn when it changes
    let sim = &game.sim;
    let first_time = sim.research_queue.first().and_then(|id| sim.data.get(id)).map_or(1.0, kenshi_game::production::research_time).max(0.01);
    let state = (
        sim.research_queue.clone(),
        (sim.research_progress / first_time * 100.0) as u32,
        sim.research_done.len(),
        sim.research_message.clone(),
        sim.bench_level(),
        (sim.research_rate * 100.0) as u32,
    );
    let craft_state = o.stations.get(o.station).and_then(|&b| sim.buildings.get(b)).map_or(Default::default(), |pb| {
        (pb.craft_queue.clone(), (pb.made.first().copied().unwrap_or(0.0) * 100.0) as u32, pb.craft_repeat, sim.crafting_benches().len())
    });
    if changed || state != o.shown || craft_state != o.craft_shown {
        o.shown = state;
        o.craft_shown = craft_state;
        redraw(&mut commands, o, &game, &gui, &assets);
    }
}

/// (testing) KENSHI_RESEARCH_AT=frame[@research name]: the window opens then; with a name, that
/// research is queued (as a right click on it). KENSHI_CRAFTING_AT=frame: the crafting tab.
pub fn script(mut frames: Local<u32>, mut keys: ResMut<ButtonInput<KeyCode>>, mut game: ResMut<Game>, win: Res<ResearchWindow>, mut done: MessageWriter<kenshi_gui::GuiEditDone>) {
    *frames += 1;
    // KENSHI_KNOW_ALL: every faction known to the player's at frame 30 (the faction tab full)
    if *frames == 30 && std::env::var("KENSHI_KNOW_ALL").is_ok() {
        let all: Vec<String> = game.sim.data.of_type(ItemType::Faction).map(|f| f.string_id.clone()).collect();
        game.sim.world.known_factions.extend(all);
    }
    // KENSHI_FACTION_NAME=frame@name: that name typed into the faction tab's name box and
    // taken (as on Enter)
    if let Ok(v) = std::env::var("KENSHI_FACTION_NAME")
        && let Some((f, name)) = v.split_once('@')
        && f.parse::<u32>().ok() == Some(*frames)
        && let Some(e) = win.open.as_ref().and_then(|o| o.ui.get("FactionNameText"))
    {
        done.write(kenshi_gui::GuiEditDone { widget: e, text: name.to_string() });
    }
    // KENSHI_OVERVIEW_AT=frame@key: that key pressed (M map, T research, Y crafting)
    if let Ok(v) = std::env::var("KENSHI_OVERVIEW_AT")
        && let Some((f, k)) = v.split_once('@')
        && f.parse::<u32>().ok().is_some_and(|f| f == *frames || f + 1 == *frames)
    {
        let key = match k {
            "M" => KeyCode::KeyM,
            "Y" => KeyCode::KeyY,
            _ => KeyCode::KeyT,
        };
        if f.parse::<u32>().ok() == Some(*frames) {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
    // KENSHI_CRAFTING_AT=frame: the crafting tab on the first crafting bench
    if std::env::var("KENSHI_CRAFTING_AT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames) {
        game.crafting_request = game.sim.crafting_benches().first().copied();
    }
    let Ok(v) = std::env::var("KENSHI_RESEARCH_AT") else { return };
    let (at, name) = match v.split_once('@') {
        Some((a, n)) => (a.parse::<u32>().unwrap_or(0), Some(n.to_string())),
        None => (v.parse::<u32>().unwrap_or(0), None),
    };
    if *frames == at {
        keys.press(KeyCode::KeyT);
        if let Some(n) = name {
            let id = game.sim.data.of_type(ItemType::Research).find(|r| r.name == n).map(|r| r.string_id.clone());
            match id {
                Some(id) => {
                    let r = game.sim.queue_research(&id);
                    eprintln!("research script: queue {n}: {r:?}");
                }
                None => eprintln!("research script: no research {n}"),
            }
        }
    }
    if *frames == at + 1 {
        keys.release(KeyCode::KeyT);
    }
}
