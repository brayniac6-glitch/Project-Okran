//! Character appearance files (.bod2, CHARACTER_APPEARANCE): a CHARACTER's "body" file, the
//! character editor's look for the game's named characters (data/character/bodies): its
//! "race", "sex female", "head", "hair style", "beard", the hair's "Hair Colour" /
//! "Hair Saturation" / "Hair Brightness", "Skin Tone", every Body and Face slider by name, and
//! its "idle stance" (an ANIMATION's anim name). Read once each.

use kenshi_data::GameDataContainer;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct BodyFile {
    /// The RACE record (string id).
    pub race: Option<String>,
    pub female: bool,
    /// HEAD and hair / beard ATTACHMENT records (string ids or names as stored).
    pub head: Option<String>,
    pub hair: Option<String>,
    pub beard: Option<String>,
    /// The sliders by name (Body, Face, hair colour), as the editor keeps them.
    pub values: Vec<(String, f32)>,
    pub skin_tone: [f32; 3],
    pub idle_stance: Option<String>,
}

static FILES: Mutex<Option<HashMap<String, Option<Arc<BodyFile>>>>> = Mutex::new(None);

/// The body file of CHARACTER `character`, if it names one that reads.
pub fn body_of(install: &std::path::Path, data: &GameDataContainer, character: &str) -> Option<Arc<BodyFile>> {
    let rel = data.get(character)?.files.get("body").filter(|f| !f.is_empty())?.clone();
    let mut cache = FILES.lock().ok()?;
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(b) = cache.get(&rel) {
        return b.clone();
    }
    let path = install.join(rel.trim_start_matches(".\\").trim_start_matches("./").replace('\\', "/"));
    let b = read(&path).map(Arc::new);
    cache.insert(rel, b.clone());
    b
}

/// A .bod2 file read.
pub fn read(path: &std::path::Path) -> Option<BodyFile> {
    let f = kenshi_io::fcs::read(path).ok()?;
    let r = f.records.into_iter().next()?;
    let string = |k: &str| r.strings.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    Some(BodyFile {
        race: r.references.iter().find(|(c, _)| c == "race").and_then(|(_, v)| v.first()).map(|x| x.target.clone()),
        female: r.bools.iter().any(|(n, v)| n == "sex female" && *v),
        head: string("head"),
        hair: string("hair style"),
        beard: string("beard"),
        values: r.floats.clone(),
        skin_tone: r.vec3s.iter().find(|(n, _)| n == "Skin Tone").map_or([1.0; 3], |(_, v)| *v),
        idle_stance: string("idle stance"),
    })
}
