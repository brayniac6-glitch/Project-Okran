//! Long-distance pathfinding (port_map `world`): routes across the whole world for squads
//! travelling while no navmesh is loaded where they go (Kenshi's UnloadedPlatoon /
//! AbstractMovementBase), and as the waypoints a loaded squad walks between on the navmesh.
//!
//! (stand-in) The game's abstract movement is not decompiled. Here: a grid over the world, 512
//! cells a side (576 units a cell), from the heightmap; A* over its 8 neighbours, a step costing
//! its length x (1 + 4 x its slope) x 3 in water (characters swim; the navmesh's water edges cost
//! more), cliffs steeper than 1.5 (rise over run, about 56 degrees) blocked; the route then cut to
//! the cells where it turns and pulled straight where the grid allows.

use crate::{Error, Terrain, WORLD_SIZE};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Cells a side.
pub const CELLS: usize = 512;
/// Steeper than this (rise over run) can't be walked.
const CLIFF: f32 = 1.5;
/// A step in water costs this many times as much.
const WATER: f32 = 3.0;
/// The water surface (units; kenshi_render's WATER_HEIGHT).
pub const WATER_HEIGHT: f32 = 100.0;

/// The world's walking grid.
#[derive(Debug, Clone)]
pub struct WorldGrid {
    /// Ground height at each cell's centre (row-major, z rows).
    pub height: Vec<f32>,
    /// Units a cell.
    pub cell: f32,
}

#[derive(PartialEq)]
struct Open(f32, usize);
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
    }
}

impl WorldGrid {
    /// The grid from the heightmap (every n-th sample).
    pub fn build(terrain: &mut Terrain) -> Result<WorldGrid, Error> {
        let samples = terrain.samples();
        let step = (samples / CELLS as u32).max(1);
        let patch = terrain.patch(step / 2, step / 2, CELLS as u32, CELLS as u32, step)?;
        let mut height = Vec::with_capacity(CELLS * CELLS);
        for j in 0..CELLS as u32 {
            for i in 0..CELLS as u32 {
                height.push(patch.at(i, j));
            }
        }
        Ok(WorldGrid { height, cell: WORLD_SIZE / CELLS as f32 })
    }

    /// The cell of a world point.
    pub fn cell_of(&self, p: [f32; 3]) -> (usize, usize) {
        let c = |v: f32| (((v + WORLD_SIZE / 2.0) / self.cell) as i64).clamp(0, CELLS as i64 - 1) as usize;
        (c(p[0]), c(p[2]))
    }

    /// A cell's centre (world units, on the ground).
    pub fn centre(&self, c: (usize, usize)) -> [f32; 3] {
        let x = (c.0 as f32 + 0.5) * self.cell - WORLD_SIZE / 2.0;
        let z = (c.1 as f32 + 0.5) * self.cell - WORLD_SIZE / 2.0;
        [x, self.h(c), z]
    }

    fn h(&self, c: (usize, usize)) -> f32 {
        self.height[c.1 * CELLS + c.0]
    }

    /// The cost of stepping from cell `a` to its neighbour `b` (None: a cliff).
    fn step(&self, a: (usize, usize), b: (usize, usize)) -> Option<f32> {
        let run = if a.0 != b.0 && a.1 != b.1 { self.cell * std::f32::consts::SQRT_2 } else { self.cell };
        let slope = (self.h(b) - self.h(a)).abs() / run;
        if slope > CLIFF {
            return None;
        }
        let wet = self.h(b) < WATER_HEIGHT;
        Some(run * (1.0 + 4.0 * slope) * if wet { WATER } else { 1.0 })
    }

    /// A route from `from` to `to` (world units): waypoints where it turns, ending at `to`; None
    /// when no way is found.
    pub fn route(&self, from: [f32; 3], to: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        let (s, g) = (self.cell_of(from), self.cell_of(to));
        let idx = |c: (usize, usize)| c.1 * CELLS + c.0;
        let mut cost = vec![f32::INFINITY; CELLS * CELLS];
        let mut came = vec![usize::MAX; CELLS * CELLS];
        let mut open = BinaryHeap::new();
        let guess = |c: (usize, usize)| {
            let (dx, dz) = (c.0.abs_diff(g.0) as f32, c.1.abs_diff(g.1) as f32);
            (dx.max(dz) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dz)) * self.cell
        };
        cost[idx(s)] = 0.0;
        open.push(Open(guess(s), idx(s)));
        let mut found = false;
        while let Some(Open(_, k)) = open.pop() {
            let c = (k % CELLS, k / CELLS);
            if c == g {
                found = true;
                break;
            }
            for dz in -1i64..=1 {
                for dx in -1i64..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let (nx, nz) = (c.0 as i64 + dx, c.1 as i64 + dz);
                    if nx < 0 || nz < 0 || nx >= CELLS as i64 || nz >= CELLS as i64 {
                        continue;
                    }
                    let n = (nx as usize, nz as usize);
                    let Some(step) = self.step(c, n) else { continue };
                    let next = cost[k] + step;
                    if next < cost[idx(n)] {
                        cost[idx(n)] = next;
                        came[idx(n)] = k;
                        open.push(Open(next + guess(n), idx(n)));
                    }
                }
            }
        }
        if !found {
            return None;
        }
        let mut cells = vec![g];
        let mut k = idx(g);
        while came[k] != usize::MAX {
            k = came[k];
            cells.push((k % CELLS, k / CELLS));
        }
        cells.reverse();
        // pulled straight: from each kept cell, the farthest cell reachable in a clear line
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < cells.len() {
            let mut j = cells.len() - 1;
            while j > i + 1 && !self.clear(cells[i], cells[j]) {
                j -= 1;
            }
            out.push(self.centre(cells[j]));
            i = j;
        }
        if let Some(last) = out.last_mut() {
            *last = to;
        } else {
            out.push(to);
        }
        Some(out)
    }

    /// Whether the straight line between two cells crosses no cliff and keeps to land or to
    /// water throughout as its start does.
    fn clear(&self, a: (usize, usize), b: (usize, usize)) -> bool {
        let n = a.0.abs_diff(b.0).max(a.1.abs_diff(b.1)).max(1);
        let wet = self.h(a) < WATER_HEIGHT;
        let mut prev = a;
        for k in 1..=n {
            let t = k as f32 / n as f32;
            let c = (
                (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t).round() as usize,
                (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t).round() as usize,
            );
            if c != prev {
                if self.step(prev, c).is_none() || (self.h(c) < WATER_HEIGHT) != wet {
                    return false;
                }
                prev = c;
            }
        }
        true
    }

    /// A route's length (world units).
    pub fn length(from: [f32; 3], route: &[[f32; 3]]) -> f32 {
        let mut d = 0.0;
        let mut p = from;
        for q in route {
            d += (q[0] - p[0]).hypot(q[2] - p[2]);
            p = *q;
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(h: impl Fn(usize, usize) -> f32) -> WorldGrid {
        let mut height = vec![0.0; CELLS * CELLS];
        for z in 0..CELLS {
            for x in 0..CELLS {
                height[z * CELLS + x] = h(x, z);
            }
        }
        WorldGrid { height, cell: WORLD_SIZE / CELLS as f32 }
    }

    #[test]
    fn straight_on_flat_land() {
        let g = flat(|_, _| 500.0);
        let (a, b) = (g.centre((10, 10)), g.centre((100, 40)));
        let r = g.route(a, b).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0], b);
    }

    #[test]
    fn round_a_wall_of_cliffs() {
        // a cliff wall at x = 50 but for a gap at z = 200
        let g = flat(|x, z| if x == 50 && z != 200 { 100_000.0 } else { 500.0 });
        let (a, b) = (g.centre((10, 100)), g.centre((90, 100)));
        let r = g.route(a, b).unwrap();
        assert!(r.len() >= 2);
        let gap = g.centre((50, 200));
        assert!(r.iter().any(|p| (p[2] - gap[2]).abs() < g.cell * 30.0));
    }
}
