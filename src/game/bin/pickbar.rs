//! Lockpicking over a character's head (Task_PickLock::update, F031344): a progress bar
//! (Kenshi_ProgressBarPanel.layout, F042120) captioned "Lockpicking (Chance {1}%)", filling toward
//! the next attempt, while someone picks a cage's or shackles' lock; each attempt's outcome rises
//! as a floater, "Pick failed" or "Pick success!", from 15 units over them.
//! (assumption) The bar sits 26 units over the feet (over the name tag) and is as wide as the
//! layout makes it on screen; the floaters rise as the damage floaters do (crate::floaters).
//! The same bar shows a prospector's progress, captioned "Prospecting" (F031299), and a trainee's
//! way to the skill's next level, "Training" (F031296).

use super::{Game, MainCam};
use bevy::prelude::*;
use kenshi_gui::ui::{Built, SetCaption};
use kenshi_gui::GuiRes;
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct PickBars {
    bars: HashMap<usize, Built>,
}

/// A "Pick failed" / "Pick success!" floater rising over someone.
#[derive(Component)]
pub struct PickFloater {
    over: usize,
    rise: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut bars: ResMut<PickBars>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    mut nodes: Query<&mut Node>,
    mut floaters: Query<(Entity, &mut PickFloater, &mut TextColor, &ComputedNode)>,
) {
    let Ok((cam, at)) = cams.single() else { return };
    let size = windows.iter().next().map_or([1600.0, 900.0], |w| [w.width(), w.height()]);
    let origin = game.zone.origin;
    let events = std::mem::take(&mut game.sim.pick_events);
    let game = &*game;
    let screen = |id: usize, up: f32| {
        let p = Vec3::from(game.sim.folk[id].mover.position_units()) - origin + Vec3::Y * up;
        cam.world_to_viewport(at, p).ok()
    };
    // the bars: one per picker
    // (and Task_Prospecting's, F031299: "Prospecting")
    let mut pickers: Vec<(usize, String, f32)> = (0..game.sim.folk.len())
        .filter_map(|i| game.sim.pick_status(i).map(|(c, f)| (i, format!("Lockpicking (Chance {}%)", (c * 100.0).round() as i32), f)))
        .collect();
    pickers.extend(game.sim.prospectors.iter().map(|(i, f)| (*i, "Prospecting".to_string(), *f)));
    // (and Task_UseTrainingDummy's, F031296: "Training", the skill's way to its next level)
    pickers.extend((0..game.sim.folk.len()).filter_map(|i| game.sim.training_status(i).map(|f| (i, "Training".to_string(), f))));
    bars.bars.retain(|id, b| {
        let keep = pickers.iter().any(|p| p.0 == *id);
        if !keep {
            for r in &b.roots {
                commands.entity(*r).try_despawn();
            }
        }
        keep
    });
    let defs = gui.0.layout("Kenshi_ProgressBarPanel.layout");
    let (w, h) = (size[0] * 0.112, size[1] * 0.0306);
    for (id, caption, frac) in pickers {
        let b = bars.bars.entry(id).or_insert_with(|| {
            let mut d = defs.clone();
            if let Some(r) = d.first_mut() {
                r.pos = kenshi_gui::res::Pos::Px([-1000.0, 0.0, w, h]);
            }
            let b = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &d, None, size);
            for r in &b.roots {
                commands.entity(*r).insert(GlobalZIndex(35));
            }
            b
        });
        if let Some(t) = b.get("Text") {
            commands.queue(SetCaption(t, caption));
        }
        // the bar's track filled (kenshi_gui's ProgressBar: Kenshi_ProgressBarFill's TrackSkin)
        if let Some(bar) = b.get("ProgressBar") {
            commands.queue(kenshi_gui::SetProgress(bar, frac));
        }
        if let Some(&root) = b.roots.first()
            && let Ok(mut n) = nodes.get_mut(root)
        {
            match screen(id, 26.0) {
                Some(s) => {
                    n.left = Val::Px(s.x - w * 0.5);
                    n.top = Val::Px(s.y - h);
                }
                None => n.left = Val::Px(-1000.0),
            }
        }
    }
    // the attempts' floaters
    let font = gui.0.fonts.get("Kenshi_FloaterFont_Medium");
    for (id, ok) in events {
        commands.spawn((
            Text::new(if ok { "Pick success!" } else { "Pick failed" }),
            TextFont {
                font: font.map_or_else(Default::default, |f| bevy::text::FontSource::Handle(assets.load(f.file.clone()))),
                font_size: bevy::text::FontSize::Px(font.map_or(18.0, |f| f.screen_px())),
                ..default()
            },
            TextColor(if ok { Color::srgb(0.4, 0.9, 0.4) } else { Color::srgb(0.95, 0.85, 0.7) }),
            Node { position_type: PositionType::Absolute, left: Val::Px(-1000.0), ..default() },
            GlobalZIndex(40),
            PickFloater { over: id, rise: 0.0 },
        ));
    }
    let dt = time.delta_secs();
    for (e, mut fl, mut colour, computed) in &mut floaters {
        fl.rise += dt * 4.0;
        if 12.0 <= fl.rise || fl.over >= game.sim.folk.len() {
            commands.entity(e).despawn();
            continue;
        }
        if 8.0 < fl.rise {
            colour.0.set_alpha(1.0 - (fl.rise - 8.0) * 0.25);
        }
        if let Ok(mut n) = nodes.get_mut(e) {
            match screen(fl.over, 15.0 + fl.rise) {
                Some(s) => {
                    let sz = computed.size() * computed.inverse_scale_factor();
                    n.left = Val::Px(s.x - sz.x * 0.5);
                    n.top = Val::Px(s.y - sz.y * 0.5);
                }
                None => n.left = Val::Px(-1000.0),
            }
        }
    }
}
