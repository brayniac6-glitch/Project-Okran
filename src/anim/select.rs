//! Choosing animations: the ANIMATION records' conditions (combat or stealth mode, weapons in
//! hand, carrying, leg injuries, speed) matched against a character's situation, weighted, and
//! normalised per layer.
//!
//! Specs: sheet rows F038934 AnimationData::AnimationData, F037153 AnimationClass::candidates,
//! F037148 AnimationData::handsMatch, F037145 AnimationData::speedWeight, F037142 rangeWeight;
//! type T00048 AnimationData.

use kenshi_data::GameData;

/// A 0/1/2 condition: 0 must not, 1 must, 2 either.
pub type Tri = i32;

fn tri(v: i32) -> Tri {
    if v == 1 {
        1
    } else if v != 0 {
        2
    } else {
        0
    }
}

/// @type AnimationData
///
/// The fields of an ANIMATION record the original keeps (offsets in comments).
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationData {
    /// +0x00: which kind of character it is for (matched against the situation's category).
    pub category: i32,
    /// +0x08 record name, +0x30 the clip ("anim name").
    pub name: String,
    pub anim_name: String,
    /// +0x60: layer (0 lower, 1 upper (default), 2 overlay, 3 tail, 4 ears).
    pub layer: i32,
    /// +0x8b: the whole body ("all").
    pub all: bool,
    /// +0x64
    pub weather_type: i32,
    /// +0x68 / +0x6c / +0x70: "move speed", "max speed", "min speed"; +0x74 "play speed".
    pub move_speed: f32,
    pub max_speed: f32,
    pub min_speed: f32,
    pub play_speed: f32,
    /// +0x78 / +0x7c: "has weapon L" / "R"; +0x80 "is combat mode"; +0x84 "stealth mode".
    pub weapon_l: Tri,
    pub weapon_r: Tri,
    pub combat_mode: Tri,
    pub stealth_mode: Tri,
    /// +0x88: "relocates": the clip's root motion moves the character.
    pub relocates: bool,
    /// +0x89 "disables movement", +0x8a "loop", +0x8d "is action".
    pub disables_movement: bool,
    pub looping: bool,
    pub is_action: bool,
    /// +0x8c "synchs", +0x90 "synch offset", +0x94 "normalise", +0x98 "possible ending".
    pub synchs: bool,
    pub synch_offset: f32,
    pub normalise: bool,
    pub possible_ending: f32,
    /// +0x9c "crouched", +0x9d "prone".
    pub crouched: bool,
    pub prone: bool,
    /// +0x9e / +0x9f: uses the right / left arm (or holds a weapon in it).
    pub uses_right_arm: bool,
    pub uses_left_arm: bool,
    /// +0xa0 "carrying left", +0xa1 "carrying right", +0xa2 "being carried".
    pub carrying_left: bool,
    pub carrying_right: bool,
    pub being_carried: bool,
    /// +0xa8 "slave anim".
    pub slave_anim: String,
    /// +0xd0..+0xe4: leg damage ranges: right (min, max, ideal), left (min, max, ideal).
    pub r_leg: [f32; 3],
    pub l_leg: [f32; 3],
    /// +0xe8 "idle time min", +0xec range to "idle time max", +0xf0 "idle chance" * 0.01.
    pub idle_min: f32,
    pub idle_range: f32,
    pub idle_chance: f32,
    /// +0x100: weapon categories it plays with, by WeaponCategory bit (katanas 1, sabres 2,
    /// blunt and one-handed 4, heavy 8, hackers 0x10, unarmed 0x20, crossbow 0x40, polearm 0x100).
    pub weapons: u32,
    /// +0x108: the body parts it is a stumble for ("stumbles").
    pub stumbles: Vec<String>,
    /// +0x148 "stumble from", +0x14c "big stumble", +0x150 "chance" * 0.01.
    pub stumble_from: i32,
    pub big_stumble: bool,
    pub chance: f32,
}

impl AnimationData {
    /// @fn F038934
    ///
    /// From an ANIMATION record (the COMBAT_TECHNIQUE path is the technique module's).
    pub fn from_record(r: &GameData) -> AnimationData {
        let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
        let i = |k: &str| r.ints.get(k).copied().unwrap_or(0);
        let f = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
        let s = |k: &str| r.strings.get(k).cloned().unwrap_or_default();
        let layer_s = s("layer");
        let layer = match layer_s.as_str() {
            "lower" => 0,
            "overlay" => 2,
            "ears" => 4,
            "tail" => 3,
            _ => 1,
        };
        let mut weapons = 0;
        for (k, bit) in [
            ("katanas", 1),
            ("sabre", 2),
            ("1 handed", 4),
            ("heavy weapons", 8),
            ("blunt", 4),
            ("hackers", 0x10),
            ("unarmed", 0x20),
            ("polearm", 0x100),
            ("crossbow", 0x40),
        ] {
            if b(k) {
                weapons |= bit;
            }
        }
        let weapon_l = tri(i("has weapon L"));
        let weapon_r = tri(i("has weapon R"));
        let mut r_leg = [
            f("R leg damage min"),
            f("R leg damage max"),
            f("R leg damage ideal"),
        ];
        let mut l_leg = [
            f("L leg damage min"),
            f("L leg damage max"),
            f("L leg damage ideal"),
        ];
        for leg in [&mut r_leg, &mut l_leg] {
            if 99.0 < leg[1] {
                leg[1] = 101.0;
            }
            if leg[0] < 1.0 {
                leg[0] = -101.0;
            }
        }
        let idle_min = i("idle time min") as f32;
        AnimationData {
            category: i("category"),
            name: r.name.clone(),
            anim_name: s("anim name"),
            layer,
            all: layer_s == "all",
            weather_type: i("weather type"),
            move_speed: f("move speed"),
            max_speed: f("max speed"),
            min_speed: f("min speed"),
            play_speed: f("play speed"),
            weapon_l,
            weapon_r,
            combat_mode: tri(i("is combat mode")),
            stealth_mode: tri(i("stealth mode")),
            relocates: b("relocates"),
            disables_movement: b("disables movement"),
            looping: b("loop"),
            is_action: b("is action"),
            synchs: b("synchs"),
            synch_offset: f("synch offset"),
            normalise: b("normalise"),
            possible_ending: f("possible ending"),
            crouched: b("crouched"),
            prone: b("prone"),
            uses_right_arm: b("uses right arm") || weapon_r != 0,
            uses_left_arm: b("uses left arm") || weapon_l != 0,
            carrying_left: b("carrying left"),
            carrying_right: b("carrying right"),
            being_carried: b("being carried"),
            slave_anim: s("slave anim"),
            r_leg,
            l_leg,
            idle_min,
            idle_range: i("idle time max") as f32 - idle_min,
            idle_chance: (i("idle chance") as f64 * 0.01) as f32,
            weapons,
            stumbles: r.refs("stumbles").iter().map(|(s, _)| s.clone()).collect(),
            stumble_from: i("stumble from"),
            big_stumble: b("big stumble"),
            chance: (i("chance") as f64 * 0.01) as f32,
        }
    }
}

/// @fn F037142
///
/// How well `x` fits a range: 1 between `min` and `ideal` (rising from `min` when that is at
/// least 1), falling to 0 from `ideal` to `max`; anything below -40 at both ends counts as fitting.
pub fn range_weight(x: f32, min: f32, max: f32, ideal: f32) -> f32 {
    if x < -40.0 && min < -40.0 {
        return 1.0;
    }
    let mut f = if x < min || ideal < x {
        if ideal <= x && (x <= max || max < 0.5) {
            1.0 - (x - ideal) / (max - ideal)
        } else {
            0.0
        }
    } else if 1.0 <= min {
        (x - min) / (ideal - min)
    } else {
        1.0
    };
    f = f.clamp(0.0, 1.0);
    f
}

/// @fn F037145
///
/// How well an animation fits the character's speed (scaled by its size): the range weight, then
/// a steep sigmoid around one half.
pub fn speed_weight(a: &AnimationData, speed: f32, size: f32) -> f32 {
    let s = range_weight(
        speed,
        size * a.min_speed,
        size * a.max_speed,
        size * a.move_speed,
    );
    if 0.0 < s {
        let e = (-((s * 2.0 - 1.0) * 5.0 * 2.0)).exp();
        1.0 / (e + 1.0)
    } else {
        s
    }
}

/// What a character is doing, as animation choice reads it (AnimationClass +0xf0 onward).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Situation {
    /// The animations' category this character uses.
    pub category: i32,
    /// Movement speed.
    pub speed: f32,
    /// Weapons in the left / right hand.
    pub weapon_l: bool,
    pub weapon_r: bool,
    /// Legs are not considered (e.g. no legs to speak of).
    pub ignore_legs: bool,
    /// Carrying in the left / right arm, being carried.
    pub carrying_left: bool,
    pub carrying_right: bool,
    pub being_carried: bool,
    /// Combat mode and stealth mode (0 / 1).
    pub combat_mode: i32,
    pub stealth_mode: i32,
    /// Right and left leg health (+0x160, +0x164 from the class).
    pub r_leg: f32,
    pub l_leg: f32,
    /// The weapon category in use (5 = unarmed).
    pub weapon_category: u32,
}

/// @fn F037148
///
/// Do the animation's weapon-in-hand conditions fit (upper and overlay layers only)? Both 'must'
/// means exactly one hand holds one.
pub fn hands_match(a: &AnimationData, s: &Situation) -> bool {
    if a.layer != 1 && a.layer != 2 {
        return true;
    }
    if a.weapon_l == 1 && a.weapon_r == 1 {
        return s.weapon_l != s.weapon_r;
    }
    let ok = |cond: Tri, held: bool| if held { cond != 0 } else { cond != 1 };
    ok(a.weapon_l, s.weapon_l) && ok(a.weapon_r, s.weapon_r)
}

/// @fn F037153
///
/// The animations from `from` that suit the situation, with weights normalised within each layer.
/// `strict` also requires the carrying flags and the weapon category (upper and overlay layers).
pub fn candidates<'a>(
    from: impl IntoIterator<Item = &'a AnimationData>,
    s: &Situation,
    size: f32,
    strict: bool,
) -> Vec<(&'a AnimationData, f32)> {
    let mut out: Vec<(&AnimationData, f32)> = Vec::new();
    for a in from {
        if a.category != s.category
            || !(a.combat_mode == 2 || a.combat_mode == s.combat_mode)
            || !(a.stealth_mode == 2 || a.stealth_mode == s.stealth_mode)
            || !hands_match(a, s)
        {
            continue;
        }
        let mut w = speed_weight(a, s.speed, size);
        let leg = if s.ignore_legs {
            1.0
        } else if s.r_leg <= s.l_leg {
            // the worse leg against its own range
            range_weight(s.r_leg, a.r_leg[0], a.r_leg[1], a.r_leg[2])
        } else {
            range_weight(s.l_leg, a.l_leg[0], a.l_leg[1], a.l_leg[2])
        };
        w *= leg;
        if strict {
            let body = a.layer == 1 || a.layer == 2;
            let carry = if body
                && (s.being_carried != a.being_carried
                    || s.carrying_left != a.carrying_left
                    || s.carrying_right != a.carrying_right)
            {
                0.0
            } else {
                1.0
            };
            let weapon = if !body {
                1.0
            } else if s.weapon_category == 5 {
                if a.weapon_l == 1 || a.weapon_r == 1 {
                    0.1
                } else {
                    1.0
                }
            } else if (a.weapons >> (s.weapon_category & 31)) & 1 == 0 {
                0.0
            } else {
                1.0
            };
            w *= carry * weapon;
        }
        if 0.0 < w {
            out.push((a, w));
        }
    }
    // normalise within each layer (0..3)
    let mut sums = [0.0f32; 4];
    for (a, w) in &out {
        if let Some(t) = sums.get_mut(a.layer as usize) {
            *t += w;
        }
    }
    for t in &mut sums {
        if 0.0 < *t {
            *t = 1.0 / *t;
        }
    }
    for (a, w) in &mut out {
        if let Some(k) = sums.get(a.layer as usize) {
            *w *= k;
        }
    }
    out
}

/// @fn F046685
///
/// The bones whose tracks an ANIMATION record's "delete ..." flags strip from its clip.
pub fn deleted_bones(r: &GameData) -> Vec<&'static str> {
    const GROUPS: [(&str, &[&str]); 8] = [
        ("delete root", &["Bip01"]),
        (
            "delete L arm",
            &[
                "Bip01 L Clavicle",
                "Bip01 L UpperArm",
                "Bip01 L Forearm",
                "Bip01 L Hand",
            ],
        ),
        (
            "delete R arm",
            &[
                "Bip01 R Clavicle",
                "Bip01 R UpperArm",
                "Bip01 R Forearm",
                "Bip01 R Hand",
            ],
        ),
        ("delete below waist", BELOW_WAIST),
        ("delete spine0", &["Bip01 Spine"]),
        (
            "delete body head",
            &[
                "Bip01 Spine",
                "Bip01 Spine1",
                "Bip01 Spine2",
                "Bip01 Neck",
                "Bip01 Head",
            ],
        ),
        ("delete weapons", &["Bip01 Prop2", "Bip01 Prop1"]),
        (
            "delete tail",
            &[
                "Bip01 Tail",
                "Bip01 Tail1",
                "Bip01 Tail2",
                "Bip01 Tail3",
                "Bip01 Tail4",
            ],
        ),
    ];
    GROUPS
        .iter()
        .filter(|(k, _)| r.bools.get(*k).copied().unwrap_or(false))
        .flat_map(|(_, b)| b.iter().copied())
        .collect()
}

/// @fn F046682
///
/// The bones a block's clip loses (blocks only move the upper body): the root, pelvis and legs.
pub const BELOW_WAIST: &[&str] = &[
    "Bip01",
    "Bip01 Pelvis",
    "Bip01 R Thigh",
    "Bip01 L Thigh",
    "Bip01 R Calf",
    "Bip01 L Calf",
    "Bip01 R Foot",
    "Bip01 L Foot",
    "Bip01 R Toe0",
    "Bip01 L Toe0",
];

/// @fn F046683
///
/// The bones a clip takes over completely ("override head" / "override L arm" / "override R
/// arm"): other clips leave those bones to it.
pub fn override_bones(r: &GameData) -> Vec<&'static str> {
    let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
    let mut out = Vec::new();
    if b("override L arm") {
        out.extend([
            "Bip01 L UpperArm",
            "Bip01 L Forearm",
            "Bip01 L Hand",
            "Bip01 L Clavicle",
            "Bip01 Prop1",
        ]);
    }
    if b("override R arm") {
        out.extend([
            "Bip01 R UpperArm",
            "Bip01 R Forearm",
            "Bip01 R Hand",
            "Bip01 R Clavicle",
            "Bip01 Prop2",
        ]);
    }
    if b("override head") {
        out.extend(["Bip01 Neck", "Bip01 Head"]);
    }
    out
}

/// @fn F040458
///
/// The pain postures of a hurt body: for each part below its pain threshold (the "pain anim"
/// reference's value, percent), that animation, the most hurt part first. `parts` are (health
/// fraction, pain animation, threshold).
// a negated comparison: a NaN threshold or health includes the part, as the original
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn pain_postures<'a>(parts: impl IntoIterator<Item = (f32, &'a str, i32)>) -> Vec<&'a str> {
    let mut hurt: Vec<(f32, &str)> = parts
        .into_iter()
        .filter(|(frac, _, threshold)| !((*threshold as f32 * 0.01) <= *frac))
        .map(|(frac, anim, _)| (frac, anim))
        .collect();
    hurt.sort_by(|a, b| a.0.total_cmp(&b.0));
    hurt.into_iter().map(|(_, a)| a).collect()
}

/// @fn F037181
///
/// Whether a human plays its pain posture: the normal animation category and moving no faster
/// than 50 (the posture is then the most hurt part's, at its play speed on its layer).
pub fn shows_pain(category: i32, speed: f32) -> bool {
    category == 0 && speed <= 50.0
}

/// The animations a character class knows, sorted as the original's animation class keeps them.
#[derive(Debug, Clone, Default)]
pub struct Library {
    /// Movement on the lower layer or the whole body (+0x00) and on the upper layer (+0x18),
    /// each by move speed.
    pub upper_moves: Vec<AnimationData>,
    pub lower_moves: Vec<AnimationData>,
    /// "idle" animations (+0x30).
    pub idles: Vec<AnimationData>,
    /// Strafing (+0x48).
    pub strafes: Vec<AnimationData>,
    /// Everything else by record name (+0x78; overlays only here), and by weather type (+0x140).
    pub other: Vec<AnimationData>,
    pub weather: Vec<AnimationData>,
    /// An animal's "turning anim" (+0xf8), its play speed times the reference's first value
    /// x 0.01 (F038936): what it plays turning on the spot (F037138).
    pub turning: Option<AnimationData>,
}

impl Library {
    /// @fn F038936
    ///
    /// An ANIMAL_CHARACTER's library: its "animations" sorted (F038935); its "turning anim"
    /// kept apart, its play speed times the reference's value x 0.01; its "strafe anim" taken as
    /// its strafe when the animations have none; both for either stealth mode.
    pub fn animal(data: &kenshi_data::GameDataContainer, a: &GameData) -> Library {
        let mut l = Library::sort(a.refs("animations").iter().filter_map(|(sid, _)| data.get(sid)));
        l.turning = a.refs("turning anim").first().and_then(|(sid, v)| {
            let mut t = AnimationData::from_record(data.get(sid)?);
            t.play_speed *= v[0] as f32 * 0.01;
            t.stealth_mode = 2;
            Some(t)
        });
        if l.strafes.is_empty()
            && let Some(mut s) = a.refs("strafe anim").first().and_then(|(sid, _)| data.get(sid)).map(AnimationData::from_record)
        {
            s.stealth_mode = 2;
            l.strafes.push(s);
        }
        l
    }
}

impl Library {
    /// @fn F038935
    ///
    /// Sort ANIMATION records: disabled ones skipped; weather animations aside; on any layer but
    /// overlay: idles, movement (non-zero move speed; upper layer apart), strafing (strafe speed
    /// max > 0), else by name; overlays by name.
    pub fn sort<'a>(records: impl IntoIterator<Item = &'a GameData>) -> Library {
        let mut l = Library::default();
        for r in records {
            if r.bools.get("disabled").copied().unwrap_or(false) {
                continue;
            }
            let a = AnimationData::from_record(r);
            let f = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
            if 0 < a.weather_type {
                l.weather.push(a);
            } else if r.strings.get("layer").map(String::as_str) == Some("overlay") {
                l.other.push(a);
            } else if r.bools.get("idle").copied().unwrap_or(false) {
                l.idles.push(a);
            } else if f("move speed") != 0.0 {
                if r.strings.get("layer").map(String::as_str) == Some("upper") {
                    l.upper_moves.push(a);
                } else {
                    l.lower_moves.push(a);
                }
            } else if 0.0 < f("strafe speed max") {
                l.strafes.push(a);
            } else {
                l.other.push(a);
            }
        }
        for v in [&mut l.upper_moves, &mut l.lower_moves] {
            v.sort_by(|x, y| x.move_speed.total_cmp(&y.move_speed));
        }
        l
    }
}

/// @fn F037154
///
/// The movement animations to blend at the situation's speed: the upper-layer list strictly
/// (carrying and weapon must match), the lower-layer and whole-body list loosely, each with its
/// weight (times `weight`) and playback rate (play speed * speed / size).
pub fn movement<'a>(
    lib: &'a Library,
    s: &Situation,
    size: f32,
    weight: f32,
) -> Vec<(&'a AnimationData, f32, f32)> {
    let rate = |a: &AnimationData| (1.0 / size) * a.play_speed * s.speed;
    let mut out = Vec::new();
    for (list, strict) in [(&lib.upper_moves, true), (&lib.lower_moves, false)] {
        for (a, w) in candidates(list, s, size, strict) {
            out.push((a, rate(a), weight * w));
        }
    }
    out
}

/// @fn F037155
///
/// The idle to play standing still: of the idles that suit the situation strictly (overlays
/// left out), the best weighted (F037213).
pub fn idle<'a>(lib: &'a Library, s: &Situation, size: f32) -> Option<&'a AnimationData> {
    let mut best: Option<(&AnimationData, f32)> = None;
    for (a, w) in candidates(&lib.idles, s, size, true) {
        if a.layer == 2 {
            continue;
        }
        if best.is_none_or(|(_, bw)| bw < w) {
            best = Some((a, w));
        }
    }
    best.map(|(a, _)| a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(layer: i32, l: Tri, r: Tri) -> AnimationData {
        AnimationData {
            category: 0,
            name: String::new(),
            anim_name: String::new(),
            layer,
            all: false,
            weather_type: 0,
            move_speed: 0.0,
            max_speed: 999.0,
            min_speed: -999.0,
            play_speed: 1.0,
            weapon_l: l,
            weapon_r: r,
            combat_mode: 2,
            stealth_mode: 2,
            relocates: false,
            disables_movement: false,
            looping: false,
            is_action: false,
            synchs: false,
            synch_offset: 0.0,
            normalise: false,
            possible_ending: 1.0,
            crouched: false,
            prone: false,
            uses_right_arm: false,
            uses_left_arm: false,
            carrying_left: false,
            carrying_right: false,
            being_carried: false,
            slave_anim: String::new(),
            r_leg: [-101.0, 101.0, 100.0],
            l_leg: [-101.0, 101.0, 100.0],
            idle_min: 0.0,
            idle_range: 0.0,
            idle_chance: 0.0,
            weapons: 1,
            stumbles: vec![],
            stumble_from: 0,
            big_stumble: true,
            chance: 1.0,
        }
    }

    #[test]
    fn ranges_and_hands() {
        // healthy legs fit the default range fully
        assert_eq!(range_weight(100.0, -101.0, 101.0, 100.0), 1.0);
        // a limp animation (ideal 30, up to 60) fades out for healthier legs
        assert!((range_weight(45.0, -101.0, 60.0, 30.0) - 0.5).abs() < 1e-6);
        assert_eq!(range_weight(80.0, -101.0, 60.0, 30.0), 0.0);
        let s = Situation {
            weapon_r: true,
            ..Default::default()
        };
        assert!(hands_match(&anim(1, 2, 2), &s));
        assert!(hands_match(&anim(1, 0, 1), &s));
        assert!(!hands_match(&anim(1, 0, 0), &s));
        assert!(hands_match(&anim(1, 1, 1), &s));
        // the lower layer ignores hands
        assert!(hands_match(&anim(0, 0, 0), &s));
    }

    #[test]
    fn weights_normalise_per_layer() {
        let all = [anim(1, 2, 2), anim(1, 2, 2), anim(0, 2, 2)];
        let s = Situation {
            speed: 0.0,
            l_leg: 100.0,
            r_leg: 100.0,
            ..Default::default()
        };
        let c = candidates(&all, &s, 1.0, false);
        assert_eq!(c.len(), 3);
        assert!((c[0].1 - 0.5).abs() < 1e-6 && (c[2].1 - 1.0).abs() < 1e-6);
    }
}
