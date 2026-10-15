//! The slave trade windows (port_map `ui`): "Selling Slaves" (F041437) and "Buying Slaves"
//! (F041440), Kenshi_CharacterSelectionWindow.layout with a Kenshi_CharacterSelectionBox.layout
//! per character (portrait, name, stats, price, a tick box), opened by a slave trader's lines
//! (CHOOSE_SLAVES_SELLING 41, CHOOSE_SLAVES_BUYING 42). Selling lists the player's people near the
//! trader ("WARNING: Character will become a slave!"); buying the slaves the trader's faction
//! keeps. Confirm trades the ticked ones (kenshi_game::slavery for the prices). The prisoner
//! bail window (CHOOSE_PRISONER_BAIL, CharacterTrading_PrisonerBailout) is the same, listing
//! the player's people the speaker's faction holds (kenshi_game::dialogue for the bail).
//! (stand-in) The portraits are left empty; the stats shown are a selection (F041440 lists
//! strength, dexterity, toughness, melee attack and defence, labouring, farming, cooking,
//! engineering, medic).

use super::Game;
use bevy::prelude::*;
use kenshi_gui::ui::{Built, Checked, SetCaption};
use kenshi_gui::{GuiClick, GuiRes};

/// Which window: selling slaves, buying them, bailing out prisoners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Sell,
    Buy,
    Bail,
    /// Buying animals (CHOOSE_ANIMALS_BUYING, kenshi_game::livestock), each at the line's price
    /// when over 0.
    Animals(i32),
}

#[derive(Resource, Default)]
pub struct SlaveTrade {
    /// A window asked for by a dialogue line: (which, the trader).
    pub pending: Option<(Mode, usize)>,
    open: Option<Open>,
}

struct Open {
    ui: Built,
    boxes: Vec<Built>,
    /// Each box's tick box, person and price.
    rows: Vec<(Entity, usize, i32, bool)>,
    mode: Mode,
    trader: usize,
    /// The boxes' height with their gap, and the first box shown (the mouse wheel scrolls).
    row: f32,
    first: usize,
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

/// F041441: an animal's strength, toughness and age (percent).
fn animal_text(game: &Game, id: usize) -> String {
    use kenshi_char::StatsEnumerated::*;
    let f = &game.sim.arena.f[id];
    format!("Strength: {:.0}
Toughness: {:.0}
Age: {:.0}%", f.level(StatStrength), f.level(StatToughness), game.sim.folk[id].age * 100.0)
}

fn open(commands: &mut Commands, gui: &GuiRes, assets: &AssetServer, game: &Game, size: [f32; 2], mode: Mode, trader: usize) -> Open {
    let selling = mode == Mode::Sell;
    let mut defs = gui.0.layout("Kenshi_CharacterSelectionWindow.layout");
    for d in &mut defs {
        d.pos = kenshi_gui::res::Pos::Real([0.0, 0.0, 1.0, 1.0]);
    }
    let ui = kenshi_gui::ui::spawn(commands, &gui.0, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(85));
    }
    let title = match mode {
        Mode::Sell => "Selling Slaves",
        Mode::Buy => "Buying Slaves",
        Mode::Bail => "Bail Out Prisoners",
        Mode::Animals(_) => "Buying Animals",
    };
    for name in ["CaptionTextBox", "TradePanel"] {
        if let Some(c) = ui.get(name) {
            commands.queue(SetCaption(c, title.into()));
        }
    }
    let people = match mode {
        Mode::Sell => game.sim.sellable(trader),
        Mode::Buy => game.sim.slaves_for_sale(trader),
        Mode::Bail => game.sim.bail_list(trader),
        Mode::Animals(_) => game.sim.animals_for_sale(trader),
    };
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
        let price = match mode {
            Mode::Sell => game.sim.slave_sell_price(id),
            Mode::Buy => game.sim.slave_buy_price(id),
            Mode::Bail => game.sim.bail_price(id),
            Mode::Animals(p) => game.sim.animal_price(id, p),
        };
        if let Some(e) = b.get("NameTextBox") {
            commands.queue(SetCaption(e, game.sim.display_name(id)));
        }
        if let Some(e) = b.get("PriceTextBox") {
            commands.queue(SetCaption(e, format!("c.{price}")));
        }
        // the stats in the box's two data panels, the selling warning under them
        let lines = if matches!(mode, Mode::Animals(_)) { animal_text(game, id) } else { stats_text(game, id) };
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
    // the list clipped to its box, scrolled by the wheel
    if let Some(p) = place {
        let node = commands.entity(p).id();
        commands.queue(move |w: &mut World| {
            if let Some(mut n) = w.get_mut::<Node>(node) {
                n.overflow = Overflow::clip();
            }
        });
    }
    let o = Open { ui, boxes, rows, mode, trader, row: bh + 4.0, first: 0 };
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
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut nodes: Query<&mut Node>,
) {
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    if let Some((mode, trader)) = trade.pending.take() {
        if let Some(o) = trade.open.take() {
            close(&mut commands, o);
        }
        trade.open = Some(open(&mut commands, &gui, &assets, &game, size, mode, trader));
    }
    let Some(mut o) = trade.open.take() else {
        clicks.clear();
        return;
    };
    let mut done = keys.just_pressed(KeyCode::Escape);
    let turn: f32 = wheel.read().map(|w| w.y).sum();
    if turn != 0.0 {
        let last = o.boxes.len().saturating_sub(3);
        o.first = if turn > 0.0 { o.first.saturating_sub(1) } else { (o.first + 1).min(last) };
        for (k, b) in o.boxes.iter().enumerate() {
            if let Some(&r) = b.roots.first()
                && let Ok(mut n) = nodes.get_mut(r)
            {
                n.top = Val::Px((k as f32 - o.first as f32) * o.row);
            }
        }
    }
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
                    match o.mode {
                        Mode::Sell => {
                            let price = game.sim.sell_slave(id, o.trader);
                            game.sim.messages.push(format!("{name} was sold as a slave for c.{price}."));
                            game.selected.retain(|&s| s != id);
                        }
                        Mode::Buy => match game.sim.buy_slave(id) {
                            Some(price) => game.sim.messages.push(format!("{name} was bought for c.{price}.")),
                            None => game.sim.messages.push(format!("Not enough money for {name}.")),
                        },
                        Mode::Animals(p) => {
                            let price = game.sim.animal_price(id, p);
                            match game.sim.buy_animal(id, o.trader, price) {
                                Some(price) => game.sim.messages.push(format!("{name} was bought for c.{price}.")),
                                None => game.sim.messages.push(format!("Not enough money for {name}.")),
                            }
                        }
                        Mode::Bail => match game.sim.bail_out(id) {
                            Some(price) => game.sim.messages.push(format!("{name} was bailed out for c.{price}.")),
                            None => game.sim.messages.push(format!("Not enough money for {name}.")),
                        },
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

/// (testing) KENSHI_SLAVE_TRADE=<frame>,<buy|sell|animals>: at that frame the window opens with
/// the first person whose faction keeps slaves here (animals: who offers animals); with
/// animals, two seconds later the first animal is bought (its price given first).
pub fn test_hook(mut trade: ResMut<SlaveTrade>, mut game: ResMut<Game>, mut frames: Local<u32>, mut buy: Local<Option<(u32, usize)>>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_SLAVE_TRADE") else { return };
    if let Some((at, t)) = *buy
        && *frames == at
    {
        let first = game.sim.animals_for_sale(t).first().copied();
        if let Some(a) = first {
            let price = game.sim.animal_price(a, 0);
            game.sim.money += price;
            let got = game.sim.buy_animal(a, t, price);
            eprintln!("slave trade test: bought {} (age {:.2}) for {got:?}; player's now {}, platoon {:?} (the player's {:?})", game.sim.display_name(a), game.sim.folk[a].age, game.sim.folk[a].player, game.sim.folk[a].platoon, game.sim.folk.iter().find(|p| p.player && p.race.is_none()).and_then(|p| p.platoon));
        }
    }
    let mut parts = v.split(',');
    if parts.next().and_then(|f| f.parse::<u32>().ok()) != Some(*frames) {
        return;
    }
    let mode = match parts.next() {
        Some("sell") => Mode::Sell,
        Some("animals") => Mode::Animals(0),
        _ => Mode::Buy,
    };
    let trader = game
        .sim
        .folk
        .iter()
        .filter(|p| !p.player && p.slave.is_none() && p.race.is_none())
        .find(|p| if mode == Mode::Animals(0) { !game.sim.animals_for_sale(p.id).is_empty() } else { !game.sim.slaves_for_sale(p.id).is_empty() })
        .map(|p| p.id);
    eprintln!("slave trade test: trader {trader:?}");
    if let Some(t) = trader {
        if mode == Mode::Animals(0) {
            let list: Vec<(String, i32)> = game.sim.animals_for_sale(t).iter().map(|&a| (game.sim.display_name(a), game.sim.animal_price(a, 0))).collect();
            eprintln!("slave trade test: animals {list:?}");
            *buy = Some((*frames + 120, t));
        }
        trade.pending = Some((mode, t));
    }
}
