//! Hit reactions: a struck character staggers with a stumble animation chosen for the body part
//! hit, the side the blow came from and how hard it was.
//!
//! An ANIMATION record lists, under "stumbles", the body parts (LOCATIONAL_DAMAGE records) it is
//! a reaction for; "big stumble" marks the heavy ones and "stumble from" the side (0 front, 1 back).
//!
//! Specs: sheet rows F047023 CharStats::staggerThreshold, F033861 Character::onHit (the stumble
//! part), F033811 Character::hitFromBehind, F050491 isPositive, F037176
//! AnimationClass::pickStumble, F038934 the ANIMATION record's stumble fields.

use kenshi_anim::select::{AnimationData, Situation, candidates};
use kenshi_data::GameData;

/// The stumble animations: ANIMATION records listing body parts under "stumbles" (disabled
/// ones skipped, F038935).
pub fn stumbles<'a>(records: impl IntoIterator<Item = &'a GameData>) -> Vec<AnimationData> {
    records
        .into_iter()
        .filter(|r| !r.bools.get("disabled").copied().unwrap_or(false))
        .map(AnimationData::from_record)
        .filter(|a| !a.stumbles.is_empty())
        .collect()
}

/// @fn F047023
///
/// The damage it takes to stagger a character: toughness (CharStats +0x90) * +0x34 * 0.01 *
/// GLOBAL CONSTANTS "stumble damage max".
pub fn stagger_threshold(toughness: f32, scale: f32, stumble_damage_max: f32) -> f32 {
    ((toughness * scale) as f64 * 0.01 * stumble_damage_max as f64) as f32
}

/// @fn F033811
///
/// Whether the blow came from behind: the direction to the attacker points away from the
/// victim's facing (F050491: dot > 0 against the reversed facing).
pub fn hit_from_behind(facing: [f32; 3], to_attacker: [f32; 3]) -> bool {
    let len = (to_attacker[0].powi(2) + to_attacker[1].powi(2) + to_attacker[2].powi(2)).sqrt();
    let d = if len > 0.0 {
        to_attacker.map(|v| v / len)
    } else {
        to_attacker
    };
    -facing[0] * d[0] - facing[1] * d[1] - facing[2] * d[2] > 0.0
}

/// @fn F033861
///
/// Whether a hit staggers and how: `damage` is the hit's cut + blunt + pierce + stun, `attacked`
/// whether fighters are attacking the victim (CombatClass +0x228). It staggers when nobody is
/// attacking it or the hit passes the threshold; the stumble is big only when both hold. None:
/// no stumble.
pub fn stagger(damage: f32, threshold: f32, attacked: bool) -> Option<bool> {
    let heavy = threshold < damage;
    (!attacked || heavy).then_some(!attacked && heavy)
}

/// @fn F037176
///
/// Choose the stumble for a hit on `part` (string id) from side `from`, big or not: the stumbles
/// for that part that suit the character's situation (F037153, not strict), those matching `big`
/// and `from`, weighted by chance (a quarter for the one played last). `rand01` is uniform in
/// [0, 1).
#[allow(clippy::too_many_arguments)]
pub fn pick_stumble<'a>(
    stumbles: &'a [AnimationData],
    part: &str,
    situation: &Situation,
    size: f32,
    from: i32,
    big: bool,
    last: Option<&str>,
    rand01: f32,
) -> Option<&'a AnimationData> {
    let for_part = stumbles
        .iter()
        .filter(|s| s.stumbles.iter().any(|p| p == part));
    let mut table: Vec<(f32, &AnimationData)> = Vec::new();
    let mut total = 0.0;
    for (s, _) in candidates(for_part, situation, size, false) {
        if s.big_stumble != big || s.stumble_from != from {
            continue;
        }
        let mut w = s.chance;
        if last == Some(s.name.as_str()) {
            w *= 0.25;
        }
        if 0.0 < w {
            total += w;
            table.push((total, s));
        }
    }
    match table.len() {
        0 => None,
        1 => Some(table[0].1),
        _ => {
            let r = rand01 * total;
            // the first whose running total exceeds the roll
            table.iter().find(|&&(k, _)| r < k).map(|&(_, s)| s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, big: bool, from: i32) -> AnimationData {
        let mut r = GameData {
            name: name.into(),
            ..Default::default()
        };
        r.strings.insert("anim name".into(), name.into());
        r.bools.insert("big stumble".into(), big);
        r.ints.insert("stumble from".into(), from);
        r.ints.insert("chance".into(), 100);
        r.ints.insert("is combat mode".into(), 2);
        r.ints.insert("stealth mode".into(), 2);
        r.ints.insert("has weapon L".into(), 2);
        r.ints.insert("has weapon R".into(), 2);
        r.floats.insert("min speed".into(), -999.0);
        r.floats.insert("max speed".into(), 999.0);
        r.floats.insert("R leg damage max".into(), 100.0);
        r.floats.insert("L leg damage max".into(), 100.0);
        r.floats.insert("R leg damage ideal".into(), 100.0);
        r.floats.insert("L leg damage ideal".into(), 100.0);
        let mut a = AnimationData::from_record(&r);
        a.stumbles = vec!["chest".into()];
        a
    }

    #[test]
    fn heavy_hits_stagger_harder() {
        // toughness 40: 32 damage to stagger
        let t = stagger_threshold(40.0, 1.0, 80.0);
        assert!((t - 32.0).abs() < 1e-4);
        // in a fight (being attacked) only a heavy hit staggers, and lightly
        assert_eq!(stagger(20.0, t, true), None);
        assert_eq!(stagger(59.0, t, true), Some(false));
        // caught alone: any hit staggers, a heavy one big
        assert_eq!(stagger(20.0, t, false), Some(false));
        assert_eq!(stagger(59.0, t, false), Some(true));
    }

    #[test]
    fn the_side_and_the_size_choose_the_stumble() {
        let all = [
            s("mid blow", true, 0),
            s("mid blow light", false, 0),
            s("back blow", true, 1),
        ];
        let sit = Situation {
            r_leg: 100.0,
            l_leg: 100.0,
            ..Default::default()
        };
        let pick = |from, big| {
            pick_stumble(&all, "chest", &sit, 1.0, from, big, None, 0.5).map(|s| s.name.as_str())
        };
        assert_eq!(pick(0, true), Some("mid blow"));
        assert_eq!(pick(0, false), Some("mid blow light"));
        assert_eq!(pick(1, true), Some("back blow"));
        assert_eq!(pick(1, false), None);
        assert_eq!(
            pick_stumble(&all, "head", &sit, 1.0, 0, true, None, 0.5),
            None
        );
        // facing +z: an attacker behind (-z) hits from behind
        assert!(hit_from_behind([0.0, 0.0, 1.0], [0.0, 0.0, -5.0]));
        assert!(!hit_from_behind([0.0, 0.0, 1.0], [1.0, 0.0, 5.0]));
    }
}
