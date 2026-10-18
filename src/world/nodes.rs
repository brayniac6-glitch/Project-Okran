//! AI nodes (port_map `ai`): the invisible "Node ..." BUILDING records ("is node", "node type")
//! that buildings place as instances in their "nodes" list. The AI's node manager picks among a
//! town's nodes by type (F031809); wandering townsfolk go to general nodes, guards to guard
//! posts, shopkeepers to theirs.

use crate::{Placement, ZONE_FOLDERS};
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::fcs::{self, Instance};
use std::collections::HashMap;
use std::path::Path;

/// Furnishing layouts (`interiors.level`, kind-30 records): (building string id, layout name)
/// -> the furniture and nodes placed in the building's frame. A building's state record names
/// its "interior layout name" and "exterior layout name". Beside each layout, which of its
/// instances are "outside" (`Layouts::outside`).
#[derive(Debug, Clone, Default)]
pub struct Layouts(pub HashMap<(String, String), Vec<Instance>>, pub HashMap<(String, String), Vec<bool>>);

impl Layouts {
    /// Every interiors.level: the leveldata root first, then the zone folders (later ones win).
    pub fn load(install: &Path) -> Layouts {
        let dir = install.join("data/newland/leveldata");
        let files = std::iter::once(dir.join("interiors.level")).chain(
            ZONE_FOLDERS
                .iter()
                .map(|f| dir.join(f).join("interiors.level")),
        );
        let mut m = HashMap::new();
        let mut outside = HashMap::new();
        for f in files.filter(|f| f.is_file()) {
            let Ok(file) = fcs::read(&f) else { continue };
            // F037789: a building's "is outside" is its state record's "flags" bit 0, else its
            // "is outside"
            let out: HashMap<String, bool> = file
                .records
                .iter()
                .filter(|r| r.kind == 35)
                .map(|r| {
                    let flags = r.ints.iter().find(|(k, _)| k == "flags").map(|(_, v)| *v);
                    let is = r.bools.iter().find(|(k, _)| k == "is outside").is_some_and(|(_, v)| *v);
                    (r.string_id.clone(), flags.map_or(is, |f| f & 1 != 0))
                })
                .collect();
            for r in file.records.into_iter().filter(|r| r.kind == 30) {
                let Some(b) = r
                    .strings
                    .iter()
                    .find(|(k, _)| k == "associated building")
                    .map(|(_, v)| v.clone())
                else {
                    continue;
                };
                let flags = r.instances.iter().map(|i| i.states.iter().any(|s| out.get(s).copied().unwrap_or(false))).collect();
                outside.insert((b.clone(), r.name.clone()), flags);
                m.insert((b, r.name.clone()), r.instances);
            }
        }
        Layouts(m, outside)
    }

    pub fn get(&self, building: &str, layout: &str) -> &[Instance] {
        self.0
            .get(&(building.to_string(), layout.to_string()))
            .map_or(&[], |v| v)
    }

    /// A placed building's layout: its own, else one of a building it shares interiors with
    /// ("shares interiors with", either way round).
    pub fn for_building(
        &self,
        data: &GameDataContainer,
        building: &str,
        layout: &str,
    ) -> &[Instance] {
        self.key_for(data, building, layout)
            .map_or(&[], |k| self.get(&k, layout))
    }

    /// Which of `for_building`'s instances are "outside" (their state record's "flags" bit 0,
    /// else its "is outside", F037789): shown while the building's interior is closed (F037736).
    pub fn outside(&self, data: &GameDataContainer, building: &str, layout: &str) -> &[bool] {
        self.key_for(data, building, layout)
            .and_then(|k| self.1.get(&(k, layout.to_string())))
            .map_or(&[], |v| v)
    }

    /// The building whose layout `for_building` takes.
    fn key_for(&self, data: &GameDataContainer, building: &str, layout: &str) -> Option<String> {
        if !self.get(building, layout).is_empty() {
            return Some(building.to_string());
        }
        let b = data.get(building)?;
        for (sid, _) in b.refs("shares interiors with") {
            if !self.get(sid, layout).is_empty() {
                return Some(sid.clone());
            }
        }
        for o in data.of_type(ItemType::Building) {
            if o.refs("shares interiors with")
                .iter()
                .any(|(s, _)| s == building)
                && !self.get(&o.string_id, layout).is_empty()
            {
                return Some(o.string_id.clone());
            }
        }
        None
    }
}

/// NodeType: 0 shopkeeper, 1 general, 2 guard post, 3 defensive / turret, 4 use, 5 light mount.
pub const NODE_SHOPKEEPER: i32 = 0;
pub const NODE_GENERAL: i32 = 1;
pub const NODE_GUARDPOST: i32 = 2;
pub const NODE_DEFENSIVE: i32 = 3;
pub const NODE_USE: i32 = 4;

/// A node placed in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: i32,
    /// World position (units) and facing (quaternion w, x, y, z).
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    /// The BUILDING record that placed it, and which placement (index into the placements).
    pub building: String,
    pub placement: usize,
}

/// Rotate `v` by the quaternion (w, x, y, z).
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [w, x, y, z] = q;
    let u = [x, y, z];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = cross(u, v).map(|c| 2.0 * c);
    let c = cross(u, t);
    [
        v[0] + w * t[0] + c[0],
        v[1] + w * t[1] + c[1],
        v[2] + w * t[2] + c[2],
    ]
}

fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

fn is_node(r: &GameData) -> bool {
    r.item_type() == Some(ItemType::Building) && r.bools.get("is node").copied().unwrap_or(false)
}

/// The node instances of a building record and its parts, in the record's frame.
fn local_nodes(
    data: &GameDataContainer,
    b: &GameData,
    depth: u32,
    out: &mut Vec<(i32, [f32; 3], [f32; 4])>,
) {
    for i in &b.instances {
        if let Some(t) = data.get(&i.target).filter(|t| is_node(t)) {
            let kind = *t.ints.get("node type").unwrap_or(&NODE_GENERAL);
            out.push((kind, i.position, i.rotation));
        }
    }
    if depth < 4 {
        for (sid, _) in b.refs("parts").iter().chain(b.refs("interior")) {
            if let Some(p) = data.get(sid) {
                local_nodes(data, p, depth + 1, out);
            }
        }
    }
}

/// The nodes of a zone's building placements, world placed: the building's own node instances
/// and those of its interior and exterior layouts.
pub fn nodes(data: &GameDataContainer, layouts: &Layouts, placements: &[Placement]) -> Vec<Node> {
    let mut out = vec![];
    for (pi, p) in placements.iter().enumerate() {
        let Some(b) = data.get(&p.building) else {
            continue;
        };
        let mut local = vec![];
        local_nodes(data, b, 0, &mut local);
        for name in p.interior.iter().chain(&p.exterior) {
            for i in layouts.for_building(data, &p.building, name) {
                if let Some(t) = data.get(&i.target).filter(|t| is_node(t)) {
                    local.push((
                        *t.ints.get("node type").unwrap_or(&NODE_GENERAL),
                        i.position,
                        i.rotation,
                    ));
                }
            }
        }
        let base = [
            p.position[0],
            p.world_y.unwrap_or(p.position[1]),
            p.position[2],
        ];
        for (kind, pos, rot) in local {
            let o = rotate(p.rotation, pos);
            out.push(Node {
                kind,
                position: [base[0] + o[0], base[1] + o[1], base[2] + o[2]],
                rotation: mul(p.rotation, rot),
                building: b.name.clone(),
                placement: pi,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quaternion_turns_a_quarter() {
        // 90 degrees about y: x -> -z
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let v = rotate([s, 0.0, s, 0.0], [1.0, 0.0, 0.0]);
        assert!((v[0]).abs() < 1e-5 && (v[2] + 1.0).abs() < 1e-5, "{v:?}");
    }
}
