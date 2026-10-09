//! Combat techniques: the attack, block and dodge moves, with their timing and per-hit frames.
//!
//! Specs: sheet row F040738 CombatTechnique::CombatTechnique; types T00036 CombatTechnique,
//! T00034 WeaponCategory.

use crate::WeaponCategory;
use kenshi_data::GameData;
use std::collections::HashMap;

/// One hit of a technique.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Attack (or block) direction.
    pub direction: i32,
    /// Power as a factor (record percent * 0.01).
    pub power: f32,
    /// Animation time the strike connects ("anim blocked frame").
    pub impact: f32,
    /// Animation time the move ends ("anim stop frame").
    pub stop: f32,
    /// The limb slot used (arms 0/1, legs 2/3, 4 = the weapon).
    pub limb: i32,
}

/// @type CombatTechnique
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CombatTechnique {
    pub anim_name: String,
    pub speed: f32,
    pub disabled: bool,
    pub is_block: bool,
    pub is_dodge: bool,
    pub is_stumble_dodge: bool,
    pub gains_ground: bool,
    pub is_prone: bool,
    pub low_strike: bool,
    pub hesitate: f32,
    pub attack_distance: f32,
    pub attack_distance_static: f32,
    pub acceptable_end: f32,
    pub min_skill: f32,
    pub max_skill: f32,
    pub max_encumbrance: f32,
    pub chance: f32,
    pub max_simultaneous_hits: i32,
    /// Animation event string id -> time.
    pub events: HashMap<String, f32>,
    /// Which weapon categories may use it.
    pub allowed: Vec<WeaponCategory>,
    pub animal: i32,
    pub frames: Vec<Frame>,
}

const CATEGORY_KEYS: [(&str, WeaponCategory); 8] = [
    ("katanas", WeaponCategory::SkillKatanas),
    ("sabre", WeaponCategory::SkillSabres),
    ("hackers", WeaponCategory::SkillHackers),
    ("heavy weapons", WeaponCategory::SkillHeavy),
    ("blunt", WeaponCategory::SkillBlunt),
    ("unarmed", WeaponCategory::SkillUnarmed),
    ("polearm", WeaponCategory::AttackPolearms),
    ("bow", WeaponCategory::SkillBow),
];

impl CombatTechnique {
    /// @fn F040738
    pub fn from_record(r: &GameData) -> CombatTechnique {
        let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
        let i = |k: &str| r.ints.get(k).copied().unwrap_or(0);
        let f = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
        let s = 1.0 / f("num frames");
        let event_scale = if s == 1.0 { 0.01 } else { 1.0 };
        let animal = i("animal");
        let mut allowed: Vec<WeaponCategory> = CATEGORY_KEYS
            .iter()
            .filter(|(k, _)| b(k) && animal == 0)
            .map(|&(_, c)| c)
            .collect();
        if animal != 0
            && let Some(c) = WeaponCategory::from_i32(animal)
        {
            allowed.push(c);
        }
        allowed.sort();
        let frames = (1..=i("num techniques"))
            .map(|n| Frame {
                direction: i(&format!("attack direction {n}")),
                power: i(&format!("power {n}")) as f32 * 0.01,
                impact: s * f(&format!("anim blocked frame {n}")),
                stop: s * f(&format!("anim stop frame {n}")),
                limb: i(&format!("limb {n}")),
            })
            .collect();
        CombatTechnique {
            anim_name: r.strings.get("anim name").cloned().unwrap_or_default(),
            speed: f("anim speed mult"),
            disabled: b("disabled"),
            is_block: b("is block"),
            is_dodge: b("is dodge"),
            is_stumble_dodge: b("is stumble dodge"),
            gains_ground: b("gains ground"),
            is_prone: b("is prone"),
            low_strike: b("low strike"),
            hesitate: f("anim hesitate point") * s,
            attack_distance: f("attack distance"),
            attack_distance_static: f("attack distance min vs static"),
            acceptable_end: f("acceptable end time") * s,
            min_skill: f("min skill"),
            max_skill: f("max skill"),
            max_encumbrance: f("max encumbrance"),
            chance: f("chance"),
            max_simultaneous_hits: i("max simultaneous hits"),
            events: r
                .refs("events")
                .iter()
                .map(|(sid, v)| (sid.clone(), v[0] as f32 * s * event_scale))
                .collect(),
            allowed,
            animal,
            frames,
        }
    }

    /// The frame a hit index refers to (clamped into range, as defendOutcome and punchFeedback
    /// read it).
    pub fn frame(&self, index: i32) -> Option<&Frame> {
        let last = self.frames.len() as i32 - 1;
        self.frames.get(index.min(last).max(0) as usize)
    }
}

/// The attack and defence moves (F047079).
#[derive(Debug, Clone, Default)]
pub struct Techniques {
    pub attacks: Vec<CombatTechnique>,
    pub defences: Vec<CombatTechnique>,
}

impl Techniques {
    /// @fn F047079
    pub fn from_records<'a>(records: impl IntoIterator<Item = &'a GameData>) -> Techniques {
        let mut t = Techniques::default();
        for r in records {
            let c = CombatTechnique::from_record(r);
            if c.disabled {
                continue;
            }
            if c.is_block || c.is_dodge {
                t.defences.push(c);
            } else {
                t.attacks.push(c);
            }
        }
        t
    }

    /// @fn F047073
    pub fn default_attack(&self, category: WeaponCategory) -> Option<&CombatTechnique> {
        self.attacks.iter().find(|t| t.allowed.contains(&category))
    }
}

/// The attacker's situation for gatherAttacks (F047074).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attacking {
    pub category: WeaponCategory,
    pub skill: f32,
    /// CharStats +0x190 (1 = unburdened).
    pub load: f32,
    pub prone: bool,
    /// The character's size factor (vtable 0x390).
    pub size: f32,
    /// Distance to the target and extra reach.
    pub distance: f32,
    pub extra_reach: f32,
    /// The previous attack may be repeated without penalty.
    pub repeat_ok: bool,
    /// Accept techniques whose usability is below 1.
    pub allow_weak: bool,
}

/// @fn F047074
///
/// The candidate attacks with their weights. `usability(t)` is techniqueUsability (F047059);
/// `last` the previous attack; `max_reach` is CharStats +0x150 (raised to the longest reach seen).
// negated comparisons keep the original's NaN behaviour (a NaN bound rejects the technique)
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn gather_attacks<'a>(
    attacks: &'a [CombatTechnique],
    a: &Attacking,
    last: Option<&CombatTechnique>,
    max_reach: &mut f32,
    usability: impl Fn(&CombatTechnique) -> f32,
) -> Vec<(&'a CombatTechnique, f32)> {
    let enc = (1.0 - a.load) * 100.0;
    let mut out = Vec::new();
    for t in attacks {
        if !t.allowed.contains(&a.category)
            || !(t.min_skill <= a.skill && a.skill <= t.max_skill)
            || !(enc <= t.max_encumbrance)
            || t.is_prone != a.prone
        {
            continue;
        }
        let w = usability(t);
        if !((a.allow_weak || 1.0 <= w) && 0.0 < w) {
            continue;
        }
        let reach = a.size * (a.extra_reach * 0.5 + t.attack_distance) + 3.0;
        let dist = a.distance + 3.0;
        let mut f = if reach <= dist {
            (1.0 / dist) * reach
        } else {
            (1.0 / reach) * dist
        };
        let r = t.attack_distance + a.extra_reach;
        if *max_reach < r {
            *max_reach = r;
        }
        if a.distance <= a.extra_reach + t.attack_distance {
            f *= w;
            if a.distance < t.attack_distance_static {
                f *= 0.02;
            }
            if last.is_some_and(|l| std::ptr::eq(l, t)) && !a.repeat_ok {
                f *= 0.2;
            }
            out.push((t, f * t.chance));
        }
    }
    out
}

/// @fn F047076
///
/// Pick the next attack: from the weapon's moves, else (not an animal) unarmed moves at
/// `martial_arts` skill, strict first and then accepting weak ones. `max_reach` is reset to 1.
#[allow(clippy::too_many_arguments)]
pub fn choose_attack<'a>(
    attacks: &'a [CombatTechnique],
    a: &Attacking,
    animal: bool,
    martial_arts: f32,
    last: Option<&CombatTechnique>,
    max_reach: &mut f32,
    usability: impl Fn(&CombatTechnique) -> f32,
    mut rand_range: impl FnMut(f32, f32) -> f32,
) -> Option<&'a CombatTechnique> {
    *max_reach = 1.0;
    let strict = Attacking {
        allow_weak: false,
        ..*a
    };
    let mut got = gather_attacks(attacks, &strict, last, max_reach, &usability);
    let unarmed = Attacking {
        category: WeaponCategory::SkillUnarmed,
        skill: martial_arts,
        ..strict
    };
    if got.is_empty() && !animal {
        got = gather_attacks(attacks, &unarmed, last, max_reach, &usability);
    }
    if got.is_empty() {
        if animal {
            return None;
        }
        let weak = Attacking {
            allow_weak: true,
            ..unarmed
        };
        got = gather_attacks(attacks, &weak, last, max_reach, &usability);
    }
    // the weighted set keys on the running total, so zero weights add nothing
    let mut table: Vec<(f32, &CombatTechnique)> = Vec::new();
    let mut total = 0.0;
    for (t, w) in got {
        if 0.0 < w {
            total += w;
            table.push((total, t));
        }
    }
    match table.len() {
        0 => None,
        1 => Some(table[0].1),
        _ => {
            let r = rand_range(0.0, total);
            table.iter().find(|&&(k, _)| r < k).map(|&(_, t)| t)
        }
    }
}

/// @fn F038914
///
/// How a technique plays on the action layer at `speed` (CharStats +0x188 * the technique's
/// speed): attacks and dodges move the body and override the base layer, blocks blend over it and
/// play backwards while fading out. Returns the parameters and the reverse-fading flag.
pub fn technique_play(t: &CombatTechnique, speed: f32) -> (kenshi_anim::layer::PlayParams, bool) {
    let moves = !t.is_block || t.is_dodge;
    (
        kenshi_anim::layer::PlayParams {
            speed,
            looping: false,
            target_weight: 1.0,
            end_time: t.acceptable_end,
            root_motion: moves,
            keep_weight: moves,
        },
        t.is_block,
    )
}

/// @fn F040715
pub fn is_rear_direction(d: i32) -> bool {
    matches!(d, 7..=9)
}

/// The defender's situation when choosing how to defend (F047075).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Defender {
    /// Weapon category, already forced to unarmed when the character's weapon isn't out.
    pub category: WeaponCategory,
    pub prone: bool,
    /// F033810.
    pub staggered: bool,
    /// May dodge: staggered, or unarmed and the blockChance roll passed.
    pub dodge_ok: bool,
    /// The defenceChance roll failed: pick a wrong (same-side) block.
    pub sloppy: bool,
}

/// @fn F047075
///
/// Choose the block or dodge to answer an attack whose block direction is `expected`
/// (F033812). `rand_range(a, b)` is uniform in [a, b).
pub fn choose_defence<'a>(
    techniques: &'a [CombatTechnique],
    d: &Defender,
    expected: i32,
    mut rand_range: impl FnMut(f32, f32) -> f32,
) -> Option<&'a CombatTechnique> {
    let mut table: Vec<(f32, &CombatTechnique)> = Vec::new();
    let mut total = 0.0;
    for t in techniques {
        if !t.allowed.contains(&d.category) || t.is_prone != d.prone {
            continue;
        }
        let ok = if t.is_dodge {
            d.dodge_ok && t.is_stumble_dodge == d.staggered
        } else if d.dodge_ok {
            false
        } else {
            let dir = t.frame(i32::MAX).map_or(0, |f| f.direction);
            if d.sloppy {
                dir != expected && is_rear_direction(dir) == is_rear_direction(expected)
            } else {
                dir == expected
            }
        };
        if ok && 0.0 < t.chance {
            total += t.chance;
            table.push((total, t));
        }
    }
    match table.len() {
        0 => None,
        1 => Some(table[0].1),
        _ => {
            let r = rand_range(0.0, total);
            table.iter().find(|&&(k, _)| r < k).map(|&(_, t)| t)
        }
    }
}

/// The unarmed-attacker factor on the defence roll (F047075): `unarmed` is the defender's
/// unarmed skill.
pub fn unarmed_attacker_factor(unarmed: f32) -> f32 {
    kenshi_char::medical::invlerp(unarmed, 0.75, 1.1).clamp(0.5, 1.1)
}

/// The attack a defender is reacting to (F039797).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threat {
    /// The attacker's swing progress at this defender (F040709).
    pub progress: f32,
    /// The attacker's current action is a block (then there's nothing to react to).
    pub attacker_blocking: bool,
    /// F039790 between the attacker and this defender.
    pub related: bool,
    /// The direction of the attacker's current frame.
    pub direction: i32,
    /// The attacker's attack skill (F047043).
    pub skill: f32,
}

/// What reactToAttack decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Reaction<'a> {
    /// No attack to react to.
    Nothing,
    /// Too early or unrelated: check again in `timer`.
    Wait,
    /// Play this block or dodge (at `speed`).
    Defend {
        technique: &'a CombatTechnique,
        speed: f32,
    },
    /// Found no defence.
    Undefended,
    /// An animal: it just faces the attacker.
    Animal,
}

/// The result of one reaction: what to do, the reaction timer (CombatClass +0x14c) and the
/// defence training it triggers (attacker skill, blocked flag for trainDefence).
#[derive(Debug, Clone, PartialEq)]
pub struct Reacted<'a> {
    pub reaction: Reaction<'a>,
    pub timer: Option<f32>,
    pub train: Option<(f32, bool)>,
}

/// @fn F039797
///
/// `choose(direction, skill)` is chooseDefence (F047075); `combat_speed` CharStats +0x18c.
pub fn react_to_attack<'a>(
    animal: bool,
    threat: Option<Threat>,
    combat_speed: f32,
    choose: impl FnOnce(i32, f32) -> Option<&'a CombatTechnique>,
) -> Reacted<'a> {
    let quiet = Reacted {
        reaction: Reaction::Nothing,
        timer: None,
        train: None,
    };
    let Some(t) = threat else { return quiet };
    if t.attacker_blocking || 1.0 <= t.progress {
        return quiet;
    }
    if animal {
        return Reacted {
            reaction: Reaction::Animal,
            timer: Some(2.0),
            train: None,
        };
    }
    if !t.related || t.progress < 0.1 {
        return Reacted {
            reaction: Reaction::Wait,
            timer: Some(0.1),
            train: None,
        };
    }
    let chosen = choose(t.direction, t.skill);
    let train = match chosen {
        None => Some((t.skill, false)),
        Some(c) if c.is_dodge => Some((t.skill, true)),
        Some(_) => None,
    };
    match chosen {
        None => Reacted {
            reaction: Reaction::Undefended,
            timer: Some(1.2),
            train,
        },
        Some(c) => Reacted {
            reaction: Reaction::Defend {
                technique: c,
                speed: combat_speed * c.speed,
            },
            timer: Some(0.2),
            train,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(name: &str, dir: i32) -> CombatTechnique {
        CombatTechnique {
            anim_name: name.into(),
            is_block: true,
            chance: 1.0,
            allowed: vec![WeaponCategory::SkillKatanas],
            frames: vec![Frame {
                direction: dir,
                power: 1.0,
                impact: 0.8,
                stop: 1.0,
                limb: 4,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn defence_matches_the_attack_unless_sloppy() {
        let moves = [
            block("up", 1),
            block("left", 2),
            block("right", 3),
            block("up rear", 7),
        ];
        let d = Defender {
            category: WeaponCategory::SkillKatanas,
            prone: false,
            staggered: false,
            dodge_ok: false,
            sloppy: false,
        };
        let pick = |d: &Defender, e: i32, r: f32| {
            choose_defence(&moves, d, e, |_, _| r).map(|t| t.anim_name.clone())
        };
        assert_eq!(pick(&d, 2, 0.0).as_deref(), Some("left"));
        // a sloppy defender picks a wrong block on the same (front) side
        let s = Defender { sloppy: true, ..d };
        assert_eq!(pick(&s, 2, 0.5).as_deref(), Some("up"));
        assert_eq!(pick(&s, 2, 1.5).as_deref(), Some("right"));
        // a rear attack has only one rear block, so sloppy finds nothing
        assert_eq!(pick(&s, 7, 0.0), None);
        // unarmed defender: nothing allowed
        let u = Defender {
            category: WeaponCategory::SkillUnarmed,
            ..d
        };
        assert_eq!(pick(&u, 2, 0.0), None);
        assert_eq!(unarmed_attacker_factor(50.0), 1.1);
    }

    #[test]
    fn attacks_by_reach_and_skill() {
        let mk = |name: &str, reach: f32, min_skill: f32| CombatTechnique {
            anim_name: name.into(),
            attack_distance: reach,
            attack_distance_static: -999.0,
            min_skill,
            max_skill: 999.0,
            max_encumbrance: 999.0,
            chance: 1.0,
            allowed: vec![WeaponCategory::SkillKatanas],
            ..Default::default()
        };
        let attacks = vec![
            mk("short", 5.0, -100.0),
            mk("long", 25.0, -100.0),
            mk("master", 5.0, 50.0),
        ];
        let a = Attacking {
            category: WeaponCategory::SkillKatanas,
            skill: 20.0,
            load: 1.0,
            prone: false,
            size: 1.0,
            distance: 10.0,
            extra_reach: 0.0,
            repeat_ok: false,
            allow_weak: false,
        };
        let mut reach = 10.0;
        let got = gather_attacks(&attacks, &a, None, &mut reach, |_| 1.0);
        // only "long" reaches 10; weight = (10 + 3) / (25 + 3)
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.anim_name, "long");
        assert!((got[0].1 - 13.0 / 28.0).abs() < 1e-6);
        assert_eq!(reach, 25.0);
        // repeating the last attack is discouraged
        let got = gather_attacks(&attacks, &a, Some(&attacks[1]), &mut reach, |_| 1.0);
        assert!((got[0].1 - 0.2 * 13.0 / 28.0).abs() < 1e-6);
    }

    #[test]
    fn unarmed_fallback_when_the_weapon_has_no_move() {
        let punch = CombatTechnique {
            anim_name: "punch".into(),
            attack_distance: 3.0,
            attack_distance_static: -999.0,
            min_skill: -100.0,
            max_skill: 999.0,
            max_encumbrance: 999.0,
            chance: 1.0,
            allowed: vec![WeaponCategory::SkillUnarmed],
            ..Default::default()
        };
        let attacks = vec![punch];
        let a = Attacking {
            category: WeaponCategory::SkillKatanas,
            skill: 20.0,
            load: 1.0,
            prone: false,
            size: 1.0,
            distance: 2.0,
            extra_reach: 0.0,
            repeat_ok: true,
            allow_weak: false,
        };
        let mut reach = 0.0;
        let got = choose_attack(
            &attacks,
            &a,
            false,
            10.0,
            None,
            &mut reach,
            |_| 1.0,
            |_, _| 0.0,
        );
        assert_eq!(got.map(|t| t.anim_name.as_str()), Some("punch"));
        assert_eq!(reach, 3.0);
        // animals don't fall back to fists
        assert!(
            choose_attack(
                &attacks,
                &a,
                true,
                10.0,
                None,
                &mut reach,
                |_| 1.0,
                |_, _| 0.0
            )
            .is_none()
        );
        // weak (crippled) moves only on the last fallback
        let got = choose_attack(
            &attacks,
            &a,
            false,
            10.0,
            None,
            &mut reach,
            |_| 0.5,
            |_, _| 0.0,
        );
        assert!(got.is_some());
    }

    #[test]
    fn attacks_override_movement_blocks_blend() {
        let cut = CombatTechnique {
            acceptable_end: 0.95,
            ..Default::default()
        };
        let (p, rev) = technique_play(&cut, 1.12);
        assert!(p.keep_weight && p.root_motion && !p.looping && !rev);
        assert_eq!((p.speed, p.end_time), (1.12, 0.95));
        let block = CombatTechnique {
            is_block: true,
            ..Default::default()
        };
        let (p, rev) = technique_play(&block, 1.0);
        assert!(!p.keep_weight && !p.root_motion && rev);
        let dodge = CombatTechnique {
            is_block: true,
            is_dodge: true,
            ..Default::default()
        };
        assert!(technique_play(&dodge, 1.0).0.root_motion);
    }

    #[test]
    fn reactions() {
        let left = CombatTechnique {
            speed: 1.3,
            ..block("left", 2)
        };
        let t = Threat {
            progress: 0.5,
            attacker_blocking: false,
            related: true,
            direction: 2,
            skill: 40.0,
        };
        let r = react_to_attack(false, Some(t), 1.0, |d, _| (d == 2).then_some(&left));
        assert_eq!(r.timer, Some(0.2));
        assert!(matches!(r.reaction, Reaction::Defend { speed, .. } if (speed - 1.3).abs() < 1e-6));
        assert_eq!(r.train, None);
        let r = react_to_attack(false, Some(Threat { direction: 3, ..t }), 1.0, |d, _| {
            (d == 2).then_some(&left)
        });
        assert_eq!(
            (r.reaction, r.timer, r.train),
            (Reaction::Undefended, Some(1.2), Some((40.0, false)))
        );
        let r = react_to_attack(
            false,
            Some(Threat {
                progress: 0.05,
                ..t
            }),
            1.0,
            |_, _| None,
        );
        assert_eq!((r.reaction, r.timer), (Reaction::Wait, Some(0.1)));
        assert_eq!(
            react_to_attack(true, Some(t), 1.0, |_, _| None).reaction,
            Reaction::Animal
        );
    }

    #[test]
    fn downward_cut() {
        let mut r = GameData::default();
        r.strings.insert("anim name".into(), "chop down".into());
        r.floats.insert("num frames".into(), 1.0);
        r.floats.insert("anim speed mult".into(), 1.12);
        r.floats.insert("anim blocked frame 1".into(), 0.56);
        r.floats.insert("anim stop frame 1".into(), 0.77);
        r.ints.insert("num techniques".into(), 1);
        r.ints.insert("attack direction 1".into(), 1);
        r.ints.insert("power 1".into(), 100);
        r.ints.insert("limb 1".into(), 4);
        r.bools.insert("katanas".into(), true);
        r.bools.insert("blunt".into(), false);
        r.references
            .push(("events".into(), vec![("95378-sfx.mod".into(), [40, 0, 0])]));
        let t = CombatTechnique::from_record(&r);
        assert_eq!(t.allowed, vec![WeaponCategory::SkillKatanas]);
        assert_eq!(
            t.frames,
            vec![Frame {
                direction: 1,
                power: 1.0,
                impact: 0.56,
                stop: 0.77,
                limb: 4
            }]
        );
        assert!((t.events["95378-sfx.mod"] - 0.4).abs() < 1e-6);
        assert_eq!(t.frame(5).unwrap().direction, 1);
        // animal moves belong to their animal's category only
        r.ints.insert("animal".into(), 10);
        let dog = CombatTechnique::from_record(&r);
        assert_eq!(dog.allowed, vec![WeaponCategory::AttackDog]);
    }
}
