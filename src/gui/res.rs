//! MyGUI resources as Kenshi's `data/gui` has them: skins (texture rectangles per state, laid out
//! by align), template layouts, image sets, TrueType fonts and named colours.
use crate::xml::{self, Elem};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Span {
    #[default]
    Start,
    End,
    Centre,
    Stretch,
}

/// MyGUI's Align: how a rectangle follows its parent's resize on each axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Align {
    pub h: Span,
    pub v: Span,
}

impl Align {
    pub fn parse(s: &str) -> Align {
        let mut a = Align::default();
        for t in s.split_whitespace() {
            match t {
                "Left" => a.h = Span::Start,
                "Right" => a.h = Span::End,
                "HCenter" => a.h = Span::Centre,
                "HStretch" => a.h = Span::Stretch,
                "Top" => a.v = Span::Start,
                "Bottom" => a.v = Span::End,
                "VCenter" => a.v = Span::Centre,
                "VStretch" => a.v = Span::Stretch,
                "Center" => {
                    a.h = Span::Centre;
                    a.v = Span::Centre;
                }
                "Stretch" => {
                    a.h = Span::Stretch;
                    a.v = Span::Stretch;
                }
                _ => {}
            }
        }
        a
    }

    /// A rectangle `r` (x, y, w, h) designed inside `design`, inside a parent now `actual` big.
    pub fn fit(&self, r: [f32; 4], design: [f32; 2], actual: [f32; 2]) -> [f32; 4] {
        let axis = |span: Span, x: f32, w: f32, d: f32, a: f32| match span {
            Span::Start => (x, w),
            Span::End => (x + a - d, w),
            Span::Centre => (x + (a - d) * 0.5, w),
            Span::Stretch => (x, (w + a - d).max(0.0)),
        };
        let (x, w) = axis(self.h, r[0], r[2], design[0], actual[0]);
        let (y, h) = axis(self.v, r[1], r[3], design[1], actual[1]);
        [x, y, w, h]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Image,
    Tile,
    Text,
    /// The widget's own image (an ImageBox's).
    Main,
}

#[derive(Debug, Clone, Default)]
pub struct PartState {
    /// The texture rectangle (x, y, w, h, pixels).
    pub rect: Option<[f32; 4]>,
    pub colour: Option<[f32; 3]>,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub kind: PartKind,
    pub offset: [f32; 4],
    pub align: Align,
    pub states: Vec<(String, PartState)>,
}

impl Part {
    /// A state's look, falling back to "normal".
    pub fn state(&self, name: &str) -> Option<&PartState> {
        self.states
            .iter()
            .find(|(n, _)| n == name)
            .or_else(|| self.states.iter().find(|(n, _)| n == "normal"))
            .map(|(_, s)| s)
    }
}

#[derive(Debug, Clone)]
pub struct Skin {
    pub size: [f32; 2],
    /// The texture's path under the install.
    pub texture: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone)]
pub enum Pos {
    /// Pixels in the parent.
    Px([f32; 4]),
    /// Fractions of the parent.
    Real([f32; 4]),
}

/// A widget as a layout or template describes it.
#[derive(Debug, Clone)]
pub struct WidgetDef {
    pub kind: String,
    pub skin: String,
    pub pos: Pos,
    pub align: Align,
    pub name: String,
    pub layer: Option<String>,
    pub props: Vec<(String, String)>,
    pub user: Vec<(String, String)>,
    pub children: Vec<WidgetDef>,
}

impl WidgetDef {
    pub fn prop(&self, k: &str) -> Option<&str> {
        self.props.iter().rev().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    fn from(e: &Elem) -> WidgetDef {
        let nums = |k: &str| -> Option<[f32; 4]> {
            let v: Vec<f32> = e.attr(k)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (v.len() >= 4).then(|| [v[0], v[1], v[2], v[3]])
        };
        let pos = nums("position_real")
            .map(Pos::Real)
            .or_else(|| nums("position").map(Pos::Px))
            .unwrap_or(Pos::Real([0.0, 0.0, 1.0, 1.0]));
        WidgetDef {
            kind: e.attr("type").unwrap_or("Widget").to_string(),
            skin: e.attr("skin").unwrap_or("").to_string(),
            pos,
            align: Align::parse(e.attr("align").unwrap_or("")),
            name: e.attr("name").unwrap_or("").to_string(),
            layer: e.attr("layer").map(str::to_string),
            props: e
                .kids("Property")
                .filter_map(|p| Some((p.attr("key")?.to_string(), p.attr("value").unwrap_or("").to_string())))
                .collect(),
            user: e
                .kids("UserString")
                .filter_map(|p| Some((p.attr("key")?.to_string(), p.attr("value").unwrap_or("").to_string())))
                .collect(),
            children: e.kids("Widget").map(WidgetDef::from).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FontDef {
    /// The TrueType file's path under the install.
    pub file: String,
    /// The font's "Size" (points).
    pub px: f32,
}

/// The UI scale (F042088): MyGUI's ScaleManager set to the smaller of screen width / 1920 and
/// screen height / 1080 (the size of Kenshi_UI.png, the layouts' design size). Kept as f32 bits.
static UI_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

/// Set the UI scale from the window's size (pixels).
pub fn set_ui_scale(window: [f32; 2]) {
    let s = (window[0] / 1920.0).min(window[1] / 1080.0).max(0.1);
    UI_SCALE.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub fn ui_scale() -> f32 {
    f32::from_bits(UI_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

impl FontDef {
    /// The glyph height on screen: FreeType's size in points at the fonts' "Resolution" (50
    /// dpi, kenshi_fonts.xml), size * 50 / 72 pixels, at the UI scale ([`ui_scale`]).
    pub fn screen_px(&self) -> f32 {
        self.px * 50.0 / 72.0 * ui_scale()
    }
}

#[derive(Debug, Clone)]
pub struct ImageGroup {
    pub texture: String,
    pub size: [f32; 2],
    /// Index name -> first frame's top-left.
    pub frames: Vec<(String, [f32; 2])>,
}

/// Every resource a layout may name.
#[derive(Debug, Default)]
pub struct Gui {
    pub skins: HashMap<String, Skin>,
    pub templates: HashMap<String, WidgetDef>,
    pub fonts: HashMap<String, FontDef>,
    pub images: HashMap<String, Vec<(String, ImageGroup)>>,
    pub colours: HashMap<String, [f32; 3]>,
    /// The `data/gui` files by lower-case name (paths under the install).
    files: HashMap<String, String>,
    pub root: PathBuf,
}

fn floats(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

fn colour(s: &str) -> Option<[f32; 3]> {
    let v = floats(s);
    if v.len() >= 3 {
        return Some([v[0], v[1], v[2]]);
    }
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6).then(|| {
        let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
        [c(0), c(2), c(4)]
    })
}

impl Gui {
    /// Read `data/gui` of an install.
    pub fn load(install: &Path) -> Gui {
        let mut g = Gui { root: install.to_path_buf(), ..Default::default() };
        let base = install.join("data/gui");
        let mut stack = vec![base.clone()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let (Some(n), Ok(rel)) = (p.file_name(), p.strip_prefix(install)) {
                    g.files
                        .entry(n.to_string_lossy().to_lowercase())
                        .or_insert_with(|| rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        // the enabled mods' files over the game's (a mod's layout, skin texture or font by name)
        for dir in kenshi_io::mods::Install::new(install).mod_dirs() {
            let mut stack = vec![dir];
            while let Some(d) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if let (Some(n), Ok(rel)) = (p.file_name(), p.strip_prefix(install)) {
                        g.files.insert(n.to_string_lossy().to_lowercase(), rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
        for dir in ["core", "skins", "colours", "images", "templates", "fonts"] {
            let Ok(rd) = std::fs::read_dir(base.join(dir)) else { continue };
            let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "xml")).collect();
            files.sort();
            for f in files {
                if let Ok(t) = std::fs::read_to_string(&f) {
                    g.read(&xml::parse(&t));
                }
            }
        }
        g
    }

    /// A gui file's path under the install, by name.
    pub fn file(&self, name: &str) -> Option<&str> {
        self.files.get(&name.to_lowercase()).map(String::as_str)
    }

    /// A layout file's top-level widgets.
    pub fn layout(&self, name: &str) -> Vec<WidgetDef> {
        let Some(p) = self.file(name) else { return Vec::new() };
        let Ok(t) = std::fs::read_to_string(self.root.join(p)) else { return Vec::new() };
        let doc = xml::parse(&t);
        doc.children
            .iter()
            .flat_map(|m| m.kids("Widget"))
            .map(WidgetDef::from)
            .collect()
    }

    fn read(&mut self, doc: &Elem) {
        for e in doc.descendants() {
            match (e.name.as_str(), e.attr("type")) {
                ("Resource", Some("ResourceSkin")) => {
                    let Some(name) = e.attr("name") else { continue };
                    let size = floats(e.attr("size").unwrap_or("0 0"));
                    let texture = e.attr("texture").and_then(|t| self.file(t)).map(str::to_string);
                    let parts = e
                        .kids("BasisSkin")
                        .filter_map(|b| {
                            let kind = match b.attr("type")? {
                                "SubSkin" => PartKind::Image,
                                "TileRect" => PartKind::Tile,
                                "SimpleText" | "EditText" => PartKind::Text,
                                "MainSkin" => PartKind::Main,
                                _ => return None,
                            };
                            let o = floats(b.attr("offset").unwrap_or(""));
                            let offset = if o.len() >= 4 { [o[0], o[1], o[2], o[3]] } else { [0.0, 0.0, size.first().copied().unwrap_or(0.0), size.get(1).copied().unwrap_or(0.0)] };
                            let states = b
                                .kids("State")
                                .map(|s| {
                                    let r = floats(s.attr("offset").unwrap_or(""));
                                    (
                                        s.attr("name").unwrap_or("normal").to_string(),
                                        PartState {
                                            rect: (r.len() >= 4).then(|| [r[0], r[1], r[2], r[3]]),
                                            colour: s.attr("colour").and_then(colour),
                                        },
                                    )
                                })
                                .collect();
                            Some(Part { kind, offset, align: Align::parse(b.attr("align").unwrap_or("")), states })
                        })
                        .collect();
                    self.skins.insert(
                        name.to_string(),
                        Skin { size: [size.first().copied().unwrap_or(0.0), size.get(1).copied().unwrap_or(0.0)], texture, parts },
                    );
                }
                ("Resource", Some("ResourceLayout")) => {
                    if let (Some(name), Some(w)) = (e.attr("name"), e.kids("Widget").next()) {
                        self.templates.insert(name.to_string(), WidgetDef::from(w));
                    }
                }
                ("Resource", Some("ResourceTrueTypeFont")) => {
                    let Some(name) = e.attr("name") else { continue };
                    let prop = |k: &str| e.kids("Property").find(|p| p.attr("key") == Some(k)).and_then(|p| p.attr("value"));
                    let Some(file) = prop("Source").and_then(|s| self.file(s)).map(str::to_string) else { continue };
                    let size: f32 = prop("Size").and_then(|v| v.parse().ok()).unwrap_or(16.0);
                    self.fonts.insert(name.to_string(), FontDef { file, px: size });
                }
                ("Resource", Some("ResourceImageSet")) => {
                    let Some(name) = e.attr("name") else { continue };
                    let groups = e
                        .kids("Group")
                        .filter_map(|gr| {
                            let texture = self.file(gr.attr("texture")?)?.to_string();
                            let s = floats(gr.attr("size").unwrap_or("0 0"));
                            let frames = gr
                                .kids("Index")
                                .filter_map(|i| {
                                    let p = floats(i.kids("Frame").next()?.attr("point").unwrap_or("0 0"));
                                    Some((i.attr("name")?.to_string(), [p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0)]))
                                })
                                .collect();
                            Some((
                                gr.attr("name").unwrap_or("").to_string(),
                                ImageGroup { texture, size: [s.first().copied().unwrap_or(0.0), s.get(1).copied().unwrap_or(0.0)], frames },
                            ))
                        })
                        .collect();
                    self.images.insert(name.to_string(), groups);
                }
                ("Property", _) if e.attr("key").is_some_and(|k| k.starts_with("Kenshi") || k.contains("Colour")) => {}
                _ => {}
            }
            // named colours (kenshi_colours.xml: Property key / value under a Colour document)
            if e.name == "MyGUI" && e.attr("type") == Some("Colour") {
                for p in e.kids("Property") {
                    if let (Some(k), Some(v)) = (p.attr("key"), p.attr("value").and_then(colour)) {
                        self.colours.insert(k.to_string(), v);
                    }
                }
            }
            // named colours: <Colour name="..." value="..."/> style entries
            if e.name == "Colour"
                && let (Some(n), Some(v)) = (e.attr("name"), e.attr("value").and_then(colour))
            {
                self.colours.insert(n.to_string(), v);
            }
        }
    }
}
