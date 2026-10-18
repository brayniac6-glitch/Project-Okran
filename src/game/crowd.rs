//! People outside a fight don't stand inside one another (F024363, Kenshi's own push in its AI
//! mover, `kenshi_char::movement::neighbour_push`, as crate::combat's arena `separate` is for
//! fighters): each free person takes its share of the deepest overlap with a neighbour, the
//! smaller one more, along the line between them, and its next steer bends round it. One that
//! stays put (seated or operating a machine, in bed, at its node, caged) is not moved, but
//! pushes. (stand-in for the mover's list of nearby characters) the neighbours are everyone
//! within 1.5 m of its height (another floor is not one).

use kenshi_char::movement::{neighbour_push, Neighbour};
use std::collections::HashMap;

/// One person as the crowd sees it.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub id: usize,
    /// metres
    pub position: [f32; 3],
    pub radius: f32,
    /// Moved by others (else it stays put and only pushes).
    pub free: bool,
}

/// (stand-in for rand()) the same numbers for the same two people at the same spot.
fn chance(seed: u64) -> impl FnMut() -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Each free person pushed this step: id, the normal away from its deepest neighbour, and the
/// depth (metres) it moves along it.
pub fn pushes(bodies: &[Body]) -> Vec<(usize, [f32; 3], f32)> {
    const CELL: f32 = 1.5;
    let cell = |p: [f32; 3]| ((p[0] / CELL).floor() as i32, (p[2] / CELL).floor() as i32);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (k, b) in bodies.iter().enumerate() {
        grid.entry(cell(b.position)).or_default().push(k);
    }
    let mut out = Vec::new();
    let mut near = Vec::new();
    for (a, b) in bodies.iter().enumerate() {
        if !b.free {
            continue;
        }
        near.clear();
        let (cx, cz) = cell(b.position);
        for dx in -1..=1 {
            for dz in -1..=1 {
                for &c in grid.get(&(cx + dx, cz + dz)).into_iter().flatten() {
                    let o = &bodies[c];
                    if c != a && (o.position[1] - b.position[1]).abs() <= 1.5 {
                        near.push(Neighbour { position: o.position, radius: o.radius });
                    }
                }
            }
        }
        let seed = (b.id as u64) << 32 ^ (b.position[0].to_bits() as u64) ^ (b.position[2].to_bits() as u64) << 7;
        if let Some((n, depth)) = neighbour_push(b.position, b.radius, &near, false, chance(seed)) {
            out.push((b.id, n, depth));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(id: usize, x: f32, free: bool) -> Body {
        Body { id, position: [x, 0.0, 0.0], radius: 0.4, free }
    }

    #[test]
    fn overlapping_people_part() {
        // overlapping by 0.3: each takes half (equal radii), opposite ways
        let p = pushes(&[body(0, 0.0, true), body(1, 0.5, true)]);
        assert_eq!(p.len(), 2);
        assert!((p[0].1[0] + 1.0).abs() < 1e-5 && (p[0].2 - 0.15).abs() < 1e-5);
        assert!((p[1].1[0] - 1.0).abs() < 1e-5 && (p[1].2 - 0.15).abs() < 1e-5);
        // one seated: it stays, the other still takes only its share
        let p = pushes(&[body(0, 0.0, false), body(1, 0.5, true)]);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].0, 1);
        assert!((p[0].2 - 0.15).abs() < 1e-5);
        // the smaller gives way more: 0.2 against 0.6 overlapping by 0.4
        let mut small = body(0, 0.0, true);
        small.radius = 0.2;
        let mut big = body(1, 0.4, true);
        big.radius = 0.6;
        let p = pushes(&[small, big]);
        assert_eq!(p.len(), 1);
        assert!(p[0].0 == 0 && (p[0].2 - 0.4).abs() < 1e-5);
        // on the same spot: a random way, as deep as the other's radius
        let p = pushes(&[body(0, 0.0, true), body(1, 0.0, true)]);
        assert_eq!(p.len(), 2);
        assert!((p[0].2 - 0.4).abs() < 1e-5);
        // apart already, or on another floor: nothing
        assert!(pushes(&[body(0, 0.0, true), body(1, 0.9, true)]).is_empty());
        let mut up = body(1, 0.0, true);
        up.position[1] = 3.0;
        assert!(pushes(&[body(0, 0.0, true), up]).is_empty());
        // both staying put: nothing
        assert!(pushes(&[body(0, 0.0, false), body(1, 0.0, false)]).is_empty());
    }
}
