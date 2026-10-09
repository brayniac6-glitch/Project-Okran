//! Name tags over heads (Kenshi_CharacterNamePanel.layout, F042142) and the mouse pointer
//! (data/gui/pointer/kenshi_pointers.xml).
//!
//! A name tag: the character's name in the panel's NameText (Kenshi_TextboxStandardText_Large,
//! shadowed), coloured "NameTagSelected" (#ffffff) for the selected, "NameTagUnselected"
//! (#b3b3b3) otherwise (kenshi_colours.xml); its StealthImage shown while sneaking. The pointer:
//! Kenshi's images from Kenshi_UI.png ("arrow"; "search", the magnifying glass, over someone to
//! loot; "talk" over someone with something to say; "attk" over an enemy; "pick_item" over
//! someone to pick up), each at its "Point".
//! (assumption) The player's characters always have their tag, anyone else when the pointer is
//! on them; the tag sits 22 units over the feet; which pointer shows where follows the game's
//! default right-click orders.
use super::{Game, MainCam, pick};
use bevy::prelude::*;
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage};
use kenshi_gui::{Built, GuiRes, SetCaption, SetVisible};
use std::collections::HashMap;

/// Kenshi's pointers: name, where in Kenshi_UI.png (frame point and size), the hotspot.
const POINTERS: [(&str, [u32; 4], (u16, u16)); 6] = [
    ("arrow", [13, 11, 32, 32], (1, 1)),
    ("search", [37, 86, 34, 34], (17, 17)),
    ("talk", [47, 1, 48, 32], (15, 10)),
    ("attk", [223, 3, 32, 32], (1, 1)),
    ("pick_item", [73, 88, 32, 32], (13, 17)),
    // pic_PointerGreen: an order being given (the right button held, or just clicked)
    ("order", [223, 48, 32, 32], (1, 1)),
];

/// How long the green order pointer stays after a quick right-click (seconds).
/// (assumption) A short flash, as the game's pointer changes back once the order is given.
const ORDER_FLASH: f32 = 0.35;

#[derive(Resource, Default)]
pub struct NameTags {
    tags: HashMap<usize, Built>,
    pointer: Option<&'static str>,
}

/// Which pointer for the person under the cursor (the default right-click order's).
fn pointer_for(game: &Game, id: Option<usize>) -> &'static str {
    let Some(i) = id else { return "arrow" };
    let p = &game.sim.folk[i];
    if p.player {
        return if p.down && p.carried_by.is_none() { "pick_item" } else { "arrow" };
    }
    if p.down || p.dead {
        return "search";
    }
    let me = game.selected.first().map(|&s| &game.sim.folk[s]);
    let enemy = me.and_then(|m| m.faction.as_ref()).and_then(|f| game.sim.relations.get(f)).is_some_and(|r| r.is_enemy(p.faction.as_deref()));
    if enemy || game.sim.arena.f[i].active {
        return "attk";
    }
    if p.dialogue.is_some() || p.shop.is_some() {
        return "talk";
    }
    "arrow"
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut tags: ResMut<NameTags>,
    game: Res<Game>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    windows: Query<(Entity, &Window)>,
    cams: Query<(&Camera, &GlobalTransform), MainCam>,
    ui: Query<&Interaction>,
    mut nodes: Query<&mut Node>,
    mut colours: Query<&mut TextColor>,
    captions: Query<&kenshi_gui::ui::Caption>,
    (children, mut images): (Query<&Children>, Query<&mut ImageNode>),
    (mouse, time, mut order_until): (Res<ButtonInput<MouseButton>>, Res<Time>, Local<f32>),
    speech: Option<Res<super::speech_ui::Speech>>,
) {
    let (Ok((win_e, win)), Ok((cam, at))) = (windows.single(), cams.single()) else { return };
    let size = [win.width(), win.height()];
    let over_ui = ui.iter().any(|i| *i != Interaction::None);
    let hovered = win.cursor_position().filter(|_| !over_ui).and_then(|c| pick(cam, at, &game, c, false).or_else(|| pick(cam, at, &game, c, true)));
    // the pointer
    // over an item lying on the ground: the pick-up pointer
    let on_item = hovered.is_none()
        && win
            .cursor_position()
            .filter(|_| !over_ui)
            .and_then(|c| super::ground_at(cam, at, &game, c))
            .and_then(|q| super::ground_items::under(&game, q))
            .is_some();
    let on_box = hovered.is_none()
        && !on_item
        && win
            .cursor_position()
            .filter(|_| !over_ui)
            .and_then(|c| super::ground_at(cam, at, &game, c))
            .and_then(|q| game.sim.box_near((q + game.zone.origin).to_array(), 15.0))
            .is_some();
    // the right button held (or just let go) with people selected, over the world: the green
    // order pointer
    if mouse.pressed(MouseButton::Right) || mouse.just_released(MouseButton::Right) {
        *order_until = time.elapsed_secs() + ORDER_FLASH;
    }
    let ordering = !game.selected.is_empty() && !over_ui && time.elapsed_secs() < *order_until && (hovered.is_none() || mouse.pressed(MouseButton::Right));
    let want = if ordering {
        "order"
    } else if on_item {
        "pick_item"
    } else if on_box {
        "search"
    } else {
        pointer_for(&game, hovered)
    };
    if tags.pointer != Some(want)
        && let Some((_, r, hot)) = POINTERS.iter().find(|p| p.0 == want)
        && let Some(file) = gui.0.file("Kenshi_UI.png")
    {
        tags.pointer = Some(want);
        commands.entity(win_e).insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle: assets.load(file.to_string()),
            rect: Some(URect::new(r[0], r[1], r[0] + r[2], r[1] + r[3])),
            hotspot: *hot,
            ..default()
        })));
    }
    // the tags wanted: the player's people standing and the one under the pointer
    let origin = game.zone.origin;
    let mut want: Vec<usize> = game.sim.folk.iter().filter(|p| p.player && !p.dead && !p.away && p.carried_by.is_none()).map(|p| p.id).collect();
    if let Some(h) = hovered
        && !want.contains(&h)
    {
        want.push(h);
    }
    tags.tags.retain(|id, b| {
        let keep = want.contains(id);
        if !keep {
            for r in &b.roots {
                commands.entity(*r).despawn();
            }
        }
        keep
    });
    for &id in &want {
        let p = &game.sim.folk[id];
        let b = tags.tags.entry(id).or_insert_with(|| {
            let defs = gui.0.layout("Kenshi_CharacterNamePanel.layout");
            let b = kenshi_gui::ui::spawn(&mut commands, &gui.0, &assets, &defs, None, size);
            for r in &b.roots {
                commands.entity(*r).insert(GlobalZIndex(5));
            }
            b
        });
        let name = if p.name.is_empty() { game.sim.display_name(id) } else { p.name.clone() };
        if let Some(e) = b.get("NameText") {
            commands.queue(SetCaption(e, name));
            let selected = game.selected.contains(&id);
            let c = if selected { Color::WHITE } else { Color::srgb(0.702, 0.702, 0.702) };
            // the caption's text entity is the widget's child: colour it there
            if let Ok(cap) = captions.get(e)
                && let Ok(mut t) = colours.get_mut(cap.0)
                && t.0 != c
            {
                t.0 = c;
            }
        }
        // sneaking (F042119): the eye coloured by who sees them (kenshi_colours.xml's
        // NameTagStealthHidden / AlmostSeen / Seen) and the light they are in (F042054: frame
        // (1 - light) x 6 of the LightLevel group, sun to moon)
        let state = game.sim.stealth_state(id);
        if std::env::var("KENSHI_SNEAK_DEBUG").is_ok() && p.player {
            let kids = b.get("StealthImage").map(|e| children.get(e).map(|c| c.len()).unwrap_or(0));
            eprintln!("nametag {id}: sneaking {} state {state} eye widget {:?} children {kids:?}", p.sneaking, b.get("StealthImage"));
        }
        for (name, show) in [("StealthImage", state != 0), ("LightLevelImage", state != 0)] {
            if let Some(e) = b.get(name) {
                commands.queue(SetVisible(e, show));
            }
        }
        if state != 0 {
            let eye = match state {
                1 => Color::srgb_u8(0x40, 0x85, 0xbe),
                2 => Color::srgb_u8(0xbe, 0xb8, 0x40),
                _ => Color::srgb_u8(0xbe, 0x40, 0x4a),
            };
            if let Some(e) = b.get("StealthImage") {
                for c in children.get(e).into_iter().flat_map(|c| c.iter()) {
                    if let Ok(mut img) = images.get_mut(c)
                        && img.color != eye
                    {
                        img.color = eye;
                    }
                }
            }
            let frame = ((1.0 - game.sim.light_on(id)) * 6.0).round().clamp(0.0, 6.0);
            let rect = Rect::new(270.0 + frame * 32.0, 2.0, 302.0 + frame * 32.0, 34.0);
            if let Some(e) = b.get("LightLevelImage") {
                for c in children.get(e).into_iter().flat_map(|c| c.iter()) {
                    if let Ok(mut img) = images.get_mut(c) {
                        if img.rect != Some(rect) {
                            img.rect = Some(rect);
                        }
                        // the layout's colour for it
                        let sun = Color::srgb(0.94152, 0.805549, 0.34137);
                        if img.color != sun {
                            img.color = sun;
                        }
                    }
                }
            }
        }
        // over the head on screen
        let head = Vec3::from(p.mover.position_units()) - origin + Vec3::Y * 22.0;
        let root = b.roots[0];
        if let Ok(mut n) = nodes.get_mut(root) {
            match cam.world_to_viewport(at, head) {
                Ok(v) => {
                    let w = size[0] * 0.105729;
                    let h = size[1] * 0.0296296;
                    n.display = Display::Flex;
                    // over a speech bubble when speaking (speech_ui)
                    let lift = speech.as_ref().and_then(|s| s.lift.get(&id).copied()).unwrap_or(0.0);
                    n.left = Val::Px(v.x - w * 0.5);
                    n.top = Val::Px(v.y - h - lift);
                }
                Err(_) => n.display = Display::None,
            }
        }
    }
}
