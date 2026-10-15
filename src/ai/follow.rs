//! Task_FollowLeader (task 141, "Following leader"): a squad member keeps to its spot in the
//! formation, or trails a leader of another platoon.
//!
//! Spec: F030763 / F030758 the constructor (follow distance +0x90 = 40), F030761 start, F031009
//! update, F030762 end, F031270 the target (the character the member was told to follow, else
//! its platoon's leader), F031007 same-platoon test, F038686 the score.
//!
//! Not ported: losing sight of the leader (F030993: the secondary vtable at +0x78 gives up and
//! goes to where the leader was last seen; here the leader is always in sight), following a
//! follower's own target when the leader itself follows someone (F031007), a leader that is a
//! vehicle (type 0x22) and the flag the end of the update leaves on the character (F039204).

use crate::formation::{Vec3, formation_spot, normalise, trailing_spot};

/// What CharMovement is told this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FollowMove {
    /// CharMovement::stop (F040646).
    Stop,
    /// Mode 2 (F040641): steer straight along `dir` (across), at most `dist` units.
    Direct { dir: Vec3, dist: f32 },
    /// goTo (vtable +0x90, mode 1): path to the spot.
    GoTo(Vec3),
}

/// The CharMovement figures the update reads (units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub position: Vec3,
    /// +0xd0: the direction it last moved in.
    pub facing: Vec3,
    /// +0xb8: its measured speed.
    pub speed: f32,
}

/// The task's own state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowLeader {
    /// +0x90.
    pub distance: f32,
    /// +0x94: where the follower stood last update.
    pub last_position: Vec3,
    /// +0xa0: the formation's facing.
    pub facing: Vec3,
    /// +0xac: steering straight at the spot.
    pub direct: bool,
}

impl Default for FollowLeader {
    /// @fn F030763
    ///
    /// Task_FollowLeader's: Task_Follow's constructor (F030758) with the distance 40 (the
    /// factory's plain Task_Follow takes a random 20 .. 40).
    fn default() -> Self {
        Self {
            distance: 40.0,
            last_position: [0.0; 3],
            facing: [0.0; 3],
            direct: false,
        }
    }
}

/// One update's outcome: the speed cap (CharMovement +0xbc), the move, and the spot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowStep {
    pub cap: f32,
    pub action: FollowMove,
    pub spot: Vec3,
}

fn saturate(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// @fn F038686
///
/// The score: half the leader's factor (its vtable +0x70 object +0xa8; 1 without one).
pub fn score(leader_factor: Option<f32>) -> f32 {
    leader_factor.unwrap_or(1.0) * 0.5
}

impl FollowLeader {
    /// @fn F030761
    ///
    /// start: animation-driven movement off (CharMovement::setAnimDriven(0)), the last position
    /// is where the follower stands, and it starts out steering straight.
    pub fn start(&mut self, me: Vec3) {
        self.last_position = me;
        self.direct = true;
    }

    /// @fn F031009
    ///
    /// update: the spot (the formation's for a member of the leader's platoon, given its
    /// `offset`, else trailing), then the speed: with the leader standing, none within 20 units of
    /// the spot, rising from 10 to 20 by 50, to 30 by 200 and on toward 999 by about 500; with
    /// the leader moving, the distance to the spot (per second), times how well the two face the
    /// same way (half the dot of their facings plus a half, at least 0.1). No speed: stop. Within
    /// 50 units, steer straight at the spot; beyond, keep steering straight only while within 100
    /// and still getting somewhere (moved further than its speed squared since the last update),
    /// else path to the spot.
    pub fn update(&mut self, me: Body, leader: Body, offset: Option<Vec3>, dt: f32) -> FollowStep {
        let spot = match offset {
            Some(o) => formation_spot(
                &mut self.facing,
                leader.position,
                leader.facing,
                leader.speed,
                o,
                dt,
            ),
            None => trailing_spot(me.position, leader.position, self.distance),
        };
        let (dx, dz) = (me.position[0] - spot[0], me.position[2] - spot[2]);
        let d2 = dz * dz + dx * dx;
        let mut speed = if d2 < 1.0 {
            0.0
        } else if leader.speed <= 0.0 {
            if d2 < 40_000.0 {
                if d2 < 2500.0 {
                    if d2 < 400.0 {
                        0.0
                    } else {
                        saturate((d2 - 400.0) / 2100.0) * 10.0 + 10.0
                    }
                } else {
                    saturate((d2 - 2500.0) / 37_500.0) * 10.0 + 20.0
                }
            } else {
                saturate((d2 - 40_000.0) / 210_000.0) * 969.0 + 30.0
            }
        } else {
            d2.sqrt()
        };
        if speed > 0.0 && leader.speed > 0.0 {
            let a = normalise([leader.facing[0], 0.0, leader.facing[2]]);
            let b = normalise([me.facing[0], 0.0, me.facing[2]]);
            let along = (a[0] * b[0] + a[2] * b[2]) * 0.5 + 0.5;
            speed *= if 0.1 <= along { along } else { 0.1 };
        }
        let straight = |spot: Vec3| {
            let dir = normalise([spot[0] - me.position[0], 0.0, spot[2] - me.position[2]]);
            FollowMove::Direct {
                dir,
                dist: d2.sqrt(),
            }
        };
        let action = if speed <= 0.0 {
            FollowMove::Stop
        } else if d2 < 2500.0 {
            self.direct = true;
            straight(spot)
        } else {
            if self.direct {
                if d2 < 10_000.0 {
                    let moved = ((me.position[0] - self.last_position[0]).powi(2)
                        + (me.position[1] - self.last_position[1]).powi(2)
                        + (me.position[2] - self.last_position[2]).powi(2))
                    .sqrt();
                    if moved <= me.speed * me.speed {
                        self.direct = false;
                    }
                } else {
                    self.direct = false;
                }
            }
            if self.direct {
                straight(spot)
            } else {
                FollowMove::GoTo(spot)
            }
        };
        self.last_position = me.position;
        FollowStep {
            cap: speed,
            action,
            spot,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(x: f32, speed: f32) -> Body {
        Body {
            position: [x, 0.0, 0.0],
            facing: [-1.0, 0.0, 0.0],
            speed,
        }
    }

    #[test]
    fn standing_leader_speeds() {
        let mut f = FollowLeader::default();
        // leader at 0 facing -x, spot 30 behind; follower 10 from the spot: stands
        let s = f.update(body(40.0, 0.0), body(0.0, 0.0), Some([30.0, 0.0, 0.0]), 0.1);
        assert_eq!(s.action, FollowMove::Stop);
        // 40 from the spot: 10 + 10 * (1600 - 400) / 2100, straight at it
        let s = f.update(body(70.0, 0.0), body(0.0, 0.0), Some([30.0, 0.0, 0.0]), 0.1);
        assert!((s.cap - (10.0 + 10.0 * 1200.0 / 2100.0)).abs() < 1e-4);
        assert!(matches!(s.action, FollowMove::Direct { .. }));
        // far: path there
        let s = f.update(
            body(400.0, 0.0),
            body(0.0, 0.0),
            Some([30.0, 0.0, 0.0]),
            0.1,
        );
        assert!(matches!(s.action, FollowMove::GoTo(_)));
    }

    #[test]
    fn moving_leader_paces_by_distance() {
        let mut f = FollowLeader {
            facing: [-1.0, 0.0, 0.0],
            ..Default::default()
        };
        // leader moving 15/s along -x: spot 15 ahead of it plus the offset
        let s = f.update(
            body(40.0, 15.0),
            body(0.0, 15.0),
            Some([30.0, 0.0, 0.0]),
            0.0,
        );
        assert_eq!(s.spot, [-15.0 + 30.0, 0.0, 0.0]);
        assert!((s.cap - 25.0).abs() < 1e-4);
    }

    #[test]
    fn trailing_another_platoon() {
        let mut f = FollowLeader::default();
        let s = f.update(body(100.0, 0.0), body(0.0, 0.0), None, 0.1);
        assert_eq!(s.spot, [40.0, 0.0, 0.0]);
    }
}
