//! The slave trade windows (port_map `ui`): "Selling Slaves" (F041437) and "Buying Slaves"
//! (F041440), Kenshi_CharacterSelectionWindow.layout with a Kenshi_CharacterSelectionBox.layout
//! per character (portrait, name, stats, price, a tick box), opened by a slave trader's lines
//! (CHOOSE_SLAVES_SELLING 41, CHOOSE_SLAVES_BUYING 42). Selling lists the player's people near the
//! trader ("WARNING: Character will become a slave!"); buying the slaves the trader's faction
//! keeps. Confirm trades the ticked ones (kenshi_game::slavery for the prices).
//! (stand-in) The portraits are left empty; the stats shown are a selection (F041440 lists
//! strength, dexterity, toughness, melee attack and defence, labouring, farming, cooking,
//! engineering, medic).

use super::Game;
use bevy::prelude::*;
use kenshi_gui::ui::{Built, Checked, SetCaption};
use kenshi_gui::{GuiClick, GuiRes};

#[derive(Resource, Default)]
pub struct SlaveTrade {
    /// A window asked for by a dialogue line: (selling, the trader).
    pub pending: Option<(bool, usize)>,
    open: Option<Open>,
}

struct Open {
    ui: Built,
    boxes: Vec<Built>,
    /// Each box's tick box, person and price.
    rows: Vec<(Entity, usize, i32, bool)>,
    selling: bool,
    trader: usize,
}

impl SlaveTrade {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

fn stats_text(game: &Game, id: usize) -> String {
    use kenshi_char::StatsEnumerated::*;
    let f = &game.sim.arena.f[id];
    [("Strength:", StatStrength), ("Dexterity:", StatDexterity), ("Toughness:", StatToughness), ("Melee Attack:", StatMeleeAttack), ("Melee Defence:", StatMeleeDefence), ("Labouring:", StatLabouring), ("Medic:", StatMedic)]
        .iter()
        .map(|(n, s)| format!("{n} {:.0}", f.level(*s)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn open(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, game: &Game, size: [f32; 2], selling: bool, trader: usize) -> Open {
    let mut defs = gui.0.layout("Kenshi_CharacterSelectionWindow.layout");
    for d in &mut defs {
        d.pos = kenshi_gui::res::Pos::Real([0.0, 0.0, 1.0, 1.0]);
    }
    let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(85));
    }
    let title = if selling { "Selling Slaves" } else { "Buying Slaves" };
    for name in ["CaptionTextBox", "TradePanel"] {
        if let Some(c) = ui.get(name) {
            commands.queue(SetCaption(c, title.into()));
        }
    }
    let people = if selling { game.sim.sellable(trader) } else { game.sim.slaves_for_sale(trader) };
    let place = ui.get("CharacterSelectionItemBox");
    let area = place.and_then(|p| ui.sizes.get(&p).copied()).unwrap_or([size[0] * 0.4, size[1] * 0.45]);
    let box_defs = gui.0.layout("Kenshi_CharacterSelectionBox.layout");
    let (bw, bh) = (area[0] * 0.98, area[1] * 0.3);
    let mut boxes = Vec::new();
    let mut rows = Vec::new();
    for (k, &id) in people.iter().enumerate() {
        let mut d = box_defs.clone();
        if let Some(root) = d.first_mut() {
            root.pos = kenshi_gui::res::Pos::Px([0.0, k as f32 * (bh + 4.0), bw, bh]);
        }
        let b = kenshi_gui::ui::spawn(commands, &gui.0, assets, &d, place, area);
        let price = if selling { game.sim.slave_sell_price(id) } else { game.sim.slave_buy_price(id) };
        if let Some(e) = b.get("NameTextBox") {
            commands.queue(SetCaption(e, game.sim.display_name(id)));
        }
        if let Some(e) = b.get("PriceTextBox") {
            commands.queue(SetCaption(e, format!("c.{price}")));
        }
        // the stats in the box's two data panels, the selling warning under them
        let lines = stats_text(game, id);
        let lines: Vec<&str> = lines.lines().collect();
        let half = lines.len().div_ceil(2);
        for (panel, part) in [("DataPanel1", &lines[..half]), ("DataPanel2", &lines[half..])] {
            let Some(e) = b.get(panel) else { continue };
            let mut text = part.join("\n");
            if selling && panel == "DataPanel2" {
                text.push_str("\nWARNING: Character will become a slave!");
            }
            let t = commands
                .spawn((
                    Text::new(text),
                    TextFont {
                        font: gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or_else(bevy::text::FontSource::default, |f| bevy::text::FontSource::Handle(assets.load(f.file.clone()))),
                        font_size: bevy::text::FontSize::Px(gui.0.fonts.get("Kenshi_StandardFont_Medium").map_or(13.0, |f| f.screen_px())),
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.8, 0.7)),
                    Node { position_type: PositionType::Absolute, left: Val::Px(6.0), top: Val::Px(4.0), ..default() },
                ))
                .id();
            commands.entity(e).add_child(t);
        }
        if let Some(c) = b.get("CheckBox") {
            commands.entity(c).insert(Checked(false));
            rows.push((c, id, price, false));
        }
        boxes.push(b);
    }
    let o = Open { ui, boxes, rows, selling, trader };
    totals(commands, game, &o);
    o
}

fn totals(commands: &mut Commands, game: &Game, o: &Open) {
    let total: i32 = o.rows.iter().filter(|r| r.3).map(|r| r.2).sum();
    if let Some(e) = o.ui.get("SelectedTotalTextBox") {
        commands.queue(SetCaption(e, format!("c.{total}")));
    }
    if let Some(e) = o.ui.get("MoneyAmountTextBox") {
        commands.queue(SetCaption(e, format!("c.{}", game.sim.money)));
    }
    if let Some(e) = o.ui.get("TotalMoneyBuyer") {
        let theirs = game.sim.folk[o.trader].shop.as_ref().map_or(0, |s| s.money);
        commands.queue(SetCaption(e, format!("c.{theirs}")));
    }
}

fn close(commands: &mut Commands, o: Open) {
    for b in o.boxes {
        for r in b.roots {
            commands.entity(r).try_despawn();
        }
    }
    for r in o.ui.roots {
        commands.entity(r).try_despawn();
    }
}

pub fn update(
    mut commands: Commands,
    mut trade: ResMut<SlaveTrade>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clicks: MessageReader<GuiClick>,
    windows: Query<&Window>,
) {
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    if let Some((selling, trader)) = trade.pending.take() {
        if let Some(o) = trade.open.take() {
            close(&mut commands, o);
        }
        trade.open = Some(open(&mut commands, &gui, &assets, &game, size, selling, trader));
    }
    let Some(mut o) = trade.open.take() else {
        clicks.clear();
        return;
    };
    let mut done = keys.just_pressed(KeyCode::Escape);
    for c in clicks.read() {
        if let Some(r) = o.rows.iter_mut().find(|r| r.0 == c.widget) {
            r.3 = !r.3;
            commands.entity(r.0).insert(Checked(r.3));
            continue;
        }
        match c.name.as_str() {
            "CancelButton" => done = true,
            "ConfirmButton" => {
                for &(_, id, _, picked) in &o.rows {
                    if !picked {
                        continue;
                    }
                    let name = game.sim.display_name(id);
                    if o.selling {
                        let price = game.sim.sell_slave(id, o.trader);
                        game.sim.messages.push(format!("{name} was sold as a slave for c.{price}."));
                        game.selected.retain(|&s| s != id);
                    } else {
                        match game.sim.buy_slave(id) {
                            Some(price) => game.sim.messages.push(format!("{name} was bought for c.{price}.")),
                            None => game.sim.messages.push(format!("Not enough money for {name}.")),
                        }
                    }
                }
                done = true;
            }
            _ => {}
        }
    }
    totals(&mut commands, &game, &o);
    if done {
        close(&mut commands, o);
    } else {
        trade.open = Some(o);
    }
}

/// (testing) KENSHI_SLAVE_TRADE=<frame>,<buy|sell>: at that frame the window opens with the first
/// person whose faction keeps slaves here.
pub fn test_hook(mut trade: ResMut<SlaveTrade>, game: Res<Game>, mut frames: Local<u32>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_SLAVE_TRADE") else { return };
    let mut parts = v.split(',');
    if parts.next().and_then(|f| f.parse::<u32>().ok()) != Some(*frames) {
        return;
    }
    let selling = parts.next() == Some("sell");
    let trader = game.sim.folk.iter().find(|p| !p.player && p.slave.is_none() && !game.sim.slaves_for_sale(p.id).is_empty()).map(|p| p.id);
    eprintln!("slave trade test: trader {trader:?}");
    if let Some(t) = trader {
        trade.pending = Some((selling, t));
    }
}
