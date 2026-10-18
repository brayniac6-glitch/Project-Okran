//! The right-click menu (ContextMenuGUI, Kenshi_ContextMenu.layout): the right button held on
//! someone for a quarter second (F033301: the hold timer past 0.25 s) lists what the selection
//! can do to them (F044603), each option labelled as the game names its order (F044568). A quick
//! click does the default instead.
//!
//! The options are F044603's orders under its conditions, labelled as F044568 names them:
//! carrying someone: Put Down (0x45) first; someone unconscious: Pick Up (0xe1) or, someone not
//! the selection's to take, Kidnap (0xf6, F044547), First Aid, Splint Injuries (0xf9), Loot; a
//! body: Pick Up, Corpse Disposal, Loot; locked shackles: Unlock Shackles, Use Tools, Use
//! Strength (0xc9, 0xb9, 0xba); one of the squad: Stand Up (0x1c, playing dead), Inventory,
//! First Aid, Splint Injuries, Follow, Bodyguard; one caged: Escape (0xcf) or Set Free (0x6e);
//! someone fighting: Attack Target (5, Shoot At 0xeb with a crossbow) and Attack All (0x10);
//! anyone else: Talk To, Trade, Pay Bounty, Stealth KO (0xe4, sneaking), First Aid, Follow,
//! Bodyguard, Attack Unprovoked, Attack All, and in the player's home or outpost Throw out of
//! home (0xdf) or town (0x111, F044505). A bed: Put in Bed (99, carrying someone) or Sleep
//! (0x102); a cage: Put in (0x6c, carrying), Get in (0x6b) or Set Free; a machine: Auto-Haul
//! (0x7c); training equipment: Training (0x61); a door: Open / Close, Smash (0x7b).
//! (Kenshi's menu never offers Capture, Stealth Kill or Break Out: they are the AI's orders.)
use super::{Game, dialogue_ui, inventory_ui};
use bevy::prelude::*;
use kenshi_gui::{Built, GuiRes, SetCaption};

/// How long the right button is held on someone before the menu shows (F033301, 0.25 s).
pub const HOLD: f32 = 0.25;

/// An order the menu offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Pick Up (orders 0x44, 0xd5, 0xe1).
    PickUp,
    /// Loot / Inventory (order 0x1a): their inventory beside the selected one's.
    Loot,
    /// Talk To (0xc).
    TalkTo,
    /// Trade with a shopkeeper.
    Trade,
    /// Attack Target (5).
    AttackTarget,
    /// Attack Unprovoked (0x3d).
    AttackUnprovoked,
    /// Follow (0x1f): keep up with them.
    Follow,
    /// Bodyguard (0x2d): keep up with them and fight those who fight them.
    Bodyguard,
    /// First Aid (0x19): treat their wounds with a medkit.
    FirstAid,
    /// Kidnap (0xf6): carry someone unconscious off, a crime against their people.
    Kidnap,
    /// A building of the player's: Build (a site), Operate (a production building), Open (its
    /// storage).
    Build,
    Operate,
    Open,
    /// A crafting bench: the overview's crafting tab on it.
    Crafting,
    /// The law (`kenshi_game::law`): pay the bounty with the target's faction ("Pay Bounty",
    /// F041452); knock the target out from hiding (Task_StealthKO); pick the way out of the cage
    /// and shackles ("Escape cage"); give oneself up to the guards.
    PayBounty,
    Knockout,
    Escape,
    Surrender,
    /// Pick the lock of someone else's cage from outside ("Free Prisoner").
    Free,
    /// Town training equipment: train on it ("Training", Task_UseTrainingDummy).
    Train,
    /// A door or gate: open or close it (F037658), bash it in (BASH_DOOR_PLAYER_ORDER).
    DoorToggle,
    Bash,
    /// A building of the player's: Repair (order 0x5f, Task_Build on it damaged) and Dismantle
    /// (0x60, Task_Dismantle).
    Repair,
    Dismantle,
    /// The `orders` errands: Put Down, Sleep, Put in Bed, Splint Injuries, the shackles' three,
    /// Put in / Get in a cage, Stand Up, Attack All, Auto-Haul, Throw out of home / town,
    /// Corpse Disposal.
    PutDown,
    Sleep,
    PutInBed,
    Splint,
    UnlockShackles,
    UseTools,
    UseStrength,
    PutInCage,
    GetInCage,
    StandUp,
    AttackAll,
    AutoHaul,
    ThrowOutHome,
    ThrowOutTown,
    DisposeCorpse,
}

/// Someone with wounds open, bleeding, or flesh short of its full health and not yet treated
/// (F02d51a's test for the First Aid order, (assumption) by its effect).
fn needs_treatment(game: &Game, id: usize) -> bool {
    let m = &game.sim.arena.f[id].medical;
    !m.dead && (!m.wounds.is_empty() || 0.0 < m.bleed_rate || m.parts.iter().any(|p| p.flesh + p.treatment < p.max_health() - 0.5))
}

#[derive(Resource, Default)]
pub struct ContextMenu {
    /// Someone held on long enough: the menu opens for them next frame.
    pub request: Option<usize>,
    /// (testing, `rclick_script`) the cursor the right click is at, and an option to take, in
    /// place of the mouse's (the player's own pointer is never moved).
    pub test_cursor: Option<Vec2>,
    pub test_pick: Option<usize>,
    open: Option<Open>,
}

struct Open {
    target: usize,
    ui: Built,
    rows: Vec<(Entity, Action)>,
}

#[derive(Component)]
pub struct OptionRow;

/// The orders the selection can give about `target` (F044603, see the module notes), with the
/// game's labels.
pub fn options(game: &Game, target: usize) -> Vec<(String, Action)> {
    let sim = &game.sim;
    let carrying = game.selected.iter().any(|&i| sim.folk[i].carrying.is_some());
    // town furniture (targets from FURN_TARGET): training equipment, beds, cages
    if let Some(f) = target.checked_sub(kenshi_game::production::FURN_TARGET) {
        let Some(k) = sim.furn.get(f) else { return Vec::new() };
        let mut out = Vec::new();
        if sim.furniture_training(f).is_some() {
            out.push(("Training".to_string(), Action::Train));
        } else if k.function == kenshi_game::sim::BF_BED && sim.bed_free(f) {
            out.push(if carrying { ("Put in Bed".to_string(), Action::PutInBed) } else { ("Sleep".to_string(), Action::Sleep) });
        } else if k.function == kenshi_world::furniture::BF_CAGE {
            if sim.cage_free(f) {
                out.push(if carrying { ("Put in".to_string(), Action::PutInCage) } else { ("Get in".to_string(), Action::GetInCage) });
            } else {
                out.push(("Set Free".to_string(), Action::Free));
            }
        }
        if carrying {
            out.insert(0, ("Put Down".to_string(), Action::PutDown));
        }
        return out;
    }
    // a door (targets from DOOR_TARGET)
    if let Some(d) = target.checked_sub(kenshi_game::doors::DOOR_TARGET) {
        let Some(s) = sim.door_states.get(d) else { return Vec::new() };
        let mut out = Vec::new();
        if !s.broken {
            let open = s.phase == kenshi_game::sim::DOOR_OPEN || s.phase == kenshi_game::sim::DOOR_OPENING;
            out.push((if open { "Close" } else { "Open" }.to_string(), Action::DoorToggle));
            if !open {
                out.push(("Smash".to_string(), Action::Bash));
            }
        }
        return out;
    }
    // a building of the player's (targets from BUILDING_OWNER)
    if let Some(b) = target.checked_sub(kenshi_game::base::BUILDING_OWNER) {
        let Some(pb) = sim.buildings.get(b) else { return Vec::new() };
        let mut out = Vec::new();
        if !pb.done {
            out.push(("Build".to_string(), Action::Build));
            out.push(("Dismantle".to_string(), Action::Dismantle));
            return out;
        }
        if sim.needs_repair(b) {
            out.push(("Repair".to_string(), Action::Repair));
        }
        if sim.is_operated(b) {
            out.push(("Operate".to_string(), Action::Operate));
        }
        // a machine: its output kept hauled to storage
        if sim.is_production(b) {
            out.push(("Auto-Haul".to_string(), Action::AutoHaul));
        }
        if pb.storage.is_some() {
            out.push(("Open".to_string(), Action::Open));
        }
        // a crafting bench: its queue in the overview's crafting tab
        if !sim.crafts(b).is_empty() {
            out.push(("Crafting".to_string(), Action::Crafting));
        }
        out.push(("Dismantle".to_string(), Action::Dismantle));
        return out;
    }
    let t = &sim.folk[target];
    let f = &sim.arena.f[target];
    let mut out = Vec::new();
    let hurt = needs_treatment(game, target);
    let splint = sim.needs_splint(target);
    let leader = game.selected.first().copied();
    // carrying someone: put them down (0x45)
    if carrying {
        out.push(("Put Down".to_string(), Action::PutDown));
    }
    // locked shackles: picked, cut or broken (0xc9, 0xb9, 0xba)
    let shackles = |out: &mut Vec<(String, Action)>| {
        if sim.shackles_lock(target).is_some() && !t.dead {
            out.push(("Unlock Shackles".to_string(), Action::UnlockShackles));
            out.push(("Use Tools".to_string(), Action::UseTools));
            out.push(("Use Strength".to_string(), Action::UseStrength));
        }
    };
    // attacking: with a crossbow ready, "Shoot At" (0xeb) in place of "Attack Target" (5)
    let shooting = leader.is_some_and(|l| sim.arena.f[l].gun.is_some());
    let attack = if shooting { "Shoot At" } else { "Attack Target" };
    let in_squad = t.platoon.is_some() && !t.player;
    if t.player {
        if t.playing_dead {
            out.push(("Stand Up".to_string(), Action::StandUp));
        }
        // a prisoner: out of the cage (the shackles first: their three ways)
        shackles(&mut out);
        if t.prison.is_some() && !t.picking && !t.shackled {
            out.push(("Escape".to_string(), Action::Escape));
        }
        if t.prison.is_none() && !t.down && sim.folk.iter().any(|g| g.arrest.as_ref().is_some_and(|a| a.target == target && !a.slaver)) {
            out.push(("Surrender".to_string(), Action::Surrender));
        }
        if t.down && t.carried_by.is_none() {
            out.push(("Pick Up".to_string(), Action::PickUp));
        }
        if hurt {
            out.push(("First Aid".to_string(), Action::FirstAid));
        }
        if splint {
            out.push(("Splint Injuries".to_string(), Action::Splint));
        }
        // a squad member's inventory: "Inventory" (order 0x1a on the player's own); following
        // them, guarding them (0x2d, 0x1f)
        if game.selected.first() != Some(&target) {
            out.push(("Inventory".to_string(), Action::Loot));
            if !t.down {
                out.push(("Follow".to_string(), Action::Follow));
                out.push(("Bodyguard".to_string(), Action::Bodyguard));
            }
        }
        return out;
    }
    // someone else in a cage: set free from outside (their shackles too)
    if t.prison.is_some() && !t.dead {
        if !sim.folk.iter().any(|p| p.freeing.as_ref().is_some_and(|f| f.0 == target)) {
            out.push(("Set Free".to_string(), Action::Free));
        }
        shackles(&mut out);
        return out;
    }
    if t.down || t.dead {
        // picked up (0xe1), or taken off unlawfully: kidnapped (0xf6, F044547)
        if t.carried_by.is_none() {
            if leader.is_some_and(|l| sim.is_kidnap(l, target)) {
                out.push(("Kidnap".to_string(), Action::Kidnap));
            } else {
                out.push(("Pick Up".to_string(), Action::PickUp));
            }
            if t.dead {
                out.push(("Corpse Disposal".to_string(), Action::DisposeCorpse));
            }
        }
        if hurt {
            out.push(("First Aid".to_string(), Action::FirstAid));
        }
        if splint {
            out.push(("Splint Injuries".to_string(), Action::Splint));
        }
        shackles(&mut out);
        out.push(("Loot".to_string(), Action::Loot));
        return out;
    }
    if f.active {
        out.push((attack.to_string(), Action::AttackTarget));
        if in_squad {
            out.push(("Attack All".to_string(), Action::AttackAll));
        }
        return out;
    }
    if t.dialogue.is_some() {
        out.push(("Talk To".to_string(), Action::TalkTo));
    }
    if t.shop.is_some() {
        out.push(("Trade".to_string(), Action::Trade));
    }
    // a bounty with their faction: pay it off
    if let (Some(&me), Some(fac)) = (game.selected.first(), t.faction.as_deref()) {
        let amount = sim.bounty_with(me, fac);
        if amount > 0 {
            out.push((format!("Pay Bounty (c.{amount})"), Action::PayBounty));
        }
    }
    // sneaking: a knockout from behind (0xe4)
    if game.selected.first().is_some_and(|&me| sim.folk[me].sneaking) {
        out.push(("Stealth KO".to_string(), Action::Knockout));
    }
    if hurt {
        out.push(("First Aid".to_string(), Action::FirstAid));
    }
    if splint {
        out.push(("Splint Injuries".to_string(), Action::Splint));
    }
    shackles(&mut out);
    // F044603 offers someone standing calm Attack Unprovoked (0x3d), Bodyguard (0x2d) and Follow
    // (0x1f); its menu lists them Follow, Bodyguard, Attack Unprovoked
    out.push(("Follow".to_string(), Action::Follow));
    out.push(("Bodyguard".to_string(), Action::Bodyguard));
    out.push(("Attack Unprovoked".to_string(), Action::AttackUnprovoked));
    if in_squad {
        out.push(("Attack All".to_string(), Action::AttackAll));
    }
    // F044505: in a building of the player's, out of their home; in their outpost, out of town
    let at = t.mover.position_units();
    if sim.building_at(at).is_some() {
        out.push(("Throw out of home".to_string(), Action::ThrowOutHome));
    } else if sim.in_outpost(at) {
        out.push(("Throw out of town".to_string(), Action::ThrowOutTown));
    }
    out
}

/// Carry out an order on `target` for the selection.
pub fn act(game: &mut Game, inv: &mut inventory_ui::InventoryWindow, talk: &mut dialogue_ui::DialogueWindow, target: usize, a: Action) {
    let selected = game.selected.clone();
    if selected.is_empty() {
        return;
    }
    if a == Action::PutDown {
        let carriers: Vec<usize> = selected.iter().copied().filter(|&i| game.sim.folk[i].carrying.is_some()).collect();
        game.sim.order_put_down(&carriers);
        return;
    }
    if let Some(f) = target.checked_sub(kenshi_game::production::FURN_TARGET) {
        match a {
            Action::Train => game.sim.order_train_at(&selected, f),
            Action::Sleep => game.sim.order_sleep(&selected, f),
            Action::PutInBed => game.sim.order_put_in_bed(&selected, f),
            Action::PutInCage => game.sim.order_cage(&selected, f, false),
            Action::GetInCage => game.sim.order_cage(&selected, f, true),
            // the cage's prisoner set free
            Action::Free => {
                if let Some(p) = game.sim.folk.iter().find(|p| p.prison.as_ref().is_some_and(|q| q.cage == f && !q.out)).map(|p| p.id) {
                    game.sim.order_free(selected[0], p);
                }
            }
            _ => {}
        }
        return;
    }
    if let Some(d) = target.checked_sub(kenshi_game::doors::DOOR_TARGET) {
        match a {
            // "Open" / "Close" (task 72): the first selected walks there and toggles it
            Action::DoorToggle => {
                if selected.is_empty() {
                    if let Some(say) = game.sim.toggle_door(d) {
                        game.sim.messages.push(say.to_string());
                    }
                } else {
                    game.sim.order_door(&selected, d);
                }
            }
            Action::Bash => game.sim.order_bash(&selected, d),
            _ => {}
        }
        return;
    }
    if let Some(b) = target.checked_sub(kenshi_game::base::BUILDING_OWNER) {
        match a {
            Action::Build => game.sim.order_build(&selected, b),
            Action::Operate => game.sim.order_operate(&selected, b),
            Action::Open => {
                let at = game.sim.buildings[b].at;
                game.sim.order_move(&selected[..1], at);
                inv.pending_loot = Some(target);
            }
            Action::Crafting => game.crafting_request = Some(b),
            Action::Repair => game.sim.order_repair(&selected, b),
            Action::Dismantle => game.dismantle_request = Some((selected.clone(), b)),
            Action::AutoHaul => game.sim.order_auto_haul(&selected, b),
            _ => {}
        }
        return;
    }
    let walk_to = |game: &mut Game| {
        let to = game.sim.folk[target].mover.position_units();
        game.sim.order_move_squad(&selected, to);
    };
    match a {
        Action::PickUp => game.sim.order_pickup(&selected, target),
        Action::Loot => {
            if !selected.contains(&target) {
                walk_to(game);
            }
            inv.pending_loot = Some(target);
        }
        Action::TalkTo => {
            walk_to(game);
            talk.pending = Some(target);
        }
        Action::Trade => {
            walk_to(game);
            inv.pending_trade = Some(target);
        }
        Action::AttackTarget | Action::AttackUnprovoked => game.sim.order_attack(&selected, target),
        Action::FirstAid => game.sim.order_first_aid(&selected, target),
        Action::Follow => game.sim.order_follow(&selected, target, false),
        Action::Bodyguard => game.sim.order_follow(&selected, target, true),
        Action::Kidnap => game.sim.order_kidnap(&selected, target),
        Action::PayBounty => {
            if let Some(fac) = game.sim.folk[target].faction.clone()
                && !game.sim.pay_bounty(selected[0], &fac)
            {
                game.sim.messages.push("Not enough money to pay the bounty.".to_string());
            }
        }
        Action::Knockout => game.sim.order_stealth_ko(selected[0], target),
        Action::Escape => game.sim.order_escape(target),
        Action::Free => {
            if let Some(&me) = selected.first() {
                game.sim.order_free(me, target);
            }
        }
        Action::Surrender => game.sim.folk[target].surrendered = true,
        Action::Splint => game.sim.order_splint(&selected, target),
        Action::UnlockShackles => game.sim.order_shackles(&selected, target, kenshi_game::orders::ShackleHow::Pick),
        Action::UseTools => game.sim.order_shackles(&selected, target, kenshi_game::orders::ShackleHow::Cut),
        Action::UseStrength => game.sim.order_shackles(&selected, target, kenshi_game::orders::ShackleHow::Strength),
        Action::StandUp => game.sim.order_stand_up(target),
        Action::AttackAll => game.sim.order_attack_all(&selected, target),
        Action::ThrowOutHome => game.sim.order_throw_out(target, false),
        Action::ThrowOutTown => game.sim.order_throw_out(target, true),
        Action::DisposeCorpse => game.sim.order_dispose(&selected, target),
        Action::Build
        | Action::Operate
        | Action::Open
        | Action::Crafting
        | Action::DoorToggle
        | Action::Bash
        | Action::Train
        | Action::Repair
        | Action::Dismantle
        | Action::PutDown
        | Action::Sleep
        | Action::PutInBed
        | Action::PutInCage
        | Action::GetInCage
        | Action::AutoHaul => {}
    }
}

fn close(commands: &mut Commands, o: &Open) {
    for r in &o.ui.roots {
        commands.entity(*r).despawn();
    }
}

/// Open the menu asked for, take a click on an option, close it on a click elsewhere or Esc.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut menu: ResMut<ContextMenu>,
    mut game: ResMut<Game>,
    mut inv: ResMut<inventory_ui::InventoryWindow>,
    mut talk: ResMut<dialogue_ui::DialogueWindow>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    rows: Query<&Interaction, With<OptionRow>>,
) {
    // a choice
    if let Some(o) = &menu.open {
        let picked = o.rows.iter().find(|(e, _)| rows.get(*e).is_ok_and(|i| *i == Interaction::Pressed)).map(|(_, a)| *a);
        let picked = picked.or_else(|| menu.test_pick.and_then(|k| o.rows.get(k)).map(|r| r.1));
        if let Some(a) = picked {
            if std::env::var("KENSHI_RCLICK_AT").is_ok() {
                eprintln!("rclick: picked {a:?}");
            }
            let target = o.target;
            close(&mut commands, o);
            menu.open = None;
            menu.test_pick = None;
            act(&mut game, &mut inv, &mut talk, target, a);
            return;
        }
        // (a click on one of its rows is a choice, the row pressed by the next frame at the latest)
        let on_row = o.rows.iter().any(|(e, _)| rows.get(*e).is_ok_and(|i| *i != Interaction::None));
        let elsewhere = !on_row && (mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right));
        if elsewhere && std::env::var("KENSHI_RCLICK_AT").is_ok() {
            eprintln!("rclick: closed by a click elsewhere at {:?}", windows.iter().next().and_then(|w| w.cursor_position()));
        }
        if elsewhere || keys.just_pressed(KeyCode::Escape) || menu.request.is_some() {
            close(&mut commands, o);
            menu.open = None;
        }
    }
    let Some(target) = menu.request.take() else { return };
    let opts = options(&game, target);
    if opts.is_empty() {
        return;
    }
    let Some(win) = windows.iter().next() else { return };
    let size = [win.width(), win.height()];
    let cursor = win.cursor_position().unwrap_or(Vec2::new(size[0] * 0.5, size[1] * 0.5));
    // Kenshi_ContextMenu: the title, then the layout's Option (a Kenshi_Button1 and its Value
    // box) made once per option, the window as tall as they need
    let mut defs = gui.0.layout("Kenshi_ContextMenu.layout");
    let n = opts.len() as f32;
    if let Some(d) = defs.first_mut()
        && let kenshi_gui::res::Pos::Real(r) = d.pos
    {
        let (title, row) = (0.472973 * r[3], 0.527027 * r[3]);
        let height = title + row * n;
        // by the cursor, kept on the screen
        let x = (cursor.x / size[0] + 0.01).min(1.0 - r[2]);
        let y = (cursor.y / size[1]).min(1.0 - height);
        d.pos = kenshi_gui::res::Pos::Real([x, y, r[2], height]);
        fn fit(d: &mut kenshi_gui::WidgetDef, title: f32, rows: f32, opts: &[(String, String)]) {
            for c in &mut d.children {
                if let kenshi_gui::res::Pos::Real(p) = &mut c.pos {
                    if c.name == "OptionsList" {
                        *p = [0.0, title, 1.0, rows];
                    } else if c.children.iter().any(|k| k.name == "NameText") {
                        *p = [0.0, 0.0, 1.0, title];
                    }
                }
                if c.name == "OptionsList" {
                    let Some(tpl) = c.children.iter().find(|k| k.name == "Option").cloned() else { continue };
                    c.children.clear();
                    let k_n = opts.len().max(1) as f32;
                    for (k, (label, value)) in opts.iter().enumerate() {
                        let mut o = tpl.clone();
                        o.name = format!("Option{k}");
                        o.pos = kenshi_gui::res::Pos::Real([0.0, k as f32 / k_n, 1.0, 1.0 / k_n]);
                        for w in &mut o.children {
                            let (name, caption) = if w.name == "Button" { (format!("CtxOption{k}"), label.clone()) } else { (format!("CtxValue{k}"), value.clone()) };
                            w.name = name;
                            w.props.retain(|(key, _)| key != "Caption");
                            w.props.push(("Caption".to_string(), caption));
                        }
                        c.children.push(o);
                    }
                    continue;
                }
                fit(c, title, rows, opts);
            }
        }
        let shown: Vec<(String, String)> = opts.iter().map(|(l, a)| (l.clone(), value(&game, target, *a))).collect();
        fit(d, title / height, row * n / height, &shown);
    }
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(150));
    }
    let name = if let Some(f) = target.checked_sub(kenshi_game::production::FURN_TARGET) {
        game.sim.furn.get(f).map_or(String::new(), |k| k.name.clone())
    } else if let Some(d) = target.checked_sub(kenshi_game::doors::DOOR_TARGET) {
        game.sim.door_states.get(d).and_then(|s| game.sim.data.get(&s.door.record)).map_or(String::new(), |r| r.name.clone())
    } else if let Some(b) = target.checked_sub(kenshi_game::base::BUILDING_OWNER) {
        game.sim.buildings.get(b).and_then(|pb| game.sim.data.get(&pb.record)).map_or(String::new(), |r| r.name.clone())
    } else if let p = &game.sim.folk[target]
        && !p.name.is_empty()
    {
        p.name.clone()
    } else {
        let p = &game.sim.folk[target];
        p.character.as_deref().and_then(|c| game.sim.data.get(c)).map_or(format!("#{target}"), |c| c.name.clone())
    };
    if let Some(e) = ui.get("NameText") {
        commands.queue(SetCaption(e, name));
    }
    let mut made = Vec::new();
    for (k, (_, a)) in opts.iter().enumerate() {
        if let Some(b) = ui.get(&format!("CtxOption{k}")) {
            commands.entity(b).insert(OptionRow);
            made.push((b, *a));
        }
    }
    menu.open = Some(Open { target, ui, rows: made });
}

/// The number beside an option (the layout's Value box): a knockout's chance; else nothing.
fn value(game: &Game, target: usize, a: Action) -> String {
    let sim = &game.sim;
    match (a, game.selected.first()) {
        (Action::Knockout, Some(&me)) if target < sim.folk.len() => {
            let lvl = sim.arena.f[me].level(kenshi_char::StatsEnumerated::StatAssassination);
            let c = if sim.arena.f[target].active { 0.0 } else { kenshi_game::law::stealth_ko_chance(lvl, sim.ko_guard(target)) };
            format!("{:.0}%", c * 100.0)
        }
        _ => String::new(),
    }
}

/// (testing) KENSHI_MENU_AT=<frame>[@<option>]: at that frame the menu opens on the nearest
/// person not of the squad (someone down first); with an option label, 30 frames later that
/// option is taken.
pub fn script(
    mut frames: Local<u32>,
    mut menu: ResMut<ContextMenu>,
    mut game: ResMut<Game>,
    mut inv: ResMut<inventory_ui::InventoryWindow>,
    mut talk: ResMut<dialogue_ui::DialogueWindow>,
) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_MENU_AT") else { return };
    let (at, pick) = match v.split_once('@') {
        Some((a, b)) => (a.parse::<u32>().unwrap_or(0), Some(b.to_string())),
        None => (v.parse::<u32>().unwrap_or(0), None),
    };
    let Some(&me) = game.selected.first() else { return };
    let here = game.sim.folk[me].mover.position_units();
    let d = |p: &kenshi_game::sim::Person| {
        let q = p.mover.position_units();
        (q[0] - here[0]).hypot(q[2] - here[2])
    };
    let target = game.sim.folk.iter().filter(|p| !p.player).min_by(|a, b| (!a.down, d(a)).partial_cmp(&(!b.down, d(b))).unwrap()).map(|p| p.id);
    let Some(target) = target else { return };
    if *frames == at {
        menu.request = Some(target);
        eprintln!("menu on {target}: {:?}", options(&game, target).iter().map(|o| o.0.clone()).collect::<Vec<_>>());
    }
    // (testing) KENSHI_ESCORT_FOE=<frame>: then the stranger nearest the menu's person goes for them
    if std::env::var("KENSHI_ESCORT_FOE").ok().and_then(|f| f.parse::<u32>().ok()) == Some(*frames)
        && let Some((target, _)) = game.sim.folk[me].escort
    {
        let there = game.sim.folk[target].mover.position_units();
        let foe = game
            .sim
            .folk
            .iter()
            .filter(|p| !p.player && p.id != target && !p.down)
            .min_by(|a, b| {
                let da = (a.mover.position_units()[0] - there[0]).hypot(a.mover.position_units()[2] - there[2]);
                let db = (b.mover.position_units()[0] - there[0]).hypot(b.mover.position_units()[2] - there[2]);
                da.total_cmp(&db)
            })
            .map(|p| p.id);
        if let Some(foe) = foe {
            eprintln!("escort test: {foe} attacks {target}");
            game.sim.order_attack(&[foe], target);
        }
    }
    if *frames == at + 30
        && let Some(label) = &pick
        && let Some((_, a)) = options(&game, target).into_iter().find(|(l, _)| l == label.as_str())
    {
        act(&mut game, &mut inv, &mut talk, target, a);
    }
}

/// (testing) KENSHI_LAW_AT="frame@what[,frame@what...]": at each frame the first selected
/// character: "bounty" gets a stealing bounty with the nearest townsperson's faction; "steal"
/// steals the nearest trader's first item; "ko" sneaks and knocks out the nearest townsperson;
/// "escape" picks its way out of its cage; "jail" puts it in the nearest free cage; "surrender" gives itself up; "skills" sets its
/// lockpicking and assassination to 50.
pub fn law_script(mut frames: Local<u32>, mut game: ResMut<Game>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_LAW_AT") else { return };
    let Some(&me) = game.selected.first() else { return };
    for step in v.split(',') {
        let Some((at, what)) = step.split_once('@') else { continue };
        if at.parse::<u32>().ok() != Some(*frames) {
            continue;
        }
        let here = game.sim.folk[me].mover.position_units();
        let d = |p: &kenshi_game::sim::Person| {
            let q = p.mover.position_units();
            (q[0] - here[0]).hypot(q[2] - here[2])
        };
        let near = |shop: bool| {
            game.sim.folk.iter().filter(|p| !p.player && !p.down && p.faction.is_some() && (!shop || p.shop.is_some())).min_by(|a, b| d(a).total_cmp(&d(b))).map(|p| p.id)
        };
        let cages = game.sim.furn.iter().filter(|f| f.function == kenshi_world::furniture::BF_CAGE).count();
        eprintln!("law script: {what} (cages loaded {cages})");
        match what {
            "bounty" => {
                if let Some(t) = near(false) {
                    let f = game.sim.folk[t].faction.clone().unwrap();
                    game.sim.add_bounty(me, &f, kenshi_ai::CrimeEnum::CrimeStealing);
                }
            }
            "steal" => {
                if let Some(t) = near(true) {
                    let main = game.sim.folk[me].inventory.section("main").unwrap_or(0);
                    let r = game.sim.steal(t, 0, me, main, None);
                    eprintln!("law script: steal from {t}: {r:?}");
                }
            }
            "ko" => {
                if let Some(t) = near(false) {
                    game.sim.folk[me].sneaking = true;
                    game.sim.order_stealth_ko(me, t);
                }
            }
            "escape" => game.sim.order_escape(me),
            // put in the nearest free cage, as a prisoner of its faction
            "jail" => {
                let free = (0..game.sim.furn.len())
                    .filter(|&k| game.sim.furn[k].function == kenshi_world::furniture::BF_CAGE && !game.sim.folk.iter().any(|p| p.prison.as_ref().is_some_and(|q| q.cage == k)))
                    .min_by(|&a, &b| {
                        let d = |k: usize| (game.sim.furn[k].position[0] - here[0]).hypot(game.sim.furn[k].position[2] - here[2]);
                        d(a).total_cmp(&d(b))
                    });
                if let Some(k) = free {
                    let f = near(false).and_then(|t| game.sim.folk[t].faction.clone()).unwrap_or_default();
                    game.sim.imprison(me, k, &f, false);
                    game.focus = Vec3::from(game.sim.furn[k].position) - game.zone.origin;
                    eprintln!("law script: jailed in cage {k}, options {:?}", options(&game, me).iter().map(|o| o.0.clone()).collect::<Vec<_>>());
                }
            }
            "skills" => {
                for st in [kenshi_char::StatsEnumerated::StatLockpicking, kenshi_char::StatsEnumerated::StatAssassination] {
                    game.sim.arena.f[me].skills.levels.insert(st, 50.0);
                }
            }
            "surrender" => game.sim.folk[me].surrendered = true,
            // limbs: "sever<slot>" loses that limb (0 left arm .. 3 right leg); "attach" puts the
            // first robotic limb in the pack where it goes
            w if w.starts_with("sever") => {
                let slot: usize = w[5..].parse().unwrap_or(0);
                if let Some(i) = game.sim.limb_part(me, slot) {
                    let m = &mut game.sim.arena.f[me].medical;
                    m.parts[i].slot = 1;
                    m.parts[i].flesh = -m.parts[i].max_health() - 1.0;
                    m.severed.push(i);
                }
            }
            "attach" => {
                let found = game.sim.folk[me].inventory.sections.iter().enumerate().find_map(|(s, sec)| {
                    sec.items.iter().position(|p| p.item.kind == Some(kenshi_data::ItemType::LimbReplacement)).map(|k| (s, k))
                });
                if let Some((s, k)) = found
                    && let Some(item) = game.sim.folk[me].inventory.take(s, k)
                {
                    let r = game.sim.attach_limb(me, item);
                    eprintln!("law script: attach: {:?}", r.as_ref().map(|_| ()).map_err(|i| i.name.clone()));
                }
            }
            _ => {}
        }
    }
}

/// (testing) KENSHI_RCLICK_AT=<frame>[@<option number>]: a right click on the nearest person not
/// of the squad (on the head, or on the body lying on the ground), as the mouse's (its place given to the click as `ContextMenu::test_cursor`,
/// the player's own pointer left alone), the right button held from that frame for 40 frames
/// (the menu opens after `HOLD`); with an option number (from 0), that option taken 20 frames
/// later. KENSHI_RCLICK_DUMMY: the click is on the nearest town training equipment instead (the
/// first selected put a little way from it first); KENSHI_SKILL=stat:level then sets a skill.
#[allow(clippy::too_many_arguments)]
pub fn rclick_script(
    mut frames: Local<u32>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    cams: Query<(&Camera, &GlobalTransform), super::MainCam>,
    mut game: ResMut<Game>,
    mut menu: ResMut<ContextMenu>,
) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_RCLICK_AT") else { return };
    let (at, pick) = match v.split_once('@') {
        Some((a, b)) => (a.parse::<u32>().unwrap_or(0), b.parse::<usize>().ok()),
        None => (v.parse::<u32>().unwrap_or(0), None),
    };
    let Some(&me) = game.selected.first() else { return };
    let dummy = std::env::var("KENSHI_RCLICK_DUMMY").is_ok();
    let here = game.sim.folk[me].mover.position_units();
    let nearest_dummy = |game: &Game| {
        (0..game.sim.furn.len()).filter(|&k| game.sim.furniture_training(k).is_some()).min_by(|&a, &b| {
            let d = |k: usize| (game.sim.furn[k].position[0] - here[0]).hypot(game.sim.furn[k].position[2] - here[2]);
            d(a).total_cmp(&d(b))
        })
    };
    if dummy && *frames + 400 == at {
        if let Some(f) = nearest_dummy(&game) {
            let p = game.sim.furn[f].position;
            game.sim.folk[me].mover.position = [p[0] / 10.0 + 20.0, p[1] / 10.0, p[2] / 10.0];
            game.focus = Vec3::from(p) - game.zone.origin;
            eprintln!("rclick: by {} ({f})", game.sim.furn[f].name);
            if let Some((st, l)) = std::env::var("KENSHI_SKILL").ok().and_then(|v| v.split_once(':').and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<f32>().ok()?))))
                && let Some(stat) = kenshi_char::StatsEnumerated::from_i32(st)
            {
                game.sim.arena.f[me].skills.levels.insert(stat, l);
            }
        }
    }
    if *frames >= at && *frames < at + 40 {
        let Ok((cam, ct)) = cams.single() else { return };
        let target = if dummy {
            nearest_dummy(&game).map(|f| (format!("{} ({f})", game.sim.furn[f].name), Vec3::from(game.sim.furn[f].position) - game.zone.origin + Vec3::Y * 8.0))
        } else {
            let d = |p: &kenshi_game::sim::Person| {
                let q = p.mover.position_units();
                (q[0] - here[0]).hypot(q[2] - here[2])
            };
            game.sim.folk.iter().filter(|p| !p.player && !p.dead).min_by(|a, b| d(a).total_cmp(&d(b))).map(|t| (format!("{} ({})", game.sim.display_name(t.id), t.id), Vec3::from(t.mover.position_units()) - game.zone.origin + Vec3::Y * if t.down { 1.0 } else { 17.0 }))
        };
        let Some((name, p)) = target else { return };
        menu.test_cursor = cam.world_to_viewport(ct, p).ok();
        if *frames == at {
            eprintln!("rclick: on {name}");
            buttons.press(MouseButton::Right);
        }
    }
    if *frames == at + 40 {
        buttons.release(MouseButton::Right);
        menu.test_cursor = None;
        eprintln!("rclick: menu open {}: {:?}", menu.open.is_some(), menu.open.as_ref().map(|o| o.rows.iter().map(|r| r.1).collect::<Vec<_>>()));
    }
    if let Some(k) = pick
        && *frames == at + 60
    {
        eprintln!("rclick: taking option {k}");
        menu.test_pick = Some(k);
    }
}
