//! An animation layer: the clips playing on part of a body, blended by weight, with clips fading
//! out after they stop.
//!
//! Specs: sheet rows F038900 AnimationLayer::play, F038905 AnimationLayer::update, F038881 /
//! F038883 / F038884 the list lookups, F038861 fadeOut; type T00040 AnimationLayer.

use crate::clip::SingleAnimation;

/// @type AnimationLayer
#[derive(Debug, Clone)]
pub struct AnimationLayer<E> {
    pub active: Vec<SingleAnimation<E>>,
    pub fading: Vec<SingleAnimation<E>>,
    /// Weight change per second (+0x40).
    pub fade_rate: f32,
}

/// How to (re)start a clip (F038900's parameters).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayParams {
    pub speed: f32,
    pub looping: bool,
    pub target_weight: f32,
    pub end_time: f32,
    pub root_motion: bool,
    /// Set on a newly created clip only.
    pub keep_weight: bool,
}

/// Which list and index the dominant clip is at.
type Slot = (bool, usize);

/// The layers a character's animation class creates, by blend multiplier (F040013): base
/// (movement), action, overlay, and two more.
pub const LAYER_BLEND: [f32; 5] = [2.0, 1.25, 1.0, 0.5, 0.5];

impl<E: Clone> AnimationLayer<E> {
    /// @fn F040175
    ///
    /// A layer fading at `blend_rate` (GLOBAL CONSTANTS "animation blend rate") times `multiplier`.
    pub fn with_blend(blend_rate: f32, multiplier: f32) -> Self {
        Self::new(blend_rate * multiplier)
    }

    pub fn new(fade_rate: f32) -> Self {
        AnimationLayer {
            active: Vec::new(),
            fading: Vec::new(),
            fade_rate,
        }
    }

    /// @fn F038881
    pub fn find_active(&self, name: &str) -> Option<&SingleAnimation<E>> {
        self.active.iter().find(|c| c.name == name)
    }

    /// @fn F038900
    ///
    /// Start `name` (or restart it, pulling it back if it is fading out); `make` builds a new clip.
    pub fn play(&mut self, name: &str, p: PlayParams, make: impl FnOnce() -> SingleAnimation<E>) {
        let set = |c: &mut SingleAnimation<E>| {
            c.speed = p.speed;
            c.looping = p.looping;
            c.target_weight = p.target_weight;
            c.stop_at_end = true;
            c.playing = true;
            c.end_time = p.end_time;
            c.root_motion = p.root_motion;
        };
        if let Some(c) = self.active.iter_mut().find(|c| c.name == name) {
            set(c);
            return;
        }
        // F038884: take it back from the fading list
        let mut c = match self.fading.iter().position(|c| c.name == name) {
            Some(i) => self.fading.remove(i),
            None => {
                let mut c = make();
                c.keep_weight = p.keep_weight;
                c
            }
        };
        set(&mut c);
        self.active.push(c);
    }

    /// @fn F038861
    ///
    /// Stop `name`: move it from the active list (F038883) to the fading list.
    pub fn stop(&mut self, name: &str) {
        if let Some(i) = self.active.iter().position(|c| c.name == name) {
            let c = self.active.remove(i);
            self.fading.push(c);
        }
    }

    /// @fn F038905
    ///
    /// Advance every clip by `dt`; returns the dominant clip's events. `instant` snaps weights to
    /// their targets; `normalise` keeps the layer's weights summing to `max_weight`.
    pub fn update(
        &mut self,
        phase: f32,
        dt: f32,
        instant: bool,
        normalise: bool,
        max_weight: f32,
    ) -> Vec<E> {
        // the dominant clip: the heaviest one with events, over both lists
        let mut dominant: Option<(Slot, f32)> = None;
        for (fading, list) in [(false, &self.active), (true, &self.fading)] {
            for (i, c) in list.iter().enumerate() {
                if !c.events.is_empty() && dominant.is_none_or(|d| d.1 < c.weight) {
                    dominant = Some(((fading, i), c.weight));
                }
            }
        }
        let dominant = dominant.map(|d| d.0);
        let active_sum: f32 = self.active.iter().map(|c| c.weight).sum();
        let avail = max_weight - self.fading.iter().map(|c| c.weight).sum::<f32>();
        let scale = if normalise && 0.0 < active_sum {
            (1.0 / active_sum) * avail
        } else {
            1.0
        };
        let mut fired = Vec::new();
        let mut sum = 0.0;
        for i in (0..self.active.len()).rev() {
            let c = &mut self.active[i];
            if c.start_near_end {
                c.start_near_end = false;
                c.time = c.length * 0.96;
            }
            if !c.keep_weight || 1.0 < scale {
                c.weight *= scale;
            }
            let rate = if instant {
                999999.0
            } else {
                c.speed.clamp(0.5, 10.0) * self.fade_rate
            };
            c.fade_weight(dt, rate);
            fired.extend(c.update(phase, dt, dominant == Some((false, i))));
            sum += c.weight;
        }
        if normalise && max_weight < sum && 0.0 < sum {
            for c in &mut self.active {
                c.weight *= max_weight / sum;
            }
            sum = max_weight;
        }
        let mut fading_sum = 0.0;
        for i in (0..self.fading.len()).rev() {
            let c = &mut self.fading[i];
            c.target_weight = 0.0;
            c.fade_weight(dt, self.fade_rate);
            if c.looping || c.reverse_fading {
                let d = if c.looping { dt } else { -dt };
                fired.extend(c.update(phase, d, dominant == Some((true, i))));
            }
            fading_sum += c.weight;
        }
        if normalise && !self.active.is_empty() && !self.fading.is_empty() {
            let n = self.fading.len() as f32;
            for c in &mut self.fading {
                c.weight = if fading_sum == 0.0 {
                    (max_weight - sum) / n
                } else {
                    (max_weight - sum) / fading_sum * c.weight
                };
            }
        }
        for c in &mut self.fading {
            if c.weight == 0.0 {
                c.progress = 1.0;
            }
        }
        self.fading.retain(|c| c.weight != 0.0);
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(looping: bool) -> PlayParams {
        PlayParams {
            speed: 1.0,
            looping,
            target_weight: 1.0,
            end_time: 1.0,
            root_motion: false,
            keep_weight: false,
        }
    }

    #[test]
    fn a_new_clip_fades_in_as_the_old_fades_out() {
        let mut l = AnimationLayer::<()>::new(5.0);
        l.play("idle", params(true), || {
            SingleAnimation::new("idle", 2.0, 1.0, 1.0, true, vec![])
        });
        l.update(0.0, 1.0, true, true, 1.0);
        assert_eq!(l.active[0].weight, 1.0);
        l.stop("idle");
        l.play("chop down", params(false), || {
            SingleAnimation::new("chop down", 0.97, 1.0, 0.95, false, vec![])
        });
        l.update(0.0, 0.1, false, true, 1.0);
        let (chop, idle) = (l.active[0].weight, l.fading[0].weight);
        assert!((chop - 0.5).abs() < 1e-5, "{chop}"); // rate 5/s for 0.1 s
        assert!(
            (chop + idle - 1.0).abs() < 1e-5,
            "weights sum to the layer's max"
        );
        for _ in 0..5 {
            l.update(0.0, 0.1, false, true, 1.0);
        }
        assert!(l.fading.is_empty());
        assert_eq!(l.active[0].weight, 1.0);
        // playing it again while it fades out pulls it back
        l.stop("chop down");
        l.play("chop down", params(false), || unreachable!());
        assert_eq!(l.active.len(), 1);
    }
}
