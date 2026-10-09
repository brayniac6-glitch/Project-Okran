//! The world's resources (port_map `base`): how much iron, stone, copper, carbon, water or
//! farming a place has, from the biome's ENVIRONMENT_RESOURCES ("resources" of the BIOME_GROUP).
//!
//! Specs: F051624 (the level of a resource type at a place), F051582 (the type's field prefix),
//! F041781 / F041780 / F041778 (the stone noise: five octaves of smoothed value noise with
//! cosine interpolation, the classic lattice hash), F050480 (a ramp (x - a) / (b - a)).

use kenshi_data::GameData;

/// A resource type (BUILDING_FUNCTIONALITY "world resource mining"; F051582's names).
pub fn resource_name(kind: i32) -> Option<&'static str> {
    Some(match kind {
        1 => "iron",
        2 => "stone",
        3 => "copper",
        4 => "carbon",
        5 => "water",
        6 => "farming",
        _ => return None,
    })
}

/// @fn F041778
///
/// The smoothed lattice noise at integer cell (x, y): the classic hash of x + 57 y (to -1..1),
/// a quarter of the cell's, an eighth of each side's and a sixteenth of each corner's.
fn smooth_noise(x: f32, y: f32) -> f32 {
    fn hash(x: i32, y: i32) -> f64 {
        let n = x.wrapping_add(y.wrapping_mul(57));
        let n = (n << 13) ^ n;
        let v = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19990303)).wrapping_add(1376312589) & 0x7fff_ffff;
        1.0 - v as f64 / 1_073_741_824.0
    }
    // the cell and its neighbours, as the game truncates them (x - 1, x + 1 in float first)
    let (xi, yi) = (x as i32, y as i32);
    let (xm, xp, ym, yp) = ((x - 1.0) as i32, (x + 1.0) as i32, (y - 1.0) as i32, (y + 1.0) as i32);
    let corners = hash(xm, ym) + hash(xp, ym) + hash(xm, yp) + hash(xp, yp);
    let sides = hash(xm, yi) + hash(xp, yi) + hash(xi, ym) + hash(xi, yp);
    (corners * 0.0625 + sides * 0.125 + hash(xi, yi) * 0.25) as f32
}

/// @fn F041780
///
/// The noise at (x, y) between the four cells round it, cosine interpolated.
fn interpolated_noise(x: f32, y: f32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let cosine = |t: f32| ((1.0 - (t as f64 * std::f64::consts::PI).cos()) * 0.5) as f32;
    let (tx, ty) = (cosine(x - fx), cosine(y - fy));
    let v1 = smooth_noise(fx, fy);
    let v2 = smooth_noise(fx + 1.0, fy);
    let v3 = smooth_noise(fx, fy + 1.0);
    let v4 = smooth_noise(fx + 1.0, fy + 1.0);
    let a = v1 * (1.0 - tx) + v2 * tx;
    let b = v3 * (1.0 - tx) + v4 * tx;
    a * (1.0 - ty) + b * ty
}

/// @fn F041781
///
/// The stone noise at world (x, z): five octaves (frequency 2^i over `scale`, weight 0.5^i)
/// summed, brought to 0..1 (x 0.5 + 0.5); none at or under `cutoff`, above it ramped from 0 at
/// the cutoff to 1, then x 3 and kept to 0..1.
pub fn stone_noise(x: f32, z: f32, scale: f32, cutoff: f32) -> f32 {
    let mut sum = 0.0f64;
    for i in 0..5 {
        let freq = 2f32.powi(i);
        let amp = 0.5f64.powi(i);
        sum += interpolated_noise(freq * x / scale, freq * z / scale) as f64 * amp;
    }
    let v = sum * 0.5 + 0.5;
    let r = if v <= cutoff as f64 { 0.0 } else { (v - cutoff as f64) / (1.0 - cutoff as f64) };
    ((r * 3.0).max(0.0) as f32).min(1.0)
}

/// @fn F051624
///
/// The level of resource `kind` at world position `at` (units) under the biome's
/// ENVIRONMENT_RESOURCES record `res`:
/// - water and farming: within the "<name> altitude min" to "max" band 1, fading to 0 over
///   "<name> altitude fade" outside it; the band x "<name> mult" + "<name> min";
/// - stone: the stone noise (zoom "stone noise zoom" x 1000, "stone noise cutoff") x "stone
///   mult";
/// - the others: "<name> mult".
pub fn resource_at(res: &GameData, kind: i32, at: [f32; 3]) -> f32 {
    let Some(name) = resource_name(kind) else { return 0.0 };
    let f = |k: &str| res.floats.get(k).copied().or_else(|| res.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
    match kind {
        5 | 6 => {
            let (min, max, fade) = (f(&format!("{name} altitude min")), f(&format!("{name} altitude max")), f(&format!("{name} altitude fade")));
            let y = at[1];
            let band = if min <= y {
                if y < max {
                    1.0
                } else if y <= max + fade {
                    ramp(y, max + fade, max)
                } else {
                    0.0
                }
            } else if min - fade <= y {
                ramp(y, min - fade, min)
            } else {
                0.0
            };
            band * f(&format!("{name} mult")) + f(&format!("{name} min"))
        }
        2 => stone_noise(at[0], at[2], f("stone noise zoom") * 1000.0, f("stone noise cutoff")) * f("stone mult"),
        _ => f(&format!("{name} mult")),
    }
}

/// @fn F050480
///
/// (x - a) / (b - a).
fn ramp(x: f32, a: f32, b: f32) -> f32 {
    if b == a { 1.0 } else { ((x - a) / (b - a)).clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_in_range_and_varies() {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for i in 0..400 {
            let v = stone_noise(i as f32 * 137.0, i as f32 * 71.0, 1000.0, 0.4);
            assert!((0.0..=1.0).contains(&v));
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(hi > lo);
        // the lattice hash of the origin: 1 - 1376312589 / 2^30
        let h = smooth_noise(0.0, 0.0);
        assert!(h.abs() <= 1.0);
    }

    #[test]
    fn ramp_ends() {
        assert_eq!(ramp(5.0, 0.0, 10.0), 0.5);
        assert_eq!(ramp(12000.0, 17000.0, 12000.0), 1.0);
    }
}
