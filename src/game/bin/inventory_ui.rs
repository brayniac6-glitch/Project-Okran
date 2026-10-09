//! The character inventory window (Kenshi_InventoryCharacterWindow.layout, "toggle_inventory":
//! I): the selected character's sections drawn on the layout's slot widgets, each item as
//! Kenshi_InventoryItem.layout has it (a tinted background, the item's icon, its quantity),
//! picked up and put down by dragging. Changed clothes are shown on the character.
//!
//! Trading: beside it, a trader's stock (Kenshi_InventoryTraderWindow.layout) with its money;
//! dragging from the trader buys, dragging to it sells (crate::trade's prices), if the buyer
//! has the money.
//!
//! (stand-in) items without an icon image (Kenshi draws those from their mesh: "auto icon
//! image") show their name; prices show on the items (the game shows them in a tooltip).
use super::icons::ItemIcons;
use super::Game;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption};

#[derive(Resource, Default)]
pub struct InventoryWindow {
    open: Option<Open>,
    /// A trader the selection is walking to: the trade opens when it is near.
    pub pending_trade: Option<usize>,
    /// Someone the selection is walking to loot (or a squad member to swap with): both
    /// inventories open side by side when it is near.
    pub pending_loot: Option<usize>,
    /// Items picked up (true) and put down (false) by the hand since the sounds last took
    /// them: their records.
    pub cues: Vec<(bool, String)>,
    /// The record of the item in the hand.
    lifted: Option<String>,
}

impl InventoryWindow {
    /// Whether an inventory window is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

struct Open {
    person: usize,
    ui: Built,
    /// The trader's window, when trading.
    trader: Option<Trader>,
    /// The other character's window, when looting (its sections numbered from [`LOOT`]).
    other: Option<(usize, Built)>,
    /// The item nodes drawn.
    items: Vec<Entity>,
    /// An item being dragged: (section, index, the picture following the cursor).
    drag: Option<(usize, usize, Entity)>,
    dirty: bool,
    /// The limbs window (the character window's LimbsButton).
    limbs: Option<Built>,
}

struct Trader {
    id: usize,
    ui: Built,
    /// Rows scrolled down the stock.
    scroll: u32,
    /// The money text.
    money: Option<Entity>,
}

/// An item drawn in the window.
#[derive(Component, Clone, Copy)]
pub struct ItemNode {
    section: usize,
    idx: usize,
}

/// The section a layout slot widget shows.
#[derive(Component, Clone, Copy)]
pub struct SectionNode(usize);

/// Sections' widget names in the layout (Kenshi_InventoryCharacterWindow.layout).
const SECTIONS: [&str; 10] = ["hip", "back", "boots", "head", "backpack_attach", "shirt", "armour", "legs", "belt", "main"];

/// The trader's stock, as a section number.
const TRADER: usize = 100;

/// The looted character's sections are numbered from this.
const LOOT: usize = 200;
/// The limb slots (Kenshi_LimbReplacementWindow), as section numbers.
const LIMBS: usize = 300;

/// How near (units) a character must be to a trader to trade.
const TRADE_REACH: f32 = 60.0;

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

fn close(commands: &mut Commands, o: &Open) {
    for r in &o.ui.roots {
        commands.entity(*r).despawn();
    }
    if let Some(t) = &o.trader {
        for r in &t.ui.roots {
            commands.entity(*r).despawn();
        }
    }
    if let Some((_, other)) = &o.other {
        for r in &other.roots {
            commands.entity(*r).despawn();
        }
    }
    if let Some((_, _, ghost)) = o.drag {
        commands.entity(ghost).despawn();
    }
    if let Some(l) = &o.limbs {
        for r in &l.roots {
            commands.entity(*r).despawn();
        }
    }
}

/// The character window's LimbsButton: the limbs window (Kenshi_LimbReplacementWindow) beside
/// it, its four slots the limb sections; again: closed.
pub fn limbs_window(
    mut commands: Commands,
    mut win: ResMut<InventoryWindow>,
    mut clicks: MessageReader<GuiClick>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
) {
    let pressed = clicks.read().any(|c| c.name == "LimbsButton");
    let Some(o) = win.open.as_mut() else { return };
    if !pressed {
        return;
    }
    if let Some(l) = o.limbs.take() {
        for r in &l.roots {
            commands.entity(*r).despawn();
        }
        return;
    }
    let size = window_size(&windows);
    let mut defs = gui.0.layout("Kenshi_LimbReplacementWindow.layout");
    if let Some(d) = defs.first_mut()
        && let kenshi_gui::res::Pos::Real(r) = d.pos
    {
        // (assumption) beside the character window
        d.pos = kenshi_gui::res::Pos::Real([0.26, 0.18, r[2], r[3]]);
    }
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(61));
    }
    for (k, name) in ["left_arm_replace", "right_arm_replace", "left_leg_replace", "right_leg_replace"].iter().enumerate() {
        if let Some(e) = ui.get(name) {
            commands.entity(e).insert((SectionNode(LIMBS + k), RelativeCursorPosition::default()));
        }
    }
    o.limbs = Some(ui);
    o.dirty = true;
}

#[allow(clippy::too_many_arguments)]
/// A building's storage window (Kenshi_InventoryGenericWindow.layout): its grid sized to the
/// storage at the character window's cell size, by the screen's right edge, the building's
/// name over it.
/// (assumption) The window's size from its grid as the game's generic window grows to its
/// inventory; the capacity line is not filled.
fn open_storage(commands: &mut Commands, gui: &kenshi_gui::Gui, assets: &AssetServer, game: &Game, owner: usize, size: [f32; 2]) -> Built {
    let (_, _, name, (w, h)) = game.sim.storage_info(owner).unwrap_or(([0.0; 3], 0.0, "Storage".into(), (8, 8)));
    let name = name.to_uppercase();
    let mut defs = gui.layout("Kenshi_InventoryGenericWindow.layout");
    if let Some(d) = defs.first_mut() {
        // the grid is 83% x 55% of the window: cells of 2.2% of the screen height
        let cell = 0.022 * size[1];
        let (gw, gh) = (w as f32 * cell / size[0], h as f32 * cell / size[1]);
        let (rw, rh) = (gw / 0.833 / 0.978, gh / 0.546);
        d.pos = kenshi_gui::res::Pos::Real([0.98 - rw, 0.12, rw, rh.min(0.85)]);
        d.props.retain(|(k, _)| k != "Caption");
        d.props.push(("Caption".into(), name.clone()));
    }
    let ui = kenshi_gui::ui::spawn(commands, gui, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(60));
    }
    if let Some(e) = ui.get("backpack_content") {
        commands.entity(e).insert((SectionNode(LOOT), RelativeCursorPosition::default()));
    }
    if let Some(e) = ui.get("TypeText") {
        commands.queue(kenshi_gui::SetCaption(e, name));
    }
    if let Some(e) = ui.get("CapacityText") {
        commands.queue(kenshi_gui::SetCaption(e, String::new()));
    }
    ui
}

fn open_character(
    commands: &mut Commands,
    gui: &kenshi_gui::Gui,
    assets: &AssetServer,
    game: &Game,
    person: usize,
    size: [f32; 2],
    offset: usize,
) -> Built {
    let mut defs = gui.layout("Kenshi_InventoryCharacterWindow.layout");
    if let Some(d) = defs.first_mut() {
        // the layout's size, by the screen's left edge (a looted character's by the right)
        if let kenshi_gui::res::Pos::Real(r) = d.pos {
            let x = if offset == LOOT { 0.98 - r[2] } else { 0.02 };
            d.pos = kenshi_gui::res::Pos::Real([x, 0.18, r[2], r[3]]);
        }
        // its name, else its character's
        let p = &game.sim.folk[person];
        let name = if p.name.is_empty() {
            p.character.as_deref().and_then(|c| game.sim.data.get(c)).map_or(format!("Character {person}"), |c| c.name.clone())
        } else {
            p.name.clone()
        };
        d.props.push(("Caption".into(), name));
    }
    let ui = kenshi_gui::ui::spawn(commands, gui, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(60));
    }
    for (k, s) in SECTIONS.iter().enumerate() {
        if let Some(e) = ui.get(s) {
            commands.entity(e).insert((SectionNode(k + offset), RelativeCursorPosition::default()));
        }
    }
    // the captions the window's code sets (F024431); the head's and belt's are the layout's
    for (name, text) in [
        ("lbWeapon1", "Weapon I"),
        ("lbWeapon2", "Weapon II"),
        ("lbShirt", "Shirt"),
        ("lbPants", "Pants"),
        ("lbArmor", "Armor"),
        ("lbBoots", "Boots"),
        ("lbBackpack", "Backpack"),
        ("InventoryMainTitle", "Inventory"),
        ("OpenBagButton", "Open bag"),
        ("LimbsButton", "Limbs"),
    ] {
        if let Some(e) = ui.get(name) {
            commands.queue(SetCaption(e, text.to_string()));
        }
    }
    ui
}

fn open_trader(commands: &mut Commands, gui: &kenshi_gui::Gui, assets: &AssetServer, game: &Game, id: usize, size: [f32; 2]) -> Trader {
    let mut defs = gui.layout("Kenshi_InventoryTraderWindow.layout");
    if let Some(d) = defs.first_mut() {
        if let kenshi_gui::res::Pos::Real(r) = d.pos {
            d.pos = kenshi_gui::res::Pos::Real([0.98 - r[2], 0.18, r[2], r[3]]);
        }
        let name = game.sim.folk[id].name.clone();
        d.props.retain(|(k, _)| k != "Caption");
        d.props.push(("Caption".into(), if name.is_empty() { "TRADER".into() } else { name.to_uppercase() }));
    }
    let ui = kenshi_gui::ui::spawn(commands, gui, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(60));
    }
    // the grid: Kenshi puts the slot panel inside the scroll view; here the items go on the
    // scroll view itself, which lies over the panel
    if let Some(e) = ui.get("scrollview_backpack_content").or(ui.get("backpack_content")) {
        commands.entity(e).insert((SectionNode(TRADER), RelativeCursorPosition::default()));
    }
    if let Some(e) = ui.get("ArrangeButton") {
        commands.queue(SetCaption(e, "ARRANGE".into()));
    }
    let money = ui.get("datapanel").map(|panel| {
        let t = commands
            .spawn((
                Text::new(""),
                TextFont {
                    font: gui.fonts.get("Kenshi_StandardFont_Medium").map_or_else(Default::default, |f| bevy::text::FontSource::Handle(assets.load(f.file.clone()))),
                    font_size: bevy::text::FontSize::Px(gui.fonts.get("Kenshi_StandardFont_Medium").map_or(16.0, |f| f.screen_px())),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.8, 0.65)),
            ))
            .id();
        commands.entity(panel).add_child(t);
        t
    });
    Trader { id, ui, scroll: 0, money }
}

#[allow(clippy::too_many_arguments)]
pub fn toggle(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    act: super::bindings::Actions,
    mut win: ResMut<InventoryWindow>,
    game: Res<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    mut clicks: MessageReader<GuiClick>,
) {
    let size = window_size(&windows);
    // a trader reached: the character's inventory and the trader's stock side by side
    if let Some(t) = win.pending_trade
        && let Some(&me) = game.selected.first()
    {
        let a = game.sim.folk[me].mover.position_units();
        let b = game.sim.folk[t].mover.position_units();
        if ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() <= TRADE_REACH {
            win.pending_trade = None;
            if let Some(o) = win.open.take() {
                close(&mut commands, &o);
            }
            let ui = open_character(&mut commands, &gui.0, &assets, &game, me, size, 0);
            let trader = open_trader(&mut commands, &gui.0, &assets, &game, t, size);
            win.open = Some(Open { person: me, ui, trader: Some(trader), other: None, items: Vec::new(), drag: None, dirty: true, limbs: None });
            return;
        }
    }
    // someone reached to loot: the character's inventory and theirs side by side
    if let Some(t) = win.pending_loot
        && let Some(&me) = game.selected.first()
    {
        let a = game.sim.folk[me].mover.position_units();
        // a building of the player's: its storage, reached at its edge
        let building = game.sim.storage_info(t);
        let (b, reach) = match &building {
            Some((at, edge, ..)) => (*at, TRADE_REACH + edge),
            None => (game.sim.folk[t].mover.position_units(), TRADE_REACH),
        };
        if me == t || ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() <= reach {
            win.pending_loot = None;
            if let Some(o) = win.open.take() {
                close(&mut commands, &o);
            }
            let ui = open_character(&mut commands, &gui.0, &assets, &game, me, size, 0);
            let other = (me != t).then(|| {
                let ui = match building {
                    Some(_) => open_storage(&mut commands, &gui.0, &assets, &game, t, size),
                    None => open_character(&mut commands, &gui.0, &assets, &game, t, size, LOOT),
                };
                (t, ui)
            });
            win.open = Some(Open { person: me, ui, trader: None, other, items: Vec::new(), drag: None, dirty: true, limbs: None });
            return;
        }
    }
    // the window's header buttons (its template's "Button"s: close and pin) close it
    let close_clicked = win.open.is_some() && clicks.read().any(|c| c.name == "Button");
    let pressed = act.just("toggle_inventory");
    if let Some(o) = &win.open
        && (pressed || close_clicked || keys.just_pressed(KeyCode::Escape))
    {
        close(&mut commands, o);
        win.open = None;
        return;
    }
    if !pressed {
        return;
    }
    let Some(&person) = game.selected.first() else { return };
    let ui = open_character(&mut commands, &gui.0, &assets, &game, person, size, 0);
    win.open = Some(Open { person, ui, trader: None, other: None, items: Vec::new(), drag: None, dirty: true, limbs: None });
}

/// Draw one item on a section widget of `size` cells (w x h), `equip` filling the slot, as
/// Kenshi_InventoryItem.layout draws it: a half-black backing, the item's image stretched over
/// it, the quantity at the bottom right in white with a shadow, and a charge bar along the
/// bottom for an item with charges (a kit, food).
#[allow(clippy::too_many_arguments)]
/// What a hovered item's tooltip says.
#[derive(Component)]
pub struct ItemTip(pub String);

/// An item's tooltip: its name (a weapon with its model, armour with its grade), a weapon's
/// maker, its weight and value.
fn tip_text(game: &Game, item: &kenshi_char::inventory::Item) -> String {
    let data = &game.sim.data;
    let mut lines = vec![item.shown_name(data)];
    if item.kind == Some(kenshi_data::ItemType::Weapon)
        && let Some(m) = item.maker.as_ref().and_then(|m| data.get(m))
    {
        lines.push(format!("Manufacturer: {}", m.name));
    }
    if item.kind == Some(kenshi_data::ItemType::Armour) && kenshi_char::quality::graded(data, &item.record) {
        lines.push(format!("Quality: {}", kenshi_char::quality::grade_name(item.quality)));
    }
    lines.push(format!("Weight: {:.1} kg", item.weight));
    lines.push(format!("Value: c.{}", item.value));
    lines.join("
")
}

/// The tooltip of the item under the pointer, beside it.
pub fn tooltip(
    mut commands: Commands,
    items: Query<(&Interaction, &ItemTip)>,
    windows: Query<&Window>,
    mut shown: Local<Option<(Entity, String)>>,
    mut nodes: Query<&mut Node>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
) {
    let mut hovered = items.iter().find(|(i, _)| **i == Interaction::Hovered).map(|(_, t)| t.0.clone());
    let mut cursor = windows.iter().next().and_then(|w| w.cursor_position());
    // (testing) KENSHI_TIP=<text>: the tip of the first item whose tip has that text, shown at
    // the window's middle
    if let Ok(want) = std::env::var("KENSHI_TIP") {
        hovered = hovered.or_else(|| items.iter().map(|(_, t)| t.0.clone()).find(|t| t.contains(&want)));
        cursor = cursor.or_else(|| windows.iter().next().map(|w| w.size() * 0.5));
    }
    match (hovered, cursor) {
        (Some(text), Some(c)) => {
            if shown.as_ref().is_none_or(|(_, t)| *t != text) {
                if let Some((e, _)) = shown.take() {
                    commands.entity(e).despawn();
                }
                let font = gui.0.fonts.get("Kenshi_StandardFont_Medium").map(|f| (assets.load(f.file.clone()), f.screen_px()));
                let mut tf = TextFont { font_size: bevy::text::FontSize::Px(font.as_ref().map_or(14.0, |f| f.1)), ..default() };
                if let Some((h, _)) = font {
                    tf.font = bevy::text::FontSource::Handle(h);
                }
                let e = commands
                    .spawn((
                        Node { position_type: PositionType::Absolute, padding: UiRect::all(Val::Px(6.0)), ..default() },
                        BackgroundColor(Color::srgba(0.08, 0.07, 0.06, 0.92)),
                        BorderColor::all(Color::srgb(0.45, 0.4, 0.3)),
                        GlobalZIndex(300),
                        Pickable::IGNORE,
                    ))
                    .with_child((Text::new(text.clone()), tf, TextColor(Color::srgb(0.88, 0.84, 0.74))))
                    .id();
                *shown = Some((e, text));
            }
            if let Some((e, _)) = shown.as_ref()
                && let Ok(mut n) = nodes.get_mut(*e)
            {
                n.left = Val::Px(c.x + 18.0);
                n.top = Val::Px(c.y + 12.0);
            }
        }
        _ => {
            if let Some((e, _)) = shown.take() {
                commands.entity(e).despawn();
            }
        }
    }
}

fn draw_item(
    commands: &mut Commands,
    game: &Game,
    assets: &AssetServer,
    icons: &mut ItemIcons,
    font: &(Handle<Font>, f32),
    widget: Entity,
    size: [f32; 2],
    cells: (u32, u32),
    p: &kenshi_char::inventory::Placed,
    equip: bool,
    node: ItemNode,
    price: Option<i32>,
) -> Entity {
    let (cw, ch) = (size[0] / cells.0 as f32, size[1] / cells.1 as f32);
    let (x, y, w, h) = if equip {
        (0.0, 0.0, size[0], size[1])
    } else {
        (p.x as f32 * cw, p.y as f32 * ch, p.item.w as f32 * cw, p.item.h as f32 * ch)
    };
    let e = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x + 1.0),
                top: Val::Px(y + 1.0),
                width: Val::Px(w - 2.0),
                height: Val::Px(h - 2.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            Button,
            Interaction::default(),
            node,
            ItemTip(tip_text(game, &p.item)),
        ))
        .id();
    let image = icons.icon(game, assets, &p.item);
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    match image {
        Some(img) => {
            let i = commands.spawn((full.clone(), ImageNode { image: img, image_mode: NodeImageMode::Stretch, ..default() })).id();
            commands.entity(e).add_child(i);
        }
        None => {
            let t = commands
                .spawn((
                    Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..full.clone() },
                    Text::new(p.item.name.clone()),
                    TextFont { font: bevy::text::FontSource::Handle(font.0.clone()), font_size: bevy::text::FontSize::Px(font.1 * 0.8), ..default() },
                    TextColor(Color::srgb(0.85, 0.8, 0.7)),
                    TextLayout::new(Justify::Center, bevy::text::LineBreak::WordBoundary),
                ))
                .id();
            commands.entity(e).add_child(t);
        }
    }
    if p.item.quantity > 1 {
        let q = commands
            .spawn((
                Text::new(p.item.quantity.to_string()),
                TextFont { font: bevy::text::FontSource::Handle(font.0.clone()), font_size: bevy::text::FontSize::Px(font.1), ..default() },
                TextColor(Color::WHITE),
                TextShadow { offset: Vec2::new(1.0, 1.0), color: Color::BLACK },
                Node { position_type: PositionType::Absolute, right: Val::Px(2.0), bottom: Val::Px(0.0), ..default() },
            ))
            .id();
        commands.entity(e).add_child(q);
    }
    // the charge bar: a black strip 4 high, its fill the charges left
    let charge = p.item.medkit.map(|k| k.charges / k.full.max(1.0)).or(p.item.food.map(|(full, left)| left / full.max(1.0)));
    if let Some(c) = charge.filter(|c| *c < 0.999) {
        let bar = commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(1.0), right: Val::Px(1.0), bottom: Val::Px(1.0), height: Val::Px(4.0), padding: UiRect::all(Val::Px(1.0)), ..default() },
                BackgroundColor(Color::BLACK),
            ))
            .with_child((Node { width: Val::Percent(c.clamp(0.0, 1.0) * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::WHITE)))
            .id();
        commands.entity(e).add_child(bar);
    }
    if let Some(c) = price {
        let t = commands
            .spawn((
                Text::new(format!("c.{c}")),
                TextFont { font: bevy::text::FontSource::Handle(font.0.clone()), font_size: bevy::text::FontSize::Px(font.1 * 0.85), ..default() },
                TextColor(Color::srgb(0.95, 0.85, 0.45)),
                TextShadow { offset: Vec2::new(1.0, 1.0), color: Color::BLACK },
                Node { position_type: PositionType::Absolute, left: Val::Px(2.0), top: Val::Px(0.0), ..default() },
            ))
            .id();
        commands.entity(e).add_child(t);
    }
    commands.entity(widget).add_child(e);
    e
}

/// The items drawn on their sections (and the trader's, with its prices and money).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    mut win: ResMut<InventoryWindow>,
    game: Res<Game>,
    assets: Res<AssetServer>,
    gui: Res<GuiRes>,
    mut icons: ResMut<ItemIcons>,
    mut texts: Query<&mut Text>,
) {
    let font = gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or((Handle::default(), 14.0), |f| (assets.load(f.file.clone()), f.screen_px()));
    // icons still being drawn: draw again once they are
    if icons.pending_redraw() && let Some(o) = win.open.as_mut() {
        o.dirty = true;
    }
    let Some(o) = win.open.as_mut() else { return };
    if !o.dirty {
        return;
    }
    o.dirty = false;
    if std::env::var("KENSHI_ICON_DEBUG").is_ok() {
        eprintln!("inventory drawn");
    }
    for e in o.items.drain(..) {
        commands.entity(e).despawn();
    }
    let sim = &game.sim;
    if let Some(l) = &o.limbs {
        for (k, name) in ["left_arm_replace", "right_arm_replace", "left_leg_replace", "right_leg_replace"].iter().enumerate() {
            let (Some(widget), Some(item)) = (l.get(name), sim.folk[o.person].limbs[k].as_ref()) else { continue };
            let size = l.sizes.get(&widget).copied().unwrap_or([60.0, 160.0]);
            let p = kenshi_char::inventory::Placed { item: item.clone(), x: 0, y: 0 };
            let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (item.w.max(1), item.h.max(1)), &p, true, ItemNode { section: LIMBS + k, idx: 0 }, None);
            o.items.push(e);
        }
    }
    let trading = o.trader.as_ref().and_then(|t| sim.folk[t.id].shop.as_ref().map(|s| (t, s)));
    let inv = &sim.folk[o.person].inventory;
    for (k, name) in SECTIONS.iter().enumerate() {
        let (Some(widget), Some(sec)) = (o.ui.get(name), inv.sections.get(k)) else { continue };
        let size = o.ui.sizes.get(&widget).copied().unwrap_or([100.0, 100.0]);
        for (idx, p) in sec.items.iter().enumerate() {
            // while trading, what the trader would pay
            let price = trading.map(|(_, s)| sim.price(&p.item, s.town, false) * p.item.quantity as i32);
            let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (sec.w, sec.h), p, sec.is_equipment(), ItemNode { section: k, idx }, price);
            o.items.push(e);
        }
    }
    if let Some((who, ui)) = &o.other
        && let Some(inv) = sim.inv(*who)
    {
        // a building's storage: its one section in the generic window's grid
        let names: &[&str] = if *who >= kenshi_game::base::BUILDING_OWNER { &["backpack_content"] } else { &SECTIONS };
        for (k, name) in names.iter().enumerate() {
            let (Some(widget), Some(sec)) = (ui.get(name), inv.sections.get(k)) else { continue };
            let size = ui.sizes.get(&widget).copied().unwrap_or([100.0, 100.0]);
            for (idx, p) in sec.items.iter().enumerate() {
                let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (sec.w, sec.h), p, sec.is_equipment(), ItemNode { section: k + LOOT, idx }, None);
                o.items.push(e);
            }
        }
    }
    if let Some((t, shop)) = trading {
        if let Some(widget) = t.ui.get("scrollview_backpack_content").or(t.ui.get("backpack_content")) {
            let size = t.ui.sizes.get(&widget).copied().unwrap_or([100.0, 100.0]);
            let sec = &shop.stock.sections[0];
            let rows = kenshi_game::trade::SHOP_H;
            for (idx, p) in sec.items.iter().enumerate() {
                // the rows in view (the grid scrolls)
                if p.y < t.scroll || p.y + p.item.h > t.scroll + rows {
                    continue;
                }
                let shown = kenshi_char::inventory::Placed { item: p.item.clone(), x: p.x, y: p.y - t.scroll };
                let price = sim.price(&p.item, shop.town, true) * p.item.quantity as i32;
                let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (sec.w, rows), &shown, false, ItemNode { section: TRADER, idx }, Some(price));
                o.items.push(e);
            }
        }
        if let Some(m) = t.money
            && let Ok(mut text) = texts.get_mut(m)
        {
            text.0 = format!("        c.{}", shop.money);
        }
    }
}

/// Drag an item from where it lies to a section's cell (the cell under its top-left corner):
/// within the character's inventory, or across to and from the trader (selling and buying).
#[allow(clippy::too_many_arguments)]
pub fn drag(
    mut commands: Commands,
    mut win: ResMut<InventoryWindow>,
    mut game: ResMut<Game>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    items: Query<(&Interaction, &ItemNode, &ComputedNode)>,
    sections: Query<(&SectionNode, &RelativeCursorPosition)>,
    mut nodes: Query<&mut Node>,
    keys: Res<ButtonInput<KeyCode>>,
    laid: Query<(&ComputedNode, &UiGlobalTransform)>,
) {
    let Some(o) = win.open.as_mut() else { return };
    let cursor = windows.iter().next().and_then(|w| w.cursor_position());
    // "Hold down the ALT key to steal items" (F024551)
    let stealing = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    // pick up
    if o.drag.is_none()
        && mouse.just_pressed(MouseButton::Left)
        && let Some((_, it, cn)) = items.iter().find(|(i, ..)| **i == Interaction::Pressed)
    {
        // a locked item ("is locked": shackles) worn stays on whoever it holds (Person::shackled);
        // only picking its lock (or its owner, once it no longer holds) takes it off
        let whose = match it.section {
            s if s >= LIMBS || s == TRADER => None,
            s if s >= LOOT => o.other.as_ref().map(|(w, _)| (*w, s - LOOT)),
            s => Some((o.person, s)),
        };
        if let Some((w, s)) = whose
            && game.sim.folk.get(w).is_some_and(|p| p.shackled)
            && game.sim.inv(w).and_then(|i| i.sections.get(s)).is_some_and(|x| {
                x.is_equipment()
                    && x.items.get(it.idx).is_some_and(|p| game.sim.data.get(&p.item.record).is_some_and(|r| r.bools.get("is locked").copied().unwrap_or(false)))
            })
        {
            game.sim.messages.push("Item is locked in place.".to_string());
            return;
        }
        let sz = cn.size() * cn.inverse_scale_factor();
        let ghost = commands
            .spawn((
                Node { position_type: PositionType::Absolute, width: Val::Px(sz.x), height: Val::Px(sz.y), ..default() },
                BackgroundColor(Color::srgba(0.8, 0.7, 0.4, 0.35)),
                GlobalZIndex(200),
            ))
            .id();
        o.drag = Some((it.section, it.idx, ghost));
        // the item in the hand (a sound's kind)
        let (section, idx) = (it.section, it.idx);
        let owner = if section >= LOOT { o.other.as_ref().map(|(w, _)| (*w, section - LOOT)) } else if section == TRADER || section >= LIMBS { None } else { Some((o.person, section)) };
        let record = owner.and_then(|(w, s)| game.sim.inv(w)?.sections.get(s)?.items.get(idx).map(|i| i.item.record.clone()));
        if let Some(r) = record {
            win.cues.push((true, r.clone()));
            win.lifted = Some(r);
        }
    }
    let Some(o) = win.open.as_mut() else { return };
    let Some((from, idx, ghost)) = o.drag else { return };
    if let (Some(c), Ok(mut n)) = (cursor, nodes.get_mut(ghost)) {
        n.left = Val::Px(c.x - 8.0);
        n.top = Val::Px(c.y - 8.0);
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    commands.entity(ghost).despawn();
    o.drag = None;
    if let Some(r) = win.lifted.take() {
        win.cues.push((false, r));
    }
    let Some(o) = win.open.as_mut() else { return };
    let person = o.person;
    let trader = o.trader.as_ref().map(|t| t.id);
    let scroll = o.trader.as_ref().map_or(0, |t| t.scroll);
    let Some((to, rel)) = sections.iter().find(|(_, r)| r.cursor_over).and_then(|(s, r)| Some((s.0, r.normalized?))) else {
        // let go outside the windows: put down on the ground at its owner's feet (not the
        // trader's goods, not a limb)
        let over_window = cursor.is_some_and(|c| {
            let roots = o.ui.roots.iter().chain(o.other.iter().flat_map(|(_, b)| b.roots.iter())).chain(o.limbs.iter().flat_map(|b| b.roots.iter()));
            roots.filter_map(|r| laid.get(*r).ok()).any(|(n, t)| {
                let k = n.inverse_scale_factor();
                let (size, centre) = (n.size() * k, t.translation * k);
                (c - centre).abs().cmplt(size * 0.5).all()
            })
        });
        if !over_window && from != TRADER && from < LIMBS {
            let owner = if from >= LOOT { o.other.as_ref().map(|(w, _)| (*w, from - LOOT)) } else { Some((person, from)) };
            if let Some((w, s)) = owner
                && let Some(item) = game.sim.inv_mut(w).and_then(|i| i.take(s, idx))
            {
                let worn = game.sim.inv(w).and_then(|i| i.sections.get(s)).is_some_and(|x| x.is_equipment());
                game.sim.drop_item(w, item);
                if worn && w == person {
                    rewear(&mut game, person);
                }
                o.dirty = true;
            }
        }
        return;
    };
    // the cell under the cursor holds the item's top-left (the cursor near its corner)
    let cell = |w: u32, h: u32| {
        (
            (((rel.x + 0.5) * w as f32).floor() as i32).clamp(0, w as i32 - 1) as u32,
            (((rel.y + 0.5) * h as f32).floor() as i32).clamp(0, h as i32 - 1) as u32,
        )
    };
    // the limbs window: a robotic limb put in its slot, or taken out to the inventory
    if from >= LIMBS || to >= LIMBS {
        if to >= LIMBS && from < LOOT {
            if let Some(item) = game.sim.folk[person].inventory.take(from, idx) {
                let back = (item.kind == Some(kenshi_data::ItemType::LimbReplacement) && kenshi_game::limbs::item_slot(&item.name) == Some(to - LIMBS))
                    .then(|| game.sim.attach_limb(person, item.clone()))
                    .map_or(Some(item), |r| r.err());
                if let Some(item) = back {
                    let _ = game.sim.folk[person].inventory.add(item, false);
                    game.sim.messages.push("That limb doesn't go there (only where a limb is lost).".to_string());
                }
            }
        } else if from >= LIMBS && to < LOOT
            && let Some(item) = game.sim.detach_limb(person, from - LIMBS)
        {
            let sec = game.sim.folk[person].inventory.sections.get(to).map(|s| (s.w, s.h));
            let at = sec.map(|(w, h)| cell(w, h));
            let placed = at.is_some_and(|(x, y)| game.sim.folk[person].inventory.put(to, item.clone(), x, y).is_ok());
            if !placed {
                let _ = game.sim.folk[person].inventory.add(item, false);
            }
        }
        o.dirty = true;
        return;
    }
    let mut equipped_changed = false;
    let mut other_changed = None;
    // a section number to (whose, which)
    let other = o.other.as_ref().map(|(w, _)| *w);
    let whose = |s: usize| if s >= LOOT { other.map(|w| (w, s - LOOT)) } else { Some((person, s)) };
    match (from == TRADER, to == TRADER, trader) {
        // within an inventory, or from one character to the other (looting)
        (false, false, _) => {
            let (Some((a, fs)), Some((b, ts))) = (whose(from), whose(to)) else { return };
            let Some(sec) = game.sim.inv(b).and_then(|i| i.sections.get(ts)) else { return };
            let (cx, cy) = cell(sec.w, sec.h);
            let eq = |g: &Game, w: usize, s: usize| g.sim.inv(w).and_then(|i| i.sections.get(s)).is_some_and(|s| s.is_equipment());
            let (fe, te) = (eq(&game, a, fs), eq(&game, b, ts));
            if a == b {
                if !game.sim.inv_mut(a).is_some_and(|i| i.move_item(fs, idx, ts, cx, cy)) {
                    return;
                }
            } else if let Err(e) = game.sim.transfer(a, fs, idx, b, ts, Some((cx, cy))) {
                eprintln!("{e}");
                return;
            }
            for (w, changed) in [(a, fe), (b, te)] {
                if changed {
                    if w == person {
                        equipped_changed = true;
                    } else {
                        other_changed = Some(w);
                    }
                }
            }
        }
        // within the trader's stock
        (true, true, Some(t)) => {
            let Some(shop) = game.sim.folk[t].shop.as_mut() else { return };
            let (cx, cy) = cell(shop.stock.sections[0].w, kenshi_game::trade::SHOP_H);
            if !shop.stock.move_item(0, idx, 0, cx, cy + scroll) {
                return;
            }
        }
        // buying: the trader's item to the character, for its price
        (true, false, Some(t)) => {
            let Some(sec) = game.sim.folk[person].inventory.sections.get(to) else { return };
            let c = cell(sec.w, sec.h);
            if stealing {
                match game.sim.steal(t, idx, person, to, Some(c)) {
                    Ok(()) => equipped_changed = game.sim.folk[person].inventory.sections[to].is_equipment(),
                    Err(e) => eprintln!("{e}"),
                }
            } else {
            match game.sim.buy(t, idx, person, to, Some(c)) {
                Ok(_) => equipped_changed = game.sim.folk[person].inventory.sections[to].is_equipment(),
                Err(e) => {
                    eprintln!("{e}");
                    return;
                }
            }
            }
        }
        // selling: the character's item to the trader, for what it pays
        (false, true, Some(t)) => {
            let Some(shop) = game.sim.folk[t].shop.as_ref() else { return };
            let (cx, cy) = cell(shop.stock.sections[0].w, kenshi_game::trade::SHOP_H);
            let c = (cx, cy + scroll);
            let was_equipped = game.sim.folk[person].inventory.sections.get(from).is_some_and(|s| s.is_equipment());
            match game.sim.sell(t, person, from, idx, Some(c)) {
                Ok(_) => equipped_changed = was_equipped,
                Err(e) => {
                    eprintln!("{e}");
                    return;
                }
            }
        }
        _ => return,
    }
    o.dirty = true;
    if let Some(w) = other_changed.filter(|&w| w < kenshi_game::base::BUILDING_OWNER) {
        rewear(&mut game, w);
    }
    if equipped_changed {
        rewear(&mut game, person);
    }
}

/// A character's worn clothes changed: the body redressed, and what it fights with and wears.
fn rewear(game: &mut Game, person: usize) {
    {
        // the clothes worn now, on the body
        let wear: Vec<String> = game.sim.folk[person]
            .inventory
            .equipped()
            .filter(|i| i.kind == Some(kenshi_data::ItemType::Armour))
            .map(|i| i.name.clone())
            .collect();
        let mut spec = game.looks.get(&person).cloned().unwrap_or_else(|| kenshi_render::character::CharacterSpec::new("Greenlander"));
        spec.wear = wear;
        game.looks.insert(person, spec.clone());
        game.rebody.push((person, spec));
        // what it fights with and wears
        game.sim.equip(person);
    }
}

/// (testing) KENSHI_INVENTORY_AT=<frame>: I pressed at that frame.
pub fn script(mut frames: Local<u32>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    *frames += 1;
    if std::env::var("KENSHI_INVENTORY_AT").ok().and_then(|f| f.parse::<u32>().ok()) == Some(*frames) {
        keys.press(KeyCode::KeyI);
    }
}

/// (testing) KENSHI_TRADE_AT=<frame>: the first selected character is put beside the nearest
/// trader and the trade opens.
pub fn trade_script(mut frames: Local<u32>, mut win: ResMut<InventoryWindow>, mut game: ResMut<Game>) {
    *frames += 1;
    if std::env::var("KENSHI_TRADE_AT").ok().and_then(|f| f.parse::<u32>().ok()) != Some(*frames) {
        return;
    }
    let Some(&me) = game.selected.first() else { return };
    let at = game.sim.folk[me].mover.position_units();
    let near = game
        .sim
        .folk
        .iter()
        .filter(|p| p.shop.is_some())
        .min_by(|a, b| {
            let d = |p: &kenshi_game::sim::Person| {
                let q = p.mover.position_units();
                (q[0] - at[0]).powi(2) + (q[2] - at[2]).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
        .map(|p| (p.id, p.mover.position));
    if let Some((t, pos)) = near {
        game.sim.folk[me].mover.position = [pos[0] + 2.0, pos[1], pos[2]];
        win.pending_trade = Some(t);
        eprintln!("trading with {t}");
    }
}

/// (testing) KENSHI_TRADE_BUY=<frame>: while trading, the cheapest stock item is bought into the
/// main inventory, and the first main item sold back; prints the money before and after.
pub fn trade_buy_script(mut frames: Local<u32>, mut win: ResMut<InventoryWindow>, mut game: ResMut<Game>) {
    *frames += 1;
    if std::env::var("KENSHI_TRADE_BUY").ok().and_then(|f| f.parse::<u32>().ok()) != Some(*frames) {
        return;
    }
    let Some(o) = win.open.as_mut() else { return };
    let Some(t) = o.trader.as_ref().map(|t| t.id) else { return };
    let me = o.person;
    let sim = &mut game.sim;
    let Some(shop) = sim.folk[t].shop.as_ref() else { return };
    let town = shop.town;
    let cheapest = shop.stock.sections[0]
        .items
        .iter()
        .enumerate()
        .min_by_key(|(_, p)| sim.price(&p.item, town, true))
        .map(|(i, p)| (i, p.item.name.clone()));
    let main = sim.folk[me].inventory.section("main").unwrap_or(0);
    let before = sim.money;
    if let Some((i, name)) = cheapest {
        let r = sim.buy(t, i, me, main, None);
        eprintln!("buy {name}: {r:?}, money {before} -> {}", sim.money);
    }
    let before = sim.money;
    let first = sim.folk[me].inventory.sections[main].items.first().map(|p| p.item.name.clone());
    if let Some(name) = first {
        let r = sim.sell(t, me, main, 0, None);
        eprintln!("sell {name}: {r:?}, money {before} -> {}", sim.money);
    }
    o.dirty = true;
}

/// The mouse wheel over the trader's stock scrolls it a row at a time.
pub fn scroll(mut win: ResMut<InventoryWindow>, game: Res<Game>, mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, sections: Query<(&SectionNode, &RelativeCursorPosition)>) {
    let over = sections.iter().any(|(s, r)| s.0 == TRADER && r.cursor_over);
    let lines: f32 = wheel.read().map(|w| w.y).sum();
    let Some(o) = win.open.as_mut() else { return };
    let Some(t) = o.trader.as_mut() else { return };
    if !over || lines == 0.0 {
        return;
    }
    let h = game.sim.folk[t.id].shop.as_ref().map_or(0, |s| s.stock.sections[0].h);
    let max = h.saturating_sub(kenshi_game::trade::SHOP_H);
    let to = (t.scroll as i32 - lines.signum() as i32).clamp(0, max as i32) as u32;
    if to != t.scroll {
        t.scroll = to;
        o.dirty = true;
    }
}

