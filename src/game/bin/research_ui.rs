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
//! (stand-in) The window shows these five tabs (the overview's dialogue and AI tabs are not
//! ported), switched by buttons over them; a queued research's or item's progress is shown
//! after its name; the repeat button's state is in its caption; every town is on the map (not
//! only those found); the map's way round as the biome map's (rows against z); squads can't be
//! made or moved between yet (one squad).
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
    /// The overview window's tab shown (0 research, 1 crafting, 2 map, 3 faction, 4 squads), if
    /// it is open.
    pub fn tab(&self) -> Option<u8> {
        self.open.as_ref().map(|o| o.tab)
    }
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
    /// The tab shown: 0 research, 1 crafting, 2 map, 3 faction, 4 squads; the tab buttons.
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
    /// The faction tab's lines scrolled past.
    faction_scroll: usize,
    tabs: Option<Built>,
    /// The crafting tab: the stations (building indices), the one chosen, its items, the item
    /// chosen, and what was last shown (queue, progress %, repeat).
    stations: Vec<usize>,
    station: usize,
    items: Vec<String>,
    item: Option<String>,
    craft_shown: (Vec<String>, u32, bool, usize),
}

/// The research types: the RESEARCH records' "category", in name order.
fn types(game: &Game) -> Vec<String> {
    let mut out: Vec<String> = game.sim.data.of_type(ItemType::Research).filter_map(|r| r.strings.get("category").cloned()).filter(|c| !c.is_empty()).collect();
    out.sort();
    out.dedup();
    out
}

fn redraw(commands: &mut Commands, o: &mut Open, game: &Game) {
    for r in o.rows.drain(..) {
        commands.entity(r).despawn();
    }
    for (name, on) in [("ResearchTab", o.tab == 0), ("CraftTab", o.tab == 1), ("MapTab", o.tab == 2), ("FactionTab", o.tab == 3), ("SquadsTab", o.tab == 4)] {
        if let Some(e) = o.ui.get(name) {
            commands.queue(SetVisible(e, on));
        }
    }
    if let (Some(cap), Some(name)) = (o.ui.get("Root.Caption"), ["", "", "MAP", "FACTION", "SQUADS"].get(o.tab as usize).filter(|n| !n.is_empty())) {
        commands.queue(SetCaption(cap, name.to_string()));
    }
    match o.tab {
        1 => {
            redraw_crafting(commands, o, game);
            return;
        }
        3 => {
            redraw_faction(commands, o, game);
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
        let rows: Vec<(String, bool)> = sim
            .research_queue
            .iter()
            .enumerate()
            .filter_map(|(k, id)| {
                let r = sim.data.get(id)?;
                let time = kenshi_game::production::research_time(r).max(0.01);
                Some(if k == 0 { (format!("{}  {:.0}%", r.name, sim.research_progress / time * 100.0), true) } else { (r.name.clone(), true) })
            })
            .collect();
        o.rows.push(list_rows(commands, l, QUEUE, &rows, None, &font, (2.0, 2.0, 96.0, 96.0)));
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

/// The crafting tab: the stations, the chosen one's items and queue, the repeat button, the
/// item's description.
fn redraw_crafting(commands: &mut Commands, o: &mut Open, game: &Game) {
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
        let rows: Vec<(String, bool)> = pb.map_or(Vec::new(), |pb| {
            pb.craft_queue
                .iter()
                .enumerate()
                .map(|(k, i)| {
                    let pct = pb.made.first().copied().unwrap_or(0.0).clamp(0.0, 1.0) * 100.0;
                    (if k == 0 { format!("{}  {pct:.0}%", name_of(i)) } else { name_of(i) }, true)
                })
                .collect()
        });
        o.rows.push(list_rows(commands, l, CRAFT_QUEUE, &rows, None, &font, (2.0, 2.0, 96.0, 96.0)));
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
    let marker = |commands: &mut Commands, at: Vec2, rect: [f32; 4], tint: Color| {
        let e = commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(at.x - rect[2] * 0.25), top: Val::Px(at.y - rect[3] * 0.25), width: Val::Px(rect[2] * 0.5), height: Val::Px(rect[3] * 0.5), ..default() },
                ImageNode { image: ui_png.clone().unwrap_or_default(), rect: Some(Rect::new(rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3])), color: tint, ..default() },
            ))
            .id();
        commands.entity(clip).add_child(e);
    };
    // the towns found (assumption: as Kenshi's, the map shows a town once the player has been
    // near it): Kenshi_MapMarkers' group by the town's type
    let mut names: Vec<Rect> = Vec::new();
    let mut found: Vec<_> = game.sim.towns.iter().filter(|t| game.sim.world.discovered.contains(&t.record)).collect();
    found.sort_by_key(|t| !(t.kind == 1 || t.kind == 2));
    for t in found {
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
        marker(commands, at, rect, Color::WHITE);
        // a name over another's is left out (the bigger places drawn first keep theirs)
        let est = Rect::new(at.x + 10.0, at.y - 8.0, at.x + 10.0 + t.name.len() as f32 * o.fonts.0.1 * 0.55, at.y - 8.0 + o.fonts.0.1 * 1.2);
        let free = names.iter().all(|r| r.intersect(est).is_empty());
        if free && (t.kind == 1 || t.kind == 2 || o.map_zoom >= 3.0) {
            names.push(est);
            let name = commands
                .spawn((
                    Text::new(t.name.clone()),
                    TextFont { font: bevy::text::FontSource::Handle(o.fonts.0.0.clone()), font_size: bevy::text::FontSize::Px(o.fonts.0.1), ..default() },
                    TextColor(hud_ui::LIGHT),
                    TextShadow::default(),
                    Node { position_type: PositionType::Absolute, left: Val::Px(at.x + 10.0), top: Val::Px(at.y - 8.0), ..default() },
                ))
                .id();
            commands.entity(clip).add_child(name);
        }
    }
    // the player's characters (the selected brighter)
    for p in game.sim.folk.iter().filter(|p| p.player && !p.dead) {
        let q = p.mover.position_units();
        let tint = if game.selected.contains(&p.id) { Color::srgb(0.4, 1.0, 0.4) } else { Color::srgb(0.25, 0.7, 0.25) };
        marker(commands, to_px(q[0], q[2]), [1305.0, 27.0, 32.0, 32.0], tint);
    }
}

/// The faction tab (F035125, F035113): the player's faction's name and numbers, every faction's
/// relation with it.
fn redraw_faction(commands: &mut Commands, o: &mut Open, game: &Game) {
    let sim = &game.sim;
    let mine = sim.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
    let rel = mine.as_ref().and_then(|f| sim.relations.get(f).cloned()).or_else(|| mine.as_ref().and_then(|f| sim.data.get(f)).map(|r| kenshi_ai::faction::FactionRelations::from_data(r, &sim.data)));
    if let Some(e) = o.ui.get("FactionNameText") {
        commands.queue(SetCaption(e, mine.as_deref().and_then(|f| sim.data.get(f)).map_or("Nameless".to_string(), |r| r.name.clone())));
    }
    if let Some(e) = o.ui.get("lbFactionName") {
        commands.queue(SetCaption(e, "FACTION NAME".to_string()));
    }
    if let Some(p) = o.ui.get("FactionInfoPanel") {
        let members = sim.folk.iter().filter(|p| p.player && !p.dead).count();
        let rows = vec![
            hud_ui::row("Members:", format!("{members}")),
            hud_ui::row("Money:", format!("c.{}", sim.money)),
            hud_ui::row("Buildings:", format!("{}", sim.buildings.iter().filter(|b| b.done).count())),
            hud_ui::row("Bounties:", format!("c.{}", sim.folk.iter().filter(|p| p.player).flat_map(|p| p.bounties.iter()).map(|b| b.amount).sum::<i32>())),
        ];
        commands.queue(hud_ui::SetRows { panel: p, rows, font: o.fonts.0.clone(), label_colour: hud_ui::LIGHT, centre: false });
    }
    let Some(panel) = o.ui.get("RelationsPanel") else { return };
    // the factions with a name, by their relation (friends first)
    let mut list: Vec<(String, f32)> = sim
        .data
        .of_type(ItemType::Faction)
        .filter(|f| Some(&f.string_id) != mine.as_ref() && !f.name.starts_with('_') && !f.bools.get("not real").copied().unwrap_or(false))
        .map(|f| {
            // the relation as the player's faction holds it, else as the other holds it, else its default
            let theirs = mine.as_ref().and_then(|m| f.refs("relations").iter().find(|(o, _)| o == m).map(|(_, v)| v[0] as f32));
            let ours = rel.as_ref().and_then(|r| r.relations.get(&f.string_id).map(|x| x.relation));
            let default = f.ints.get("default relation").copied().unwrap_or(0) as f32;
            (f.name.clone(), ours.or(theirs).unwrap_or(default))
        })
        .collect();
    // (assumption) the strongest feelings first, friend or foe, then by name
    list.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()).then(a.0.cmp(&b.0)));
    list.dedup_by(|a, b| a.0 == b.0);
    o.faction_scroll = o.faction_scroll.min(list.len().saturating_sub(1));
    let scale = kenshi_gui::res::ui_scale();
    let column = commands
        .spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, overflow: Overflow::clip(), ..default() }, ZIndex(3)))
        .id();
    commands.entity(panel).add_child(column);
    o.rows.push(column);
    let text = |s: String, c: Color| (Text::new(s), TextFont { font: bevy::text::FontSource::Handle(o.fonts.0.0.clone()), font_size: bevy::text::FontSize::Px(o.fonts.0.1), ..default() }, TextColor(c));
    for (name, r) in list.into_iter().skip(o.faction_scroll).take(60) {
        // Kenshi_FactionRelationsLine: a bar from the middle, right for friends, left for foes
        let line = commands.spawn(Node { width: Val::Percent(100.0), height: Val::Px(22.0 * scale), flex_shrink: 0.0, margin: UiRect::bottom(Val::Px(2.0)), ..default() }).id();
        let frac = (r / 100.0).clamp(-1.0, 1.0);
        let bar = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(if frac >= 0.0 { 50.0 } else { 50.0 + 49.0 * frac }),
                    width: Val::Percent(49.0 * frac.abs()),
                    top: Val::Percent(21.0),
                    height: Val::Percent(58.0),
                    ..default()
                },
                BackgroundColor(if frac >= 0.0 { Color::srgba(0.35, 0.55, 0.25, 0.8) } else { Color::srgba(0.6, 0.18, 0.12, 0.8) }),
            ))
            .id();
        let n = commands.spawn((text(name, hud_ui::LIGHT), Node { position_type: PositionType::Absolute, left: Val::Percent(2.0), ..default() })).id();
        let v = commands.spawn((text(format!("{r:.0}"), hud_ui::LIGHT), Node { position_type: PositionType::Absolute, right: Val::Percent(2.0), ..default() })).id();
        commands.entity(line).add_children(&[bar, n, v]);
        commands.entity(column).add_child(line);
    }
}

/// The squads tab (F035124): each squad (one: the player's) as a Kenshi_SquadBox with its
/// people's portraits, the faction's size.
fn redraw_squads(commands: &mut Commands, o: &mut Open, game: &Game, gui: &GuiRes, assets: &AssetServer, portraits: &super::portraits::Portraits) {
    for e in o.map_nodes.drain(..) {
        commands.entity(e).despawn();
    }
    let members: Vec<usize> = game.sim.folk.iter().filter(|p| p.player && !p.dead).map(|p| p.id).collect();
    if let Some(e) = o.ui.get("FactionSizeText") {
        commands.queue(SetCaption(e, format!("{}/256", members.len())));
    }
    let Some(panel) = o.ui.get("SquadsPanel") else { return };
    let size = o.ui.sizes.get(&panel).copied().unwrap_or([800.0, 600.0]);
    let mut defs = gui.0.layout("Kenshi_SquadBox.layout");
    if let Some(d) = defs.first_mut() {
        d.pos = kenshi_gui::res::Pos::Px([0.0, 0.0, size[0], size[1] * 0.3]);
    }
    let b = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, Some(panel), size);
    o.map_nodes.extend(b.roots.iter().copied());
    let faction = game.sim.folk.iter().find(|p| p.player).and_then(|p| p.faction.as_deref()).and_then(|f| game.sim.data.get(f)).map_or("Squad".to_string(), |r| r.name.clone());
    if let Some(e) = b.get("SquadNameText") {
        commands.queue(SetCaption(e, format!("{faction}_0").to_uppercase()));
    }
    if let Some(e) = b.get("SquadSizeText") {
        commands.queue(SetCaption(e, format!("{} / 20", members.len())));
    }
    let Some(area) = b.get("PortraitsPanel") else { return };
    let asize = b.sizes.get(&area).copied().unwrap_or([600.0, 100.0]);
    let side = asize[1] * 0.9;
    for (k, &id) in members.iter().enumerate() {
        let mut defs = gui.0.layout("Kenshi_SquadsPanel_PortraitCharacter.layout");
        if let Some(d) = defs.first_mut() {
            d.pos = kenshi_gui::res::Pos::Px([4.0 + k as f32 * (side + 6.0), asize[1] * 0.05, side, side]);
        }
        let pb = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, Some(area), asize);
        if let Some(e) = pb.get("NameText") {
            commands.queue(SetCaption(e, game.sim.display_name(id)));
        }
        if let (Some(e), Some((_, image))) = (pb.get("PortraitImage"), portraits.by.get(&id)) {
            commands.entity(e).insert(ImageNode { image: image.clone(), color: Color::linear_rgb(super::portraits::EXPOSURE, super::portraits::EXPOSURE, super::portraits::EXPOSURE), ..default() });
        }
    }
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
) {
    // the tab asked for (controls.cfg: T research, Y crafting, M map); the main panel's TCH, MAP and SQD
    let mut asked: Option<u8> = None;
    for (a, t) in [("toggle_research", 0u8), ("toggle_crafting", 1), ("toggle_map", 2)] {
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
            redraw(&mut commands, o, &game);
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
    for c in clicks.read() {
        match c.name.as_str() {
            "ShortcutTechButton" => asked = Some(0),
            "ShortcutMapButton" => asked = Some(2),
            "ShortcutSquadButton" => asked = Some(4),
            "OverviewTabResearch" => switch = Some(0),
            "OverviewTabCrafting" => switch = Some(1),
            "OverviewTabMap" => switch = Some(2),
            "OverviewTabFaction" => switch = Some(3),
            "OverviewTabSquads" => switch = Some(4),
            "MapZoomInButton" => map_zoom = 1.5,
            "MapZoomOutButton" => map_zoom = 1.0 / 1.5,
            "MapCenterButton" => map_centre = true,
            "QueueRepeat" => repeat = true,
            // the window's header buttons (close, pin)
            "Button" if win.open.as_ref().is_some_and(|o| o.ui.named.iter().any(|(_, e)| *e == c.widget)) => close = true,
            _ => {}
        }
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
                d.children.retain(|c| ["ResearchTab", "CraftTab", "MapTab", "FactionTab", "SquadsTab"].contains(&c.name.as_str()));
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
        };
        // the two tab buttons (Kenshi_TabControl's ButtonWidth 140), over the tab area
        if let Some(place) = o.ui.get("TabsMain") {
            let scale = kenshi_gui::res::ui_scale();
            let size = o.ui.sizes.get(&place).copied().unwrap_or([800.0, 600.0]);
            let button = |name: &str, caption: &str, k: f32| kenshi_gui::res::WidgetDef {
                kind: "Button".into(),
                skin: "Kenshi_Button1".into(),
                pos: kenshi_gui::res::Pos::Px([k * 144.0 * scale, 0.0, 140.0 * scale, size[1] * 0.045]),
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
                ],
                Some(place),
                size,
            ));
        }
        redraw(&mut commands, &mut o, &game);
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
    // the faction tab's list scrolled by the wheel
    if o.tab == 3 {
        let mut moved = false;
        for w in wheel.read() {
            o.faction_scroll = if w.y > 0.0 { o.faction_scroll.saturating_sub(3) } else { o.faction_scroll + 3 };
            moved = true;
        }
        if moved {
            redraw(&mut commands, o, &game);
        }
    }
    if o.tab == 4 {
        let now = time.elapsed_secs();
        if now - o.map_drawn > 1.0 {
            o.map_drawn = now;
            redraw_squads(&mut commands, o, &game, &gui, &assets, &portraits);
        }
    }
    // the lists: left click chooses, right click learns (a blueprint) or takes off (the queue)
    let mut changed = false;
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
        redraw(&mut commands, o, &game);
    }
}

/// (testing) KENSHI_RESEARCH_AT=frame[@research name]: the window opens then; with a name, that
/// research is queued (as a right click on it). KENSHI_CRAFTING_AT=frame: the crafting tab.
pub fn script(mut frames: Local<u32>, mut keys: ResMut<ButtonInput<KeyCode>>, mut game: ResMut<Game>) {
    *frames += 1;
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
