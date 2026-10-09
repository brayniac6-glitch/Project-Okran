//! The fighting state machine's decisions that don't need the animation/movement runtime: state
//! changes and how the attack state picks a swing.
//!
//! Specs: sheet rows F039842 CombatClass::setState, F029603 AttackState::canEnter,
//! F029615 AttackState::tryAttack, F039784 CombatClass::extraReach, F040708 isLocked; types
//! T00037 CombatClass, T00038 SwordState.

use crate::SwordState;
use crate::technique::{Attacking, CombatTechnique, choose_attack};
use kenshi_char::medical::lerp;

/// @fn F039842
///
/// Returns whether the current action must stop (entering idle while acting).
pub fn set_state(state: &mut SwordState, to: SwordState, acting: bool) -> bool {
    if *state == SwordState::Locked {
        return false;
    }
    *state = to;
    to == SwordState::Idle && acting
}

/// @fn F029603
pub fn attack_can_enter(attackers_on_me: usize, target_reachable: bool) -> bool {
    attackers_on_me == 0 && target_reachable
}

/// @fn F039784
pub fn extra_reach(weapon_length: f32) -> f32 {
    weapon_length * 0.5
}

/// The free distance between attacker and target (F029615): centre distance minus both radii,
/// the target's scaled by the attacker's size.
pub fn attack_gap(distance: f32, my_radius: f32, my_size: f32, target_radius: f32) -> f32 {
    let g = distance - my_radius - lerp(my_size, 0.45, 1.0) * target_radius;
    if g < 0.0 { 0.0 } else { g }
}

/// What tryAttack decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Swing<'a> {
    /// Start this attack at `speed` (CharStats +0x188 * the technique's speed).
    Start {
        technique: &'a CombatTechnique,
        speed: f32,
    },
    /// Nothing reaches: move toward the target.
    Approach,
}

/// @fn F029615
///
/// `current` is the fighter's current action (a block doesn't count as the last attack);
/// `target_state` the target's state (a locked target may be hit with the same move again).
#[allow(clippy::too_many_arguments)]
pub fn try_attack<'a>(
    attacks: &'a [CombatTechnique],
    a: &Attacking,
    animal: bool,
    martial_arts: f32,
    current: Option<&CombatTechnique>,
    target_state: SwordState,
    attack_speed: f32,
    max_reach: &mut f32,
    usability: impl Fn(&CombatTechnique) -> f32,
    rand_range: impl FnMut(f32, f32) -> f32,
) -> Swing<'a> {
    let last = current.filter(|c| !c.is_block);
    let a = Attacking {
        repeat_ok: target_state == SwordState::Locked,
        ..*a
    };
    match choose_attack(
        attacks,
        &a,
        animal,
        martial_arts,
        last,
        max_reach,
        usability,
        rand_range,
    ) {
        Some(t) => Swing::Start {
            technique: t,
            speed: attack_speed * t.speed,
        },
        None => Swing::Approach,
    }
}

/// @fn F040729
///
/// How many of the action's frames have struck by animation `progress`.
pub fn frame_index(t: &CombatTechnique, progress: f32) -> i32 {
    t.frames.iter().filter(|f| f.impact < progress).count() as i32
}

/// A swing in progress: which frames have already struck (CombatClass +0x268 / +0x26c).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SwingProgress {
    pub index: i32,
    pub struck: i32,
}

/// What one swing tick asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwingTick {
    /// Strike with frame `frame` (collect targets first if this action hasn't yet).
    Strike {
        frame: i32,
    },
    Continue,
    /// The move is over (or broken off): go idle.
    Done,
}

/// @fn F039804
///
/// One tick of a swing at animation `progress` (negative when the action stopped). `defending`
/// is CombatClass +0x120.
pub fn update_swing(
    t: &CombatTechnique,
    s: &mut SwingProgress,
    progress: f32,
    defending: bool,
) -> SwingTick {
    s.index = frame_index(t, progress);
    // the strike reads the frame before the counter moves on: the first strike is frame 0
    let strike = (s.struck < s.index).then_some(s.struck);
    s.struck = s.index;
    let last = t.frames.len() as i32 - 1;
    if last < s.index {
        s.index = last;
    }
    let done =
        progress < 0.0 || (defending && t.frame(s.index).is_some_and(|f| f.stop <= progress));
    match (strike, done) {
        (Some(frame), _) => SwingTick::Strike { frame },
        (None, true) => SwingTick::Done,
        _ => SwingTick::Continue,
    }
}

/// The swing's hit box in the attacker's frame (forward = +z): its centre's distance ahead and
/// its half extents (side, up, forward).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwingBox {
    pub ahead: f32,
    pub half: [f32; 3],
}

/// @fn F039793
///
/// The box before the world-dependent steps (ground height, pull-back): `moved` is how far the
/// attacker has travelled since the swing began; `leg` whether the frame strikes with a leg.
pub fn swing_box(
    weapon_length: f32,
    radius: f32,
    size: f32,
    leg: bool,
    attack_distance: f32,
    moved: f32,
) -> SwingBox {
    let h = weapon_length * 0.5;
    let mut ahead = h * 0.5 + radius;
    let mut half = if leg {
        [13.0, 8.0, h]
    } else {
        [radius + 0.5, 25.0, h]
    };
    let mut adv = attack_distance - moved;
    if adv < -1.0 {
        adv = -1.0;
    }
    ahead += adv;
    let k = lerp(size, 0.45, 1.0);
    ahead *= k;
    for v in &mut half {
        *v *= k;
    }
    SwingBox { ahead, half }
}

/// @fn F039793
///
/// The pull-back step: the box centre moves back toward the attacker by half of how far it is
/// beyond the attacker's collision radius, and the box grows forward by the same.
pub fn pull_back(b: SwingBox, radius: f32) -> SwingBox {
    let pb = ((b.ahead - radius) * 0.5).max(0.0);
    SwingBox {
        ahead: b.ahead - pb,
        half: [b.half[0], b.half[1], b.half[2] + pb],
    }
}

/// Whether a character of `radius` at `side`, `forward` from the attacker (in its facing frame)
/// touches the swing box (the box query of F039793, with characters as circles).
pub fn in_swing_box(b: &SwingBox, side: f32, forward: f32, radius: f32) -> bool {
    side.abs() <= b.half[0] + radius && (forward - b.ahead).abs() <= b.half[2] + radius
}

/// @fn F040725
///
/// How many fighters may attack one defender at once. `max_slots` is GLOBAL CONSTANTS "max num
/// attack slots"; `race_bonus` the race's int +0x84.
pub fn slot_count(
    defender_attacking: bool,
    defender_moving: bool,
    max_slots: i32,
    race_bonus: i32,
) -> usize {
    if defender_attacking || defender_moving {
        return 1;
    }
    (max_slots + race_bonus).clamp(1, 5) as usize
}

/// The defender's attacker slots (CombatClass +0x10..).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttackerSlots<T> {
    pub slots: Vec<Option<T>>,
}

impl<T: PartialEq> AttackerSlots<T> {
    /// @fn F040727
    ///
    /// Give `attacker` a slot among the first `count`; false when they are all taken.
    pub fn add(&mut self, attacker: T, count: usize) -> bool {
        if self.slots.len() < count {
            self.slots.resize_with(count, || None);
        }
        if self.slots[..count]
            .iter()
            .any(|s| s.as_ref() == Some(&attacker))
        {
            return true;
        }
        match self.slots[..count].iter_mut().find(|s| s.is_none()) {
            Some(s) => {
                *s = Some(attacker);
                true
            }
            None => false,
        }
    }
}

/// @fn F040707
///
/// Whether the defender (skill `mine`) turns an attacker (skill `theirs`) away; `factor` is
/// GLOBAL CONSTANTS "attack chance factor"; `r1`, `r2` uniform in [0, 1).
pub fn attack_contest(mine: f32, theirs: f32, factor: f32, r1: f32, r2: f32) -> bool {
    let (fa, fb) = if mine <= theirs {
        ((theirs - mine) * factor + 1.0, 1.0)
    } else {
        (1.0, (mine - theirs) * factor + 1.0)
    };
    fa * r1 < fb * r2
}

/// What the idle state decided (F039837): the next state and the desired state behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IdleChoice {
    /// Nothing to do this tick (busy).
    Busy,
    TargetLost,
    Disengage,
    /// Go engaged (state 3) wanting `desired`.
    Engage {
        desired: SwordState,
    },
}

/// @fn F039837
///
/// `engage` is tryEngage (F039834): Some(desired) when the target granted a state.
pub fn idle(
    busy: bool,
    has_target: bool,
    valid_enemy: bool,
    engage: Option<SwordState>,
    attacked: bool,
    circle_ok: bool,
    wait_ok: bool,
) -> IdleChoice {
    if busy {
        return IdleChoice::Busy;
    }
    if !has_target {
        return IdleChoice::TargetLost;
    }
    if !valid_enemy {
        return IdleChoice::Disengage;
    }
    let desired = if let Some(d) = engage {
        d
    } else if attacked {
        SwordState::Block
    } else if circle_ok {
        SwordState::Circle
    } else if wait_ok {
        SwordState::Waiting
    } else {
        SwordState::Circle
    };
    IdleChoice::Engage { desired }
}

/// What the engaged state did this tick (F039848).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngagedStep {
    TargetLost,
    Disengage,
    /// The desired state started: switch to it.
    Enter(SwordState),
    /// Waiting to block: stay engaged and close on the target.
    Hold,
    /// The wait ran out: go idle.
    Idle,
}

/// @fn F039848
///
/// `start` tries to start the desired state's object (None when that state has no object);
/// `reacted` is whether reactToAttack chose a defence this tick; `recovery` is CombatClass +0x148.
#[allow(clippy::too_many_arguments)]
pub fn engaged_step(
    has_target: bool,
    valid_enemy: bool,
    desired: SwordState,
    start: Option<bool>,
    reaction_timer: f32,
    reacted: bool,
    recovery: &mut f32,
    dt: f32,
) -> EngagedStep {
    if !has_target {
        return EngagedStep::TargetLost;
    }
    if !valid_enemy {
        return EngagedStep::Disengage;
    }
    if start == Some(true) {
        return EngagedStep::Enter(desired);
    }
    if desired == SwordState::Block && (0.0 <= reaction_timer || !reacted) {
        *recovery -= dt;
        if *recovery < 0.0 {
            return EngagedStep::Idle;
        }
    }
    EngagedStep::Hold
}

/// @fn F047022
///
/// The pause after a strike by how it went (CombatClass +0x148).
pub fn recovery_time(outcome: crate::melee::Defence) -> f32 {
    match outcome {
        crate::melee::Defence::Hit => 0.6,
        crate::melee::Defence::Blocked => 0.3,
        crate::melee::Defence::Dodged => 0.2,
    }
}

/// The trainFromCombat event a strike raises for the attacker (F039803).
pub fn strike_event(outcome: crate::melee::Defence) -> i32 {
    match outcome {
        crate::melee::Defence::Dodged => 6,
        crate::melee::Defence::Blocked => 1,
        crate::melee::Defence::Hit => 0,
    }
}

/// @fn F039803
///
/// The damage one frame of a swing carries: attackDamage scaled by the frame's power.
pub fn frame_damage(base: crate::hit::Hit, t: &CombatTechnique, frame: i32) -> crate::hit::Hit {
    let p = t.frame(frame).map_or(1.0, |f| f.power);
    crate::hit::Hit {
        cut: base.cut * p,
        blunt: base.blunt * p,
        pierce: base.pierce * p,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_box_pulls_back() {
        let b = swing_box(21.0, 4.0, 1.0, false, 25.0, 0.0);
        let p = pull_back(b, 4.0);
        assert!(p.ahead < b.ahead && p.half[2] > b.half[2]);
        // the front edge doesn't move back
        assert!((p.ahead + p.half[2] - (b.ahead + b.half[2])).abs() < 1e-4 + (b.ahead - p.ahead));
        assert!(in_swing_box(&p, 0.0, p.ahead, 4.0));
        assert!(!in_swing_box(&p, 0.0, p.ahead + p.half[2] + 5.0, 4.0));
    }
    use crate::WeaponCategory;

    #[test]
    fn a_combo_strikes_once_per_frame() {
        use crate::technique::Frame;
        let combo = CombatTechnique {
            frames: vec![
                Frame {
                    direction: 2,
                    power: 0.8,
                    impact: 0.51,
                    stop: 0.53,
                    limb: 4,
                },
                Frame {
                    direction: 1,
                    power: 0.6,
                    impact: 0.79,
                    stop: 0.85,
                    limb: 4,
                },
            ],
            ..Default::default()
        };
        let mut s = SwingProgress::default();
        let ticks: Vec<SwingTick> = [0.2, 0.5, 0.52, 0.6, 0.8, 0.9]
            .iter()
            .map(|&p| update_swing(&combo, &mut s, p, false))
            .collect();
        assert_eq!(
            ticks,
            vec![
                SwingTick::Continue,
                SwingTick::Continue,
                SwingTick::Strike { frame: 0 },
                SwingTick::Continue,
                SwingTick::Strike { frame: 1 },
                SwingTick::Continue,
            ]
        );
        assert_eq!(update_swing(&combo, &mut s, -1.0, false), SwingTick::Done);
        let hit = crate::hit::Hit {
            cut: 10.0,
            blunt: 5.0,
            pierce: 0.0,
            bleed: 1.0,
            penetration: 0.0,
        };
        assert_eq!(frame_damage(hit, &combo, 0).cut, 8.0);
        assert_eq!(frame_damage(hit, &combo, 5).cut, 6.0); // clamped to the last frame
        assert_eq!(recovery_time(crate::melee::Defence::Hit), 0.6);
    }

    #[test]
    fn swing_box_follows_weapon_length() {
        let katana = swing_box(21.0, 4.0, 1.0, false, 5.0, 5.0);
        assert_eq!(
            katana,
            SwingBox {
                ahead: 21.0 * 0.25 + 4.0,
                half: [4.5, 25.0, 10.5]
            }
        );
        let kick = swing_box(18.0, 4.0, 1.0, true, 3.0, 0.0);
        assert_eq!(kick.half, [13.0, 8.0, 9.0]);
        assert_eq!(kick.ahead, 4.5 + 4.0 + 3.0);
        // a small character's box shrinks
        assert!((swing_box(21.0, 4.0, 0.0, false, 0.0, 0.0).half[1] - 25.0 * 0.45).abs() < 1e-5);
    }

    #[test]
    fn attack_slots_limit_attackers() {
        assert_eq!(slot_count(false, false, 1, 0), 1);
        assert_eq!(slot_count(false, false, 1, 9), 5);
        assert_eq!(slot_count(true, false, 3, 0), 1);
        let mut s = AttackerSlots::default();
        assert!(s.add("a", 1));
        assert!(s.add("a", 1)); // already holds it
        assert!(!s.add("b", 1)); // full: b circles
        assert!(s.add("b", 2));
        // equal skill: plain coin flips; a much stronger defender turns attackers away more
        assert!(!attack_contest(50.0, 50.0, 0.05, 0.5, 0.4));
        assert!(attack_contest(90.0, 50.0, 0.05, 0.5, 0.4)); // 1*0.5 < 3*0.4
    }

    #[test]
    fn idle_picks_what_to_want() {
        assert_eq!(
            idle(false, false, true, None, false, true, true),
            IdleChoice::TargetLost
        );
        assert_eq!(
            idle(false, true, true, None, true, true, true),
            IdleChoice::Engage {
                desired: SwordState::Block
            }
        );
        assert_eq!(
            idle(
                false,
                true,
                true,
                Some(SwordState::Attack),
                true,
                true,
                true
            ),
            IdleChoice::Engage {
                desired: SwordState::Attack
            }
        );
        assert_eq!(
            idle(false, true, true, None, false, false, true),
            IdleChoice::Engage {
                desired: SwordState::Waiting
            }
        );
    }

    #[test]
    fn engaged_waits_then_idles() {
        let mut rec = 0.15;
        let step =
            |rec: &mut f32| engaged_step(true, true, SwordState::Block, None, 0.1, false, rec, 0.1);
        assert_eq!(step(&mut rec), EngagedStep::Hold);
        assert_eq!(step(&mut rec), EngagedStep::Idle);
        let mut rec = 1.0;
        assert_eq!(
            engaged_step(
                true,
                true,
                SwordState::Attack,
                Some(true),
                0.0,
                false,
                &mut rec,
                0.1
            ),
            EngagedStep::Enter(SwordState::Attack)
        );
        assert_eq!(
            engaged_step(
                false,
                true,
                SwordState::Attack,
                None,
                0.0,
                false,
                &mut rec,
                0.1
            ),
            EngagedStep::TargetLost
        );
    }

    #[test]
    fn locked_state_sticks() {
        let mut s = SwordState::Attack;
        assert!(set_state(&mut s, SwordState::Idle, true));
        assert_eq!(s, SwordState::Idle);
        s = SwordState::Locked;
        assert!(!set_state(&mut s, SwordState::Attack, false));
        assert_eq!(s, SwordState::Locked);
    }

    #[test]
    fn gap_and_swing() {
        assert_eq!(attack_gap(10.0, 2.0, 1.0, 3.0), 5.0);
        assert_eq!(extra_reach(21.0), 10.5);
        assert_eq!(attack_gap(3.0, 2.0, 0.0, 3.0), 0.0); // 3 - 2 - 0.45*3 < 0
        let cut = CombatTechnique {
            attack_distance: 6.0,
            attack_distance_static: -999.0,
            min_skill: -100.0,
            max_skill: 999.0,
            max_encumbrance: 999.0,
            chance: 1.0,
            speed: 1.12,
            allowed: vec![WeaponCategory::SkillKatanas],
            ..Default::default()
        };
        let attacks = vec![cut];
        let a = Attacking {
            category: WeaponCategory::SkillKatanas,
            skill: 20.0,
            load: 1.0,
            prone: false,
            size: 1.0,
            distance: attack_gap(10.0, 2.0, 1.0, 3.0),
            extra_reach: 0.0,
            repeat_ok: false,
            allow_weak: false,
        };
        let mut reach = 0.0;
        let s = try_attack(
            &attacks,
            &a,
            false,
            0.0,
            None,
            SwordState::Idle,
            1.0,
            &mut reach,
            |_| 1.0,
            |_, _| 0.0,
        );
        assert!(matches!(s, Swing::Start { speed, .. } if (speed - 1.12).abs() < 1e-6));
        let far = Attacking {
            distance: 20.0,
            ..a
        };
        let s = try_attack(
            &attacks,
            &far,
            false,
            0.0,
            None,
            SwordState::Idle,
            1.0,
            &mut reach,
            |_| 1.0,
            |_, _| 0.0,
        );
        assert_eq!(s, Swing::Approach);
    }
}
