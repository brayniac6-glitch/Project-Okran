//! Perception (port_map `ai`): how far a character sees and whether it sees another character.
//!
//! Spec: F046475 the vision range, F046498 the visibility of a target (view cone, stance,
//! stealth, range and light), F046539 the plain sight test that uses it (a target counts as seen
//! past a visibility of 0.01), F046551 the sensor update (everyone within the vision range is
//! tested), F033833 in combat.
//!
//! The stealth detection roll for moving or fighting targets (F046538, hearing) is the game's
//! (`kenshi_game::law`, `Sim::heard`). Not ported here: the
//! squad's shared sightings (platoon +0x68), the per-target knowledge entries (F046540), and the
//! sneaking observer's narrower range (F046475's second half; the town has no sneakers).

/// Below this, a target is not seen (F046539, `_DAT_141682ad8`).
pub const SEEN_THRESHOLD: f32 = 0.01;

/// A character's stance (Character vtable +0x368): 1 crouching, 2 prone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stance {
    #[default]
    Standing = 0,
    Crouching = 1,
    Prone = 2,
}

/// The observer, as the sensor reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observer {
    /// Units.
    pub position: [f32; 3],
    /// Where its head faces (the animation object +0x448, vtable +0xc0).
    pub facing: [f32; 3],
    /// The race's "vision range mult".
    pub vision_mult: f32,
    /// Fighting (F033833): the range is held to half.
    pub in_combat: bool,
    /// Inside a building and not (vtable +0x1e0) in an open one: the short 500 range.
    pub indoors: bool,
    /// Character vtable +0x1e0 (doubles the range); not identified: false.
    pub flag_1e0: bool,
    /// Out of it (the state object +0xb4 == 1, F044458): sees nothing.
    pub out: bool,
    /// The light where it stands (Character +0xd8), 0..1.
    pub light: f32,
    /// Character vtable +0x78 is set: the light floor 0.2, else 0.4.
    pub has_78: bool,
}

/// The target, as the observer sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub position: [f32; 3],
    pub stance: Stance,
    /// Sneaking (F039177).
    pub sneaking: bool,
    /// Moving (CharMovement +0x24).
    pub moving: bool,
    /// Its stealth level, 0..1 (F047035).
    pub stealth: f32,
    pub light: f32,
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}

fn invlerp(x: f32, a: f32, b: f32) -> f32 {
    (x - a) / (b - a)
}

fn normalised(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-8 { v.map(|x| x / l) } else { v }
}

/// The sky's ramp at dawn and dusk (hours).
const TWILIGHT: f32 = 1.0 / 12.0;

/// @fn F040781
///
/// The daylight at a time of day (hours): none before sunrise or after sunset (GLOBAL CONSTANTS
/// "sunrise" / "sunset", 5 and 23; 6 and 20 when they make no sense, F040803), rising over the
/// first twelfth of an hour and falling over the last.
pub fn daylight(hour: f32, sunrise: f32, sunset: f32) -> f32 {
    let (sunrise, sunset) = if sunset <= sunrise {
        (6.0, 20.0)
    } else {
        (sunrise, sunset)
    };
    if hour > sunset || sunrise > hour {
        return 0.0;
    }
    if hour < sunrise + TWILIGHT {
        return invlerp(hour, sunrise, sunrise + TWILIGHT);
    }
    if hour > sunset - TWILIGHT {
        return invlerp(hour, sunset, sunset - TWILIGHT);
    }
    1.0
}

/// @fn F046475
///
/// The vision range (units): 500 indoors, else 1500 times the race's "vision range mult"; times
/// half again by night (lerp of the daylight between 0.5 and 1).
pub fn vision_range(vision_mult: f32, indoors: bool, daylight: f32) -> f32 {
    let base = if indoors { 500.0 } else { vision_mult * 1500.0 };
    base * lerp(daylight, 0.5, 1.0)
}

/// @fn F046498
///
/// How visible `t` is to `o` (0 when not seen): the view cone (0 at 78 degrees off the facing, 1
/// straight ahead), halved for a crouching or prone target; a sneaking or lowered target
/// standing still counts half (under 0.5) or three quarters, then less by its stealth (lerp
/// 1 -> 0.6); the target is seen when nearer than the vision range (doubled by vtable +0x1e0, at
/// most half while fighting) times that, times a light term (the lighter target against the
/// observer, and their mean light from 0.2 or 0.4 up).
pub fn visibility(o: &Observer, t: &Target, daylight: f32) -> f32 {
    if o.out {
        return 0.0;
    }
    let to = [
        t.position[0] - o.position[0],
        t.position[1] - o.position[1],
        t.position[2] - o.position[2],
    ];
    let dist = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
    let (a, b) = (normalised(o.facing), normalised(to));
    let cone = invlerp(a[0] * b[0] + a[1] * b[1] + a[2] * b[2], 0.2, 1.0);
    if cone <= 0.0 {
        return 0.0;
    }
    let lowered = matches!(t.stance, Stance::Crouching | Stance::Prone);
    let mut f = if lowered { cone * 0.5 } else { cone };
    if t.sneaking || lowered {
        if !t.moving {
            f *= if cone < 0.5 { 0.5 } else { 0.75 };
        }
        f *= lerp(t.stealth, 1.0, 0.6);
    }
    let mut range = vision_range(o.vision_mult, o.indoors && !o.flag_1e0, daylight);
    if o.flag_1e0 {
        range *= 2.0;
    }
    let mut reach = range * f;
    if o.in_combat && range * 0.5 < reach {
        reach = range * 0.5;
    }
    let contrast = (t.light - o.light + 1.0).clamp(0.0, 2.0);
    let mean = ((t.light + o.light) * 0.5).clamp(0.0, 1.0);
    let floor = if o.has_78 { 0.2 } else { 0.4 };
    let seen_within = lerp(mean, floor, 1.0) * contrast * reach;
    if dist > seen_within { 0.0 } else { f }
}

/// @fn F046539
///
/// The plain sight test for a townsperson: visible past `SEEN_THRESHOLD`.
pub fn sees(o: &Observer, t: &Target, daylight: f32) -> bool {
    visibility(o, t, daylight) > SEEN_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs() -> Observer {
        Observer {
            position: [0.0; 3],
            facing: [1.0, 0.0, 0.0],
            vision_mult: 1.0,
            in_combat: false,
            indoors: false,
            flag_1e0: false,
            out: false,
            light: 1.0,
            has_78: true,
        }
    }

    fn tgt(x: f32, z: f32) -> Target {
        Target {
            position: [x, 0.0, z],
            stance: Stance::Standing,
            sneaking: false,
            moving: true,
            stealth: 0.0,
            light: 1.0,
        }
    }

    #[test]
    fn day_and_night() {
        assert_eq!(daylight(12.0, 5.0, 23.0), 1.0);
        assert_eq!(daylight(2.0, 5.0, 23.0), 0.0);
        assert!((daylight(5.0 + 1.0 / 24.0, 5.0, 23.0) - 0.5).abs() < 1e-4);
        assert_eq!(daylight(21.0, 20.0, 6.0), 0.0);
    }

    #[test]
    fn ranges() {
        assert_eq!(vision_range(1.0, false, 1.0), 1500.0);
        assert_eq!(vision_range(1.0, false, 0.0), 750.0);
        assert_eq!(vision_range(2.0, true, 1.0), 500.0);
    }

    #[test]
    fn cone_and_distance() {
        let o = obs();
        // straight ahead in daylight: seen out to 1500
        assert!(sees(&o, &tgt(1400.0, 0.0), 1.0));
        assert!(!sees(&o, &tgt(1600.0, 0.0), 1.0));
        // behind: never
        assert!(!sees(&o, &tgt(-10.0, 0.0), 1.0));
        // at night half as far
        assert!(!sees(&o, &tgt(800.0, 0.0), 0.0));
        // a sneaking, skilled target standing still is much harder to see
        let mut s = tgt(500.0, 0.0);
        s.sneaking = true;
        s.moving = false;
        s.stealth = 1.0;
        assert!((visibility(&o, &s, 1.0) - 0.75 * 0.6).abs() < 1e-5);
        assert!(sees(&o, &tgt(500.0, 0.0), 1.0));
    }
}
