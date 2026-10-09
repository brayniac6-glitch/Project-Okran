//! The conversation window (Kenshi_ConversationPanel.layout): the NPC's name and line above,
//! the player character's name, money and replies below; a reply is clicked or picked with its
//! number key, and its effects play out (trade opens the trader window, joining puts the NPC
//! in the squad, attacking makes it hostile).
use super::Game;
use bevy::prelude::*;
use kenshi_game::dialogue::{Conversation, Effect};
use kenshi_gui::{Built, GuiRes, SetCaption};

#[derive(Resource, Default)]
pub struct DialogueWindow {
    /// Someone the selection is walking to, to talk to.
    pub pending: Option<usize>,
    open: Option<Open>,
}

struct Open {
    conv: Conversation,
    ui: Built,
    /// The reply buttons and their line ids.
    choices: Vec<(Entity, String)>,
    npc_text: Option<Entity>,
    /// Kenshi_StandardFont_Medium and its size.
    font: (Handle<Font>, f32),
}

/// A reply button.
#[derive(Component)]
pub struct Reply(usize);

/// (assumption, by eye from Kenshi) a reply's colour, and hovered.
const REPLY: Color = Color::srgb(0.86, 0.8, 0.62);
const REPLY_HOVER: Color = Color::srgb(1.0, 0.95, 0.8);

/// How near (units) to talk.
const TALK_REACH: f32 = 60.0;

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

/// A person's name: its own, else its CHARACTER record's (as the game titles unnamed NPCs).
fn name_of(game: &Game, id: usize) -> String {
    let p = &game.sim.folk[id];
    if !p.name.is_empty() {
        return p.name.clone();
    }
    if p.player {
        return format!("Character {id}");
    }
    p.character
        .as_deref()
        .and_then(|c| game.sim.data.get(c))
        .map_or_else(|| format!("Character {id}"), |r| r.name.clone())
}

fn close(commands: &mut Commands, o: &Open) {
    for r in &o.ui.roots {
        commands.entity(*r).despawn();
    }
}

/// The NPC's line and the replies drawn.
fn fill(commands: &mut Commands, o: &mut Open, game: &Game) {
    let (font, px) = o.font.clone();
    let tf = |size: f32| TextFont { font: bevy::text::FontSource::Handle(font.clone()), font_size: bevy::text::FontSize::Px(size), ..default() };
    for (e, _) in o.choices.drain(..) {
        commands.entity(e).despawn();
    }
    if let Some(t) = o.npc_text {
        commands.entity(t).despawn();
    }
    if let Some(panel) = o.ui.get("NpcDialog") {
        let t = commands
            .spawn((
                Text::new(o.conv.npc_text.clone()),
                tf(px),
                TextColor(Color::srgb(0.93, 0.9, 0.82)),
                Node { margin: UiRect::all(Val::Px(4.0)), ..default() },
            ))
            .id();
        commands.entity(panel).add_child(t);
        o.npc_text = Some(t);
    }
    let Some(panel) = o.ui.get("PlayerDialog") else { return };
    let panel_node = commands.entity(panel).id();
    commands.queue(move |w: &mut World| {
        if let Some(mut n) = w.get_mut::<Node>(panel_node) {
            n.flex_direction = FlexDirection::Column;
            n.overflow = Overflow::clip();
        }
    });
    let choices: Vec<(String, String)> = if o.conv.choices.is_empty() {
        vec![(String::new(), "(Leave)".into())]
    } else {
        o.conv.choices.clone()
    };
    for (k, (id, text)) in choices.iter().enumerate() {
        let b = commands
            .spawn((
                Node { padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)), margin: UiRect::bottom(Val::Px(2.0)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                Button,
                Interaction::default(),
                Reply(k),
            ))
            .with_child((
                Text::new(format!("{}. {text}", k + 1)),
                tf(px),
                TextColor(REPLY),
            ))
            .id();
        commands.entity(panel).add_child(b);
        o.choices.push((b, id.clone()));
    }
    let _ = game;
}

/// What a line's effects do in the game.
fn apply(game: &mut Game, inv: &mut super::inventory_ui::InventoryWindow, slaves: &mut super::slave_ui::SlaveTrade, conv: &Conversation, effects: &[Effect]) {
    for e in effects {
        match e {
            Effect::Trade => inv.pending_trade = Some(conv.npc),
            Effect::Recruit => {
                game.sim.recruit(conv.npc);
                eprintln!("{} joined the squad", name_of(game, conv.npc));
            }
            Effect::Attack => {
                // the speaker's squad on the one it talks to
                let pl = game.sim.folk[conv.npc].platoon;
                let squad: Vec<usize> = game.sim.folk.iter().filter(|p| pl.is_some() && p.platoon == pl && !p.down).map(|p| p.id).collect();
                let squad = if squad.is_empty() { vec![conv.npc] } else { squad };
                eprintln!("{} attacks", name_of(game, conv.npc));
                game.sim.order_attack(&squad, conv.player);
            }
            Effect::Alarm => game.sim.alarm(conv.npc, conv.player),
            Effect::SellSlaves => slaves.pending = Some((true, conv.npc)),
            Effect::BuySlaves => slaves.pending = Some((false, conv.npc)),
            Effect::EnslaveCarried => {
                if let Some(c) = game.sim.folk[conv.player].carrying.take() {
                    let (owner, faction) = (game.sim.folk[conv.npc].platoon, game.sim.folk[conv.npc].faction.clone().unwrap_or_default());
                    game.sim.folk[c].carried_by = None;
                    game.sim.enslave(c, owner, &faction);
                }
            }
            // DA_TALK_TO_LEADER: the conversation taken up by the speaker's squad leader
            Effect::TalkToLeader => {
                let pl = game.sim.folk[conv.npc].platoon;
                let leader = game.sim.folk.iter().find(|p| pl.is_some() && p.platoon == pl && p.role == 5 && p.id != conv.npc && !p.down && !p.dead).map(|p| p.id);
                if let Some(l) = leader
                    && let Some(c) = game.sim.talk(l, conv.player)
                {
                    game.sim.npc_talks.push((c, Vec::new()));
                }
            }
            Effect::Money(_) | Effect::Relations(_) => {}
        }
    }
}

/// Open the window on a conversation.
fn open(commands: &mut Commands, gui: &kenshi_gui::Gui, assets: &AssetServer, game: &Game, conv: Conversation, size: [f32; 2]) -> Open {
    let font = gui.fonts.get("Kenshi_StandardFont_Medium").map_or((Handle::default(), 15.0), |f| (assets.load(f.file.clone()), f.screen_px()));
    let defs = gui.layout("Kenshi_ConversationPanel.layout");
    let ui = kenshi_gui::ui::spawn(commands, gui, assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(70));
    }
    for (w, text) in [("NpcName", name_of(game, conv.npc)), ("PlayerName", name_of(game, conv.player)), ("MoneyLabelText", "Money:".into()), ("MoneyAmountText", format!("c. {}", game.sim.money))] {
        if let Some(e) = ui.get(w) {
            commands.queue(SetCaption(e, text));
        }
    }
    let mut o = Open { conv, ui, choices: Vec::new(), npc_text: None, font };
    fill(commands, &mut o, game);
    o
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut win: ResMut<DialogueWindow>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    keys: Res<ButtonInput<KeyCode>>,
    replies: Query<(&Interaction, &Reply), Changed<Interaction>>,
    mut buttons: Query<(&Interaction, &Children), With<Reply>>,
    mut colours: Query<&mut TextColor>,
    mut portraits: ResMut<super::portraits::Portraits>,
    mut faces: Query<&mut ImageNode>,
    mut inv: ResMut<super::inventory_ui::InventoryWindow>,
    mut slaves: ResMut<super::slave_ui::SlaveTrade>,
) {
    let size = window_size(&windows);
    // walking up to someone: talk when near
    if let Some(npc) = win.pending
        && let Some(&me) = game.selected.first()
    {
        let a = game.sim.folk[me].mover.position_units();
        let b = game.sim.folk[npc].mover.position_units();
        if ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() <= TALK_REACH {
            win.pending = None;
            if let Some(o) = win.open.take() {
                close(&mut commands, &o);
            }
            match game.sim.talk(npc, me) {
                Some(conv) => {
                    let o = open(&mut commands, &gui.0, &assets, &game, conv, size);
                    win.open = Some(o);
                }
                None => {
                    // nothing to say: a trader still trades
                    if game.sim.folk[npc].shop.is_some() {
                        inv.pending_trade = Some(npc);
                    } else {
                        eprintln!("{} has nothing to say", name_of(&game, npc));
                    }
                }
            }
        }
    }
    // a conversation someone else started (a squad meeting the player's): with replies, the
    // window; a line with none, said aloud
    if win.open.is_none() && !game.sim.npc_talks.is_empty() {
        let (conv, effects) = game.sim.npc_talks.remove(0);
        eprintln!("{} says: {:?} (replies {:?})", name_of(&game, conv.npc), conv.npc_text, conv.choices.iter().map(|c| &c.1).collect::<Vec<_>>());
        apply(&mut game, &mut inv, &mut slaves, &conv, &effects);
        // a line without replies was said aloud (`speech_ui`): only its effects
        if conv.choices.is_empty() {
            return;
        }
        let o = open(&mut commands, &gui.0, &assets, &game, conv, size);
        win.open = Some(o);
        return;
    }
    // the one talked to has a portrait while the window is open
    let talking: Vec<usize> = win.open.as_ref().map(|o| vec![o.conv.npc]).unwrap_or_default();
    if portraits.extra != talking {
        portraits.extra = talking;
    }
    let Some(o) = win.open.as_mut() else { return };
    // the two faces (PlayerPortrait, NpcPortrait), as the squad bar's
    for (w, id) in [("PlayerPortrait", o.conv.player), ("NpcPortrait", o.conv.npc)] {
        let (Some(img), Some((_, image))) = (o.ui.get(w), portraits.by.get(&id)) else { continue };
        match faces.get_mut(img) {
            Ok(mut i) => {
                if i.image != *image {
                    i.image = image.clone();
                }
            }
            Err(_) => {
                let e = super::portraits::EXPOSURE;
                commands.entity(img).insert(ImageNode { image: image.clone(), color: Color::linear_rgb(e, e, e), ..default() });
            }
        }
    }
    for (i, kids) in buttons.iter_mut() {
        for k in kids.iter() {
            if let Ok(mut c) = colours.get_mut(k) {
                c.0 = if *i == Interaction::Hovered { REPLY_HOVER } else { REPLY };
            }
        }
    }
    // a reply: clicked, or its number key
    let mut pick = replies.iter().find(|(i, _)| **i == Interaction::Pressed).map(|(_, r)| r.0);
    for (k, key) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9].iter().enumerate() {
        if keys.just_pressed(*key) {
            pick = Some(k);
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        let o = win.open.take().expect("open");
        close(&mut commands, &o);
        return;
    }
    let Some(k) = pick else { return };
    let Some((_, id)) = o.choices.get(k).cloned() else { return };
    if id.is_empty() {
        let o = win.open.take().expect("open");
        close(&mut commands, &o);
        return;
    }
    let conv = o.conv.clone();
    let (effects, next) = game.sim.choose(&conv, &id);
    apply(&mut game, &mut inv, &mut slaves, &conv, &effects);
    match next {
        Some(n) => {
            o.conv = n;
            if let Some(e) = o.ui.get("MoneyAmountText") {
                commands.queue(SetCaption(e, format!("c. {}", game.sim.money)));
            }
            fill(&mut commands, o, &game);
        }
        None => {
            let o = win.open.take().expect("open");
            close(&mut commands, &o);
        }
    }
}

/// (testing) KENSHI_DIALOGUE_AT=<frame>: the first selected character is put beside the
/// nearest trader with something to say (else anyone) and talks to them.
pub fn script(mut frames: Local<u32>, mut later: Local<Option<(u32, usize)>>, mut win: ResMut<DialogueWindow>, mut game: ResMut<Game>) {
    *frames += 1;
    // the talk a second after arriving (being indoors is found as the world runs)
    if let Some((at, t)) = *later
        && *frames >= at
    {
        win.pending = Some(t);
        *later = None;
    }
    if std::env::var("KENSHI_DIALOGUE_AT").ok().and_then(|f| f.parse::<u32>().ok()) != Some(*frames) {
        return;
    }
    let Some(&me) = game.selected.first() else { return };
    let at = game.sim.folk[me].mover.position_units();
    let near = game
        .sim
        .folk
        .iter()
        .filter(|p| !p.player && p.dialogue.is_some())
        .min_by(|a, b| {
            let d = |p: &kenshi_game::sim::Person| {
                let q = p.mover.position_units();
                (q[0] - at[0]).powi(2) + (q[2] - at[2]).powi(2)
            };
            (a.shop.is_none(), d(a)).partial_cmp(&(b.shop.is_none(), d(b))).unwrap()
        })
        .map(|p| (p.id, p.mover.position));
    if let Some((t, pos)) = near {
        game.sim.folk[me].mover.position = [pos[0] + 2.0, pos[1], pos[2]];
        *later = Some((*frames + 60, t));
    }
}
