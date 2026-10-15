//! Long-distance movement (port_map `world`): how a squad away from the player crosses the
//! world (Kenshi's UnloadedPlatoon moving its AbstractMovementBase, `crate::roads`).
//!
//! AbstractMovementBase goes straight at its goal, over hills and water alike; for a goal over
//! 10000 units away it takes the roads when they pay (F040645 -> F040644, `crate::roads`). A
//! route here is the points such a mover heads for in turn: the road markers of the way it
//! takes, then the goal.

use crate::roads::{AbstractMovement, RoadNetwork, road_factor};
use crate::{Error, Terrain};

/// (assumption) The "road preference" a route takes when the caller names none: 0.6, the most
/// common among the factions (35 of them; 0.8 for the big ones, 0 for wildlife and many bandits).
pub const DEFAULT_ROAD_PREFERENCE: f32 = 0.6;

/// The world's long-distance movement: the road network (the name is kept from the grid this
/// replaced).
#[derive(Debug, Clone, Default)]
pub struct WorldGrid {
    pub roads: RoadNetwork,
}

impl WorldGrid {
    /// The road network of the install the terrain comes from.
    pub fn build(terrain: &mut Terrain) -> Result<WorldGrid, Error> {
        Ok(WorldGrid { roads: RoadNetwork::load(terrain.install()) })
    }

    /// The route from `from` to `to` (world units) for a mover of a faction with the default
    /// road preference: `route_with`.
    pub fn route(&self, from: [f32; 3], to: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        Some(self.route_with(from, to, DEFAULT_ROAD_PREFERENCE))
    }

    /// @fn F040645
    ///
    /// The points an abstract mover at `from` heads for in turn when sent to `to`: the road
    /// markers of the way it takes (F040644: over 10000 units away, when the roads pay at its
    /// faction's `road_preference`), then `to`.
    pub fn route_with(&self, from: [f32; 3], to: [f32; 3], road_preference: f32) -> Vec<[f32; 3]> {
        let mut m = AbstractMovement::new(from, road_preference);
        m.go_to(Some(&self.roads), to);
        let mut out = match &m.road {
            Some(r) if r.is_active() => r.waypoints(&self.roads),
            _ => Vec::new(),
        };
        out.push(to);
        out
    }

    /// The road factor of a road preference (re-exported for callers).
    pub fn road_factor(road_preference: f32) -> f32 {
        road_factor(road_preference)
    }

    /// A route's length (world units, across).
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
    use crate::roads::{Junction, Marker, Road};

    #[test]
    fn straight_without_roads() {
        let g = WorldGrid::default();
        let r = g.route([0.0; 3], [50_000.0, 0.0, 0.0]).unwrap();
        assert_eq!(r, vec![[50_000.0, 0.0, 0.0]]);
    }

    #[test]
    fn along_a_road_when_far() {
        // one straight road from x = 0 to x = 40000, a marker every 10000
        let mut roads = RoadNetwork::default();
        roads.junctions = vec![Junction { id: 0, roads: vec![0] }, Junction { id: 1, roads: vec![0] }];
        roads.roads.push(Road {
            name: "r".into(),
            start: 0,
            end: 1,
            markers: (0..=4).map(|i| Marker { position: [i as f32 * 10_000.0, 0.0, 0.0], t: i as f32 / 4.0 }).collect(),
            length: 40_000.0,
            weight: 1.0,
            width: 100.0,
            hidden: false,
        });
        let g = WorldGrid { roads };
        let r = g.route_with([1_000.0, 0.0, 500.0], [39_000.0, 0.0, 500.0], 0.8);
        let xs: Vec<f32> = r.iter().map(|p| p[0]).collect();
        assert_eq!(xs, vec![10_000.0, 20_000.0, 30_000.0, 39_000.0]);
        // a faction without road preference goes straight
        assert_eq!(g.route_with([1_000.0, 0.0, 500.0], [39_000.0, 0.0, 500.0], 0.0).len(), 1);
    }
}
