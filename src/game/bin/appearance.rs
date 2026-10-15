//! A race's appearance sliders (its "editor limits" file, e.g. editor_data_human.xml) and how
//! Kenshi rolls and reads them (port_map `char`):
//!
//! Spec:
//! - F021088 (the file read): each Range's min, max, mid (else the middle), its "random_group"
//!   (-1 none) and its spread, round((max - min) x the deviation): "random_variation" / 100 when
//!   it gives one, else GLOBAL CONSTANTS "appearance random deviation percentage" x 0.5
//!   (F046688); a Range with a "target" (and "target_opposite") is a face morph;
//! - F021064 / F021066 (a category rolled, the editor's RANDOMISE and a new NPC's look, F038266
//!   -> F021092 Body, F021095 Face): each slider round(spread x (2r - 1) + mid), r uniform 0..1,
//!   one r shared by the sliders of a random group;
//! - F021065 (a morph slider's value v as the mesh's poses): the target's weight v x 0.01; with an
//!   opposite, a negative v is the opposite's weight -v x 0.01 instead (the other pose 0).
//!
//! Body files (.bod2) keep the same map: slider values by name ("Height" 110) and pose weights
//! by pose name ("wide_nose" 0.3).
use super::Game;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One slider of a category.
#[derive(Debug, Clone, PartialEq)]
pub struct Slider {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub mid: f32,
    /// The morph poses it moves (a face slider), the second for negative values.
    pub target: Option<String>,
    pub opposite: Option<String>,
    /// "random_variation" (percent), "random_group" (-1 none).
    pub variation: Option<f32>,
    pub group: i32,
}

/// A race's sliders for one sex: its Face and Body categories.
#[derive(Debug, Clone, Default)]
pub struct Limits {
    pub face: Vec<Slider>,
    pub body: Vec<Slider>,
}

static CACHE: Mutex<Option<HashMap<(String, bool), Option<Arc<Limits>>>>> = Mutex::new(None);

/// The sliders of race `race` (a RACE record's name or string id) for the sex, its "editor
/// limits" file read once.
pub fn limits(game: &Game, race: &str, female: bool) -> Option<Arc<Limits>> {
    let key = (race.to_string(), female);
    if let Some(l) = CACHE.lock().ok()?.get_or_insert_with(HashMap::new).get(&key) {
        return l.clone();
    }
    let r = game.sim.data.get(race).or_else(|| game.sim.data.of_type(kenshi_data::ItemType::Race).find(|r| r.name == race));
    let l = r.and_then(|r| r.files.get("editor limits")).and_then(|f| game.res.find(f)).and_then(|f| std::fs::read_to_string(game.root.join(f)).ok()).map(|text| Arc::new(parse(&text, female)));
    CACHE.lock().ok()?.get_or_insert_with(HashMap::new).insert(key, l.clone());
    l
}

/// An editor limits file's sliders for one sex.
pub fn parse(text: &str, female: bool) -> Limits {
    let doc = kenshi_gui::xml::parse(text);
    let gender = if female { "Female" } else { "Male" };
    let num = |e: &kenshi_gui::xml::Elem, k: &str| e.attr(k).and_then(|v| v.trim().parse::<f32>().ok());
    let mut out = Limits::default();
    for c in doc.descendants().into_iter().filter(|c| c.name == "Config" && c.attr("gender") == Some(gender)) {
        for cat in c.kids("Category") {
            let sliders: Vec<Slider> = cat
                .kids("Range")
                .filter_map(|r| {
                    let (min, max) = (num(r, "min")?, num(r, "max")?);
                    Some(Slider {
                        name: r.attr("name")?.to_string(),
                        min,
                        max,
                        mid: num(r, "mid").unwrap_or((min + max) * 0.5),
                        target: r.attr("target").map(str::to_string),
                        opposite: r.attr("target_opposite").map(str::to_string),
                        variation: num(r, "random_variation"),
                        group: num(r, "random_group").map_or(-1, |g| g as i32),
                    })
                })
                .collect();
            match cat.attr("name") {
                Some("Face") => out.face.extend(sliders),
                Some("Body") => out.body.extend(sliders),
                _ => {}
            }
        }
    }
    out
}

/// GLOBAL CONSTANTS "appearance random deviation percentage" x 0.5 (F046688): the spread of a
/// slider without its own "random_variation".
pub fn deviation(game: &Game) -> f32 {
    game.sim.data.get("110-gamedata.quack").and_then(|g| g.floats.get("appearance random deviation percentage").copied()).unwrap_or(0.4) * 0.5
}

/// @fn F021064
///
/// The sliders rolled (`r` uniform 0..1): each value round(spread x (2r - 1) + mid), a random
/// group's sliders on one r; written by name into `values`, a morph slider also as its poses'
/// weights (F021065).
pub fn roll(sliders: &[Slider], deviation: f32, r: &mut impl FnMut() -> f32, values: &mut HashMap<String, f32>) {
    let mut groups: HashMap<i32, f32> = HashMap::new();
    for s in sliders {
        let t = if s.group < 0 { r() } else { *groups.entry(s.group).or_insert_with(&mut *r) };
        let spread = ((s.max - s.min) * s.variation.map_or(deviation, |v| v * 0.01)).round();
        let v = (spread * (t * 2.0 - 1.0) + s.mid).round();
        values.insert(s.name.clone(), v);
        put_poses(s, v, values);
    }
}

/// @fn F021065
///
/// Morph slider `s` at `v` as its poses' weights.
pub fn put_poses(s: &Slider, v: f32, values: &mut HashMap<String, f32>) {
    let Some(target) = &s.target else { return };
    match &s.opposite {
        None => {
            values.insert(target.clone(), v * 0.01);
        }
        Some(opp) => {
            let (t, o) = if v < 0.0 { (0.0, -v * 0.01) } else { (v * 0.01, 0.0) };
            values.insert(target.clone(), t);
            values.insert(opp.clone(), o);
        }
    }
}

/// A morph slider's value from its poses' weights (a body file keeps those, not the slider).
pub fn slider_of_poses(s: &Slider, values: &HashMap<String, f32>) -> Option<f32> {
    let w = |p: &Option<String>| p.as_ref().and_then(|p| values.get(p)).copied();
    match (w(&s.target), w(&s.opposite)) {
        (None, None) => None,
        (t, o) => {
            let (t, o) = (t.unwrap_or(0.0), o.unwrap_or(0.0));
            Some(if o > 0.0 && t <= 0.0 { -o * 100.0 } else { t * 100.0 })
        }
    }
}

/// The face sliders' values filled from their poses where only the poses are kept.
pub fn fill_sliders(l: &Limits, values: &mut HashMap<String, f32>) {
    for s in &l.face {
        if !values.contains_key(&s.name)
            && let Some(v) = slider_of_poses(s, values)
        {
            values.insert(s.name.clone(), v);
        }
    }
}

/// A stable 0..1 sequence for person `id` (its look the same every time it is drawn).
pub fn seeded(id: usize, salt: u64) -> impl FnMut() -> f32 {
    let mut s = (id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ salt ^ 0x5851_f42d_4c95_7f2d;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<Root><Config gender="Male"><Category name="Face">
        <Range max="110" min="90" name="Head size" random_variation="10"/>
        <Range target="wide_cheekbones" target_opposite="narrow_cheekbones" min="-150" mid="0" max="100" name="Cheekbones"/>
        </Category><Category name="Body">
        <Range max="120" min="80" name="Height" random_group="0"/>
        <Range max="115" min="85" name="Leg length" random_group="0"/>
        </Category></Config></Root>"#;

    #[test]
    fn rolls_as_kenshi() {
        let l = parse(XML, false);
        assert_eq!(l.face.len(), 2);
        assert_eq!(l.body[0].group, 0);
        // r = 1: every slider at its middle plus its spread
        let mut v = HashMap::new();
        roll(&l.body, 0.2, &mut || 1.0, &mut v);
        assert_eq!(v["Height"], 108.0);
        assert_eq!(v["Leg length"], 106.0);
        roll(&l.face, 0.2, &mut || 0.0, &mut v);
        // Head size's own 10%: 20 x 0.1 = 2 below the middle
        assert_eq!(v["Head size"], 98.0);
        // Cheekbones 250 x 0.2 = 50 below 0: the opposite pose at 0.5
        assert_eq!(v["Cheekbones"], -50.0);
        assert_eq!(v["narrow_cheekbones"], 0.5);
        assert_eq!(v["wide_cheekbones"], 0.0);
        assert_eq!(slider_of_poses(&l.face[1], &v), Some(-50.0));
    }
}
