//! Hunger: the stomach digests into the hunger level; an empty stomach makes it fall at the
//! character's hunger rate; below zero the character starves (MedicalSystem::update).
//!
//! Specs: sheet rows F040466 MedicalSystem::updateHunger, F047009 CharStats::hungerRate,
//! F032933 starvationPeriod; fields T00028 hunger/stomach, T00026 hunger_rate/load.

/// Highest hunger level (F040466 caps it here).
pub const HUNGER_MAX: f32 = 3.0;
/// Food moved into an empty stomach at once (F040466).
pub const STOMACH_CHUNK: f32 = 0.04;

/// What the character is doing, for the hunger rate (F047009).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Activity {
    /// In bed, knocked out or dead.
    Resting,
    /// Carrying a heavy load; `load` is CharStats +0x190.
    Encumbered {
        load: f32,
    },
    /// The special state that uses the fixed 0.8 rate.
    Special,
    Normal,
}

/// @fn F047009
///
/// `activity_factor` is the character state's multiplier (+0x70), `base` the stats' rate (+0x178).
pub fn hunger_rate(
    activity: Activity,
    activity_factor: f32,
    base: f32,
    bed_hunger_rate: f32,
    encumbrance_hunger_rate: f32,
) -> f32 {
    activity_factor
        * match activity {
            Activity::Resting => bed_hunger_rate,
            Activity::Encumbered { load } => (1.0 - load) * encumbrance_hunger_rate + 1.0,
            Activity::Special => 0.8,
            Activity::Normal => base,
        }
}

/// @fn F032933
///
/// Seconds over which a full hunger level is used up: starvation time * the time scale.
pub fn starvation_period(starvation_time: f32, time_scale: f32) -> f32 {
    starvation_time * time_scale
}

/// @type MedicalSystem
///
/// The hunger fields of the medical system.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Hunger {
    pub level: f32,
    pub stomach: f32,
    /// Food eaten and not yet in the stomach (the character's digest queue, +0x4bc), in
    /// hunger units.
    pub queued: f32,
}

/// The most hunger shows (F039256: a bite fills up to it).
pub const HUNGER_SHOWN_MAX: f32 = 300.0;
/// The most one bite takes (F039256).
pub const BITE_MAX: f32 = 50.0;

/// @fn F039256
///
/// How much one bite eats of a food with `left` food in it, the eater's hunger at `shown`
/// (0..300): what is left, the room under 300, at most 50. (not ported) items of class 0x11
/// count as 30 and are used up whole.
pub fn bite(left: f32, shown: f32) -> f32 {
    left.min(HUNGER_SHOWN_MAX - shown).clamp(0.0, BITE_MAX)
}

impl Hunger {
    /// @fn F040466
    ///
    /// Advance by `dt` game seconds. `max` is the character's max hunger, `period` the
    /// starvation period (no change when it is 0 or the game is paused), `rate` the hunger rate,
    /// `size` the character size, `fed` the fed recovery rate mult, `activity_factor` as for
    /// `hunger_rate`.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f32,
        max: f32,
        period: f32,
        rate: f32,
        size: f32,
        fed: f32,
        activity_factor: f32,
        paused: bool,
    ) {
        if self.level < max && self.stomach <= 0.0 {
            self.stomach = (max - self.level).clamp(0.0, STOMACH_CHUNK);
        }
        if self.level > HUNGER_MAX {
            self.level = HUNGER_MAX;
        }
        if period > 0.0 && !paused {
            let step = dt / period;
            if self.stomach <= 0.0 {
                self.level -= step * rate;
            } else {
                let a = (step * size * fed * activity_factor).min(self.stomach);
                self.stomach -= a;
                self.level += a;
            }
        }
    }

    pub fn starving(&self) -> bool {
        self.level < 0.0
    }

    /// Hunger as the game shows it (0..300).
    pub fn shown(&self) -> f32 {
        self.level * 100.0
    }

    /// F039256's last step: a bite's food joins the digest queue (bite x 0.01).
    pub fn eat(&mut self, bite: f32) {
        self.queued += bite * 0.01;
    }

    /// One hunger tick: the stomach takes its next chunk from the digest queue, then
    /// F040466 runs. (assumption) F040466's "max hunger" is the hunger the queued food would
    /// reach: the stomach only ever holds food that was eaten.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(&mut self, dt: f32, period: f32, rate: f32, size: f32, fed: f32, activity_factor: f32) {
        if self.stomach <= 0.0 && self.queued > 0.0 {
            let chunk = self.queued.min(STOMACH_CHUNK);
            self.stomach = chunk;
            self.queued -= chunk;
        }
        let level = self.level;
        self.update(dt, level, period, rate, size, fed, activity_factor, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eating_fills_then_digests() {
        // a bite: what is left, the room under 300, at most 50
        assert_eq!(bite(30.0, 100.0), 30.0);
        assert_eq!(bite(80.0, 100.0), 50.0);
        assert_eq!(bite(80.0, 280.0), 20.0);
        let mut h = Hunger { level: 1.0, stomach: 0.0, queued: 0.0 };
        h.eat(30.0);
        assert!((h.queued - 0.3).abs() < 1e-6);
        // digesting moves it into hunger, never more than was eaten
        for _ in 0..100_000 {
            h.tick(1.0, 600.0, 1.0, 1.0, 3.0, 1.0);
            if h.queued <= 0.0 && h.stomach <= 0.0 {
                break;
            }
        }
        assert!(h.level > 1.2 && h.level <= 1.3 + 1e-4, "{}", h.level);
        // with nothing eaten it only falls
        let before = h.level;
        h.tick(60.0, 600.0, 1.0, 1.0, 3.0, 1.0);
        assert!(h.level < before);
    }

    #[test]
    fn rates_by_activity() {
        assert_eq!(hunger_rate(Activity::Resting, 1.0, 1.0, 0.33, 0.7), 0.33);
        assert_eq!(hunger_rate(Activity::Normal, 2.0, 1.5, 0.33, 0.7), 3.0);
        assert!(
            (hunger_rate(Activity::Encumbered { load: 0.0 }, 1.0, 1.0, 0.33, 0.7) - 1.7).abs()
                < 1e-6
        );
        assert_eq!(hunger_rate(Activity::Special, 1.0, 1.0, 0.33, 0.7), 0.8);
    }

    #[test]
    fn digests_then_starves() {
        // full stomach chunk digests into hunger
        let mut h = Hunger {
            level: 1.0,
            stomach: 0.0,
            queued: 0.0,
        };
        h.update(1.0, 2.0, 10.0, 1.0, 1.0, 3.0, 1.0, false);
        assert!((h.level - 1.04).abs() < 1e-6 && h.stomach == 0.0);
        // at max, nothing is pulled into the stomach: hunger falls by dt/period*rate
        let mut h = Hunger {
            level: 2.0,
            stomach: 0.0,
            queued: 0.0,
        };
        h.update(5.0, 2.0, 10.0, 0.5, 1.0, 3.0, 1.0, false);
        assert!((h.level - 1.75).abs() < 1e-6);
        // paused or period 0: unchanged
        let before = h;
        h.update(5.0, 1.0, 10.0, 0.5, 1.0, 3.0, 1.0, true);
        assert_eq!(h.level, before.level);
        // capped
        let mut h = Hunger {
            queued: 0.0,
            level: 5.0,
            stomach: 0.0,
        };
        h.update(0.0, 10.0, 0.0, 0.0, 1.0, 1.0, 1.0, false);
        assert_eq!(h.level, HUNGER_MAX);
        let starving = Hunger {
            level: -0.1,
            stomach: 0.0,
            queued: 0.0,
        };
        assert!(starving.starving());
        assert_eq!(starvation_period(60.0, 2.0), 120.0);
    }
}
