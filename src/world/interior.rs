//! Building interiors and doors (port_map `ai`): which building a character stands inside, and
//! where a building's doors are.
//!
//! Spec: F037793 Building::setupInteriorMask (the "interior mask" BUILDING_PART's mesh, placed with
//! the building's position and orientation, becomes a trigger volume, type 0xd), Character vtable
//! +0x1d8 F039242 (the building the character is inside: RootObject +0x80, set by that volume).
//! Doors: a building's "doors" references are BUILDING records whose parts ("is door") are meshes
//! modelled in the building's own frame.
//!
//! The original tests the character's physics body against the volume; here the character's
//! position, a little above the ground, is tested against the mask mesh by ray parity.

use crate::Placement;
use crate::nodes::rotate;
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::{ogre_mesh, resources::Resources};
use std::path::Path;

/// Height above the character's feet the inside test uses (units).
pub const TEST_HEIGHT: f32 = 10.0;

/// One building's interior volume, world placed (units).
#[derive(Debug, Clone)]
pub struct Interior {
    pub placement: usize,
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub lo: [f32; 3],
    pub hi: [f32; 3],
}

impl Interior {
    /// Whether a point lies inside the volume: a ray straight up crosses its surface an odd
    /// number of times.
    pub fn contains(&self, p: [f32; 3]) -> bool {
        if (0..3).any(|k| p[k] < self.lo[k] || p[k] > self.hi[k]) {
            return false;
        }
        let mut n = 0;
        for t in &self.triangles {
            if let Some(y) = up_hit(p, t)
                && y > p[1]
            {
                n += 1;
            }
        }
        n % 2 == 1
    }
}

/// Where a vertical line through (p.x, p.z) meets the triangle (its height), if it does.
fn up_hit(p: [f32; 3], t: &[[f32; 3]; 3]) -> Option<f32> {
    let [a, b, c] = *t;
    let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if d.abs() < 1e-9 {
        return None;
    }
    let l1 = ((b[2] - c[2]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[2] - c[2])) / d;
    let l2 = ((c[2] - a[2]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[2] - c[2])) / d;
    let l3 = 1.0 - l1 - l2;
    // half-open on one side so a ray through a shared edge counts once
    if l1 < 0.0 || l2 < 0.0 || l3 <= 0.0 {
        return None;
    }
    Some(l1 * a[1] + l2 * b[1] + l3 * c[1])
}

fn base(p: &Placement) -> [f32; 3] {
    [
        p.position[0],
        p.world_y.unwrap_or(p.position[1]),
        p.position[2],
    ]
}

fn triangles(path: &Path, p: &Placement) -> Option<Vec<[[f32; 3]; 3]>> {
    let mesh = ogre_mesh::read(path).ok()?;
    let b = base(p);
    let mut out = vec![];
    for s in &mesh.submeshes {
        let geo = if s.use_shared {
            mesh.shared.as_ref()
        } else {
            s.geometry.as_ref()
        }?;
        let pos = geo.positions()?;
        let world: Vec<[f32; 3]> = pos
            .iter()
            .map(|v| {
                let r = rotate(p.rotation, *v);
                [b[0] + r[0], b[1] + r[1], b[2] + r[2]]
            })
            .collect();
        for t in s.indices.as_chunks::<3>().0 {
            out.push([
                world[t[0] as usize],
                world[t[1] as usize],
                world[t[2] as usize],
            ]);
        }
    }
    Some(out)
}

fn part_file<'a>(data: &'a GameDataContainer, sid: &str) -> Option<&'a str> {
    let part = data.get(sid)?;
    part.files
        .get("phs or mesh")
        .map(|s| s.as_str())
        .filter(|s| s.to_lowercase().ends_with(".mesh"))
}

/// The interior volumes of a zone's placements (buildings with an "interior mask").
pub fn interiors(
    data: &GameDataContainer,
    res: &Resources,
    placements: &[Placement],
) -> Vec<Interior> {
    let mut out = vec![];
    for (i, p) in placements.iter().enumerate() {
        let Some(b) = data.get(&p.building) else {
            continue;
        };
        let mut tris = vec![];
        for (sid, _) in b.refs("interior mask") {
            if let Some(path) = part_file(data, sid).and_then(|f| res.path(f))
                && let Some(t) = triangles(&path, p)
            {
                tris.extend(t);
            }
        }
        if tris.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for t in &tris {
            for v in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        out.push(Interior {
            placement: i,
            triangles: tris,
            lo,
            hi,
        });
    }
    out
}

/// The building (placement index) a character standing at `feet` is inside (F039242's default:
/// RootObject +0x80, kept by the interior volumes).
pub fn building_at(interiors: &[Interior], feet: [f32; 3]) -> Option<usize> {
    let p = [feet[0], feet[1] + TEST_HEIGHT, feet[2]];
    interiors
        .iter()
        .find(|i| i.contains(p))
        .map(|i| i.placement)
}

/// A door of a placed building.
#[derive(Debug, Clone, PartialEq)]
pub struct Door {
    pub placement: usize,
    /// The BUILDING record whose door this is.
    pub building: String,
    /// Which of the building's doors (the state record's "locked{n}" index).
    pub index: usize,
    /// The door BUILDING record.
    pub record: String,
    /// The centre of the door's part meshes, world placed (units).
    pub position: [f32; 3],
    /// The door record's sliding (DoorStuff, F029394): "door move speed" (of its opening a
    /// second), "door move dist" (units), "door axis" (0, 1, 2: its collision box's x, y, z),
    /// "door comes out" (units it first moves out of its frame), "always locks".
    pub speed: f32,
    pub dist: f32,
    pub axis: i32,
    pub out: f32,
    pub always_locks: bool,
    /// Where it slides and comes out (unit vectors in the building's frame; F029383): its
    /// collision box's "door axis" column, and its thinnest other axis turned away from the
    /// building's origin toward the box.
    pub slide: [f32; 3],
    pub out_dir: [f32; 3],
    /// The door's opening as a line (units): its ends across the doorway (along the slide axis,
    /// as far as its meshes reach), and the heights it spans.
    pub ends: [[f32; 2]; 2],
    pub heights: [f32; 2],
}

/// A door collision box's axis `k` in the building's frame: the collision is made Z-up, its
/// actors turned Y-up ((x, y, z) -> (x, z, -y), kenshi_io::nxu), so x stays x, y is -z and z up.
pub fn box_axis(k: i32) -> [f32; 3] {
    match k {
        1 => [0.0, 0.0, -1.0],
        2 => [0.0, 1.0, 0.0],
        _ => [1.0, 0.0, 0.0],
    }
}

/// @fn F029383
///
/// A door's slide and come-out directions and the axis its doorway spans (building frame), from
/// its first collision box (the door part's "xml collision"): the slide its "door axis" column
/// (the box's global orientation's), the way out the box's thinnest other axis, turned toward the
/// box from the door's actor (at the building's origin), the doorway its "door navmesh axis"
/// column. Without a box, the box axes as made (`box_axis`) and the thinner of the meshes'
/// other extents.
fn door_frame(data: &GameDataContainer, res: &Resources, d: &kenshi_data::GameData, extents: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let axis = d.ints.get("door axis").copied().unwrap_or(0).clamp(0, 2);
    let nav = d.ints.get("door navmesh axis").copied().unwrap_or(0).clamp(0, 2);
    let boxed = d.refs("parts").iter().find_map(|(psid, _)| {
        let part = data.get(psid)?;
        let file = part.files.get("xml collision").filter(|f| !f.is_empty())?;
        let shapes = kenshi_io::nxu::read(&res.path(file)?).ok()?;
        shapes.into_iter().find_map(|sh| match sh.kind {
            kenshi_io::nxu::ShapeKind::Box(dims) => Some((sh.pose, dims)),
            _ => None,
        })
    });
    match boxed {
        Some((pose, dims)) => {
            let col = |k: usize| [pose.rows[0][k], pose.rows[1][k], pose.rows[2][k]];
            let a = axis as usize;
            let thin = (0..3).filter(|&k| k != a).min_by(|&x, &y| dims[x].total_cmp(&dims[y])).unwrap_or(0);
            let mut out = col(thin);
            if out[0] * pose.pos[0] + out[1] * pose.pos[1] + out[2] * pose.pos[2] < 0.0 {
                out = out.map(|v| -v);
            }
            (col(a), out, col(nav as usize))
        }
        None => {
            let slide = box_axis(axis);
            let k = slide.iter().position(|v| v.abs() > 0.5).unwrap_or(0);
            let thin = (0..3).filter(|&j| j != k).min_by(|&x, &y| extents[x].total_cmp(&extents[y])).unwrap_or(2);
            let mut out = [0.0; 3];
            out[thin] = 1.0;
            (slide, out, box_axis(nav))
        }
    }
}

/// The doors of a zone's placements. A ruin has none (F037851 makes a building's "doors" only
/// when it is not destroyed, +0x1a1).
pub fn doors(data: &GameDataContainer, res: &Resources, placements: &[Placement]) -> Vec<Door> {
    let mut out = vec![];
    for (i, p) in placements.iter().enumerate() {
        if p.destroyed {
            continue;
        }
        let Some(b) = data.get(&p.building) else {
            continue;
        };
        for (n, (sid, _)) in b.refs("doors").iter().enumerate() {
            let Some(d) = data.get(sid) else { continue };
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for (psid, _) in d.refs("parts") {
                if data
                    .get(psid)
                    .is_none_or(|r| r.item_type() != Some(ItemType::BuildingPart))
                {
                    continue;
                }
                if let Some(path) = part_file(data, psid).and_then(|f| res.path(f))
                    && let Some(t) = triangles(&path, p)
                {
                    for v in t.iter().flatten() {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v[k]);
                            hi[k] = hi[k].max(v[k]);
                        }
                    }
                }
            }
            if lo[0] > hi[0] {
                continue;
            }
            let [w, x, y, z] = p.rotation;
            // the door's extents along the building's axes (its meshes' world bounds turned back)
            let unrot = |v: [f32; 3]| {
                let (qx, qy, qz) = (-x, -y, -z);
                let t = [2.0 * (qy * v[2] - qz * v[1]), 2.0 * (qz * v[0] - qx * v[2]), 2.0 * (qx * v[1] - qy * v[0])];
                [v[0] + w * t[0] + (qy * t[2] - qz * t[1]), v[1] + w * t[1] + (qz * t[0] - qx * t[2]), v[2] + w * t[2] + (qx * t[1] - qy * t[0])]
            };
            let (mut llo, mut lhi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for c in 0..8 {
                let v = unrot([if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }]);
                for k in 0..3 {
                    llo[k] = llo[k].min(v[k]);
                    lhi[k] = lhi[k].max(v[k]);
                }
            }
            let (slide, out_dir, doorway) = door_frame(data, res, d, [lhi[0] - llo[0], lhi[1] - llo[1], lhi[2] - llo[2]]);
            // the doorway's line ("door navmesh axis") turned with the building, and how far the
            // door reaches along it
            let local = if doorway[1].abs() > 0.5 { [1.0, 0.0, 0.0] } else { doorway };
            let rot = |v: [f32; 3]| {
                // q v q*, q = (w, x, y, z)
                let (qx, qy, qz) = (x, y, z);
                let t = [2.0 * (qy * v[2] - qz * v[1]), 2.0 * (qz * v[0] - qx * v[2]), 2.0 * (qx * v[1] - qy * v[0])];
                [v[0] + w * t[0] + (qy * t[2] - qz * t[1]), v[1] + w * t[1] + (qz * t[0] - qx * t[2]), v[2] + w * t[2] + (qx * t[1] - qy * t[0])]
            };
            let dir = rot(local);
            let l = dir[0].hypot(dir[2]).max(1e-4);
            let dir = [dir[0] / l, dir[2] / l];
            let mid = [(lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5];
            let reach = [[lo[0], lo[2]], [hi[0], lo[2]], [lo[0], hi[2]], [hi[0], hi[2]]]
                .iter()
                .map(|c| ((c[0] - mid[0]) * dir[0] + (c[1] - mid[1]) * dir[1]).abs())
                .fold(0.0f32, f32::max);
            out.push(Door {
                placement: i,
                building: p.building.clone(),
                index: n,
                record: sid.clone(),
                ends: [[mid[0] - dir[0] * reach, mid[1] - dir[1] * reach], [mid[0] + dir[0] * reach, mid[1] + dir[1] * reach]],
                heights: [lo[1], hi[1]],
                position: [(lo[0] + hi[0]) * 0.5, lo[1], (lo[2] + hi[2]) * 0.5],
                speed: d.floats.get("door move speed").copied().unwrap_or(0.5),
                dist: d.floats.get("door move dist").copied().unwrap_or(30.0),
                axis: d.ints.get("door axis").copied().unwrap_or(0),
                out: d.floats.get("door comes out").copied().unwrap_or(0.0),
                always_locks: d.bools.get("always locks").copied().unwrap_or(false),
                slide,
                out_dir,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_a_box() {
        // a closed unit box from (0,0,0) to (10,10,10), two triangles per face
        let v = |x: f32, y: f32, z: f32| [x, y, z];
        let quad = |a, b, c, d| [[a, b, c], [a, c, d]];
        let mut tris = vec![];
        for q in [
            quad(
                v(0., 0., 0.),
                v(10., 0., 0.),
                v(10., 0., 10.),
                v(0., 0., 10.),
            ),
            quad(
                v(0., 10., 0.),
                v(10., 10., 0.),
                v(10., 10., 10.),
                v(0., 10., 10.),
            ),
            quad(
                v(0., 0., 0.),
                v(0., 10., 0.),
                v(0., 10., 10.),
                v(0., 0., 10.),
            ),
            quad(
                v(10., 0., 0.),
                v(10., 10., 0.),
                v(10., 10., 10.),
                v(10., 0., 10.),
            ),
            quad(
                v(0., 0., 0.),
                v(10., 0., 0.),
                v(10., 10., 0.),
                v(0., 10., 0.),
            ),
            quad(
                v(0., 0., 10.),
                v(10., 0., 10.),
                v(10., 10., 10.),
                v(0., 10., 10.),
            ),
        ] {
            tris.extend(q);
        }
        let i = Interior {
            placement: 0,
            triangles: tris,
            lo: [0.0; 3],
            hi: [10.0; 3],
        };
        assert!(i.contains([5.0, 5.0, 5.0]));
        assert!(i.contains([2.5, 1.0, 7.5]));
        assert!(!i.contains([5.0, 11.0, 5.0]));
        assert!(!i.contains([12.0, 5.0, 5.0]));
    }
}
