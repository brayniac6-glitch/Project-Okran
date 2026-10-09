//! ParticleUniverse's dynamic attributes (ParticleUniverseDynamicAttribute): a value that is
//! fixed, random between a min and max, a curve through control points (linear or Ogre's
//! SimpleSpline), or an oscillation, read at a time x.

use super::math::Rand;
use super::script::Node;
use bevy::math::Vec2;

#[derive(Debug, Clone, PartialEq)]
pub enum Dyn {
    Fixed(f32),
    Random { min: f32, max: f32 },
    /// Control points sorted by x (processControlPoints, done when cloned from the template).
    Linear(Vec<Vec2>),
    Spline(Spline),
    Oscillate { square: bool, frequency: f32, phase: f32, base: f32, amplitude: f32 },
}

impl Dyn {
    pub fn get(&self, x: f32, rng: &mut Rand) -> f32 {
        match self {
            Dyn::Fixed(v) => *v,
            Dyn::Random { min, max } => rng.range(*min, *max),
            Dyn::Linear(points) => {
                if points.is_empty() {
                    return 0.0;
                }
                let i1 = nearest(points, x);
                match points.get(i1 + 1) {
                    Some(p2) => {
                        let p1 = points[i1];
                        p1.y + (p2.y - p1.y) * (x - p1.x) / (p2.x - p1.x)
                    }
                    None => points[i1].y,
                }
            }
            Dyn::Spline(s) => {
                if s.points.is_empty() {
                    return 0.0;
                }
                let f = x / s.range;
                s.interpolate(if f < 1.0 { f } else { 1.0 }).y
            }
            Dyn::Oscillate { square, frequency, phase, base, amplitude } => {
                let s = (phase + frequency * x * std::f32::consts::TAU).sin();
                if *square {
                    // Math::Sign
                    base + amplitude * if s > 0.0 { 1.0 } else if s < 0.0 { -1.0 } else { 0.0 }
                } else {
                    base + amplitude * s
                }
            }
        }
    }

    pub fn is_fixed(&self) -> bool {
        matches!(self, Dyn::Fixed(_))
    }

    /// A dynamic attribute object (`dyn_random`, `dyn_curved_linear`, `dyn_curved_spline`,
    /// `dyn_oscillate`; anything else is fixed at 0).
    pub fn from_node(n: &Node) -> Dyn {
        let f = |name: &str| n.props.iter().rev().find(|p| p.name == name).and_then(|p| p.f(0));
        match n.name.as_str() {
            "dyn_random" => Dyn::Random { min: f("min").unwrap_or(0.0), max: f("max").unwrap_or(0.0) },
            "dyn_curved_linear" | "dyn_curved_spline" => {
                let mut points: Vec<Vec2> = n
                    .props
                    .iter()
                    .filter(|p| p.name == "control_point")
                    .filter_map(|p| Some(Vec2::new(p.f(0)?, p.f(1)?)))
                    .collect();
                // std::sort with ControlPointSorter (by x)
                points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                if n.name == "dyn_curved_linear" {
                    Dyn::Linear(points)
                } else {
                    Dyn::Spline(Spline::new(points))
                }
            }
            "dyn_oscillate" => {
                let ty = n.props.iter().rev().find(|p| p.name == "oscillate_type");
                Dyn::Oscillate {
                    square: ty.and_then(|p| p.string()) == Some("square"),
                    frequency: f("oscillate_frequency").unwrap_or(1.0),
                    phase: f("oscillate_phase").unwrap_or(0.0),
                    base: f("oscillate_base").unwrap_or(0.0),
                    amplitude: f("oscillate_amplitude").unwrap_or(1.0),
                }
            }
            _ => Dyn::Fixed(0.0),
        }
    }
}

/// _findNearestControlPointIterator: the last point at or before x (the first if x is before
/// them all).
fn nearest(points: &[Vec2], x: f32) -> usize {
    for (i, p) in points.iter().enumerate() {
        if x < p.x {
            return i.saturating_sub(1);
        }
    }
    points.len() - 1
}

/// Ogre::SimpleSpline through the control points (z 0): Catmull-Rom tangents, Hermite segments,
/// evenly spaced in t over the points; `range` the last point's x minus the first's.
#[derive(Debug, Clone, PartialEq)]
pub struct Spline {
    pub points: Vec<Vec2>,
    tangents: Vec<Vec2>,
    pub range: f32,
}

impl Spline {
    fn new(points: Vec<Vec2>) -> Spline {
        let range = match (points.first(), points.last()) {
            (Some(a), Some(b)) => b.x - a.x,
            _ => 0.0,
        };
        let n = points.len();
        let mut tangents = vec![Vec2::ZERO; n];
        if n >= 2 {
            let closed = points[0] == points[n - 1];
            for i in 0..n {
                tangents[i] = if i == 0 {
                    if closed {
                        0.5 * (points[1] - points[n - 2])
                    } else {
                        0.5 * (points[1] - points[0])
                    }
                } else if i == n - 1 {
                    if closed {
                        tangents[0]
                    } else {
                        0.5 * (points[i] - points[i - 1])
                    }
                } else {
                    0.5 * (points[i + 1] - points[i - 1])
                };
            }
        }
        Spline { points, tangents, range }
    }

    fn interpolate(&self, t: f32) -> Vec2 {
        let seg = t * (self.points.len() - 1) as f32;
        let idx = seg as usize;
        self.segment(idx, seg - idx as f32)
    }

    fn segment(&self, i: usize, t: f32) -> Vec2 {
        if i + 1 >= self.points.len() {
            return self.points[i.min(self.points.len() - 1)];
        }
        if t == 0.0 {
            return self.points[i];
        } else if t == 1.0 {
            return self.points[i + 1];
        }
        let (t2, t3) = (t * t, t * t * t);
        let h1 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h2 = -2.0 * t3 + 3.0 * t2;
        let h3 = t3 - 2.0 * t2 + t;
        let h4 = t3 - t2;
        self.points[i] * h1 + self.points[i + 1] * h2 + self.tangents[i] * h3 + self.tangents[i + 1] * h4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves() {
        let mut r = Rand::default();
        let l = Dyn::Linear(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 10.0)]);
        assert_eq!(l.get(0.5, &mut r), 5.0);
        assert_eq!(l.get(2.0, &mut r), 10.0);
        assert_eq!(l.get(-1.0, &mut r), -10.0);
        let s = Dyn::Spline(Spline::new(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]));
        assert!((s.get(0.5, &mut r) - 0.5).abs() < 1e-5);
    }
}
