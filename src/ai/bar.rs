//! Package_HangAroundBar (the "hang out in a bar" package's handler): which bar a squad goes to
//! and which chair each member takes there, the target of Relaxing (task 214, finder F050252
//! asks the squad's package handler, AI +0x10 -> +0xf8 -> +0x48).
//!
//! Spec: F029219 the handler's setup (the squad's town, F029191: its bars, buildings of
//! designation BD_BAR, from a random one on), F029228 the chair for a member, F029240 a member
//! letting go of its chair, F050548 randInt.

/// BuildingDesignation BD_BAR.
pub const BD_BAR: i32 = 3;

/// A chair the handler can choose (BuildingFunction 17).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chair {
    /// Index into the caller's furniture.
    pub id: usize,
    /// World position (units).
    pub position: [f32; 3],
    /// The building it stands in (placement index).
    pub building: usize,
    /// Someone else sits on it (its functionality's operator, vtable +0x300; F029228 skips a
    /// chair whose occupant is not the member, vtable +0x500).
    pub taken: bool,
}

/// @fn F050548
///
/// randInt(a, b): a + rand() / 32768 * (b - a + 1), truncated (the original multiplies by
/// -1/32768 and subtracts), then held to at most b and at least a.
pub fn rand_int(a: i32, b: i32, rand01: f32) -> i32 {
    let v = a - (-rand01 * ((b - a) + 1) as f32) as i32;
    let v = if b >= v { v } else { b };
    if v >= a { v } else { a }
}

/// @fn F029219
///
/// The bar a squad takes: of its town's bars, the first one from a random start.
pub fn choose_bar(bars: &[usize], rand01: f32) -> Option<usize> {
    if bars.is_empty() {
        return None;
    }
    let start = rand_int(0, bars.len() as i32 - 1, rand01) as usize;
    Some(bars[start % bars.len()])
}

/// @fn F029228
///
/// The chair for a member: the one it already holds while nobody else sits there; else, of the
/// bar's chairs (the town's when the bar is shut, not ported), those no other member holds and
/// nobody else sits on, the nearest to the bar. `held` is the member's chair, `others` the
/// chairs the squad's other members hold.
pub fn choose_chair(
    held: Option<usize>,
    chairs: &[Chair],
    bar: usize,
    bar_position: [f32; 3],
    others: &[usize],
) -> Option<usize> {
    if let Some(h) = held
        && chairs.iter().any(|c| c.id == h && !c.taken)
    {
        return Some(h);
    }
    let mut best = None;
    let mut d = f32::MAX;
    for c in chairs {
        if c.building != bar || others.contains(&c.id) || c.taken {
            continue;
        }
        let e = (0..3)
            .map(|k| (c.position[k] - bar_position[k]).powi(2))
            .sum::<f32>();
        if e < d {
            d = e;
            best = Some(c.id);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chair(id: usize, x: f32, building: usize) -> Chair {
        Chair {
            id,
            position: [x, 0.0, 0.0],
            building,
            taken: false,
        }
    }

    #[test]
    fn nearest_free_chair_in_the_bar() {
        let chairs = [
            chair(0, 50.0, 1),
            chair(1, 10.0, 2),
            chair(2, 20.0, 1),
            chair(3, 5.0, 1),
        ];
        // chair 3 is held by another member, chair 1 stands in another building
        assert_eq!(choose_chair(None, &chairs, 1, [0.0; 3], &[3]), Some(2));
        // a member keeps its own chair
        assert_eq!(choose_chair(Some(0), &chairs, 1, [0.0; 3], &[3]), Some(0));
    }
}
