//! Kenshi's message box (Kenshi_MessageBox, F042114 / F043457): a title, a message, and Yes and
//! No (or OK alone), over everything; the game waits for the answer. Asked here:
//! - "Are you sure you want to exit?" (F048526), from the Esc menu's Exit;
//! - dismantling a building of the player's (F037762): "You cannot delete this building while it
//!   has mounted buildings." / "There are still items in the inventory, they will be lost. Are
//!   you sure you want to destroy this building?" / "Dismantling this building will destroy all
//!   furniture. Do you want to continue?" / "Are you sure?";
//! - notes the game gives (a save that failed, F034848).
//!
//! (assumption) The buttons' captions (Yes, No, OK) and which of the three buttons a question
//! uses are not read from the game.
use super::Game;
use bevy::prelude::*;
use kenshi_gui::{Built, GuiClick, GuiRes, SetCaption, SetVisible};

/// What a question does when answered Yes.
#[derive(Clone, Debug)]
pub enum Question {
    /// Quit the game.
    Exit,
    /// The selection dismantles building `b` (its furniture first, if it says so).
    Dismantle { ids: Vec<usize>, building: usize, furniture: bool },
    /// Nothing (a note, OK alone).
    Note,
}

/// A question waiting to be asked: title, message, what Yes does.
#[derive(Clone, Debug)]
pub struct Ask {
    pub title: String,
    pub text: String,
    pub question: Question,
}

#[derive(Resource, Default)]
pub struct MessageBoxes {
    /// Asked by anything (the next frame shows it).
    pub pending: Option<Ask>,
    open: Option<(Built, Question)>,
}

impl MessageBoxes {
    pub fn ask(&mut self, title: &str, text: &str, question: Question) {
        self.pending = Some(Ask { title: title.to_string(), text: text.to_string(), question });
    }
}

/// @fn F037762
///
/// Ask before dismantling building `b` (or say why it can't be).
pub fn ask_dismantle(boxes: &mut MessageBoxes, game: &Game, ids: Vec<usize>, b: usize) {
    let sim = &game.sim;
    let Some(pb) = sim.buildings.get(b) else { return };
    let rec = sim.data.get(&pb.record);
    // mounted buildings (a turret on a wall)
    let mounted = sim.buildings.iter().enumerate().any(|(k, o)| {
        k != b && sim.data.get(&o.record).is_some_and(|r| r.ints.get("building category").is_none() && r.bools.get("is exterior furniture").copied().unwrap_or(false)) && (o.at[0] - pb.at[0]).hypot(o.at[2] - pb.at[2]) < pb.radius * 0.5
    });
    let items = pb.storage.as_ref().is_some_and(|s| s.sections.iter().any(|x| !x.items.is_empty()));
    let furniture = rec.is_some_and(|r| !r.refs("interior").is_empty())
        && sim.buildings.iter().enumerate().any(|(k, o)| k != b && o.confirmed && sim.data.get(&o.record).is_some_and(|r| r.bools.get("is interior furniture").copied().unwrap_or(false)) && (o.at[0] - pb.at[0]).hypot(o.at[2] - pb.at[2]) < pb.radius);
    if mounted {
        boxes.ask("Dismantle", "You cannot delete this building while it has mounted buildings.", Question::Note);
    } else if items {
        boxes.ask("Dismantle", "There are still items in the inventory, they will be lost. Are you sure you want to destroy this building?", Question::Dismantle { ids, building: b, furniture: false });
    } else if furniture {
        boxes.ask("Dismantle", "Dismantling this building will destroy all furniture. Do you want to continue?", Question::Dismantle { ids, building: b, furniture: true });
    } else {
        boxes.ask("Dismantle", "Are you sure?", Question::Dismantle { ids, building: b, furniture: false });
    }
}

fn window_size(windows: &Query<&Window>) -> [f32; 2] {
    windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()])
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut boxes: ResMut<MessageBoxes>,
    mut game: Option<ResMut<Game>>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    mut clicks: MessageReader<GuiClick>,
    windows: Query<&Window>,
    mut exit: MessageWriter<AppExit>,
) {
    let clicked: Vec<GuiClick> = clicks.read().cloned().collect();
    // asked through the game (the systems without the boxes)
    if let Some(g) = game.as_mut() {
        if let Some(a) = g.ask.take() {
            boxes.pending = Some(a);
        }
        if let Some((ids, b)) = g.dismantle_request.take() {
            ask_dismantle(&mut boxes, g, ids, b);
        }
    }
    // an answer
    if let Some((ui, _)) = &boxes.open {
        let hit = |n: &str| clicked.iter().any(|c| ui.get(n) == Some(c.widget));
        let (yes, no) = (hit("ButtonA") || hit("ButtonB"), hit("ButtonC"));
        if yes || no {
            let (ui, q) = boxes.open.take().unwrap();
            for r in ui.roots {
                commands.entity(r).insert(Visibility::Hidden);
                commands.entity(r).despawn();
            }
            if yes {
                match q {
                    Question::Exit => {
                        exit.write(AppExit::Success);
                    }
                    Question::Dismantle { ids, building, furniture } => {
                        if let Some(g) = game.as_mut() {
                            if furniture {
                                // its furniture goes first (F037762: destroyed)
                                let (at, r) = (g.sim.buildings[building].at, g.sim.buildings[building].radius);
                                let mut k = g.sim.buildings.len();
                                let mut b = building;
                                while k > 0 {
                                    k -= 1;
                                    let o = &g.sim.buildings[k];
                                    let inside = k != b && g.sim.data.get(&o.record).is_some_and(|x| x.bools.get("is interior furniture").copied().unwrap_or(false)) && (o.at[0] - at[0]).hypot(o.at[2] - at[2]) < r;
                                    if inside {
                                        g.sim.remove_building(k);
                                        if k < b {
                                            b -= 1;
                                        }
                                    }
                                }
                                g.sim.order_dismantle(&ids, b);
                            } else {
                                g.sim.order_dismantle(&ids, building);
                            }
                        }
                    }
                    Question::Note => {}
                }
            }
        }
        return;
    }
    let Some(a) = boxes.pending.take() else { return };
    let size = window_size(&windows);
    let mut defs = gui.0.layout("Kenshi_MessageBox.layout");
    if let Some(d) = defs.first_mut()
        && let kenshi_gui::res::Pos::Real(r) = d.pos
    {
        d.pos = kenshi_gui::res::Pos::Real([0.5 - r[2] * 0.5, 0.5 - r[3] * 0.5, r[2], r[3]]);
    }
    let ui = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
    for r in &ui.roots {
        commands.entity(*r).insert(GlobalZIndex(130));
    }
    if let Some(e) = ui.get("Root.Caption") {
        commands.queue(SetCaption(e, a.title.clone()));
    }
    if let Some(e) = ui.get("MessageText") {
        commands.queue(SetCaption(e, a.text.clone()));
    }
    let note = matches!(a.question, Question::Note);
    for (b, cap, shown) in [("ButtonA", "Yes", !note), ("ButtonB", "OK", note), ("ButtonC", "No", !note)] {
        if let Some(e) = ui.get(b) {
            commands.queue(SetCaption(e, cap.to_string()));
            commands.queue(SetVisible(e, shown));
        }
    }
    boxes.open = Some((ui, a.question));
}
