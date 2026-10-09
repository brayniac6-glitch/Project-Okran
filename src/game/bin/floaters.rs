//! Damage floaters (settings.cfg "floaters", "Show damage floaters"): the numbers that rise over a
//! character when it is hit, with what its armour stopped in brackets. Spec: F040476 (resolveHit's
//! floater), F042086 (the floater's construction), F042136 (its update).
use super::{Game, MainCam};
use bevy::prelude::*;
use kenshi_gui::GuiRes;

/// A floater on screen: whom it follows, how far it has risen (world units) and its text.
#[derive(Component)]
pub struct Floater {
    over: usize,
    rise: f32,
}

/// Rise per second (F042086: the speed table's second entry, 4).
const RISE_SPEED: f32 = 4.0;
/// Units above the character's feet it starts at (F040476: Vector3(0, 15, 0)).
const START_HEIGHT: f32 = 15.0;
/// It fades out from this rise and is gone at `GONE` (F042136: alpha 1 - (rise - 8) * 0.25).
const FADE: f32 = 8.0;
const GONE: f32 = 12.0;
/// Only hits within 4000 units of the camera show (F040476: squared distance < 16,000,000).
const RANGE: f32 = 4000.0;

/// The "floaters" option: 0 off, 1 on, 2 [Full Info]; 1 when missing.
#[derive(Resource)]
pub struct FloaterMode(pub i32);

pub fn read_mode(root: &std::path::Path) -> FloaterMode {
    let cfg = kenshi_io::cfg::text(root, "settings.cfg");
    FloaterMode(
        cfg.lines()
            .find_map(|l| l.strip_prefix("floaters=").and_then(|v| v.trim().parse().ok()))
            .unwrap_or(1),
    )
}

/// Spawn a floater for every hit the arena landed, then move, fade and remove them.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mode: Option<Res<FloaterMode>>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    mut floaters: Query<(Entity, &mut Floater, &mut Node, &mut TextColor, &ComputedNode)>,
) {
    let mode = match mode {
        Some(m) => m.0,
        None => {
            let m = read_mode(&game.root);
            let v = m.0;
            commands.insert_resource(m);
            v
        }
    };
    let hits = std::mem::take(&mut game.sim.arena.sh.hits);
    let Ok((cam, at)) = cams.single() else { return };
    let origin = game.zone.origin;
    if 0 < mode {
        let full = mode == 2;
        // (F042086) normal floaters in the medium floater font, full info in the small one
        let font_name = if full { "Kenshi_FloaterFont_Small" } else { "Kenshi_FloaterFont_Medium" };
        let font = gui.0.fonts.get(font_name);
        for (attacker, victim, part, f) in hits {
            let Some(p) = game.sim.folk.get(victim) else { continue };
            let pos = Vec3::from(p.mover.position_units()) - origin;
            if RANGE * RANGE <= pos.distance_squared(at.translation()) {
                continue;
            }
            let part_name = game.sim.arena.f[victim]
                .medical
                .parts
                .get(part)
                .map(|q| game.sim.data.get(&q.id).map_or(q.id.clone(), |r| r.name.clone()))
                .unwrap_or_default();
            // (assumption) the colour tests (F044443, F040883 on the player's faction): red when
            // the one hit is the player's, green when the player's hit someone, else light grey
            let ours = |i: usize| game.sim.folk.get(i).is_some_and(|q| q.player);
            let colour = if ours(victim) {
                Color::srgb(1.0, 0.31, 0.31)
            } else if ours(attacker) {
                Color::srgb(0.11, 0.6, 0.11)
            } else {
                Color::srgb(0.9, 0.9, 0.9)
            };
            commands.spawn((
                Text::new(f.text(full, &part_name)),
                TextFont {
                    font: font.map_or_else(Default::default, |f| bevy::text::FontSource::Handle(assets.load(f.file.clone()))),
                    font_size: bevy::text::FontSize::Px(font.map_or(18.0, |f| f.screen_px())),
                    ..default()
                },
                TextColor(colour),
                Node { position_type: PositionType::Absolute, left: Val::Px(-1000.0), ..default() },
                GlobalZIndex(40),
                Floater { over: victim, rise: 0.0 },
            ));
        }
    }
    let dt = time.delta_secs();
    for (e, mut fl, mut node, mut colour, computed) in &mut floaters {
        fl.rise += dt * RISE_SPEED;
        let Some(p) = game.sim.folk.get(fl.over).filter(|_| fl.rise < GONE) else {
            commands.entity(e).despawn();
            continue;
        };
        if FADE < fl.rise {
            colour.0.set_alpha(1.0 - (fl.rise - FADE) * 0.25);
        }
        let world = Vec3::from(p.mover.position_units()) - origin + Vec3::Y * (START_HEIGHT + fl.rise);
        match cam.world_to_viewport(at, world) {
            Ok(s) => {
                let size = computed.size() * computed.inverse_scale_factor();
                node.left = Val::Px(s.x - size.x * 0.5);
                node.top = Val::Px(s.y - size.y * 0.5);
            }
            Err(_) => node.left = Val::Px(-1000.0),
        }
    }
}
