//! How a fighter engages: it asks its target for permission to attack (tryEngage), and the target
//! answers (requestAttack) with the state the attacker should take: attack, block or circle.
//!
//! Specs: sheet rows F039834 CombatClass::tryEngage, F040732 CombatClassAI::requestAttack.

use crate::SwordState;

/// What the asking fighter knows when it tries to engage (F039834).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Approach {
    /// The target may be attacked at all (F? 8f53) and the path query allows it.
    pub attackable: bool,
    /// Horizontal distance between the two, centre to centre.
    pub distance: f32,
    /// The target's collision radius.
    pub target_radius: f32,
    /// My max reach (CharStats +0x150).
    pub max_reach: f32,
    /// My reach margin (CombatClass +0x27c).
    pub reach_margin: f32,
    /// The target is inside my hold area (or I have none).
    pub in_hold_area: bool,
    /// Busy (F? 1cf35) or flagged (+0x170 +0x168).
    pub busy: bool,
}

/// @fn F039834
///
/// Whether the fighter is close enough to ask its target for an attack; when true the caller
/// sets its desired state (+0x1f4) to the target's answer (`request_attack`).
pub fn can_engage(a: &Approach) -> bool {
    a.attackable
        && a.distance - a.target_radius <= a.max_reach + a.reach_margin
        && a.in_hold_area
        && !a.busy
}

/// The asked fighter's (the target's) side of requestAttack (F040732).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Asked {
    /// Fighting at all (+0x130).
    pub fighting: bool,
    pub state: SwordState,
    /// Desired state behind an engaged state (+0x1f4; 2 = has no target of its own).
    pub desired: i32,
    /// My swing progress at the asker (F040709).
    pub progress_at_asker: f32,
    /// Staggered (the character can't act).
    pub staggered: bool,
    /// I have a target of my own (+0x2a0 set).
    pub has_target: bool,
    /// That target is the asker (+0x298).
    pub targets_asker: bool,
    /// A hit reaction is pending (+0x144).
    pub reacting: bool,
    /// Fighters already attacking me (+0x240).
    pub attackers: usize,
    /// Held passive (F? 1cf35 on the stats).
    pub passive: bool,
    /// The asker's attack wins the skill contest against my defence (F040707 run by the caller:
    /// false = I turn it away).
    pub asker_wins_contest: bool,
    /// I have a hold point (+0x2d0) and the asker is outside it.
    pub outside_hold: bool,
    /// Flagged (+0x170 +0x168).
    pub flagged: bool,
    /// Doing an action right now (F? 473a2).
    pub acting: bool,
}

/// The asked fighter's own changes from answering.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AskedUpdate {
    /// Start defending (+0x120).
    pub defending: bool,
    /// New desired state (+0x1f4).
    pub desired: Option<i32>,
    /// Go engaged (state 3).
    pub engage: bool,
    /// Turn to face the asker (F? 54444).
    pub face_asker: bool,
    /// Reset the recovery timer (+0x148) when it ran out.
    pub reset_recovery: bool,
}

/// @fn F040732
///
/// The state the asker should take: Attack (granted), Block (I am already striking at it) or
/// Circle (wait its turn). `take_slot` gives the asker one of my attacker slots (F040727) and
/// says whether it got one; `recovery_expired` is +0x148 <= 0.
pub fn request_attack(
    me: &Asked,
    mut take_slot: impl FnMut() -> bool,
    recovery_expired: bool,
) -> (SwordState, AskedUpdate) {
    use SwordState::*;
    let mut u = AskedUpdate::default();
    if !me.fighting {
        return (Attack, u);
    }
    let attacking = me.state == Attack || (me.state == Engaged && me.desired == 0);
    if attacking && me.progress_at_asker < 1.0 && 0.0 < me.progress_at_asker {
        return (Block, u);
    }
    if !take_slot() {
        return (Circle, u);
    }
    let locked = me.state == Locked || (me.state == Engaged && me.desired == 8);
    if locked || me.staggered {
        if !take_slot() {
            return (Circle, u);
        }
        u.defending = true;
        return (Attack, u);
    }
    if !me.has_target {
        u.desired = Some(2);
        return (if take_slot() { Attack } else { Circle }, u);
    }
    if me.has_target && !me.reacting {
        let blocking = me.state == Block || (me.state == Engaged && me.desired == 1);
        if blocking {
            return (if take_slot() { Attack } else { Circle }, u);
        }
        if attacking {
            if !me.asker_wins_contest {
                return (Circle, u);
            }
            if take_slot() {
                u.defending = true;
                u.desired = Some(1);
                return (Attack, u);
            }
            return (Circle, u);
        }
    }
    u.reset_recovery = recovery_expired;
    let defend = |u: &mut AskedUpdate| {
        u.desired = Some(1);
        if me.state != Block {
            u.defending = true;
            u.engage = !me.acting;
        }
    };
    if me.attackers != 0 {
        defend(&mut u);
        return (Attack, u);
    }
    if me.passive {
        return (Attack, u);
    }
    if me.asker_wins_contest && take_slot() {
        defend(&mut u);
        return (Attack, u);
    }
    if !me.outside_hold && !me.flagged {
        u.desired = Some(0);
        if me.state != Attack && !me.acting {
            u.engage = true;
            u.face_asker = true;
        }
    }
    (if me.targets_asker { Block } else { Circle }, u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use SwordState::*;

    fn idle_target() -> Asked {
        Asked {
            fighting: true,
            state: Idle,
            desired: 0,
            progress_at_asker: 0.0,
            staggered: false,
            has_target: true,
            targets_asker: true,
            reacting: false,
            attackers: 0,
            passive: false,
            asker_wins_contest: false,
            outside_hold: false,
            flagged: false,
            acting: false,
        }
    }

    #[test]
    fn engaging_needs_reach() {
        let a = Approach {
            attackable: true,
            distance: 30.0,
            target_radius: 4.0,
            max_reach: 20.0,
            reach_margin: 6.0,
            in_hold_area: true,
            busy: false,
        };
        assert!(can_engage(&a));
        assert!(!can_engage(&Approach {
            distance: 31.0,
            ..a
        }));
        assert!(!can_engage(&Approach { busy: true, ..a }));
    }

    #[test]
    fn the_target_answers() {
        // not fighting: free to attack
        let me = Asked {
            fighting: false,
            ..idle_target()
        };
        assert_eq!(request_attack(&me, || true, true).0, Attack);
        // already swinging at the asker: block
        let me = Asked {
            state: Attack,
            progress_at_asker: 0.4,
            ..idle_target()
        };
        assert_eq!(request_attack(&me, || true, true).0, Block);
        // slots full: circle
        assert_eq!(request_attack(&idle_target(), || false, true).0, Circle);
        // an idle target facing the asker turns to fight it: the asker should block
        let (s, u) = request_attack(&idle_target(), || true, true);
        assert_eq!(s, Block);
        assert!(u.engage && u.face_asker && u.desired == Some(0));
        // the asker wins the contest: it may attack, the target defends
        let me = Asked {
            asker_wins_contest: true,
            ..idle_target()
        };
        let (s, u) = request_attack(&me, || true, true);
        assert_eq!(s, Attack);
        assert!(u.defending && u.desired == Some(1));
    }
}
