//! MyGUI layouts drawn with Bevy UI: each widget a node placed by its position (pixels or
//! fractions of its parent) and align, its skin's parts cut from the skin texture and laid out by
//! their own align, template skins expanded (children going into the template's "Client"), and
//! captions in the layout's TrueType fonts. Buttons change their parts and text colour by state.
use crate::res::{Align, Gui, PartKind, Pos, Span, WidgetDef};
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource, LineBreak};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Resource, Clone)]
pub struct GuiRes(pub Arc<Gui>);

/// A widget from a layout.
#[derive(Component, Debug, Clone)]
pub struct GuiWidget {
    pub name: String,
    pub kind: String,
    pub user: Vec<(String, String)>,
}

/// A widget's caption text entity.
#[derive(Component, Debug, Clone, Copy)]
pub struct Caption(pub Entity);

/// How a widget's parts and text look in each state (normal, highlighted, pushed, disabled).
#[derive(Component, Debug, Clone, Default)]
pub struct Looks {
    parts: Vec<(Entity, Vec<(String, Rect)>)>,
    text: Option<StateColours>,
}

/// A text entity and its colour per state.
type StateColours = (Entity, Vec<(String, [f32; 3])>);

/// A button held down (MyGUI's StateSelected, a tick box ticked): its skin's "_checked"
/// states are shown.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Checked(pub bool);

/// A widget that does not react (MyGUI's Enabled false).
#[derive(Component, Debug, Clone, Copy)]
pub struct Disabled;

/// A template's named parts (a window's "Caption", a scroll bar's "Track").
#[derive(Component, Debug, Clone, Default)]
pub struct Parts(pub HashMap<String, Entity>);

/// A scroll bar: its position within 0 ..= range.
#[derive(Component, Debug, Clone)]
pub struct Scroll {
    pub range: f32,
    pub pos: f32,
    track: Option<Entity>,
    place: Option<(Entity, f32)>,
    track_w: f32,
    /// The template's "VerticalAlignment": the bar runs down, not across.
    vertical: bool,
}

/// An edit box's text, and whether it takes the keyboard; `max` its "MaxTextLength" (MyGUI's
/// default 2048 when unset).
#[derive(Component, Debug, Clone, Default)]
pub struct EditText {
    pub text: String,
    pub focused: bool,
    pub max: usize,
}

/// A scroll bar moved.
#[derive(Message, Debug, Clone)]
pub struct GuiScroll {
    pub widget: Entity,
    pub pos: f32,
}

/// An edit box's text changed.
#[derive(Message, Debug, Clone)]
pub struct GuiEdit {
    pub widget: Entity,
    pub text: String,
}

/// A button pressed.
#[derive(Message, Debug, Clone)]
pub struct GuiClick {
    pub widget: Entity,
    pub name: String,
}

/// What spawning a layout made: its root and the named widgets.
#[derive(Debug, Default, Clone)]
pub struct Built {
    pub roots: Vec<Entity>,
    pub named: Vec<(String, Entity)>,
    /// Each widget's rect (pixels, in its parent).
    pub sizes: HashMap<Entity, [f32; 2]>,
}

impl Built {
    pub fn get(&self, name: &str) -> Option<Entity> {
        self.named.iter().find(|(n, _)| n == name).map(|(_, e)| *e)
    }
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Entity> + 'a {
        self.named.iter().filter(move |(n, _)| n == name).map(|(_, e)| *e)
    }
}

const LAYERS: [&str; 14] = [
    "Wallpaper", "Back", "Overlapped", "Dialog", "Middle", "MiddleFront", "Modal", "Main", "Window", "Popup",
    "FadeMiddle", "Info", "ToolTip", "Pointer",
];

fn rect(r: [f32; 4]) -> Rect {
    Rect::new(r[0], r[1], r[0] + r[2], r[1] + r[3])
}

fn node(r: [f32; 4]) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(r[0]),
        top: Val::Px(r[1]),
        width: Val::Px(r[2].max(0.0)),
        height: Val::Px(r[3].max(0.0)),
        ..default()
    }
}

struct Ctx<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    gui: &'a Gui,
    assets: &'a AssetServer,
    built: Built,
    /// The alpha of the widget being built into (MyGUI's real alpha, inherited).
    alpha: f32,
}

/// Spawn layout widgets inside `parent` (or as UI roots) whose size is `size` pixels.
pub fn spawn(commands: &mut Commands, gui: &Gui, assets: &AssetServer, defs: &[WidgetDef], parent: Option<Entity>, size: [f32; 2]) -> Built {
    spawn_design(commands, gui, assets, defs, parent, size, size)
}

/// [`spawn`] with the widgets' pixel positions made for a parent `design` pixels, fitted (by
/// their align) to one `size` pixels: a template laid into a place of another size.
pub fn spawn_design(
    commands: &mut Commands,
    gui: &Gui,
    assets: &AssetServer,
    defs: &[WidgetDef],
    parent: Option<Entity>,
    design: [f32; 2],
    size: [f32; 2],
) -> Built {
    if parent.is_none() {
        crate::res::set_ui_scale(size);
    }
    let mut ctx = Ctx { commands, gui, assets, built: Built::default(), alpha: 1.0 };
    for d in defs {
        let e = widget(&mut ctx, d, &[], parent, design, size, None);
        if parent.is_none()
            && let Some(l) = d.layer.as_deref().and_then(|l| LAYERS.iter().position(|x| *x == l))
        {
            ctx.commands.entity(e).insert(GlobalZIndex(l as i32 * 10));
        }
        ctx.built.roots.push(e);
    }
    ctx.built
}

fn text_align(s: &str) -> (JustifyContent, AlignItems, Justify) {
    let a = Align::parse(s);
    let h = if s.split_whitespace().any(|t| t == "Center" || t == "HCenter") {
        Span::Centre
    } else {
        a.h
    };
    let v = if s.split_whitespace().any(|t| t == "Center" || t == "VCenter") {
        Span::Centre
    } else {
        a.v
    };
    let jc = match h {
        Span::End => JustifyContent::FlexEnd,
        Span::Centre => JustifyContent::Center,
        _ => JustifyContent::FlexStart,
    };
    let ai = match v {
        Span::End => AlignItems::FlexEnd,
        Span::Centre => AlignItems::Center,
        _ => AlignItems::FlexStart,
    };
    let j = match h {
        Span::End => Justify::Right,
        Span::Centre => Justify::Center,
        _ => Justify::Left,
    };
    (jc, ai, j)
}

fn floats(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

/// One widget and everything below it. `extra` are properties a template's user sets on its
/// parts (the widget's caption onto the template's "Caption").
fn widget(
    ctx: &mut Ctx,
    def: &WidgetDef,
    extra: &[(String, String)],
    parent: Option<Entity>,
    design: [f32; 2],
    actual: [f32; 2],
    template_of: Option<&WidgetDef>,
) -> Entity {
    let r = match def.pos {
        Pos::Real(p) => [p[0] * actual[0], p[1] * actual[1], p[2] * actual[0], p[3] * actual[1]],
        Pos::Px(p) => def.align.fit(p, design, actual),
    };
    let size = [r[2], r[3]];
    let tpl = ctx.gui.templates.get(&def.skin);
    let skin_name = tpl.map_or(def.skin.as_str(), |t| t.skin.as_str());
    // properties: the template root's, then the widget's, then the caller's
    let mut props: Vec<(String, String)> = tpl.map(|t| t.props.clone()).unwrap_or_default();
    props.extend(def.props.iter().cloned());
    props.extend(extra.iter().cloned());
    let prop = |k: &str| props.iter().rev().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    let target = tpl
        .and_then(|t| t.user.iter().find(|(k, _)| k == "LE_TargetWidgetType").map(|(_, v)| v.clone()))
        .unwrap_or_else(|| def.kind.clone());
    let mut n = node(r);
    if prop("Visible").as_deref() == Some("false") {
        n.display = Display::None;
    }
    let e = ctx
        .commands
        .spawn((
            n,
            GuiWidget { name: def.name.clone(), kind: target.clone(), user: def.user.clone() },
        ))
        .id();
    if let Some(p) = parent {
        ctx.commands.entity(p).add_child(e);
    }
    let _ = template_of;
    if !def.name.is_empty() {
        ctx.built.named.push((def.name.clone(), e));
    }
    ctx.built.sizes.insert(e, size);
    let button = target == "Button" || def.kind == "Button";
    if button {
        ctx.commands.entity(e).insert((Button, Interaction::default()));
        if prop("Enabled").as_deref() == Some("false") {
            ctx.commands.entity(e).insert(Disabled);
        }
    }
    // the skin's parts
    let mut looks = Looks::default();
    let mut text_rect = [0.0, 0.0, size[0], size[1]];
    let mut text_colours: Vec<(String, [f32; 3])> = Vec::new();
    let mut main_rect = None;
    // MyGUI's alpha (Widget::_updateAlpha): the widget's "Alpha" times its parent's real alpha
    // (unless "InheritsAlpha" is false), on its skin, its text, its picture and all it holds
    // (the jobs panel's "[No jobs assigned]" at 0.5; the Esc menu's backdrop at 0.75, its
    // children not inheriting it)
    let parent_alpha = ctx.alpha;
    let alpha = prop("Alpha").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0)
        * if prop("InheritsAlpha").as_deref() == Some("false") { 1.0 } else { parent_alpha };
    // MyGUI's "Colour" (r g b, 0 .. 1) tints the widget's skin, not its text (the Esc menu's
    // backdrop: WhiteSkin coloured black)
    let tint = {
        let c: Vec<f32> = prop("Colour").map(|v| v.split_whitespace().filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
        let (r, g, b) = if c.len() >= 3 { (c[0], c[1], c[2]) } else { (1.0, 1.0, 1.0) };
        Color::srgba(r, g, b, alpha)
    };
    if let Some(skin) = ctx.gui.skins.get(skin_name) {
        let tex = skin.texture.as_ref().map(|t| ctx.assets.load::<Image>(t.clone()));
        for part in &skin.parts {
            let pr = part.align.fit(part.offset, skin.size, size);
            match part.kind {
                PartKind::Image | PartKind::Tile => {
                    let (Some(tex), Some(st)) = (&tex, part.state("normal").and_then(|s| s.rect)) else { continue };
                    let mode = if part.kind == PartKind::Tile {
                        NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1.0 }
                    } else {
                        NodeImageMode::Stretch
                    };
                    let img = ctx
                        .commands
                        .spawn((node(pr), ImageNode { image: tex.clone(), rect: Some(rect(st)), image_mode: mode, color: tint, ..default() }))
                        .id();
                    ctx.commands.entity(e).add_child(img);
                    let states = part.states.iter().filter_map(|(n, s)| Some((n.clone(), rect(s.rect?)))).collect();
                    looks.parts.push((img, states));
                }
                PartKind::Text => {
                    text_rect = pr;
                    text_colours = part.states.iter().filter_map(|(n, s)| Some((n.clone(), s.colour?))).collect();
                }
                // the widget's own picture fills it
                PartKind::Main => main_rect = Some([0.0, 0.0, size[0], size[1]]),
            }
        }
    }
    // an image box's picture
    if def.kind == "ImageBox" {
        let mr = main_rect.unwrap_or([0.0, 0.0, size[0], size[1]]);
        let picture = if let Some(t) = prop("ImageTexture").and_then(|t| ctx.gui.file(&t).map(str::to_string)) {
            Some((t, None))
        } else if let Some(res) = prop("ImageResource") {
            ctx.gui.images.get(&res).and_then(|groups| {
                let g = prop("ImageGroup");
                let (_, group) = groups.iter().find(|(n, _)| Some(n) == g.as_ref()).or_else(|| groups.first())?;
                let name = prop("ImageName");
                let at = group.frames.iter().find(|(n, _)| Some(n) == name.as_ref()).map_or([0.0, 0.0], |f| f.1);
                Some((group.texture.clone(), Some([at[0], at[1], group.size[0], group.size[1]])))
            })
        } else {
            None
        };
        if let Some((tex, sub)) = picture {
            let img = ctx
                .commands
                .spawn((
                    node(mr),
                    ImageNode { image: ctx.assets.load(tex), rect: sub.map(rect), image_mode: NodeImageMode::Stretch, color: Color::srgba(1.0, 1.0, 1.0, alpha), ..default() },
                ))
                .id();
            ctx.commands.entity(e).add_child(img);
        }
    }
    // the caption (a button whose skin is all picture, with no text part, shows none: the map's
    // centre button's "O")
    let caption = prop("Caption");
    let picture_button = target == "Button" && ctx.gui.skins.get(skin_name).is_some_and(|s| !s.parts.is_empty() && !s.parts.iter().any(|p| p.kind == PartKind::Text));
    let texty = !picture_button && (matches!(target.as_str(), "TextBox" | "EditBox" | "Button") || caption.is_some());
    if texty {
        // MyGUI's default font (core_settings.xml) when the widget names none
        let font = prop("FontName")
            .and_then(|f| ctx.gui.fonts.get(&f).cloned())
            .or_else(|| ctx.gui.fonts.get("Kenshi_StandardFont_Medium").cloned());
        // an edit box writes with its client's text (MyGUI's EditBox: its "Client" part's text, the
        // TextBox the template puts there): the client's text colour and shadow unless the edit
        // box sets its own ("[No jobs assigned]" in Kenshi_TextboxStandardText's dark brown)
        let gui = ctx.gui;
        let own = |k: &str| def.props.iter().chain(extra.iter()).rev().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        let client = if target == "EditBox" { tpl.and_then(|t| t.children.iter().find(|c| c.name == "Client")) } else { None };
        let client_prop = |k: &str| client.and_then(|c| c.prop(k).map(str::to_string).or_else(|| gui.templates.get(&c.skin).and_then(|t| t.prop(k)).map(str::to_string)));
        let text_prop = |k: &str| own(k).or_else(|| client_prop(k)).or_else(|| prop(k));
        let client_skin_colour = client
            .and_then(|c| gui.skins.get(gui.templates.get(&c.skin).map_or(c.skin.as_str(), |t| t.skin.as_str())))
            .and_then(|s| s.parts.iter().find(|p| p.kind == PartKind::Text))
            .and_then(|p| p.state("normal"))
            .and_then(|s| s.colour);
        let colour = text_prop("TextColour")
            .map(|c| floats(&c))
            .filter(|v| v.len() >= 3)
            .map(|v| [v[0], v[1], v[2]])
            .or(client_skin_colour)
            .or_else(|| text_colours.iter().find(|(n, _)| n == "normal").map(|(_, c)| *c))
            .unwrap_or([1.0, 1.0, 1.0]);
        let (jc, ai, j) = text_align(&prop("TextAlign").unwrap_or_default());
        let wrap = if prop("WordWrap").as_deref() == Some("true") { LineBreak::WordBoundary } else { LineBreak::NoWrap };
        let holder = ctx
            .commands
            .spawn(Node { justify_content: jc, align_items: ai, ..node(text_rect) })
            .id();
        let t = ctx
            .commands
            .spawn((
                Text::new(caption.unwrap_or_default().replace("\\n", "\n")),
                TextFont {
                    font: font.as_ref().map_or_else(FontSource::default, |f| FontSource::Handle(ctx.assets.load(f.file.clone()))),
                    font_size: FontSize::Px(font.as_ref().map_or(14.0 * crate::res::ui_scale(), |f| f.screen_px())),
                    ..default()
                },
                TextColor(Color::srgba(colour[0], colour[1], colour[2], alpha)),
                TextLayout::new(j, wrap),
            ))
            .id();
        // MyGUI's text shadow ("TextShadow", "TextShadowColour", black unless set): the text again
        // a pixel right and down, under it
        if text_prop("TextShadow").as_deref() == Some("true") {
            let c = text_prop("TextShadowColour").map(|c| floats(&c)).filter(|v| v.len() >= 3).unwrap_or(vec![0.0, 0.0, 0.0]);
            ctx.commands.entity(t).insert(TextShadow { offset: Vec2::new(1.0, 1.0), color: Color::srgba(c[0], c[1], c[2], alpha) });
        }
        ctx.commands.entity(holder).add_child(t);
        ctx.commands.entity(e).add_child(holder);
        ctx.commands.entity(e).insert(Caption(t));
        if !text_colours.is_empty() && text_prop("TextColour").is_none() {
            looks.text = Some((t, text_colours));
        }
    }
    if button {
        ctx.commands.entity(e).insert(looks);
    }
    if def.kind == "EditBox" && prop("Static").as_deref() != Some("true") && prop("ReadOnly").as_deref() != Some("true") {
        ctx.commands.entity(e).insert((
            EditText {
                text: prop("Caption").unwrap_or_default(),
                focused: false,
                max: prop("MaxTextLength").and_then(|m| m.parse().ok()).unwrap_or(2048),
            },
            Interaction::default(),
            bevy::ui::RelativeCursorPosition::default(),
        ));
    }
    // the template's own parts, then the widget's children in its client area, under its alpha
    ctx.alpha = alpha;
    let mut client = (e, size);
    if let Some(t) = tpl {
        let tdesign = match t.pos {
            Pos::Px(p) => [p[2], p[3]],
            Pos::Real(_) => size,
        };
        let caption_prop: Vec<(String, String)> = def.prop("Caption").map(|c| vec![("Caption".to_string(), c.to_string())]).unwrap_or_default();
        let mut parts = Parts::default();
        for c in &t.children {
            let before = ctx.built.named.len();
            let ce = widget(ctx, c, &[], Some(e), tdesign, size, Some(t));
            // the template's named parts are its own, not the layout's
            let mut found = None;
            for (n, w) in ctx.built.named.drain(before..).collect::<Vec<_>>() {
                parts.0.entry(n.clone()).or_insert(w);
                if n == "Client" {
                    found = Some(w);
                } else if n == "Caption" && !caption_prop.is_empty() {
                    // the widget's caption onto the template's caption box
                    if let Some(sz) = ctx.built.sizes.get(&w).copied() {
                        let _ = sz;
                    }
                    ctx.built.named.push((format!("{}.Caption", def.name), w));
                }
            }
            let _ = ce;
            if let Some(w) = found {
                client = (w, ctx.built.sizes.get(&w).copied().unwrap_or(size));
            }
        }
        if !caption_prop.is_empty()
            && let Some(cap) = ctx.built.get(&format!("{}.Caption", def.name))
        {
            ctx.commands.queue(SetCaption(cap, caption_prop[0].1.clone()));
        }
        // a scroll view's and an item box's bars show only while their canvas is larger than
        // their view (MyGUI's ScrollViewBase): hidden here until their user shows them
        if target == "ScrollView" || target == "ItemBox" {
            for bar in ["VScroll", "HScroll"] {
                if let Some(&b) = parts.0.get(bar) {
                    ctx.commands.entity(b).insert(Visibility::Hidden);
                }
            }
        }
        // a scroll bar: its track place takes the mouse, its track (the thumb) shows the position
        if target == "ScrollBar" {
            let range = prop("Range").and_then(|r| r.parse().ok()).unwrap_or(100.0);
            let place = parts.0.get("TrackPlace").copied();
            if let Some(pl) = place {
                ctx.commands.entity(pl).insert((Interaction::default(), bevy::ui::RelativeCursorPosition::default()));
            }
            let track = parts.0.get("Track").copied();
            let vertical = t.prop("VerticalAlignment") == Some("true");
            let axis = vertical as usize;
            ctx.commands.entity(e).insert(Scroll {
                range,
                pos: 0.0,
                track,
                place: place.map(|p| (p, ctx.built.sizes.get(&p).map_or(0.0, |s| s[axis]))),
                track_w: track.and_then(|t| ctx.built.sizes.get(&t)).map_or(0.0, |s| s[axis]),
                vertical,
            });
        }
        // a progress bar ("TrackFill"): its track, in the "TrackSkin" skin, fills its track
        // place from the left, as far as `SetProgress` says (none at first)
        if target == "ProgressBar"
            && let Some(place) = parts.0.get("TrackPlace").copied()
            && let Some(skin) = t.user.iter().find(|(k, _)| k == "TrackSkin").and_then(|(_, v)| ctx.gui.skins.get(v))
        {
            let fill = ctx
                .commands
                .spawn(Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() })
                .id();
            ctx.commands.entity(place).add_child(fill);
            let tex = skin.texture.as_ref().map(|t| ctx.assets.load::<Image>(t.clone()));
            if let (Some(tex), Some(st)) = (tex, skin.parts.iter().find_map(|p| p.state("normal").and_then(|s| s.rect))) {
                let img = ctx
                    .commands
                    .spawn((
                        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        ImageNode { image: tex, rect: Some(rect(st)), image_mode: NodeImageMode::Stretch, color: tint, ..default() },
                    ))
                    .id();
                ctx.commands.entity(fill).add_child(img);
            }
            ctx.commands.entity(e).insert(Progress { fill });
        }
        ctx.commands.entity(e).insert(parts);
    }
    let child_design = match def.pos {
        Pos::Px(p) if client.0 == e => [p[2], p[3]],
        _ => client.1,
    };
    for c in &def.children {
        widget(ctx, c, &[], Some(client.0), child_design, client.1, None);
    }
    ctx.alpha = parent_alpha;
    e
}

/// A skin as one nine-sliced picture (Bevy's TextureSlicer), when its parts are a MyGUI frame's
/// 3 x 3 grid side by side on its texture: the corners kept at their size, the edges and the
/// middle stretched, as the parts' aligns do. A node Bevy lays out at any size can wear it (a
/// tooltip's box, sized by its text).
pub fn sliced(gui: &Gui, assets: &AssetServer, skin: &str) -> Option<ImageNode> {
    let s = gui.skins.get(skin)?;
    let tex = s.texture.as_ref()?;
    let rects: Vec<Rect> = s
        .parts
        .iter()
        .filter(|p| matches!(p.kind, PartKind::Image | PartKind::Tile))
        .filter_map(|p| p.state("normal").and_then(|st| st.rect))
        .map(rect)
        .collect();
    if rects.len() != 9 {
        return None;
    }
    let min = rects.iter().fold(Vec2::MAX, |m, r| m.min(r.min));
    let max = rects.iter().fold(Vec2::MIN, |m, r| m.max(r.max));
    let tl = rects.iter().find(|r| r.min == min)?;
    let br = rects.iter().find(|r| r.max == max)?;
    Some(ImageNode {
        image: assets.load(tex.clone()),
        rect: Some(Rect { min, max }),
        image_mode: NodeImageMode::Sliced(TextureSlicer {
            border: BorderRect::from([tl.width(), br.width(), tl.height(), br.height()]),
            center_scale_mode: SliceScaleMode::Stretch,
            sides_scale_mode: SliceScaleMode::Stretch,
            max_corner_scale: 1.0,
        }),
        ..default()
    })
}

/// Set a widget's caption (its text entity's text), once spawned.
pub struct SetCaption(pub Entity, pub String);

impl Command for SetCaption {
    type Out = ();
    fn apply(self, world: &mut World) {
        // an edit box's own text too: what is typed goes on from what it shows
        if let Some(mut e) = world.get_mut::<EditText>(self.0) {
            e.text = self.1.clone();
        }
        let Some(t) = world.get::<Caption>(self.0).map(|c| c.0) else { return };
        if let Some(mut text) = world.get_mut::<Text>(t) {
            text.0 = self.1;
        }
    }
}

/// A widget's alpha with all it holds (MyGUI Widget::setAlpha): its images' and texts' alpha set
/// to this.
pub struct SetAlpha(pub Entity, pub f32);

impl Command for SetAlpha {
    type Out = ();
    fn apply(self, world: &mut World) {
        let a = self.1.clamp(0.0, 1.0);
        let mut stack = vec![self.0];
        while let Some(e) = stack.pop() {
            if let Some(mut i) = world.get_mut::<ImageNode>(e) {
                i.color.set_alpha(a);
            }
            if let Some(mut t) = world.get_mut::<TextColor>(e) {
                t.0.set_alpha(a);
            }
            if let Some(c) = world.get::<Children>(e) {
                stack.extend(c.iter());
            }
        }
    }
}

/// MyGUI Widget::setColour: the widget's own skin tinted (not its text, nor the widgets it
/// holds), its alpha kept.
pub struct SetColour(pub Entity, pub Color);

impl Command for SetColour {
    type Out = ();
    fn apply(self, world: &mut World) {
        let parts: Vec<Entity> = world.get::<Children>(self.0).map(|c| c.iter().collect()).unwrap_or_default();
        for p in parts {
            if world.get::<GuiWidget>(p).is_some() {
                continue;
            }
            if let Some(mut i) = world.get_mut::<ImageNode>(p) {
                let a = i.color.alpha();
                i.color = self.1.with_alpha(a);
            }
        }
    }
}

/// MyGUI TextBox::setTextColour: the widget's caption in this colour, its alpha kept.
pub struct SetTextColour(pub Entity, pub Color);

impl Command for SetTextColour {
    type Out = ();
    fn apply(self, world: &mut World) {
        let Some(t) = world.get::<Caption>(self.0).map(|c| c.0) else { return };
        if let Some(mut c) = world.get_mut::<TextColor>(t) {
            let a = c.0.alpha();
            c.0 = self.1.with_alpha(a);
        }
    }
}

/// Show or hide a widget.
pub struct SetVisible(pub Entity, pub bool);

impl Command for SetVisible {
    type Out = ();
    fn apply(self, world: &mut World) {
        if let Some(mut n) = world.get_mut::<Node>(self.0) {
            n.display = if self.1 { Display::Flex } else { Display::None };
        }
    }
}

/// A button's parts and text by its state (MyGUI's skin states): disabled, pushed, highlighted
/// or normal, "_checked" when ticked; again when it is disabled or enabled.
#[allow(clippy::type_complexity)]
fn looks(
    changed: Query<Entity, (With<Looks>, Or<(Changed<Interaction>, Changed<Checked>, Added<Disabled>)>)>,
    mut enabled: RemovedComponents<Disabled>,
    q: Query<(&Interaction, &Looks, Has<Disabled>, Option<&Checked>)>,
    mut images: Query<&mut ImageNode>,
    mut colours: Query<&mut TextColor>,
) {
    let mut which: Vec<Entity> = changed.iter().collect();
    which.extend(enabled.read());
    for (i, l, off, checked) in which.into_iter().filter_map(|e| q.get(e).ok()) {
        let base = match (off, i) {
            (true, _) => "disabled",
            (_, Interaction::Pressed) => "pushed",
            (_, Interaction::Hovered) => "highlighted",
            _ => "normal",
        };
        let ticked = format!("{base}_checked");
        let state = if checked.is_some_and(|c| c.0) { ticked.as_str() } else { base };
        for (img, states) in &l.parts {
            if let (Ok(mut im), Some((_, r))) = (
                images.get_mut(*img),
                states
                    .iter()
                    .find(|(n, _)| n == state)
                    .or_else(|| states.iter().find(|(n, _)| n == base))
                    .or_else(|| states.iter().find(|(n, _)| n == "normal")),
            ) {
                im.rect = Some(*r);
            }
        }
        if let Some((t, cs)) = &l.text
            && let (Ok(mut c), Some((_, rgb))) =
                (colours.get_mut(*t), cs.iter().find(|(n, _)| n == state).or_else(|| cs.iter().find(|(n, _)| n == "normal")))
        {
            let a = c.0.alpha();
            c.0 = Color::srgba(rgb[0], rgb[1], rgb[2], a);
        }
    }
}

#[allow(clippy::type_complexity)]
fn clicks(q: Query<(Entity, &Interaction, &GuiWidget), (Changed<Interaction>, Without<Disabled>)>, mut out: MessageWriter<GuiClick>) {
    for (e, i, w) in &q {
        if *i == Interaction::Pressed {
            out.write(GuiClick { widget: e, name: w.name.clone() });
        }
    }
}

fn scrolls(
    mouse: Res<ButtonInput<MouseButton>>,
    mut bars: Query<(Entity, &mut Scroll)>,
    places: Query<(&bevy::ui::RelativeCursorPosition, &Interaction)>,
    mut nodes: Query<&mut Node>,
    mut clicks: MessageReader<GuiClick>,
    parents: Query<&ChildOf>,
    mut out: MessageWriter<GuiScroll>,
) {
    let arrows: Vec<(Entity, f32)> = clicks
        .read()
        .filter_map(|c| match c.name.as_str() {
            "Start" => Some((c.widget, -1.0)),
            "End" => Some((c.widget, 1.0)),
            _ => None,
        })
        .collect();
    for (e, mut s) in &mut bars {
        let mut pos = s.pos;
        // dragged along the track place (held from a press on it)
        if mouse.pressed(MouseButton::Left)
            && let Some((pl, w)) = s.place
            && let Ok((rel, i)) = places.get(pl)
            && *i == Interaction::Pressed
            && let Some(n) = rel.normalized
        {
            let usable = (w - s.track_w).max(1.0);
            let x = (if s.vertical { n.y } else { n.x } + 0.5) * w - s.track_w * 0.5;
            pos = (x / usable).clamp(0.0, 1.0) * s.range;
        }
        // the arrows: the bar's own Start and End buttons
        for &(b, d) in &arrows {
            if parents.iter_ancestors(b).any(|a| a == e) {
                pos = (pos + d).clamp(0.0, s.range);
            }
        }
        if (pos - s.pos).abs() > f32::EPSILON {
            s.pos = pos;
            out.write(GuiScroll { widget: e, pos });
        }
        // the thumb where the position puts it
        if let (Some(t), Some((_, w))) = (s.track, s.place)
            && let Ok(mut n) = nodes.get_mut(t)
        {
            let x = if s.range > 0.0 { s.pos / s.range } else { 0.0 } * (w - s.track_w).max(0.0);
            if s.vertical {
                n.top = Val::Px(x);
            } else {
                n.left = Val::Px(x);
            }
        }
    }
}

/// A progress bar's track (the fill).
#[derive(Component, Debug, Clone, Copy)]
pub struct Progress {
    fill: Entity,
}

/// Set a progress bar's fill, 0 ..= 1 of its track place.
pub struct SetProgress(pub Entity, pub f32);

impl Command for SetProgress {
    type Out = ();
    fn apply(self, world: &mut World) {
        let Some(p) = world.get::<Progress>(self.0).copied() else { return };
        if let Some(mut n) = world.get_mut::<Node>(p.fill) {
            n.width = Val::Percent(self.1.clamp(0.0, 1.0) * 100.0);
        }
    }
}

/// Set a scroll bar's position (and its thumb).
pub struct SetScroll(pub Entity, pub f32);

impl Command for SetScroll {
    type Out = ();
    fn apply(self, world: &mut World) {
        if let Some(mut s) = world.get_mut::<Scroll>(self.0) {
            s.pos = self.1.clamp(0.0, s.range);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn typing(
    mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut boxes: Query<(Entity, &Interaction, &mut EditText, Option<&Caption>, Option<&bevy::ui::RelativeCursorPosition>, &InheritedVisibility)>,
    mut texts: Query<&mut Text>,
    mut out: MessageWriter<GuiEdit>,
    time: Res<Time<Real>>,
) {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::Key;
    // a click focuses the box under it (its own skin's parts over it count as it) and
    // unfocuses the rest
    if mouse.just_pressed(MouseButton::Left) {
        for (_, i, mut t, _, rel, shown) in &mut boxes {
            t.focused = shown.get() && (*i == Interaction::Pressed || rel.is_some_and(|r| r.cursor_over()));
        }
    }
    let events: Vec<_> = keys.read().filter(|k| k.state == ButtonState::Pressed).cloned().collect();
    // MyGUI's text cursor: a bar after the text, blinking (on half a second, off half)
    let blink = time.elapsed_secs() % 1.0 < 0.5;
    for (e, _, mut t, cap, _, shown) in &mut boxes {
        // a box gone from view lets the keyboard go
        if !shown.get() {
            t.focused = false;
        }
        if t.focused {
            let before = t.text.clone();
            for k in &events {
                match &k.logical_key {
                    Key::Backspace => {
                        t.text.pop();
                    }
                    Key::Enter | Key::Escape => t.focused = false,
                    _ => {
                        if let Some(s) = &k.text {
                            let room = t.max.saturating_sub(t.text.chars().count());
                            t.text.extend(s.chars().filter(|c| !c.is_control()).take(room));
                        }
                    }
                }
            }
            if t.text != before {
                out.write(GuiEdit { widget: e, text: t.text.clone() });
            }
        }
        // what it shows: its text, and the cursor while it has the keyboard
        let want = if t.focused && blink { format!("{}|", t.text) } else { t.text.clone() };
        if let Some(c) = cap
            && let Ok(tx) = texts.get(c.0)
            && tx.0 != want
            && (t.focused || tx.0 == format!("{}|", t.text))
            && let Ok(mut tx) = texts.get_mut(c.0)
        {
            tx.0 = want;
        }
    }
}

/// Whether any edit box has the keyboard (the game's keys should then wait).
pub fn typing_now(q: &Query<&EditText>) -> bool {
    q.iter().any(|t| t.focused)
}

pub struct GuiPlugin;

impl Plugin for GuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<GuiClick>()
            .add_message::<GuiScroll>()
            .add_message::<GuiEdit>()
            .add_systems(Update, (looks, clicks, scrolls.after(clicks), typing));
    }
}
