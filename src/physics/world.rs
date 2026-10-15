//! What ragdolls collide with besides each other: the ground, the buildings' collision (their
//! parts' "xml collision", PhysX NxuStream2 shapes) and the people standing about.
use glam::{Affine3A, Mat3, Vec3, Vec3A};
use kenshi_io::nxu::{Shape, ShapeKind};
use std::collections::HashMap;
use std::sync::Arc;

/// A solid in its own frame.
#[derive(Debug, Clone)]
pub enum Solid {
    /// Half extents.
    Box(Vec3),
    Sphere(f32),
    /// Along local Y, its end spheres' centres `half` either side.
    Capsule { half: f32, radius: f32 },
    /// Outward face planes (normal, offset: n . x = d).
    Convex(Vec<(Vec3, f32)>),
    Mesh(Arc<TriMesh>),
}

/// Triangles binned on a ground-plane grid.
#[derive(Debug)]
pub struct TriMesh {
    tris: Vec<[Vec3; 3]>,
    lo: Vec3,
    cell: f32,
    cols: i32,
    rows: i32,
    bins: Vec<Vec<u32>>,
}

const MESH_CELL: f32 = 8.0;

impl TriMesh {
    fn new(verts: &[[f32; 3]], idx: &[[u32; 3]]) -> TriMesh {
        let tris: Vec<[Vec3; 3]> = idx
            .iter()
            .map(|t| t.map(|i| Vec3::from(verts[i as usize])))
            .collect();
        let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
        for t in &tris {
            for v in t {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        if tris.is_empty() {
            lo = Vec3::ZERO;
            hi = Vec3::ZERO;
        }
        let cols = (((hi.x - lo.x) / MESH_CELL).ceil() as i32).max(1);
        let rows = (((hi.z - lo.z) / MESH_CELL).ceil() as i32).max(1);
        let mut bins = vec![Vec::new(); (cols * rows) as usize];
        for (k, t) in tris.iter().enumerate() {
            let a = t[0].min(t[1]).min(t[2]);
            let b = t[0].max(t[1]).max(t[2]);
            let c0 = (((a.x - lo.x) / MESH_CELL) as i32).clamp(0, cols - 1);
            let c1 = (((b.x - lo.x) / MESH_CELL) as i32).clamp(0, cols - 1);
            let r0 = (((a.z - lo.z) / MESH_CELL) as i32).clamp(0, rows - 1);
            let r1 = (((b.z - lo.z) / MESH_CELL) as i32).clamp(0, rows - 1);
            for r in r0..=r1 {
                for c in c0..=c1 {
                    bins[(r * cols + c) as usize].push(k as u32);
                }
            }
        }
        TriMesh { tris, lo, cell: MESH_CELL, cols, rows, bins }
    }

    fn near(&self, p: Vec3, r: f32) -> impl Iterator<Item = &[Vec3; 3]> {
        let c0 = (((p.x - r - self.lo.x) / self.cell) as i32).clamp(0, self.cols - 1);
        let c1 = (((p.x + r - self.lo.x) / self.cell) as i32).clamp(0, self.cols - 1);
        let r0 = (((p.z - r - self.lo.z) / self.cell) as i32).clamp(0, self.rows - 1);
        let r1 = (((p.z + r - self.lo.z) / self.cell) as i32).clamp(0, self.rows - 1);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                for &k in &self.bins[(row * self.cols + col) as usize] {
                    if seen.insert(k) {
                        out.push(&self.tris[k as usize]);
                    }
                }
            }
        }
        out.into_iter()
    }
}

/// The closest point of triangle `t` to `p`.
fn closest_on_triangle(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    let [a, b, c] = *t;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let den = 1.0 / (va + vb + vc);
    a + ab * (vb * den) + ac * (vc * den)
}

/// The face planes of the hull of `pts` (every plane through three of them with the rest
/// behind it).
fn hull_planes(pts: &[Vec3]) -> Vec<(Vec3, f32)> {
    let n = pts.len();
    let size = pts.iter().fold(0.0f32, |m, p| m.max(p.abs().max_element())).max(1.0);
    let eps = size * 1e-4;
    let mut planes: Vec<(Vec3, f32)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let nrm = (pts[j] - pts[i]).cross(pts[k] - pts[i]);
                let len = nrm.length();
                if len < eps * eps {
                    continue;
                }
                let nrm = nrm / len;
                let d = nrm.dot(pts[i]);
                let (mut above, mut below) = (false, false);
                for p in pts {
                    let s = nrm.dot(*p) - d;
                    above |= s > eps;
                    below |= s < -eps;
                    if above && below {
                        break;
                    }
                }
                let plane = match (above, below) {
                    (false, true) => (nrm, d),
                    (true, false) => (-nrm, -d),
                    _ => continue,
                };
                if !planes.iter().any(|(m, e)| m.dot(plane.0) > 0.9999 && (e - plane.1).abs() < eps * 10.0) {
                    planes.push(plane);
                }
            }
        }
    }
    planes
}

/// A collision file's solids, each with its frame in the file's (the part's) frame.
#[derive(Debug, Default)]
pub struct Solids {
    pub items: Vec<(Affine3A, Solid, Vec3)>,
    /// Each item's hull points (its own frame) when it is a hull; else empty.
    pub hulls: Vec<Vec<Vec3>>,
}

impl Solids {
    pub fn new(shapes: &[Shape]) -> Solids {
        let all: Vec<_> = shapes
            .iter()
            .filter_map(|s| {
                let r = s.pose.rows;
                let m = Mat3::from_cols(
                    Vec3::new(r[0][0], r[1][0], r[2][0]),
                    Vec3::new(r[0][1], r[1][1], r[2][1]),
                    Vec3::new(r[0][2], r[1][2], r[2][2]),
                );
                let pose = Affine3A::from_mat3_translation(m, Vec3::from(s.pose.pos));
                let (solid, reach) = match &s.kind {
                    ShapeKind::Box(h) => (Solid::Box(Vec3::from(*h)), Vec3::from(*h)),
                    ShapeKind::Sphere(r) => (Solid::Sphere(*r), Vec3::splat(*r)),
                    ShapeKind::Capsule { radius, height } => {
                        (Solid::Capsule { half: height * 0.5, radius: *radius }, Vec3::new(*radius, height * 0.5 + radius, *radius))
                    }
                    ShapeKind::Convex(v) => {
                        let pts: Vec<Vec3> = v.iter().map(|p| Vec3::from(*p)).collect();
                        let planes = hull_planes(&pts);
                        if planes.len() < 4 {
                            return None;
                        }
                        let reach = pts.iter().fold(Vec3::ZERO, |m, p| m.max(p.abs()));
                        return Some((pose, Solid::Convex(planes), reach, pts));
                    }
                    ShapeKind::Mesh(v, t) => {
                        let reach = v.iter().fold(Vec3::ZERO, |m, p| m.max(Vec3::from(*p).abs()));
                        (Solid::Mesh(Arc::new(TriMesh::new(v, t))), reach)
                    }
                };
                Some((pose, solid, reach, Vec::new()))
            })
            .collect();
        let hulls = all.iter().map(|a| a.3.clone()).collect();
        let items = all.into_iter().map(|(p, s, r, _)| (p, s, r)).collect();
        Solids { items, hulls }
    }

    /// What stands on the ground, for cutting the navmesh: each solid as points whose hull is
    /// its body (boxes, spheres and capsules by their bounding box corners, hulls by their
    /// points), a triangle mesh by its steep triangles (walls; floors and ramps are walked on),
    /// all in the part's frame; each with whether it is a mesh's wall.
    pub fn footprints(&self) -> Vec<(Vec<Vec3>, bool)> {
        let mut out = Vec::new();
        for (k, (pose, solid, reach)) in self.items.iter().enumerate() {
            match solid {
                Solid::Mesh(m) => {
                    for t in &m.tris {
                        let t = t.map(|v| pose.transform_point3(v));
                        let n = (t[1] - t[0]).cross(t[2] - t[0]);
                        let len = n.length();
                        // (assumption) steeper than 60 degrees from level is a wall
                        if len > 1e-6 && (n.y / len).abs() < 0.5 {
                            out.push((t.to_vec(), true));
                        }
                    }
                }
                Solid::Convex(_) if !self.hulls[k].is_empty() => out.push((self.hulls[k].iter().map(|v| pose.transform_point3(*v)).collect(), false)),
                _ => {
                    let r = *reach;
                    let corners = (0..8).map(|i| Vec3::new(if i & 1 == 0 { -r.x } else { r.x }, if i & 2 == 0 { -r.y } else { r.y }, if i & 4 == 0 { -r.z } else { r.z }));
                    out.push((corners.map(|v| pose.transform_point3(v)).collect(), false));
                }
            }
        }
        out
    }
}

struct Placed {
    /// World bounds.
    lo: Vec3,
    hi: Vec3,
    to_local: Affine3A,
    to_world: Mat3,
    solid: Solid,
}

/// Placed solids binned on a ground-plane grid.
#[derive(Default)]
pub struct StaticWorld {
    placed: Vec<Placed>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

const CELL: f32 = 40.0;
/// How far beyond a solid's bounds a sphere is looked for (units).
const MARGIN: f32 = 3.0;

impl StaticWorld {
    /// The solids of every (frame, collision) given.
    pub fn new<'a>(parts: impl IntoIterator<Item = (Affine3A, &'a Solids)>) -> StaticWorld {
        let mut w = StaticWorld::default();
        for (frame, solids) in parts {
            for (pose, solid, reach) in &solids.items {
                let world = frame * *pose;
                // the bounds of the solid's local box, placed
                let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
                for k in 0..8 {
                    let c = Vec3::new(
                        if k & 1 == 0 { -reach.x } else { reach.x },
                        if k & 2 == 0 { -reach.y } else { reach.y },
                        if k & 4 == 0 { -reach.z } else { reach.z },
                    );
                    let p = world.transform_point3(c);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                let i = w.placed.len() as u32;
                w.placed.push(Placed {
                    lo,
                    hi,
                    to_local: world.inverse(),
                    to_world: Mat3::from(world.matrix3),
                    solid: solid.clone(),
                });
                let cell = |v: f32| ((v / CELL).floor()) as i32;
                for cx in cell(lo.x - MARGIN)..=cell(hi.x + MARGIN) {
                    for cz in cell(lo.z - MARGIN)..=cell(hi.z + MARGIN) {
                        w.cells.entry((cx, cz)).or_default().push(i);
                    }
                }
            }
        }
        w
    }

    pub fn len(&self) -> usize {
        self.placed.len()
    }

    /// Each solid's world bounds.
    pub fn bounds(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        self.placed.iter().map(|p| (p.lo, p.hi))
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// Where a sphere at `p` of radius `r` overlaps a solid: the way out and how deep.
    pub fn touches(&self, p: Vec3, r: f32, out: &mut Vec<(Vec3, f32)>) {
        let key = ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
        let Some(list) = self.cells.get(&key) else { return };
        for &i in list {
            let s = &self.placed[i as usize];
            let q = Vec3::from(s.to_local.transform_point3a(Vec3A::from(p)));
            if let Some((n, d)) = sphere_in(&s.solid, q, r) {
                out.push(((s.to_world * n).normalize_or(Vec3::Y), d));
            }
        }
    }
}

impl StaticWorld {
    /// The distance along the ray from `from` (unit direction `dir`) to the first solid it
    /// enters within `len`, if any (a solid it starts inside is passed out of): the cells it
    /// crosses, each solid met in its own frame.
    pub fn raycast(&self, from: Vec3, dir: Vec3, len: f32) -> Option<f32> {
        if self.placed.is_empty() || len <= 0.0 {
            return None;
        }
        let mut seen = std::collections::HashSet::new();
        let mut best: Option<f32> = None;
        // the cells under the segment, sampled at a quarter cell
        let steps = ((len / (CELL * 0.25)).ceil() as i32).max(1);
        for k in 0..=steps {
            let p = from + dir * (len * k as f32 / steps as f32);
            let key = ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
            let Some(list) = self.cells.get(&key) else { continue };
            for &i in list {
                if !seen.insert(i) {
                    continue;
                }
                let s = &self.placed[i as usize];
                // the ray against the world bounds first
                if ray_box(from - (s.lo + s.hi) * 0.5, dir, (s.hi - s.lo) * 0.5, len).is_none() {
                    continue;
                }
                let o = Vec3::from(s.to_local.transform_point3a(Vec3A::from(from)));
                let d = Vec3::from(s.to_local.transform_vector3a(Vec3A::from(dir)));
                // (rigid frames: the local direction keeps its length)
                if let Some(t) = ray_in(&s.solid, o, d, len).filter(|&t| 0.0 < t)
                    && best.is_none_or(|b| t < b)
                {
                    best = Some(t);
                }
            }
        }
        best
    }

    /// [`StaticWorld::raycast`] with plain arrays.
    pub fn raycast_at(&self, from: [f32; 3], dir: [f32; 3], len: f32) -> Option<f32> {
        self.raycast(Vec3::from(from), Vec3::from(dir), len)
    }
}

/// A ray from `o` along `d` against a box of half extents `h` about the origin: the entering
/// distance within `len` (0 inside).
fn ray_box(o: Vec3, d: Vec3, h: Vec3, len: f32) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, len);
    for k in 0..3 {
        if d[k].abs() < 1e-9 {
            if o[k] < -h[k] || h[k] < o[k] {
                return None;
            }
            continue;
        }
        let (mut a, mut b) = ((-h[k] - o[k]) / d[k], (h[k] - o[k]) / d[k]);
        if b < a {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t1 < t0 {
            return None;
        }
    }
    Some(t0)
}

/// A ray against a sphere of radius `r` about `c`: the entering distance within `len`.
fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32, len: f32) -> Option<f32> {
    let m = o - c;
    let b = m.dot(d);
    let cc = m.dot(m) - r * r;
    if cc <= 0.0 {
        return Some(0.0);
    }
    let disc = b * b - d.dot(d) * cc;
    if b > 0.0 || disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / d.dot(d);
    (0.0..=len).contains(&t).then_some(t)
}

/// A ray (the solid's frame) against the solid: the entering distance within `len`.
fn ray_in(solid: &Solid, o: Vec3, d: Vec3, len: f32) -> Option<f32> {
    match solid {
        Solid::Box(h) => ray_box(o, d, *h, len),
        Solid::Sphere(r) => ray_sphere(o, d, Vec3::ZERO, *r, len),
        Solid::Capsule { half, radius } => {
            // the end spheres, and the side (a cylinder about y cut to the segment)
            let mut best = [ray_sphere(o, d, Vec3::Y * *half, *radius, len), ray_sphere(o, d, -Vec3::Y * *half, *radius, len)]
                .into_iter()
                .flatten()
                .fold(None, |m: Option<f32>, t| Some(m.map_or(t, |m| m.min(t))));
            let (a, b, c) = (d.x * d.x + d.z * d.z, o.x * d.x + o.z * d.z, o.x * o.x + o.z * o.z - radius * radius);
            let t = if c <= 0.0 {
                Some(0.0)
            } else if a > 1e-12 && b * b - a * c >= 0.0 {
                Some((-b - (b * b - a * c).sqrt()) / a)
            } else {
                None
            };
            if let Some(t) = t.filter(|t| (0.0..=len).contains(t))
                && (o.y + d.y * t).abs() <= *half
            {
                best = Some(best.map_or(t, |m| m.min(t)));
            }
            best
        }
        Solid::Convex(planes) => {
            let (mut t0, mut t1) = (0.0f32, len);
            for (n, pd) in planes {
                let (dn, s) = (n.dot(d), n.dot(o) - pd);
                if dn.abs() < 1e-9 {
                    if s > 0.0 {
                        return None;
                    }
                    continue;
                }
                let t = -s / dn;
                if dn < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
                if t1 < t0 {
                    return None;
                }
            }
            Some(t0)
        }
        Solid::Mesh(m) => {
            let mut best: Option<f32> = None;
            let steps = ((len / (m.cell * 0.5)).ceil() as i32).max(1);
            let mut seen = std::collections::HashSet::new();
            for k in 0..=steps {
                let p = o + d * (len * k as f32 / steps as f32);
                for t in m.near(p, m.cell * 0.5) {
                    let key = t as *const [Vec3; 3] as usize;
                    if !seen.insert(key) {
                        continue;
                    }
                    if let Some(u) = ray_triangle(o, d, t)
                        && u <= len
                        && best.is_none_or(|b| u < b)
                    {
                        best = Some(u);
                    }
                }
            }
            best
        }
    }
}

/// A ray against a triangle (either side): its distance (Moller-Trumbore).
fn ray_triangle(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let w = e2.dot(q) * inv;
    (w >= 0.0).then_some(w)
}

/// A sphere at `q` (the solid's frame) of radius `r` against the solid: outward normal (solid's
/// frame) and depth.
fn sphere_in(solid: &Solid, q: Vec3, r: f32) -> Option<(Vec3, f32)> {
    let push = |c: Vec3| {
        let d = q - c;
        let dist = d.length();
        (dist < r && dist > 1e-6).then(|| (d / dist, r - dist))
    };
    match solid {
        Solid::Sphere(s) => {
            let d = q.length();
            (d < r + s).then(|| (q.normalize_or(Vec3::Y), r + s - d))
        }
        Solid::Capsule { half, radius } => {
            let c = Vec3::new(0.0, q.y.clamp(-half, *half), 0.0);
            let d = q - c;
            let dist = d.length();
            (dist < r + radius).then(|| (d.normalize_or(Vec3::Y), r + radius - dist))
        }
        Solid::Box(h) => {
            let inside = q.abs().cmple(*h).all();
            if inside {
                // out through the nearest face
                let gap = *h - q.abs();
                let axis = if gap.x <= gap.y && gap.x <= gap.z {
                    Vec3::X
                } else if gap.y <= gap.z {
                    Vec3::Y
                } else {
                    Vec3::Z
                };
                let g = gap.dot(axis);
                let s = q.dot(axis).signum();
                Some((axis * if s == 0.0 { 1.0 } else { s }, g + r))
            } else {
                push(q.clamp(-*h, *h))
            }
        }
        Solid::Convex(planes) => {
            let (mut best, mut n) = (f32::MIN, Vec3::Y);
            for (pn, pd) in planes {
                let s = pn.dot(q) - pd;
                if s > best {
                    best = s;
                    n = *pn;
                }
            }
            (best < r).then_some((n, r - best))
        }
        Solid::Mesh(m) => {
            let mut deepest: Option<(Vec3, f32)> = None;
            for t in m.near(q, r) {
                if let Some((n, d)) = push(closest_on_triangle(q, t))
                    && deepest.is_none_or(|(_, e)| d > e)
                {
                    deepest = Some((n, d));
                }
            }
            deepest
        }
    }
}

/// Everything a ragdoll collides with apart from itself.
pub trait Surroundings {
    /// The ground's height (scene units).
    fn ground(&self, x: f32, z: f32) -> f32;
    /// Solids a sphere overlaps: outward normal and depth each.
    fn touches(&self, p: Vec3, r: f32, out: &mut Vec<(Vec3, f32)>);
}

/// The ground alone (flat or given).
pub struct Ground<F: Fn(f32, f32) -> f32>(pub F);

impl<F: Fn(f32, f32) -> f32> Surroundings for Ground<F> {
    fn ground(&self, x: f32, z: f32) -> f32 {
        (self.0)(x, z)
    }
    fn touches(&self, _: Vec3, _: f32, _: &mut Vec<(Vec3, f32)>) {}
}

/// The ground, the buildings and people (as upright capsules: foot, head, radius).
pub struct Scene<'a, F: Fn(f32, f32) -> f32> {
    pub ground: F,
    pub statics: &'a StaticWorld,
    pub people: &'a [(Vec3, Vec3, f32)],
}

impl<F: Fn(f32, f32) -> f32> Surroundings for Scene<'_, F> {
    fn ground(&self, x: f32, z: f32) -> f32 {
        (self.ground)(x, z)
    }
    fn touches(&self, p: Vec3, r: f32, out: &mut Vec<(Vec3, f32)>) {
        self.statics.touches(p, r, out);
        for &(a, b, pr) in self.people {
            let lo = a.y.min(b.y);
            let hi = a.y.max(b.y);
            let c = Vec3::new(a.x, p.y.clamp(lo + pr, (hi - pr).max(lo + pr)), a.z);
            let d = p - c;
            let dist = d.length();
            if dist < r + pr && dist > 1e-6 {
                out.push((d / dist, r + pr - dist));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenshi_io::nxu::Pose;

    #[test]
    fn hull_of_a_cube() {
        let pts: Vec<Vec3> = (0..8)
            .map(|k| Vec3::new((k & 1) as f32, ((k >> 1) & 1) as f32, ((k >> 2) & 1) as f32))
            .collect();
        assert_eq!(hull_planes(&pts).len(), 6);
    }

    #[test]
    fn sphere_on_a_box() {
        let s = Solids::new(&[Shape { pose: Pose { rows: Pose::IDENTITY.rows, pos: [0.0, 5.0, 0.0] }, kind: ShapeKind::Box([2.0, 1.0, 2.0]) }]);
        let w = StaticWorld::new([(Affine3A::IDENTITY, &s)]);
        let mut out = vec![];
        w.touches(Vec3::new(0.5, 6.4, 0.0), 0.5, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].0.y > 0.99 && (out[0].1 - 0.1).abs() < 1e-4, "{out:?}");
    }

    #[test]
    fn rays_meet_solids() {
        let s = Solids::new(&[
            Shape { pose: Pose { rows: Pose::IDENTITY.rows, pos: [50.0, 5.0, 0.0] }, kind: ShapeKind::Box([2.0, 5.0, 2.0]) },
            Shape { pose: Pose { rows: Pose::IDENTITY.rows, pos: [0.0, 0.0, 30.0] }, kind: ShapeKind::Mesh(vec![[-10., 0., 0.], [10., 0., 0.], [0., 20., 0.]], vec![[0, 1, 2]]) },
        ]);
        let w = StaticWorld::new([(Affine3A::IDENTITY, &s)]);
        let t = w.raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::X, 100.0).unwrap();
        assert!((t - 48.0).abs() < 1e-4, "{t}");
        assert!(w.raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::X, 40.0).is_none());
        assert!(w.raycast(Vec3::new(0.0, 12.0, 0.0), Vec3::X, 100.0).is_none());
        let t = w.raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::Z, 100.0).unwrap();
        assert!((t - 30.0).abs() < 1e-4, "{t}");
    }

    #[test]
    fn sphere_on_a_mesh() {
        let s = Solids::new(&[Shape {
            pose: Pose::IDENTITY,
            kind: ShapeKind::Mesh(vec![[-10., 0., -10.], [10., 0., -10.], [0., 0., 10.]], vec![[0, 2, 1]]),
        }]);
        let w = StaticWorld::new([(Affine3A::IDENTITY, &s)]);
        let mut out = vec![];
        w.touches(Vec3::new(0.0, 0.3, 0.0), 0.5, &mut out);
        assert!(out.len() == 1 && out[0].0.y > 0.99 && (out[0].1 - 0.2).abs() < 1e-4, "{out:?}");
    }
}
