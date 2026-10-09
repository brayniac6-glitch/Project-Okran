//! PhysX 2.8 NxuStream2 XML (`NXUSTREAM2`): the collision of building parts ("xml collision").
//!
//! The files are exported Z-up; shapes come out in the model's Y-up axes, (x, y, z) -> (x, z, -y).
//! (fitted: with that turn and row-major poses the collision falls within the parts' meshes'
//! bounds best, 83% of points against 60% for the next mapping.) Convex hulls and triangle meshes
//! are PhysX "cooked" data: `CVHL` hulls give their vertices, `MESH` meshes their vertices and
//! triangles (8-bit indices under flag 0x08, 16-bit under 0x10, else 32-bit).
use std::io;
use std::path::Path;

/// A rigid transform: three rows of a rotation and a translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub rows: [[f32; 3]; 3],
    pub pos: [f32; 3],
}

impl Pose {
    pub const IDENTITY: Pose = Pose { rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], pos: [0.0; 3] };

    pub fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        let r = &self.rows;
        [
            r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2] + self.pos[0],
            r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2] + self.pos[1],
            r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2] + self.pos[2],
        ]
    }

    /// `self` after `inner` (inner's frame placed in self's).
    pub fn then(&self, inner: &Pose) -> Pose {
        let a = &self.rows;
        let b = &inner.rows;
        let mut rows = [[0.0; 3]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        let t = self.apply(inner.pos);
        Pose { rows, pos: t }
    }

    /// Into Y-up: (x, y, z) -> (x, z, -y) applied after.
    fn y_up(&self) -> Pose {
        let r = &self.rows;
        Pose {
            rows: [r[0], r[2], [-r[1][0], -r[1][1], -r[1][2]]],
            pos: [self.pos[0], self.pos[2], -self.pos[1]],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShapeKind {
    /// Half extents.
    Box([f32; 3]),
    Sphere(f32),
    /// Radius and the distance between its end spheres' centres, along local Y.
    Capsule { radius: f32, height: f32 },
    /// Hull vertices (shape frame).
    Convex(Vec<[f32; 3]>),
    /// Vertices and triangles (shape frame).
    Mesh(Vec<[f32; 3]>, Vec<[u32; 3]>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The shape's frame in the file's (Y-up) space.
    pub pose: Pose,
    pub kind: ShapeKind,
}

fn floats(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

fn pose(s: &str) -> Option<Pose> {
    let v = floats(s);
    (v.len() >= 12).then(|| Pose {
        rows: [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]],
        pos: [v[9], v[10], v[11]],
    })
}

/// The text of the first `<tag>...</tag>` in `s`.
fn inner<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&format!("</{tag}>"))? + a;
    Some(&s[a..b])
}

/// The value of attribute `name` in an element's opening tag text.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let k = format!(" {name}=\"");
    let a = tag.find(&k)? + k.len();
    let b = tag[a..].find('"')? + a;
    Some(&tag[a..b])
}

/// Each `<tag ...>...</tag>` element: (opening tag text, body).
fn elements<'a>(s: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let open = format!("<{tag} ");
    let open2 = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = [s[at..].find(&open), s[at..].find(&open2)].into_iter().flatten().min() {
        let start = at + i;
        let Some(gt) = s[start..].find('>') else { break };
        let head = &s[start..start + gt];
        let body_start = start + gt + 1;
        let Some(end) = s[body_start..].find(&close) else { break };
        out.push((head, &s[body_start..body_start + end]));
        at = body_start + end + close.len();
    }
    out
}

fn flag(s: &str, name: &str) -> bool {
    inner(s, name).is_some_and(|v| v.trim() == "true")
}

fn cooked(body: &str) -> Option<Vec<u8>> {
    let hex: String = inner(body, "cookedData")?.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()).collect()
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn f32s(b: &[u8], o: usize, n: usize) -> Option<Vec<[f32; 3]>> {
    (0..n)
        .map(|k| {
            let at = o + 12 * k;
            let f = |i: usize| Some(f32::from_le_bytes(b.get(at + 4 * i..at + 4 * i + 4)?.try_into().ok()?));
            Some([f(0)?, f(1)?, f(2)?])
        })
        .collect()
}

/// Vertices and triangles.
type MeshData = (Vec<[f32; 3]>, Vec<[u32; 3]>);

/// A cooked convex hull's vertices (`CVHL`: version, vertex count, five counts, vertices).
fn hull(b: &[u8]) -> Option<Vec<[f32; 3]>> {
    let i = b.windows(4).position(|w| w == b"CVHL")?;
    let n = u32_at(b, i + 8)? as usize;
    f32s(b, i + 0x20, n)
}

/// A cooked triangle mesh (`MESH`: version, flags, threshold, height axis, extent, counts,
/// vertices, indices).
fn mesh(b: &[u8]) -> Option<MeshData> {
    let i = b.windows(4).position(|w| w == b"MESH")?;
    let flags = u32_at(b, i + 8)?;
    let nv = u32_at(b, i + 0x18)? as usize;
    let nt = u32_at(b, i + 0x1c)? as usize;
    let verts = f32s(b, i + 0x20, nv)?;
    let mut o = i + 0x20 + 12 * nv;
    let size = if flags & 0x08 != 0 { 1 } else if flags & 0x10 != 0 { 2 } else { 4 };
    let mut tris = Vec::with_capacity(nt);
    for _ in 0..nt {
        let mut t = [0u32; 3];
        for v in &mut t {
            let s = b.get(o..o + size)?;
            *v = match size {
                1 => s[0] as u32,
                2 => u16::from_le_bytes([s[0], s[1]]) as u32,
                _ => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            };
            o += size;
        }
        if t.iter().any(|&v| v as usize >= nv) {
            return None;
        }
        tris.push(t);
    }
    Some((verts, tris))
}

/// The colliding shapes of a collection's static actors (actors with a body, triggers and shapes
/// with collision off are left out; so are planes).
pub fn parse(text: &str) -> Vec<Shape> {
    let hulls: Vec<(&str, Vec<[f32; 3]>)> = elements(text, "NxConvexMeshDesc")
        .into_iter()
        .filter_map(|(h, b)| Some((attr(h, "id")?, hull(&cooked(b)?)?)))
        .collect();
    let meshes: Vec<(&str, MeshData)> = elements(text, "NxTriangleMeshDesc")
        .into_iter()
        .filter_map(|(h, b)| Some((attr(h, "id")?, mesh(&cooked(b)?)?)))
        .collect();
    let mut out = Vec::new();
    for (head, actor) in elements(text, "NxActorDesc") {
        if attr(head, "hasBody") == Some("true") || flag(actor, "NX_AF_DISABLE_COLLISION") {
            continue;
        }
        let Some(global) = inner(actor, "globalPose").and_then(pose) else { continue };
        for tag in ["NxBoxShapeDesc", "NxSphereShapeDesc", "NxCapsuleShapeDesc", "NxConvexShapeDesc", "NxTriangleMeshShapeDesc"] {
            for (h, body) in elements(actor, tag) {
                if ["NX_TRIGGER_ON_ENTER", "NX_TRIGGER_ON_LEAVE", "NX_TRIGGER_ON_STAY", "NX_SF_DISABLE_COLLISION"]
                    .iter()
                    .any(|f| flag(body, f))
                {
                    continue;
                }
                let local = inner(body, "localPose").and_then(pose).unwrap_or(Pose::IDENTITY);
                let num = |k: &str| attr(h, k).and_then(|v| v.trim().parse::<f32>().ok());
                let kind = match tag {
                    "NxBoxShapeDesc" => {
                        let d = floats(attr(h, "dimensions").unwrap_or(""));
                        if d.len() < 3 {
                            continue;
                        }
                        ShapeKind::Box([d[0], d[1], d[2]])
                    }
                    "NxSphereShapeDesc" => ShapeKind::Sphere(num("radius").unwrap_or(0.0)),
                    "NxCapsuleShapeDesc" => ShapeKind::Capsule {
                        radius: num("radius").unwrap_or(0.0),
                        height: num("height").unwrap_or(0.0),
                    },
                    "NxConvexShapeDesc" => {
                        let Some(v) = attr(h, "meshData").and_then(|id| hulls.iter().find(|x| x.0 == id)) else {
                            continue;
                        };
                        ShapeKind::Convex(v.1.clone())
                    }
                    _ => {
                        let Some(m) = attr(h, "meshData").and_then(|id| meshes.iter().find(|x| x.0 == id)) else {
                            continue;
                        };
                        ShapeKind::Mesh(m.1.0.clone(), m.1.1.clone())
                    }
                };
                out.push(Shape { pose: global.then(&local).y_up(), kind });
            }
        }
    }
    out
}

pub fn read(path: &Path) -> io::Result<Vec<Shape>> {
    let b = std::fs::read(path)?;
    Ok(parse(&String::from_utf8_lossy(&b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_in_y_up() {
        let x = r#"<NxActorDesc id="Actor_0" hasBody="false">
          <globalPose>1 0 0  0 1 0  0 0 1  1 2 3</globalPose>
          <NxBoxShapeDesc dimensions="4 5 6">
            <NxShapeDesc><localPose>1 0 0  0 1 0  0 0 1  0 0 10</localPose>
            <NX_TRIGGER_ON_ENTER>false</NX_TRIGGER_ON_ENTER></NxShapeDesc>
          </NxBoxShapeDesc></NxActorDesc>"#;
        let s = parse(x);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].kind, ShapeKind::Box([4.0, 5.0, 6.0]));
        // (1, 2, 13) Z-up is (1, 13, -2) Y-up; local Z (the box's 6) is world Y
        assert_eq!(s[0].pose.pos, [1.0, 13.0, -2.0]);
        assert_eq!(s[0].pose.apply([0.0, 0.0, 1.0]), [1.0, 14.0, -2.0]);
    }
}
