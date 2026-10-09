//! One playing clip with its own clock, progress and animation events, as the original's
//! animation layers keep them.
//!
//! Specs: sheet rows F038899 SingleAnimation::SingleAnimation, F038819 SingleAnimation::update;
//! type T00039 SingleAnimation.

/// @type SingleAnimation
#[derive(Debug, Clone, PartialEq)]
pub struct SingleAnimation<E> {
    pub name: String,
    pub length: f32,
    pub speed: f32,
    /// Seconds into the clip.
    pub time: f32,
    /// time / length: what combat reads as the action's progress.
    pub progress: f32,
    pub end_time: f32,
    pub looping: bool,
    /// Follows a shared phase (plus this offset) instead of its own clock.
    pub sync_offset: Option<f32>,
    pub stop_at_end: bool,
    pub playing: bool,
    /// Current and target blend weight.
    pub weight: f32,
    pub target_weight: f32,
    /// Not rescaled by the layer's normalisation.
    pub keep_weight: bool,
    /// The first layer update jumps to 96% of the clip.
    pub start_near_end: bool,
    /// Plays backwards while fading out.
    pub reverse_fading: bool,
    /// Moves the character (its root translation is extracted).
    pub root_motion: bool,
    /// (event, progress) sorted by progress.
    pub events: Vec<(E, f32)>,
    cursor: usize,
    last: f32,
}

impl<E: Clone> SingleAnimation<E> {
    /// @fn F038899
    pub fn new(
        name: &str,
        length: f32,
        speed: f32,
        end_time: f32,
        looping: bool,
        mut events: Vec<(E, f32)>,
    ) -> Self {
        events.sort_by(|a, b| a.1.total_cmp(&b.1));
        SingleAnimation {
            name: name.to_string(),
            length,
            speed,
            time: 0.0,
            progress: if speed < 0.0 { end_time - 0.01 } else { 0.01 },
            end_time,
            looping,
            sync_offset: None,
            stop_at_end: !looping,
            playing: true,
            weight: 0.0,
            target_weight: 1.0,
            keep_weight: false,
            start_near_end: false,
            reverse_fading: false,
            root_motion: false,
            events,
            cursor: 0,
            last: 0.0,
        }
    }

    /// @fn F038818
    ///
    /// Move the weight toward its target at `rate` per second without overshooting; true while
    /// the clip still has weight.
    pub fn fade_weight(&mut self, dt: f32, rate: f32) -> bool {
        let target = self.target_weight;
        let rate = if target < self.weight { -rate } else { rate };
        let w = rate * dt + self.weight;
        self.weight = w;
        if target < w && 0.0 < rate {
            self.weight = target;
        }
        if self.weight < target && rate <= 0.0 {
            self.weight = target;
        }
        if self.weight <= 0.0 {
            self.weight = 0.0;
        }
        0.0 < self.weight
    }

    /// @fn F038819
    ///
    /// Advance by `dt` (or follow `phase` when synced); returns the events passed, in order.
    pub fn update(&mut self, phase: f32, dt: f32, fire: bool) -> Vec<E> {
        let len = self.length;
        match self.sync_offset {
            None => {
                let t = dt * self.speed + self.time;
                self.time = t;
                if len < t {
                    if self.looping {
                        self.time = t - len;
                    } else {
                        self.time = len;
                        if self.stop_at_end {
                            self.playing = false;
                        }
                    }
                }
                if self.time < 0.0 {
                    if self.looping {
                        self.time += len;
                    } else {
                        self.time = 0.0;
                        if self.stop_at_end {
                            self.playing = false;
                        }
                    }
                }
            }
            Some(off) => {
                let mut p = phase + off;
                if 2.0 < p {
                    p = 0.0;
                } else if 1.0 < p {
                    p -= p.floor();
                } else if p < 0.0 {
                    p = p.fract() + 1.0;
                }
                self.time = len * p;
            }
        }
        let mut t = self.time;
        if len <= t {
            t = len;
        }
        if t <= 0.0 {
            t = 0.0;
        }
        self.time = t;
        self.progress = (1.0 / len) * t;
        self.fire_events(fire)
    }

    fn fire_events(&mut self, fire: bool) -> Vec<E> {
        let mut out = Vec::new();
        let n = self.events.len();
        if n == 0 || self.progress == self.last {
            return out;
        }
        let mut wrapped = self.progress < self.last;
        self.last = self.progress;
        if !fire {
            return out;
        }
        if wrapped && self.cursor == n {
            self.cursor = 0;
            wrapped = false;
        }
        while self.cursor < n {
            if !wrapped && self.progress < self.events[self.cursor].1 {
                break;
            }
            out.push(self.events[self.cursor].0.clone());
            self.cursor += 1;
            if wrapped && self.cursor == n {
                self.cursor = 0;
                wrapped = false;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swing_plays_once_and_fires_its_event() {
        // "chop down": 0.97 s, an "Attack" event at 40%
        let mut a =
            SingleAnimation::new("chop down", 0.97, 1.12, 0.95, false, vec![("Attack", 0.4)]);
        assert_eq!(a.progress, 0.01);
        assert!(a.update(0.0, 0.1, true).is_empty());
        let mut fired = Vec::new();
        for _ in 0..10 {
            fired.extend(a.update(0.0, 0.1, true));
        }
        assert_eq!(fired, vec!["Attack"]);
        assert_eq!(a.progress, 1.0);
        assert!(!a.playing);
    }

    #[test]
    fn looping_clips_wrap_and_refire() {
        let mut a = SingleAnimation::new(
            "walk",
            1.0,
            1.0,
            1.0,
            true,
            vec![("step", 0.25), ("step2", 0.75)],
        );
        let mut fired = Vec::new();
        for _ in 0..14 {
            fired.extend(a.update(0.0, 0.1, true));
        }
        // a lap and a bit: both steps, then the first again after the wrap
        assert_eq!(fired, vec!["step", "step2", "step"]);
        assert!(a.playing);
        // synced clips follow the phase
        let mut s = SingleAnimation::<()>::new("run", 2.0, 1.0, 1.0, true, vec![]);
        s.sync_offset = Some(0.25);
        s.update(1.5, 0.0, false);
        assert!((s.progress - 0.75).abs() < 1e-6);
    }
}
