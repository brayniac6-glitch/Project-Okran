//! The prospecting window (Kenshi_ProspectingWindow.layout, F035160 / F035164 / F035161), opened
//! on a prospector's results (`kenshi_game::prospect`), in the middle of the screen:
//! - OverviewDatapanel1: "Zone:" (the biome), "Altitude:" ("{1,num} m"), "Prospector Skill:"
//!   (its Science), "Area Size:" ((skill x 0.01 x 6000 + 2000) x 0.2, "{1,num} m");
//! - OverviewDatapanel2: "Wind Speed:" ("{1,num=fix,p=1} mph", the wind x 0.223694),
//!   "Environment:" the biome's "green", "arid" and "swamp" as "Green 40%" a line each ("Dead"
//!   with none);
//! - ResourcesList: a line (Kenshi_ProspectingWindowResourceLine.layout) per resource, Water,
//!   Fertility, Stone, Iron, Copper, its level x 100, over 0.75 good, under 0.5 poor; the
//!   first ticked; ticking one shows it on the map;
//! - Map: the world map round the place, the area's size each way, the ticked resource's level
//!   over it (F035161: water, fertility and stone sampled over a grid, brighter where higher).
//!
//! The good and poor colours are the game's #a8b774 and #59231a (`hud_ui::GREEN`, `RED`).
//! (assumption) The altitude is the ground's height in metres; iron and copper (the game marks
//! the resource objects near) show no overlay, the rewrite's levels for them being flat over a
//! biome.
use super::Game;
use super::hud_ui;
use bevy::prelude::*;
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption};

/// The resource lines in the game's order (F035164) and their names (F051593).
const RESOURCES: [(i32, &str); 5] = [(5, "Water"), (6, "Fertility"), (2, "Stone"), (1, "Iron"), (3, "Copper")];
/// The map overlay's cells each way.
const GRID: usize = 16;

#[derive(Resource, Default)]
pub struct ProspectWindow {
    open: Option<Open>,
}

impl ProspectWindow {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

struct Open {
    ui: Built,
    result: kenshi_game::prospect::ProspectResult,
    /// The resource lines (their roots, tick buttons) and the one ticked.
    lines: Vec<Built>,
    ticked: usize,
    /// The map's nodes (drawn again on a new tick).
    map: Vec<Entity>,
}

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

/// The area's half size (units) for a prospector of `skill` (F035164).
fn area(skill: f32) -> f32 {
    skill * 0.01 * 6000.0 + 2000.0
}

fn close(commands: &mut Commands, o: Open) {
    for r in o.ui.roots.iter().chain(o.lines.iter().flat_map(|l| l.roots.iter())).chain(o.map.iter()) {
        commands.entity(*r).try_despawn();
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut win: ResMut<ProspectWindow>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clicks: MessageReader<GuiClick>,
    windows: Query<&Window>,
    mut frames: Local<u32>,
) {
    // (testing) KENSHI_PROSPECT_AT=frame: the selection prospects then
    *frames += 1;
    if std::env::var("KENSHI_PROSPECT_AT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(*frames) {
        let sel = game.selected.clone();
        game.sim.order_prospect(&sel);
    }
    let clicked: Vec<GuiClick> = clicks.read().cloned().collect();
    // new results: the window (again) on them
    if let Some(r) = game.sim.prospect_result.take() {
        if let Some(o) = win.open.take() {
            close(&mut commands, o);
        }
        let o = open(&mut commands, &gui, &assets, &mut game, r, window_size(&windows));
        win.open = Some(o);
        return;
    }
    let Some(o) = win.open.as_mut() else { return };
    let header = clicked.iter().any(|c| c.name == "Button" && o.ui.named.iter().any(|(_, e)| *e == c.widget));
    if header || keys.just_pressed(KeyCode::Escape) {
        let o = win.open.take().unwrap();
        close(&mut commands, o);
        return;
    }
    // a resource ticked
    let pick = clicked.iter().find_map(|c| o.lines.iter().position(|l| l.get("CheckboxButton") == Some(c.widget)));
    if let Some(k) = pick
        && k != o.ticked
    {
        o.ticked = k;
        for (j, l) in o.lines.iter().enumerate() {
            if let Some(b) = l.get("CheckboxButton") {
                commands.entity(b).insert(kenshi_gui::ui::Checked(j == k));
            }
        }
        draw_map(&mut commands, o, &mut game, &assets);
    }
}

fn open(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, game: &mut Game, result: kenshi_game::prospect::ProspectResult, size: [f32; 2]) -> Open {
    let mut defs = gui.0.layout("Kenshi_ProspectingWindow.layout");
    if let Some(d) = defs.first_mut()
        && let kenshi_gui::res::Pos::Real(r) = d.pos
    {
        d.pos = kenshi_gui::res::Pos::Real([0.5 - r[2] * 0.5, 0.5 - r[3] * 0.5, r[2], r[3]]);
    }
    let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(60));
    }
    let font = gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
    let sim = &mut game.sim;
    let at = result.at;
    let ground = sim.terrain.height_at(at[0], at[2]).unwrap_or(at[1]);
    let biome = sim.biome_at(at[0], at[2]);
    let zone = biome.map_or("Unknown".to_string(), |b| b.name.clone());
    let skill = result.skill;
    // the overview
    let rows1 = vec![
        hud_ui::row("Zone:", zone),
        hud_ui::row("Altitude:", format!("{} m", (ground * kenshi_world::navmesh::METRES) as i32)),
        hud_ui::row("Prospector Skill:", format!("{}", skill as i32)),
        hud_ui::row("Area Size:", format!("{} m", (area(skill) * 0.2) as i32)),
    ];
    let res = sim.resources_at(at[0], at[2]);
    let f = |k: &str| res.and_then(|r| r.floats.get(k).copied()).unwrap_or(0.0);
    let mut env: Vec<String> = [("Green", f("green")), ("Arid", f("arid")), ("Swamp", f("swamp"))]
        .iter()
        .filter(|(_, v)| *v > 0.0)
        .map(|(n, v)| format!("{n} {}%", (v * 100.0) as i32))
        .collect();
    if env.is_empty() {
        env.push("Dead".into());
    }
    let mut rows2 = vec![hud_ui::row("Wind Speed:", format!("{:.1} mph", sim.wind * 100.0 * 0.223694)), hud_ui::row("Environment:", env[0].clone())];
    for (k, e) in env.iter().enumerate().skip(1) {
        rows2.push(hud_ui::row(" ".repeat(k), e.clone()));
    }
    for (panel, rows) in [("OverviewDatapanel1", rows1), ("OverviewDatapanel2", rows2)] {
        if let Some(p) = ui.get(panel) {
            commands.queue(hud_ui::SetRows { panel: p, rows, font: font.clone(), label_colour: hud_ui::LIGHT, centre: false });
        }
    }
    for (w, t) in [("lbOverview", "OVERVIEW"), ("lbResources", "RESOURCES")] {
        if let Some(e) = ui.get(w) {
            commands.queue(SetCaption(e, t.to_string()));
        }
    }
    // the resource lines
    let mut lines = Vec::new();
    if let Some(list) = ui.get("ResourcesList") {
        let lw = ui.sizes.get(&list).map_or(200.0, |s| s[0]);
        let line_defs = gui.0.layout("Kenshi_ProspectingWindowResourceLine.layout");
        let lh = line_defs.first().map_or(24.0, |d| match d.pos {
            kenshi_gui::res::Pos::Real(r) => r[3] * size[1],
            kenshi_gui::res::Pos::Px(p) => p[3],
        });
        for (k, (kind, name)) in RESOURCES.iter().enumerate() {
            let v = sim.resource_level(*kind, [at[0], ground, at[2]]).clamp(0.0, 1.0);
            let mut d = line_defs.clone();
            if let Some(r) = d.first_mut() {
                r.pos = kenshi_gui::res::Pos::Px([0.0, k as f32 * lh, lw, lh]);
            }
            let l = kenshi_gui::ui::spawn(commands, &gui.0, assets, &d, Some(list), [lw, lh * RESOURCES.len() as f32]);
            if let Some(b) = l.get("CheckboxButton") {
                commands.queue(SetCaption(b, name.to_string()));
                commands.entity(b).insert(kenshi_gui::ui::Checked(k == 0));
            }
            if let Some(t) = l.get("ValueText") {
                commands.queue(SetCaption(t, format!("{}", (v * 100.0) as i32)));
                // F035164: #a8b774 over 0.75, #59231a under 0.5, else #140806
                let colour = if v > 0.75 {
                    hud_ui::GREEN
                } else if v < 0.5 {
                    hud_ui::RED
                } else {
                    hud_ui::DARK_TEXT
                };
                commands.queue(move |w: &mut World| {
                    let Some(c) = w.get::<kenshi_gui::Caption>(t).map(|c| c.0) else { return };
                    if let Some(mut tc) = w.get_mut::<TextColor>(c) {
                        tc.0 = colour;
                    }
                });
            }
            lines.push(l);
        }
    }
    let mut o = Open { ui, result, lines, ticked: 0, map: Vec::new() };
    draw_map(commands, &mut o, game, assets);
    o
}

/// @fn F035161
///
/// The map: the world map round the place (the area's size each way), the ticked resource over
/// it, a marker where the prospector stood.
fn draw_map(commands: &mut Commands, o: &mut Open, game: &mut Game, assets: &AssetServer) {
    for e in o.map.drain(..) {
        commands.entity(e).try_despawn();
    }
    let Some(map) = o.ui.get("Map") else { return };
    let size = o.ui.sizes.get(&map).copied().unwrap_or([200.0, 200.0]);
    let at = o.result.at;
    let r = area(o.result.skill);
    let ws = kenshi_world::WORLD_SIZE;
    // the whole map at the scale that puts 2r across the box
    let side = size[0] * ws / (2.0 * r);
    let left = size[0] * 0.5 - (at[0] + ws * 0.5) / ws * side;
    let top = size[1] * 0.5 - (ws * 0.5 - at[2]) / ws * side;
    let clip = commands
        .spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), overflow: Overflow::clip(), ..default() }, ZIndex(2)))
        .id();
    commands.entity(map).add_child(clip);
    o.map.push(clip);
    let image = game.res.find("GUI_Map.dds").map(|f| assets.load::<Image>(f));
    let img = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(left), top: Val::Px(top), width: Val::Px(side), height: Val::Px(side), ..default() },
            ImageNode { image: image.unwrap_or_default(), ..default() },
        ))
        .id();
    commands.entity(clip).add_child(img);
    // the ticked resource sampled over the area (water, fertility, stone)
    let kind = RESOURCES[o.ticked].0;
    if matches!(kind, 5 | 6 | 2) {
        let (cw, ch) = (size[0] / GRID as f32, size[1] / GRID as f32);
        for j in 0..GRID {
            for i in 0..GRID {
                let x = at[0] - r + (i as f32 + 0.5) / GRID as f32 * 2.0 * r;
                let z = at[2] + r - (j as f32 + 0.5) / GRID as f32 * 2.0 * r;
                let y = game.sim.terrain.height_at(x, z).unwrap_or(at[1]);
                let v = game.sim.resource_level(kind, [x, y, z]).clamp(0.0, 1.0);
                if v <= 0.01 {
                    continue;
                }
                let cell = commands
                    .spawn((
                        Node { position_type: PositionType::Absolute, left: Val::Px(i as f32 * cw), top: Val::Px(j as f32 * ch), width: Val::Px(cw + 0.5), height: Val::Px(ch + 0.5), ..default() },
                        BackgroundColor(Color::srgba(1.0, 1.0 - (1.0 - v) * v, 0.2, 0.15 + v * 0.45)),
                    ))
                    .id();
                commands.entity(clip).add_child(cell);
            }
        }
    }
    // where it was done
    let mark = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(size[0] * 0.5 - 4.0), top: Val::Px(size[1] * 0.5 - 4.0), width: Val::Px(8.0), height: Val::Px(8.0), ..default() },
            BackgroundColor(Color::srgb(0.3, 1.0, 0.3)),
        ))
        .id();
    commands.entity(clip).add_child(mark);
    if let Some(m) = o.ui.get("CameraMarker") {
        commands.queue(kenshi_gui::SetVisible(m, false));
    }
}
