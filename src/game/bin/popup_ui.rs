//! The game's messages (Kenshi_MessagePanel, F043031 / F043040): each shown in the message panel
//! at the top of the screen for 4 seconds, then the next; a message already waiting, or the
//! one showing, is not queued again.
//! (assumption) The panel's place (centred, a tenth down the screen) and its fade (the last half
//! second) are not read from the game: MyGUI's controller that fades it is set elsewhere.
use super::Game;
use bevy::prelude::*;
use kenshi_gui::{Built, GuiRes, SetAlpha, SetCaption};
use std::collections::VecDeque;

/// How long a message stays (F043031: 4 s).
const SHOW: f32 = 4.0;
const FADE: f32 = 0.5;

#[derive(Resource, Default)]
pub struct Popup {
    queue: VecDeque<String>,
    showing: Option<(Built, String, f32)>,
}

pub fn update(mut commands: Commands, mut popup: ResMut<Popup>, mut game: ResMut<Game>, gui: Res<GuiRes>, assets: Res<AssetServer>, time: Res<Time>, windows: Query<&Window>) {
    let p = &mut *popup;
    // (testing) KENSHI_MESSAGE: a message, again whenever none shows
    if p.showing.is_none()
        && p.queue.is_empty()
        && let Ok(m) = std::env::var("KENSHI_MESSAGE")
    {
        game.sim.messages.push(m);
    }
    for m in std::mem::take(&mut game.sim.messages) {
        eprintln!("message: {m}");
        super::log(&mut game.log, m.clone());
        if p.queue.contains(&m) || p.showing.as_ref().is_some_and(|s| s.1 == m) {
            continue;
        }
        p.queue.push_back(m);
    }
    let dt = time.delta_secs();
    if let Some((ui, _, left)) = p.showing.as_mut() {
        *left -= dt;
        if let Some(&r) = ui.roots.first() {
            commands.queue(SetAlpha(r, (*left / FADE).clamp(0.0, 1.0)));
        }
        if *left > 0.0 {
            return;
        }
        for r in &ui.roots {
            commands.entity(*r).despawn();
        }
        p.showing = None;
    }
    let Some(text) = p.queue.pop_front() else { return };
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let mut defs = gui.0.layout("Kenshi_MessagePanel.layout");
    if let Some(d) = defs.first_mut()
        && let kenshi_gui::res::Pos::Real(r) = d.pos
    {
        d.pos = kenshi_gui::res::Pos::Real([0.5 - r[2] * 0.5, 0.1, r[2], r[3]]);
    }
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(90));
    }
    if let Some(t) = ui.get("MessageTextBox") {
        commands.queue(SetCaption(t, text.clone()));
    }
    p.showing = Some((ui, text, SHOW));
}
