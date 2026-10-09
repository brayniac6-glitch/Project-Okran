//! Furniture (port_map `ai`): the BUILDING records with a "functionality" (chairs, beds, benches,
//! machines) that buildings place as instances or through their interior / exterior layouts.
//!
//! The functionality record (BUILDING_FUNCTIONALITY) gives the BuildingFunction ("function":
//! 6 bed, 16 table, 17 chair, ...), how many can use it at once ("max operators") and from how
//! far ("use range", units).

use crate::Placement;
use crate::nodes::{Layouts, rotate};
use kenshi_data::{GameData, GameDataContainer, ItemType};

/// BuildingFunction values the AI asks for.
pub const BF_BED: i32 = 6;
pub const BF_TABLE: i32 = 16;
pub const BF_CHAIR: i32 = 17;
pub const BF_THRONE: i32 = 24;
/// A prisoner cage (BUILDING_FUNCTIONALITY 'Cage').
pub const BF_CAGE: i32 = 8;

/// A usable piece of furniture placed in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct Furniture {
    /// BuildingFunction.
    pub function: i32,
    pub max_operators: i32,
    /// Units.
    pub use_range: f32,
    /// World position (units) and facing (quaternion w, x, y, z).
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    /// The furniture's BUILDING record (string id and name).
    pub record: String,
    pub name: String,
    /// The placement (index into the zone's placements) whose building holds it.
    pub placement: usize,
}

/// The functionality of a BUILDING record: (function, max operators, use range).
pub fn functionality(data: &GameDataContainer, b: &GameData) -> Option<(i32, i32, f32)> {
    if b.item_type() != Some(ItemType::Building) {
        return None;
    }
    let (sid, _) = b.refs("functionality").first()?;
    let f = data.get(sid)?;
    Some((
        *f.ints.get("function")?,
        *f.ints.get("max operators").unwrap_or(&1),
        f.floats.get("use range").copied().unwrap_or(10.0),
    ))
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

/// The furniture of a zone's building placements, world placed: a building with a functionality
/// itself, instances of the building record (and its parts and interiors) and of its interior
/// and exterior layouts.
pub fn furniture(
    data: &GameDataContainer,
    layouts: &Layouts,
    placements: &[Placement],
) -> Vec<Furniture> {
    fn local<'a>(
        data: &'a GameDataContainer,
        b: &'a GameData,
        depth: u32,
        out: &mut Vec<(&'a GameData, [f32; 3], [f32; 4])>,
    ) {
        for i in &b.instances {
            if let Some(t) = data.get(&i.target) {
                out.push((t, i.position, i.rotation));
            }
        }
        if depth < 4 {
            for (sid, _) in b.refs("parts").iter().chain(b.refs("interior")) {
                if let Some(p) = data.get(sid) {
                    local(data, p, depth + 1, out);
                }
            }
        }
    }
    let mut out = vec![];
    for (pi, p) in placements.iter().enumerate() {
        let Some(b) = data.get(&p.building) else {
            continue;
        };
        let mut items = vec![];
        local(data, b, 0, &mut items);
        for name in p.interior.iter().chain(&p.exterior) {
            for i in layouts.for_building(data, &p.building, name) {
                if let Some(t) = data.get(&i.target) {
                    items.push((t, i.position, i.rotation));
                }
            }
        }
        let base = [
            p.position[0],
            p.world_y.unwrap_or(p.position[1]),
            p.position[2],
        ];
        // a building that is itself a machine (a mounted turret on a wall): itself, where it
        // stands
        if let Some((function, max_operators, use_range)) = functionality(data, b) {
            out.push(Furniture {
                function,
                max_operators,
                use_range,
                position: base,
                rotation: p.rotation,
                record: b.string_id.clone(),
                name: b.name.clone(),
                placement: pi,
            });
        }
        for (t, pos, rot) in items {
            let Some((function, max_operators, use_range)) = functionality(data, t) else {
                continue;
            };
            let o = rotate(p.rotation, pos);
            out.push(Furniture {
                function,
                max_operators,
                use_range,
                position: [base[0] + o[0], base[1] + o[1], base[2] + o[2]],
                rotation: mul(p.rotation, rot),
                record: t.string_id.clone(),
                name: t.name.clone(),
                placement: pi,
            });
        }
    }
    out
}

/// A storage container placed in the world: a chest, crate, box or storage building (a
/// BUILDING with "has inventory"), as a building itself or as furniture in one.
#[derive(Debug, Clone)]
pub struct Container {
    /// The BUILDING record (string id) and its name.
    pub record: String,
    pub name: String,
    /// World position (units).
    pub position: [f32; 3],
    /// The placement it is (or is in).
    pub placement: usize,
    /// "storage size width" x "height".
    pub size: (u32, u32),
}

/// The containers of `placements`: the buildings with "has inventory", and the furniture in
/// buildings (their instances, parts and layouts) that has it. Player-buildable storage
/// (`base`) is left to the base code: here only what towns and ruins place.
pub fn containers(data: &GameDataContainer, layouts: &Layouts, placements: &[Placement]) -> Vec<Container> {
    let holds = |b: &GameData| b.item_type() == Some(ItemType::Building) && b.bools.get("has inventory").copied().unwrap_or(false);
    let size = |b: &GameData| {
        let i = |k: &str| b.ints.get(k).copied().unwrap_or(6).clamp(1, 40) as u32;
        (i("storage size width"), i("storage size height"))
    };
    let mut out = vec![];
    for (pi, p) in placements.iter().enumerate() {
        let Some(b) = data.get(&p.building) else { continue };
        let base = [p.position[0], p.world_y.unwrap_or(p.position[1]), p.position[2]];
        if holds(b) {
            out.push(Container { record: b.string_id.clone(), name: b.name.clone(), position: base, placement: pi, size: size(b) });
        }
        let mut items = vec![];
        for i in &b.instances {
            if let Some(t) = data.get(&i.target) {
                items.push((t, i.position));
            }
        }
        for name in p.interior.iter().chain(&p.exterior) {
            for i in layouts.for_building(data, &p.building, name) {
                if let Some(t) = data.get(&i.target) {
                    items.push((t, i.position));
                }
            }
        }
        for (t, pos) in items {
            if !holds(t) {
                continue;
            }
            let o = rotate(p.rotation, pos);
            out.push(Container {
                record: t.string_id.clone(),
                name: t.name.clone(),
                position: [base[0] + o[0], base[1] + o[1], base[2] + o[2]],
                placement: pi,
                size: size(t),
            });
        }
    }
    out
}
