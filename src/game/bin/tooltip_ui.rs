//! Kenshi's tooltips (the ToolTip classes, F048674 - F048706), for the widgets with a tip
//! (`ItemTip`): a box of lines, each a left text (word-wrapped) and a right one, Kenshi_
//! WordWrapEmpty edit boxes (F048673) in the standard font and text colour, laid down the box
//! from its top padding with a small gap between (F048699), the box as high as its lines and the
//! padding again (F048667). Three kinds show them:
//!
//! - the game's tooltip (ToolTip, F042096: the GUI's, in Kenshi_GenericTextBoxSkin), a quarter
//!   of the screen wide, shown once the mouse rests on the widget half a second (MyGUI's
//!   ToolTipManager), its bottom right corner at the mouse (F048667: at the mouse where that
//!   would leave the screen), and staying there while the mouse moves on the widget (F048650:
//!   Move ignored);
//! - the main panel's (ToolTipFixed, F043077 / F048680), in its TooltipPanel over the
//!   portraits: the panel, as wide as the layout has it, grows upward from its bottom to fit the
//!   lines, never lower than the layout's (F048669);
//! - an inventory item's (ToolTipInventory, F042779 / F048681: the Kenshi_FloatingPanelLight
//!   frame, 0.17 of the screen wide, F048670), shown as soon as the mouse moves over the item,
//!   beside it at its top, right of it unless that leaves the screen, else left (F048702).
//!
//! Line margins: the line's left and right inset 0.025 of the box's width (0.04 the inventory's)
//! at the UI scale; the top padding round(0.1 of the box's first height, 0.2 of the screen) and
//! the gap round(0.01 of it) (F048674, F048681).
//!
//! The tip's text (`ItemTip`, `lines`): a plain tip's its text (type 0, F048700); a key and
//! value list's (type 1, F048701: a datapanel line's, F047093) and an item's (`item_tip`) its
//! lines "label\tvalue\tcolour", the colours the game's colour strings (`hud_ui::tag`).
use super::hud_ui;
use super::inventory_ui::ItemTip;
use bevy::prelude::*;
use kenshi_gui::GuiRes;

/// MyGUI's ToolTipManager: the mouse rests this long (seconds) on a widget before its tip shows
/// (its default delay, 0.5).
const DELAY: f32 = 0.5;
/// The tooltip text's colour (Kenshi_WordWrapEmpty's client, Kenshi_TextboxStandardText).
const TEXT: Color = Color::srgb(0.0784314, 0.0313726, 0.0235294);
/// The tooltips' layer ("ToolTip") over the windows.
const LAYER: i32 = 300;

/// An inventory item's tip (ToolTipInventory): shown at once, beside the item.
#[derive(Component)]
pub struct InventoryTip;

/// Which tooltip shows a widget's tip.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Generic,
    Fixed,
    Inventory,
}

/// The tip under the mouse and the one shown.
#[derive(Default)]
pub struct State {
    /// The widget with a tip under the mouse, where the mouse was, and how long it rested.
    over: Option<Entity>,
    mouse: Vec2,
    rest: f32,
    /// The tip shown: its widget, its root, its text, its kind, and whether it is placed yet.
    shown: Option<(Entity, Entity, String, Kind, bool)>,
}

/// The tooltip's lines inside `parent` (F048673): a row each, its left text growing and
/// wrapping, its right text at the right. A tip's text: a line "label\tvalue\tcolour" a row (the
/// colour: empty, both in the text colour; v, the value #afa68b; c, both #afa68b; g / r, the
/// value #a8b774 / #59231a; t, both #492620; h, the label #492620 and the value #afa68b; k, the
/// label #afa68b; #rrggbb, both in that colour), a
/// line without a tab a paragraph, its MyGUI colour tags ("#rrggbb", "##" a #) colouring what
/// follows them, on into the next paragraphs.
fn lines(commands: &mut Commands, parent: Entity, text: &str, font: &TextFont) {
    let text_node = |s: &str, c: Color, j: Justify| {
        (Text::new(s.to_string()), font.clone(), TextColor(c), TextLayout::new(j, bevy::text::LineBreak::WordBoundary))
    };
    let mut colour = TEXT;
    for line in text.lines() {
        let row = match line.split('\t').collect::<Vec<_>>().as_slice() {
            [label, value, c] => {
                let (lc, vc) = match *c {
                    "v" => (TEXT, hud_ui::TAN),
                    "c" => (hud_ui::TAN, hud_ui::TAN),
                    "g" => (TEXT, hud_ui::GREEN),
                    "r" => (TEXT, hud_ui::RED),
                    "t" => (hud_ui::DARK_RED, hud_ui::DARK_RED),
                    "h" => (hud_ui::DARK_RED, hud_ui::TAN),
                    "k" => (hud_ui::TAN, TEXT),
                    // a colour of its own ("#rrggbb", an item's grade line)
                    c if c.len() == 7 && c.starts_with('#') && u32::from_str_radix(&c[1..], 16).is_ok() => {
                        let v = hud_ui::tag(u32::from_str_radix(&c[1..], 16).unwrap_or(0));
                        (v, v)
                    }
                    _ => (TEXT, TEXT),
                };
                let row = commands
                    .spawn(Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, ..default() })
                    .id();
                let l = commands.spawn((text_node(label, lc, Justify::Left), Node { flex_shrink: 1.0, ..default() })).id();
                commands.entity(row).add_child(l);
                if !value.is_empty() {
                    let v = commands.spawn((text_node(value, vc, Justify::Right), Node { flex_shrink: 0.0, ..default() })).id();
                    commands.entity(row).add_child(v);
                }
                row
            }
            _ => {
                let parts = coloured(line, &mut colour);
                let (first, c) = parts.first().cloned().unwrap_or_default();
                let e = commands.spawn((text_node(&first, c, Justify::Left), Node { width: Val::Percent(100.0), ..default() })).id();
                for (t, c) in parts.into_iter().skip(1) {
                    let span = commands.spawn((TextSpan::new(t), font.clone(), TextColor(c))).id();
                    commands.entity(e).add_child(span);
                }
                e
            }
        };
        commands.entity(parent).add_child(row);
    }
}

/// A paragraph's runs by MyGUI's colour tags ("#rrggbb" sets the colour from there on, "##" is a
/// #), starting in `colour` and leaving it as the paragraph ends it.
fn coloured(line: &str, colour: &mut Color) -> Vec<(String, Color)> {
    let mut out: Vec<(String, Color)> = vec![(String::new(), *colour)];
    let b = line.as_bytes();
    let mut i = 0;
    while i < line.len() {
        if b[i] == b'#' {
            if b.get(i + 1) == Some(&b'#') {
                out.last_mut().unwrap().0.push('#');
                i += 2;
                continue;
            }
            if let Some(hex) = line.get(i + 1..i + 7)
                && let Ok(v) = u32::from_str_radix(hex, 16)
                && hex.bytes().all(|c| c.is_ascii_hexdigit())
            {
                *colour = hud_ui::tag(v);
                out.push((String::new(), *colour));
                i += 7;
                continue;
            }
        }
        let ch = line[i..].chars().next().unwrap();
        out.last_mut().unwrap().0.push(ch);
        i += ch.len_utf8();
    }
    out.retain(|(t, _)| !t.is_empty());
    if out.is_empty() {
        out.push((String::new(), *colour));
    }
    out
}

/// @fn F048667
///
/// The tooltips: the tip of the widget under the mouse shown by its kind (module doc), hidden
/// when the mouse leaves the widget or a button is held (MyGUI hides it while the mouse is
/// captured).
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut st: Local<State>,
    tips: Query<(Entity, &Interaction, &ItemTip, Has<InventoryTip>)>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    gui: Res<GuiRes>,
    assets: Res<AssetServer>,
    hud: Option<Res<hud_ui::Hud>>,
    parents: Query<&ChildOf>,
    mut nodes: Query<(&mut Node, &ComputedNode, &UiGlobalTransform, &mut Visibility)>,
) {
    let Some(window) = windows.iter().next() else { return };
    let screen = Vec2::new(window.width(), window.height());
    // the tip went with an unloaded game
    if st.shown.as_ref().is_some_and(|s| nodes.get(s.1).is_err()) {
        st.shown = None;
    }
    let mut cursor = window.cursor_position();
    let mut hovered = tips.iter().find(|(_, i, _, _)| **i == Interaction::Hovered).map(|(e, _, t, inv)| (e, t.0.clone(), inv));
    // (testing) KENSHI_TIP=<text>: the tip of the first widget whose tip has that text, at once,
    // as if the mouse rested at the window's middle
    let testing = std::env::var("KENSHI_TIP").ok();
    if let Some(want) = &testing {
        hovered = hovered.or_else(|| tips.iter().find(|(_, _, t, _)| t.0.contains(want.as_str())).map(|(e, _, t, inv)| (e, t.0.clone(), inv)));
        cursor = cursor.or(Some(screen * 0.5));
    }
    let hide = |commands: &mut Commands, st: &mut State| {
        if let Some((_, root, ..)) = st.shown.take() {
            commands.entity(root).try_despawn();
        }
    };
    let (Some((over, text, inventory)), Some(at)) = (hovered, cursor) else {
        hide(&mut commands, &mut st);
        st.over = None;
        return;
    };
    // a held button captures the mouse: no tip
    if mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right) {
        hide(&mut commands, &mut st);
        st.over = Some(over);
        st.rest = 0.0;
        st.mouse = at;
        return;
    }
    let kind = if inventory {
        Kind::Inventory
    } else if hud.as_ref().is_some_and(|h| {
        let roots = h.roots();
        roots.contains(&over) || parents.iter_ancestors(over).any(|a| roots.contains(&a))
    }) {
        Kind::Fixed
    } else {
        Kind::Generic
    };
    if st.over != Some(over) {
        hide(&mut commands, &mut st);
        st.over = Some(over);
        st.rest = 0.0;
        st.mouse = at;
    } else if st.shown.is_none() && at != st.mouse {
        // the mouse moving starts the wait again
        st.rest = 0.0;
        st.mouse = at;
    } else {
        st.rest += time.delta_secs();
    }
    // its text changed while shown: made again in place
    if st.shown.as_ref().is_some_and(|s| s.2 != text) {
        hide(&mut commands, &mut st);
        st.rest = DELAY;
    }
    let due = kind == Kind::Inventory || st.rest >= DELAY || testing.is_some();
    if st.shown.is_none() && due {
        let font_def = gui.0.fonts.get("Kenshi_StandardFont_Medium");
        let font = TextFont {
            font: font_def.map_or_else(Default::default, |f| bevy::text::FontSource::Handle(assets.load(f.file.clone()))),
            font_size: bevy::text::FontSize::Px(font_def.map_or(14.0, |f| f.screen_px())),
            ..default()
        };
        let scale = kenshi_gui::res::ui_scale();
        // the box's first height (0.2 of the screen): its padding and line gap
        let first_h = screen.y * 0.2;
        let pad = (first_h * 0.1).round();
        let gap = (first_h * 0.01).round();
        // a skin behind a node's lines: a node of its own over the whole of it (Bevy draws a
        // node's picture inside its padding)
        let backdrop = |commands: &mut Commands, parent: Entity, skin: &str, inset: f32| {
            if let Some(img) = kenshi_gui::sliced(&gui.0, &assets, skin) {
                let b = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(inset),
                            top: Val::Px(inset),
                            right: Val::Px(inset),
                            bottom: Val::Px(inset),
                            ..default()
                        },
                        img,
                        Pickable::IGNORE,
                    ))
                    .id();
                commands.entity(parent).add_child(b);
            }
        };
        let column = |left: Val, top: Val, w: f32, side: f32| Node {
            position_type: PositionType::Absolute,
            left,
            top,
            width: Val::Px(w),
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(Val::Px(side), Val::Px(side), Val::Px(pad), Val::Px(pad)),
            row_gap: Val::Px(gap),
            ..default()
        };
        let root = match kind {
            Kind::Generic => {
                let w = (screen.x * 0.25).floor();
                let root = commands.spawn((column(Val::Px(at.x), Val::Px(at.y), w, w * scale * 0.025), GlobalZIndex(LAYER), Visibility::Hidden, Pickable::IGNORE)).id();
                backdrop(&mut commands, root, "Kenshi_GenericTextBoxSkin", 0.0);
                lines(&mut commands, root, &text, &font);
                root
            }
            Kind::Fixed => {
                // the TooltipPanel's place in the main panel (its layout, of the screen)
                let p = [0.319792, 0.712963, 0.208333, 0.0666667];
                let (x, bottom, w, h) = (p[0] * screen.x, (p[1] + p[3]) * screen.y, p[2] * screen.x, p[3] * screen.y);
                let root = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x),
                            bottom: Val::Px(screen.y - bottom),
                            width: Val::Px(w),
                            min_height: Val::Px(h),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        GlobalZIndex(LAYER),
                        Pickable::IGNORE,
                    ))
                    .id();
                backdrop(&mut commands, root, "Kenshi_FloatingPanelSkin", 0.0);
                // its box: the layout's child, 0.972222 of the panel's height from its top
                let boxed = commands
                    .spawn(Node {
                        position_type: PositionType::Relative,
                        min_height: Val::Px(h * 0.972222),
                        margin: UiRect::bottom(Val::Px(h * (1.0 - 0.972222))),
                        ..column(Val::Auto, Val::Auto, w, w * scale * 0.025)
                    })
                    .id();
                commands.entity(root).add_child(boxed);
                backdrop(&mut commands, boxed, "Kenshi_GenericTextBoxSkin", 0.0);
                lines(&mut commands, boxed, &text, &font);
                root
            }
            Kind::Inventory => {
                let w = (screen.x * 0.17).floor();
                let root = commands.spawn((column(Val::Px(at.x), Val::Px(at.y), w, w * scale * 0.04), GlobalZIndex(LAYER), Visibility::Hidden, Pickable::IGNORE)).id();
                // Kenshi_FloatingPanelLight: the frame, the generic box 5 in from it
                backdrop(&mut commands, root, "Kenshi_FloatingPanelSkin", 0.0);
                backdrop(&mut commands, root, "Kenshi_GenericTextBoxSkin", 5.0);
                lines(&mut commands, root, &text, &font);
                root
            }
        };
        st.shown = Some((over, root, text, kind, kind == Kind::Fixed));
    }
    // placed once laid out (its size known): the game's tooltip with its bottom right at the
    // mouse, an inventory item's beside the item
    if let Some((_, root, _, kind, placed)) = st.shown.as_mut()
        && !*placed
    {
        let item = match *kind {
            Kind::Inventory => nodes.get(over).ok().map(|(_, c, t, _)| {
                let s = c.size() * c.inverse_scale_factor();
                let centre = t.translation * c.inverse_scale_factor();
                Rect::from_center_size(centre, s)
            }),
            _ => None,
        };
        if let Ok((mut n, c, _, mut v)) = nodes.get_mut(*root)
            && c.size().y > 0.0
        {
            let s = c.size() * c.inverse_scale_factor();
            let (x, y) = match (*kind, item) {
                (Kind::Inventory, Some(r)) => (if r.max.x + s.x > screen.x { r.min.x - s.x } else { r.max.x }, r.min.y),
                _ => {
                    let x = if at.x - s.x < 0.0 { at.x } else { at.x - s.x };
                    let y = if at.y - s.y < 0.0 { at.y } else { at.y - s.y };
                    (x, y)
                }
            };
            n.left = Val::Px(x);
            n.top = Val::Px(y);
            *v = Visibility::Inherited;
            *placed = true;
        }
    }
}
