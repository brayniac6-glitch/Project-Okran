//! Speech over heads (DialogueSpeechBubble, Kenshi_SpeechPanel.layout): each line someone says
//! (`Sim::said`) in a speech panel over their head, following them, gone after a while.
//!
//! Spec: F043044 (the bubble: its "SpeechText" in Kenshi_StandardFont_Large for the characters'
//! bubbles (Medium for the other kind); the gap between
//! the panel's and the text's sizes in the layout kept), F042985 (a line: the panel back at its
//! layout size, the text set, the panel then sized to the text's size plus that gap: a short
//! line gets a small panel, a long one wraps at the layout's width), F040910 (a line stays 2 s
//! plus 0.12 s a letter; one bubble a speaker, its line replaced).
//! (assumption) The panel sits on the head (22 units over the feet) and the name tag rises over
//! it, as Kenshi shows them; the large font (the bubble's +0x149 flag, not traced) as Kenshi's
//! screens show over characters, the size of their name tags'. The text is measured off screen in the same font and wrap width first,
//! then the panel made at that size (the skin is drawn at the size the panel is made with).
use super::{Game, MainCam};
use bevy::prelude::*;
use bevy::text::TextLayoutInfo;
use kenshi_gui::ui::Caption;
use kenshi_gui::{Built, GuiRes, SetCaption};
use std::collections::HashMap;

/// A speaker's line: being measured (the off-screen text), or shown (the panel and its size).
enum Bubble {
    Measuring(Entity, String),
    Shown(Built, Vec2),
}

#[derive(Resource, Default)]
pub struct Speech {
    /// Speaker -> its bubble and when it goes (seconds).
    by: HashMap<usize, (Bubble, f32)>,
    /// Each speaker's panel height (pixels): the name tag rises by it.
    pub lift: HashMap<usize, f32>,
}

/// The layout's sizes (fractions of the screen) and the text's place in the panel.
const PANEL: [f32; 2] = [0.234375, 0.0222222];
const TEXT: [f32; 4] = [0.0688889, 0.125, 0.855556, 0.75];

fn despawn(commands: &mut Commands, b: &Bubble) {
    match b {
        Bubble::Measuring(e, _) => {
            commands.entity(*e).try_despawn();
        }
        Bubble::Shown(b, _) => {
            for r in &b.roots {
                commands.entity(*r).try_despawn();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut speech: ResMut<Speech>,
    mut game: ResMut<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    mut nodes: Query<&mut Node>,
    captions: Query<&Caption>,
    mut fonts: Query<&mut TextFont>,
    laid: Query<(&TextLayoutInfo, &ComputedNode)>,
    time: Res<Time>,
) {
    let (Ok(win), Ok((cam, at))) = (windows.single(), cams.single()) else { return };
    let size = [win.width(), win.height()];
    let now = time.elapsed_secs();
    let medium = gui.0.fonts.get("Kenshi_StandardFont_Large");
    let font = || TextFont {
        font: medium.map_or_else(Default::default, |f| bevy::text::FontSource::Handle(assets.load(f.file.clone()))),
        font_size: bevy::text::FontSize::Px(medium.map_or(16.0, |f| f.screen_px())),
        ..default()
    };
    let (w0, h0) = (size[0] * PANEL[0], size[1] * PANEL[1]);
    // (testing) KENSHI_SAY="text|text": the first player character says each, 6 s apart
    if let Ok(v) = std::env::var("KENSHI_SAY") {
        let lines: Vec<&str> = v.split('|').collect();
        let k = (now / 6.0) as usize;
        if now % 6.0 < time.delta_secs()
            && let (Some(l), Some(me)) = (lines.get(k % lines.len().max(1)), game.sim.folk.iter().find(|p| p.player).map(|p| p.id))
        {
            game.sim.said.push((me, l.to_string()));
        }
    }
    for (who, text) in std::mem::take(&mut game.sim.said) {
        if std::env::var("KENSHI_SPEECH_DEBUG").is_ok() {
            eprintln!("speech: {} says {text:?}", game.sim.display_name(who));
        }
        if let Some((old, _)) = speech.by.remove(&who) {
            despawn(&mut commands, &old);
        }
        // measured first: the text in the bubble's font, wrapped at the layout's text width
        let e = commands
            .spawn((
                Text::new(text.clone()),
                font(),
                TextLayout::justify(Justify::Center),
                Node { position_type: PositionType::Absolute, left: Val::Px(-10000.0), max_width: Val::Px(w0 * TEXT[2]), ..default() },
            ))
            .id();
        let stay = 2.0 + 0.12 * text.len() as f32;
        speech.by.insert(who, (Bubble::Measuring(e, text), now + stay));
    }
    let origin = game.zone.origin;
    let mut lift = HashMap::new();
    let mut keep = HashMap::new();
    for (who, (bubble, until)) in std::mem::take(&mut speech.by) {
        if now > until || game.sim.folk.get(who).is_none_or(|p| p.dead || p.away) {
            despawn(&mut commands, &bubble);
            continue;
        }
        let bubble = match bubble {
            Bubble::Measuring(e, text) => {
                let Ok((info, computed)) = laid.get(e) else {
                    keep.insert(who, (Bubble::Measuring(e, text), until));
                    continue;
                };
                if info.glyphs.is_empty() {
                    keep.insert(who, (Bubble::Measuring(e, text), until));
                    continue;
                }
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for g in &info.glyphs {
                    let half = g.atlas_info.rect.size().x * 0.5;
                    lo = lo.min(g.position.x - half);
                    hi = hi.max(g.position.x + half);
                }
                let k = computed.inverse_scale_factor();
                // (assumption) some room over the glyphs, so the caption wraps where the measure did
                let text_size = Vec2::new((hi - lo) * k + 12.0, info.size.y * k);
                commands.entity(e).despawn();
                // the panel: the text plus the layout's gap round it
                let panel = Vec2::new(text_size.x + w0 * (1.0 - TEXT[2]), text_size.y + h0 * (1.0 - TEXT[3]));
                let mut defs = gui.0.layout("Kenshi_SpeechPanel.layout");
                if let Some(r) = defs.first_mut() {
                    r.pos = kenshi_gui::res::Pos::Px([-10000.0, 0.0, panel.x, panel.y]);
                }
                let b = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
                for r in &b.roots {
                    commands.entity(*r).insert(GlobalZIndex(6));
                }
                if let Some(t) = b.get("SpeechText") {
                    commands.queue(SetCaption(t, text));
                }
                Bubble::Shown(b, panel)
            }
            shown => shown,
        };
        if let Bubble::Shown(b, panel) = &bubble {
            // the caption in the bubble's font
            if let Some(cap) = b.get("SpeechText").and_then(|e| captions.get(e).ok())
                && let Ok(mut f) = fonts.get_mut(cap.0)
            {
                let want = font();
                if f.font_size != want.font_size {
                    *f = want;
                }
            }
            let p = &game.sim.folk[who];
            let head = Vec3::from(p.mover.position_units()) - origin + Vec3::Y * 22.0;
            if let Ok(mut n) = nodes.get_mut(b.roots[0]) {
                match cam.world_to_viewport(at, head) {
                    Ok(v) => {
                        n.display = Display::Flex;
                        n.left = Val::Px(v.x - panel.x * 0.5);
                        n.top = Val::Px(v.y - panel.y);
                        lift.insert(who, panel.y);
                    }
                    Err(_) => n.display = Display::None,
                }
            }
        }
        keep.insert(who, (bubble, until));
    }
    speech.by = keep;
    speech.lift = lift;
}
