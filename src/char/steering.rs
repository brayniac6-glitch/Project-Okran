//! Formation steering: keeping within a band of distances around a target (circling an enemy
//! while waiting for a turn to attack), with the speed limits the original applies close in.
//!
//! The original's steering also pushes away from nearby characters and alternates strafing with
//! walking forward; those parts are not ported yet (the neighbour query belongs to the movement
//! replacement).
//!
//! Specs: sheet rows F029535 FormationSteering::setBand, F030319 bandForce, F029549
//! FormationSteering::update (the band and speed parts), F039843 CombatClass::circleDistance.

/// The band around the target (FormationSteering +0x118 inner, +0x11c outer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub inner: f32,
    pub outer: f32,
    /// Sideways (orbit) speed (+0x114).
    pub orbit: f32,
}

impl Band {
    /// @fn F029535
    ///
    /// The band for distances `min`..`max` (centre to centre) for a character of `radius`
    /// (+0x110): both shrink by the radius; the inner edge stays at least 1 and the outer at least
    /// 5 beyond it.
    pub fn new(min: f32, max: f32, radius: f32, orbit: f32) -> Band {
        let maxc = (max - radius).clamp(0.0, 9999.0);
        let inner = (min - radius).min(maxc).max(1.0);
        let outer = maxc.min(99999.0).max(inner + 5.0);
        Band {
            inner,
            outer,
            orbit,
        }
    }
}

/// @fn F030319
///
/// The pull toward the target at distance `dist` against band edge `limit`: positive (toward)
/// beyond it, negative (away) inside it, 100 per band width.
pub fn band_force(limit: f32, dist: f32) -> f32 {
    -(1.0 - (1.0 / limit) * dist) * 100.0
}

/// @fn F039843
///
/// The outer distance a fighter circles at: max reach (CharStats +0x150) + radius (CombatClass
/// +0x278) - half the extra reach (F039784).
pub fn circle_distance(max_reach: f32, radius: f32, extra_reach: f32) -> f32 {
    ((max_reach + radius) as f64 - extra_reach as f64 * 0.5) as f32
}

/// The steering's running state (+0x12c speed ramp, +0xe4 far flag).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Steering {
    pub speed: f32,
    pub far: bool,
}

/// @fn F029549
///
/// One tick of band keeping: the desired velocity (units/s) for a character at `pos` around a
/// target at `target` (collision radius `target_radius`), and the direction to face (the
/// target). `move_speed` is CharStats +0x17c; `cap` the order's speed limit (+0x128, 200 for
/// combat circling).
#[allow(clippy::too_many_arguments)]
pub fn steer_band(
    s: &mut Steering,
    band: &Band,
    pos: [f32; 3],
    target: [f32; 3],
    target_radius: f32,
    move_speed: f32,
    cap: f32,
    dt: f32,
) -> ([f32; 3], [f32; 3]) {
    let to = [target[0] - pos[0], 0.0, target[2] - pos[2]];
    let dist = (to[0] * to[0] + to[2] * to[2]).sqrt();
    let facing = if dist > 0.0 {
        [to[0] / dist, 0.0, to[2] / dist]
    } else {
        [0.0, 0.0, 1.0]
    };
    // toward the target beyond the outer edge, away inside the inner edge
    let mut radial = band_force(target_radius + band.outer, dist) * 2.0;
    if radial < 0.0 {
        radial = band_force(target_radius + band.inner, dist).min(0.0);
    }
    let mut v = facing.map(|f| f * radial);
    // the far flag: set beyond 90 of free distance, cleared within 70
    let free = dist - target_radius;
    if 90.0 < free {
        s.far = true;
    }
    if free < 70.0 {
        s.far = false;
    }
    // the speed ramps up at 15/s toward the lesser of the cap and the move speed
    let top = cap.min(move_speed);
    s.speed = (s.speed + dt * 15.0).min(top);
    let len = (v[0] * v[0] + v[2] * v[2]).sqrt();
    let mut limit = s.speed;
    if len > 0.0 {
        let along = (v[0] * facing[0] + v[2] * facing[2]) / len;
        if along < -0.5 {
            // backing away: slowly
            limit = 16.0;
        }
    }
    if !s.far && 18.0 < limit {
        limit = 18.0;
    }
    if limit < len {
        v = v.map(|x| x / len * limit);
    }
    (v, facing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_to_the_band() {
        let b = Band::new(4.0, 30.0, 4.0, 0.0);
        assert_eq!((b.inner, b.outer), (1.0, 26.0));
        assert!(band_force(30.0, 60.0) > 0.0 && band_force(30.0, 15.0) < 0.0);
        let mut s = Steering {
            speed: 50.0,
            far: true,
        };
        // far out: comes in at speed
        let (v, f) = steer_band(
            &mut s,
            &b,
            [0.0; 3],
            [200.0, 0.0, 0.0],
            4.0,
            90.0,
            200.0,
            0.1,
        );
        assert!(v[0] > 50.0 && f == [1.0, 0.0, 0.0]);
        // inside the band: holds
        let mut s = Steering::default();
        let (v, _) = steer_band(
            &mut s,
            &b,
            [0.0; 3],
            [20.0, 0.0, 0.0],
            4.0,
            90.0,
            200.0,
            0.1,
        );
        assert_eq!(v, [0.0; 3]);
        // too close: backs off slowly
        let mut s = Steering {
            speed: 50.0,
            far: false,
        };
        let (v, _) = steer_band(&mut s, &b, [0.0; 3], [4.5, 0.0, 0.0], 4.0, 90.0, 200.0, 0.1);
        assert!(v[0] < 0.0 && v[0] >= -16.0);
    }
}
