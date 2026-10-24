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
//! image") show their name; an item's price shows in its tooltip (`item_tip`), as the game's.
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

/// The character (or animal) window's OpenBagButton: its worn backpack's storage in the generic
/// inventory window beside it (as a storage's, `open_storage`), in place of anything looted;
/// again: closed. (testing) KENSHI_OPEN_BAG: opened with the window.
pub fn bag_window(
    mut commands: Commands,
    mut win: ResMut<InventoryWindow>,
    mut clicks: MessageReader<GuiClick>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    game: Res<Game>,
    windows: Query<&Window>,
    mut tested: Local<bool>,
) {
    let mut pressed = clicks.read().any(|c| c.name == "OpenBagButton");
    let Some(o) = win.open.as_mut() else { return };
    if !*tested && std::env::var("KENSHI_OPEN_BAG").is_ok() {
        *tested = true;
        pressed = true;
    }
    if !pressed {
        return;
    }
    let bag = kenshi_game::loot::BAG_OWNER + o.person;
    if let Some((who, other)) = o.other.take() {
        for r in &other.roots {
            commands.entity(*r).despawn();
        }
        o.dirty = true;
        if who == bag {
            return;
        }
    }
    if game.sim.inv(bag).is_none() {
        return;
    }
    let size = window_size(&windows);
    let ui = open_storage(&mut commands, &gui.0, &assets, &game, bag, size);
    o.other = Some((bag, ui));
    o.dirty = true;
}

/// Which window a building's inventory shows in (F024434 / F024437 / F024440): a research bench's
/// in the research inventory window, a machine's (a production building, not a farm) in the
/// generic building window (its inputs and output), any other's in the generic inventory window.
fn storage_layout(game: &Game, owner: usize) -> &'static str {
    let Some(b) = owner.checked_sub(kenshi_game::base::BUILDING_OWNER) else { return "Kenshi_InventoryGenericWindow.layout" };
    if game.sim.research_bench(b).is_some() {
        return "Kenshi_InventoryResearchWindow.layout";
    }
    let rec = game.sim.buildings.get(b).and_then(|pb| game.sim.data.get(&pb.record));
    if game.sim.is_production(b) && rec.is_some_and(|r| !kenshi_game::base::is_farm(r)) {
        return "Kenshi_GenericBuildingWindow.layout";
    }
    "Kenshi_InventoryGenericWindow.layout"
}

/// @fn F024417
///
/// A storage's capacity line: "Max Capacity: {n}" and " (Full)" when its stack is at it; set by
/// a StorageBuilding's update (F029361) from its first typed container (F029410): n its limit
/// (one item kept: its "stackable bonus mult"), full when the stack there holds that many.
/// (assumption) Not shown for one of no limit (several items kept).
fn capacity_text(game: &Game, owner: usize) -> Option<String> {
    let b = owner.checked_sub(kenshi_game::base::BUILDING_OWNER)?;
    if !game.sim.is_storage_building(b) {
        return None;
    }
    let pb = game.sim.buildings.get(b)?;
    let rec = game.sim.data.get(&pb.record)?;
    let (_, cap) = kenshi_game::base::storage_entries(rec).into_iter().next().filter(|(_, n)| *n > 0)?;
    let sec = pb.storage.as_ref()?.sections.first()?;
    let full = sec.items.first().is_some_and(|p| p.item.quantity as i32 == cap);
    Some(if full { format!("Max Capacity: {cap} (Full)") } else { format!("Max Capacity: {cap}") })
}

#[allow(clippy::too_many_arguments)]
/// A building's storage window (Kenshi_InventoryGenericWindow.layout, or the research or the
/// generic building window, `storage_layout`): its grid sized to the storage at the character
/// window's cell size, by the screen's right edge, the building's name over it, the capacity
/// line (`capacity_text`).
/// (assumption) The window's size from its grid as the game's generic window grows to its
/// inventory.
fn open_storage(commands: &mut Commands, gui: &kenshi_gui::Gui, assets: &AssetServer, game: &Game, owner: usize, size: [f32; 2]) -> Built {
    let (_, _, name, (w, h)) = game.sim.storage_info(owner).unwrap_or(([0.0; 3], 0.0, "Storage".into(), (8, 8)));
    let name = name.to_uppercase();
    let layout = storage_layout(game, owner);
    if layout == "Kenshi_GenericBuildingWindow.layout" {
        let mut defs = gui.layout(layout);
        if let Some(d) = defs.first_mut()
            && let kenshi_gui::res::Pos::Real(r) = d.pos
        {
            d.pos = kenshi_gui::res::Pos::Real([0.98 - r[2], 0.12, r[2], r[3]]);
            d.props.retain(|(k, _)| k != "Caption");
            d.props.push(("Caption".into(), name.clone()));
        }
        let ui = kenshi_gui::ui::spawn(commands, gui, assets, &defs, None, size);
        for r in &ui.roots {
            commands.entity(*r).insert(GlobalZIndex(60));
        }
        // its containers (F029392) in the widgets of their names (F024440: "in1", "in2", "out");
        // a machine still set up the old way (a crafting bench) its storage grid in "out"
        if let Some(inv) = game.sim.inv(owner) {
            if inv.section("out").is_some() {
                for name in ["in1", "in2", "out"] {
                    if let (Some(e), Some(k)) = (ui.get(name), inv.section(name)) {
                        commands.entity(e).insert((SectionNode(k + LOOT), RelativeCursorPosition::default()));
                    }
                }
            } else if let Some(e) = ui.get("out") {
                commands.entity(e).insert((SectionNode(LOOT), RelativeCursorPosition::default()));
            }
        }
        for (n, t) in [("InputTitleText", "INPUTS"), ("OutputTitleText", "OUTPUT")] {
            if let Some(e) = ui.get(n) {
                commands.queue(kenshi_gui::SetCaption(e, t.to_string()));
            }
        }
        return ui;
    }
    let mut defs = gui.layout(layout);
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
        match capacity_text(game, owner) {
            Some(t) => {
                commands.queue(kenshi_gui::SetCaption(e, t));
                // (assumption) the capacity line takes the arrange button's place (they overlap in
                // the layout)
                if let Some(a) = ui.get("ArrangeButton") {
                    commands.queue(kenshi_gui::SetVisible(a, false));
                }
            }
            None => commands.queue(kenshi_gui::SetVisible(e, false)),
        }
    }
    if let Some(e) = ui.get("ResearchButton") {
        commands.queue(kenshi_gui::SetCaption(e, "SHOW RESEARCH".into()));
    }
    ui
}

/// @fn F024440
///
/// A machine's window kept up to date: what it is crafting (a crafting bench, with its
/// "QUEUE..." button), its ingredients ("consumes", up to two: each with what of it the
/// player's storage holds, and how much of the next one it has taken), its product
/// ("produces", or the queue's first) with how far the next one is made, its output in its
/// grid. A machine made with containers (F029392) shows its inputs in their containers ("in1",
/// "in2": filled by hand or by its workers, crate::jobs) with what they hold of their limit,
/// and each ingredient's bar its banked level (F029327).
/// (assumption) The other machines (crafting benches) take what they need from the player's
/// storage, so their slots show what there is to take; the status line's "{n} / {limit}".
/// Tells the game whether an inventory window is open (ForgottenGUI's open InventoryGUIs,
/// F042066: a trader's visit stays while one is, crate::world).
pub fn mark_open(win: Res<InventoryWindow>, mut game: ResMut<Game>) {
    let open = win.is_open();
    if game.sim.inventory_open != open {
        game.sim.inventory_open = open;
    }
}

pub fn machine_window(mut commands: Commands, win: Res<InventoryWindow>, mut game: ResMut<Game>, mut clicks: MessageReader<GuiClick>) {
    let Some(o) = &win.open else { return };
    let Some((who, ui)) = &o.other else { return };
    let Some(b) = who.checked_sub(kenshi_game::base::BUILDING_OWNER) else { return };
    if ui.get("InputOutputPanel").is_none() {
        return;
    }
    for c in clicks.read() {
        if c.name == "QueueButton" && ui.get("QueueButton") == Some(c.widget) {
            game.crafting_request = Some(b);
        }
    }
    let sim = &game.sim;
    let Some(pb) = sim.buildings.get(b) else { return };
    let Some(rec) = sim.data.get(&pb.record) else { return };
    let Some((f, _)) = kenshi_game::production::function_of(&sim.data, rec) else { return };
    let crafting = !f.refs("item crafts").is_empty();
    let name = |s: &str| sim.data.get(s).map_or(s.to_string(), |r| r.name.clone());
    let set = |commands: &mut Commands, w: &str, t: String| {
        if let Some(e) = ui.get(w) {
            commands.queue(kenshi_gui::SetCaption(e, t));
        }
    };
    let bar = |commands: &mut Commands, w: &str, frac: f32| {
        if let Some(e) = ui.get(w) {
            commands.queue(kenshi_gui::SetProgress(e, frac));
        }
    };
    // what it makes
    let product = if crafting { pb.craft_queue.first().cloned() } else { f.refs("produces").first().map(|(s, _)| s.clone()) };
    set(&mut commands, "CraftingName", if crafting { product.as_deref().map_or("Nothing".to_string(), name) } else { String::new() });
    if let Some(e) = ui.get("CraftingPanel") {
        commands.queue(kenshi_gui::SetVisible(e, crafting));
    }
    set(&mut commands, "QueueButton", "QUEUE...".into());
    set(&mut commands, "OutputText", product.as_deref().map_or(String::new(), name));
    bar(&mut commands, "OutputProgressBar", pb.made.first().copied().unwrap_or(0.0));
    // its ingredients
    let consumes = f.refs("consumes");
    for k in 0..2 {
        let (t, st, p, panel) = (format!("Input{}Text", k + 1), format!("Input{}StatusText", k + 1), format!("Input{}ProgressBar", k + 1), format!("Input{}Panel", k + 1));
        match consumes.get(k) {
            Some((item, _)) if pb.containers_made => {
                let sec = pb.storage.as_ref().and_then(|i| i.section(kenshi_game::production::INPUTS[k]).map(|s| &i.sections[s]));
                let (have, most) = sec.map_or((0, 0), |s| (s.count(), s.stack));
                set(&mut commands, &t, name(item));
                set(&mut commands, &st, if have == 0 { "Missing".to_string() } else { format!("{have} / {most}") });
                bar(&mut commands, &p, pb.taken.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0));
                if let Some(e) = ui.get(&panel) {
                    commands.queue(kenshi_gui::SetVisible(e, true));
                }
            }
            Some((item, _)) => {
                let have = sim.count_material(b, b, item);
                set(&mut commands, &t, name(item));
                set(&mut commands, &st, if have == 0 { "Missing".to_string() } else { format!("{have} in storage") });
                bar(&mut commands, &p, pb.taken.get(k).copied().unwrap_or(0.0).fract());
                if let Some(e) = ui.get(&panel) {
                    commands.queue(kenshi_gui::SetVisible(e, true));
                }
            }
            None => {
                if let Some(e) = ui.get(&panel) {
                    commands.queue(kenshi_gui::SetVisible(e, false));
                }
            }
        }
    }
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
    // an animal's in the animal window (its backpack slot and grid)
    let animal = game.sim.folk[person].race.is_some();
    let mut defs = gui.layout(if animal { "Kenshi_InventoryAnimalWindow.layout" } else { "Kenshi_InventoryCharacterWindow.layout" });
    if let Some(d) = defs.first_mut() {
        // the layout's size, by the screen's left edge (a looted character's by the right)
        if let kenshi_gui::res::Pos::Real(r) = d.pos {
            let x = if offset == LOOT { 0.98 - r[2] } else { 0.02 };
            d.pos = kenshi_gui::res::Pos::Real([x, 0.18, r[2], r[3]]);
        }
        // its name as shown
        d.props.push(("Caption".into(), game.sim.display_name(person)));
    }
    let ui = kenshi_gui::ui::spawn(commands, gui, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(60));
    }
    let inv = &game.sim.folk[person].inventory;
    for s in SECTIONS.iter() {
        if let (Some(e), Some(k)) = (ui.get(s), inv.section(s)) {
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
        fn no_caption(d: &mut kenshi_gui::WidgetDef) {
            if d.name == "datapanel" {
                d.props.retain(|(k, _)| k != "Caption");
            }
            d.children.iter_mut().for_each(no_caption);
        }
        no_caption(d);
    }
    let ui = kenshi_gui::ui::spawn(commands, gui, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(60));
    }
    // the grid: the slot panel (its cells the character inventory's; the scroll view over it
    // is wider, its scroll bar's room). The layout puts the scroll view (Kenshi_ScrollView, a
    // Kenshi_GenericTextBoxSkin box) after the slot panel, over it; Kenshi's code has the slot
    // panel in the scroll view's canvas, so it shows. (stand-in for that reparenting) the slot
    // panel is drawn above its sibling, else the box hid the stock.
    if let Some(e) = ui.get("backpack_content").or(ui.get("scrollview_backpack_content")) {
        commands.entity(e).insert((SectionNode(TRADER), RelativeCursorPosition::default(), ZIndex(1)));
    }
    if let Some(e) = ui.get("ArrangeButton") {
        commands.queue(SetCaption(e, "ARRANGE".into()));
    }
    // the money: a datapanel line ("Money", "c. 3,000"; `draw`)
    let money = ui.get("datapanel");
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
    holder: usize,
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
            // its holder: whose inventory it is in (Item +0x160, for "Stolen", F044591)
            ItemTip(super::item_tip::tip(game, &p.item, price, Some(holder))),
            super::tooltip_ui::InventoryTip,
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
    if std::env::var("KENSHI_ICON_DEBUG").is_ok() {
        eprintln!("item {} in {widget:?}: {x:.0},{y:.0} {w:.0}x{h:.0} (cells {cells:?}, equip {equip}) as {e:?}", p.item.name);
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
            let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (item.w.max(1), item.h.max(1)), &p, true, ItemNode { section: LIMBS + k, idx: 0 }, None, o.person);
            o.items.push(e);
        }
    }
    let trading = o.trader.as_ref().and_then(|t| sim.folk[t.id].shop.as_ref().map(|s| (t, s)));
    let inv = &sim.folk[o.person].inventory;
    for name in SECTIONS.iter() {
        let Some(k) = inv.section(name) else { continue };
        if std::env::var("KENSHI_ICON_DEBUG").is_ok() {
            eprintln!("section {name} ({k}): widget {:?} size {:?}, {} items", o.ui.get(name), o.ui.get(name).and_then(|w| o.ui.sizes.get(&w)), inv.sections[k].items.len());
        }
        let (Some(widget), Some(sec)) = (o.ui.get(name), inv.sections.get(k)) else { continue };
        let size = o.ui.sizes.get(&widget).copied().unwrap_or([100.0, 100.0]);
        for (idx, p) in sec.items.iter().enumerate() {
            // while trading, what the trader would pay
            let price = trading.map(|(_, s)| sim.price(&p.item, s.town, false) * p.item.quantity as i32);
            let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (sec.w, sec.h), p, sec.is_equipment(), ItemNode { section: k, idx }, price, o.person);
            o.items.push(e);
        }
    }
    // the character's info box (the window's "datapanel")
    if let Some(panel) = o.ui.get("datapanel")
        && sim.folk[o.person].race.is_none()
    {
        commands.queue(super::hud_ui::SetRows { panel, rows: info_rows(sim, o.person), font: font.clone(), label_colour: super::hud_ui::LIGHT, centre: false });
    }
    if let Some((who, ui)) = &o.other
        && let Some(inv) = sim.inv(*who)
    {
        // a building's storage: its one section in the generic window's grid, a machine's
        // containers in the building window's widgets of their names
        let window = *who >= kenshi_game::base::BUILDING_OWNER && ui.get("out").is_some();
        let machine = window && inv.section("out").is_some();
        let names: &[&str] = if machine {
            &["in1", "in2", "out"]
        } else if window {
            &["out"]
        } else if *who >= kenshi_game::base::BUILDING_OWNER {
            &["backpack_content"]
        } else {
            &SECTIONS
        };
        for (j, name) in names.iter().enumerate() {
            // a storage's one grid, a character's sections and a machine's containers by name
            let Some(k) = (if *who >= kenshi_game::base::BUILDING_OWNER && !machine { Some(j) } else { inv.section(name) }) else { continue };
            let (Some(widget), Some(sec)) = (ui.get(name), inv.sections.get(k)) else { continue };
            let size = ui.sizes.get(&widget).copied().unwrap_or([100.0, 100.0]);
            for (idx, p) in sec.items.iter().enumerate() {
                let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (sec.w, sec.h), p, sec.is_equipment(), ItemNode { section: k + LOOT, idx }, None, *who);
                o.items.push(e);
            }
        }
    }
    if let Some((t, shop)) = trading {
        if let Some(widget) = t.ui.get("backpack_content").or(t.ui.get("scrollview_backpack_content")) {
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
                let e = draw_item(&mut commands, &game, &assets, &mut icons, &font, widget, size, (sec.w, rows), &shown, false, ItemNode { section: TRADER, idx }, Some(price), t.id);
                o.items.push(e);
            }
        }
        if let Some(m) = t.money {
            commands.queue(super::hud_ui::SetRows {
                panel: m,
                rows: vec![super::hud_ui::row("Money", format!("c.{}", super::hud_ui::num(shop.money as i64)))],
                font: font.clone(),
                label_colour: super::hud_ui::LIGHT,
                centre: false,
            });
        }
    }
}

/// F039272 mode 15, a character's inventory info: "Total Weight:" what it carries against what
/// it can carry, how heavy that is by its load factor (CharStats +0x190: under 0.1 Overloaded and
/// under 0.5 Heavy in red, under 0.75 Moderate, under 0.95 Lightweight, else Weightless); its
/// disguise (its faction, else "None"; its effective percentage, "Blown!"), with the tooltip's
/// breakdown: the clothing disguise (the state object +0x100 x 100), then x 0.75 "In secure
/// location" (indoors in a private town), and the observers' modifiers shown for 14 s after they
/// applied (Character +0x1b0: x 0.6 "Same faction", x 0.5 "Slave owner recognition", x 0.2
/// "Someone knows me personally" (the look itself halves for it, F046537), "Disguise blown"
/// with its seconds and nothing left); whose slave it is ("The faction that owns you.", "You
/// are a free person."); how much it looks like a slave ("Looks like slave:", +0xf4 x 100) with
/// its breakdown (the clothing, 100 less +0xf0 x 100 taken off; "x2" escaping; the minimum,
/// +0xec, escaping or as an ex-slave; the total).
/// The tooltips' lines are key and value rows (`tooltip_ui`). (the trader's money and merchant
/// lines are the trader window's)
fn info_rows(sim: &kenshi_game::sim::Sim, id: usize) -> Vec<super::hud_ui::Row> {
    use super::hud_ui::{LIGHT, MODERATE, PENALTY, Row, row};
    let p = &sim.folk[id];
    let f = &sim.arena.f[id];
    let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
    let strength = f.level(kenshi_char::StatsEnumerated::StatStrength);
    let capacity = ec.carry_weight_mult * strength + ec.encumbrance_base;
    let weight = kenshi_char::encumbrance::carried_weight(p.inventory.weight(), p.carrying.is_some(), &ec);
    let load = kenshi_char::encumbrance::load(weight, 1.0, kenshi_char::encumbrance::strength_level(strength, 1.0), &ec);
    let (class, colour) = if load < 0.1 {
        ("Overloaded", PENALTY)
    } else if load < 0.5 {
        ("Heavy", PENALTY)
    } else if load < 0.75 {
        ("Moderate", MODERATE)
    } else if load < 0.95 {
        ("Lightweight", LIGHT)
    } else {
        ("Weightless", LIGHT)
    };
    let tipped = |label: &str, value: String, colour, tip: &str| Row { label: label.into(), value, colour, tip: tip.into(), button: None };
    let mut rows = vec![row("Total Weight:", format!("{}/{}", weight.round() as i64, capacity.round() as i64)), tipped("", class.into(), colour, "")];
    let o = sim.outfit(id);
    let mut total = o.disguise * 100.0;
    let disguise_tip = if total > 0.0 {
        let mut lines = vec![format!("Clothing disguise:\t{}\t", total as i32)];
        let recent = sim.recognition.recent.get(&id).copied().unwrap_or([0.0; 3]);
        if p.inside.is_some() && p.town.and_then(|t| sim.towns.get(t)).is_some_and(|t| !t.public) {
            total *= 0.75;
            lines.push("In secure location:\t-25%\t".into());
        }
        for (k, (mult, line)) in [(0.6, "Same faction:\t-40%\t"), (0.5, "Slave owner recognition:\t-50%\t"), (0.2, "Someone knows me personally:\t-80%\t")].into_iter().enumerate() {
            if recent[k] > 0.0 {
                total *= mult;
                lines.push(line.into());
            }
        }
        if p.disguise_blown > 0.0 {
            total = 0.0;
            let n = p.disguise_blown as i32;
            lines.push(format!("Disguise blown:\t{n} {}\t", if n == 1 { "sec" } else { "secs" }));
        }
        lines.push(format!("Total:\t{}\t", total as i32));
        lines.join("\n")
    } else {
        "Not currently in disguise.".to_string()
    };
    let name = |f: &str| sim.data.get(f).map_or(f.to_string(), |r| r.name.clone());
    let as_who = o.disguise_as.as_deref().map_or("None".to_string(), name);
    let (value, colour) = if o.disguise <= 0.0 {
        (as_who, LIGHT)
    } else if p.disguise_blown > 0.0 {
        (format!("{as_who} Blown!"), PENALTY)
    } else {
        (format!("{as_who} {}%", total as i32), LIGHT)
    };
    rows.push(tipped("Disguise:", value, colour, &disguise_tip));
    let state = sim.slave_state(id);
    let looks = (o.looks * 100.0) as i32;
    match p.slave.as_ref().filter(|_| state > 0) {
        Some(s) => {
            rows.push(tipped("Slave of:", name(&s.faction), LIGHT, "The faction that owns you."));
            let clothing = 100 - (o.clothing * 100.0) as i32;
            let mut lines = vec![format!("Clothing:\t-{}\t", clothing.abs())];
            if state == kenshi_game::slavery::ESCAPING_SLAVE {
                lines.push("Clothing:\tx2\t".into());
                if o.minimum > 0 {
                    lines.push(format!("Minimum (Escaping):\t+{}\t", o.minimum));
                }
            } else if state == kenshi_game::slavery::EX_SLAVE && o.minimum > 0 {
                lines.push(format!("Minimum (Ex-Slave):\t+{}\t", o.minimum));
            }
            lines.push(format!("Total:\t{looks}%\t"));
            lines.push("How much you look like an escaped slave. Based on how you are dressed/undressed. Wearing shackles and rags is a big giveaway. Try to avoid hanging around too long near people who may recognise you.".into());
            rows.push(tipped("Looks like slave:", format!("{looks}"), if looks > 0 { PENALTY } else { LIGHT }, &lines.join("\n")));
        }
        None => {
            rows.push(tipped("Slave of:", "None".into(), LIGHT, "You are a free person."));
            rows.push(tipped("Looks like slave:", format!("{looks}"), LIGHT, "You are no slave."));
        }
    }
    rows
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


/// The windows' ARRANGE buttons (F042738): the inventory of the window pressed arranged, every
/// section (F043557), with the click.
pub fn arrange_button(mut win: ResMut<InventoryWindow>, mut game: ResMut<Game>, mut clicks: MessageReader<GuiClick>) {
    let pressed: Vec<Entity> = clicks.read().filter(|c| c.name == "ArrangeButton").map(|c| c.widget).collect();
    let Some(o) = win.open.as_mut() else { return };
    for w in pressed {
        let owner = if o.ui.get("ArrangeButton") == Some(w) {
            Some(o.person)
        } else if let Some((who, ui)) = &o.other
            && ui.get("ArrangeButton") == Some(w)
        {
            Some(*who)
        } else {
            None
        };
        let inv = match (owner, &o.trader) {
            (Some(who), _) => game.sim.inv_mut(who),
            (None, Some(t)) if t.ui.get("ArrangeButton") == Some(w) => game.sim.folk.get_mut(t.id).and_then(|p| p.shop.as_mut()).map(|s| &mut s.stock),
            _ => None,
        };
        if let Some(inv) = inv {
            for s in &mut inv.sections {
                s.arrange();
            }
            o.dirty = true;
        }
    }
}
