//! Navmesh cutting (port_map `ai`/pathing): Kenshi cuts the shipped ground navmesh at runtime
//! round what stands on it (Havok AI's navmesh cutter, fed the silhouettes of the buildings'
//! physics bodies), so characters walk round buildings the tiles were not baked with (player
//! buildings, town buildings of other world states, props).
//!
//! (replacement) The cutter is Havok library code, not Kenshi's: here each ground face a cutter
//! overlaps (at walking height) is shut (FACE_BLOCKED) and the parts of it outside every cutter
//! become the faces of a section of their own (uid: the ground's + CUT_UID), joined to their
//! neighbours by streaming-style links along the edges they share (the portals are the shared
//! stretches, as `NavWorld::across` measures them). Faces linked to a building's own navmesh
//! (its doors and stairs, baked with it) are left whole.
//! (assumption) A cutter blocks a face when it reaches from below 1.5 m to above 0.6 m over it
//! and is at least 0.4 m tall (lower things are stepped over); the silhouettes are widened by
//! 0.1 m.

use crate::navmesh::{Edge, FACE_BLOCKED, Face, FaceRef, NavWorld, Section, grid_cell, tile_of, tile_uid};
use std::collections::{HashMap, HashSet};

/// Added to a ground section's uid for its cut pieces' section.
pub const CUT_UID: i64 = 1 << 40;
/// The cutter grid's cell (metres).
const CELL: f32 = 8.0;
/// How far a silhouette is widened (metres).
pub const PAD: f32 = 0.1;

/// What stands on the ground: its outline (x, z metres, counter-clockwise, convex) and the
/// heights it stands between.
#[derive(Debug, Clone)]
pub struct Cutter {
    pub outline: Vec<[f32; 2]>,
    pub lo: f32,
    pub hi: f32,
    /// The part it is of (the caller's number; cutters of a part cut together or not at all).
    pub group: u32,
    min: [f32; 2],
    max: [f32; 2],
}

impl Cutter {
    /// The cutter round `pts` (metres): their hull on the ground plane widened by `pad`,
    /// standing between the lowest and highest of them.
    pub fn from_points(pts: &[[f32; 3]], pad: f32) -> Option<Cutter> {
        if pts.is_empty() {
            return None;
        }
        let mut flat = Vec::with_capacity(pts.len() * 4);
        for p in pts {
            for (dx, dz) in [(-pad, -pad), (pad, -pad), (pad, pad), (-pad, pad)] {
                flat.push([p[0] + dx, p[2] + dz]);
            }
        }
        let outline = hull(flat);
        if outline.len() < 3 || area(&outline) < 1e-4 {
            return None;
        }
        let lo = pts.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let hi = pts.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        let min = outline.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
        let max = outline.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
        Some(Cutter { outline, lo, hi, group: 0, min, max })
    }
}

fn cross(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// The polygon's signed area (positive counter-clockwise).
fn area(p: &[[f32; 2]]) -> f32 {
    (0..p.len()).map(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        a[0] * b[1] - b[0] * a[1]
    }).sum::<f32>() * 0.5
}

/// The convex hull, counter-clockwise (monotone chain).
fn hull(mut pts: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<[f32; 2]> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<[f32; 2]> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// A convex polygon split by the line a -> b: the part on its left (inside a counter-clockwise
/// outline), and the part on its right.
fn split(p: &[[f32; 2]], a: [f32; 2], b: [f32; 2]) -> (Vec<[f32; 2]>, Vec<[f32; 2]>) {
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for i in 0..p.len() {
        let (c, d) = (p[i], p[(i + 1) % p.len()]);
        let (sc, sd) = (cross(a, b, c), cross(a, b, d));
        if sc >= 0.0 {
            left.push(c);
        }
        if sc <= 0.0 {
            right.push(c);
        }
        if (sc > 0.0 && sd < 0.0) || (sc < 0.0 && sd > 0.0) {
            let t = sc / (sc - sd);
            let x = [c[0] + (d[0] - c[0]) * t, c[1] + (d[1] - c[1]) * t];
            left.push(x);
            right.push(x);
        }
    }
    (left, right)
}

/// Convex `p` less convex `q` (both counter-clockwise): convex pieces.
fn subtract(p: &[[f32; 2]], q: &[[f32; 2]]) -> Vec<Vec<[f32; 2]>> {
    let mut rest = p.to_vec();
    let mut out = Vec::new();
    for i in 0..q.len() {
        let (inside, outside) = split(&rest, q[i], q[(i + 1) % q.len()]);
        if outside.len() >= 3 && area(&outside) > 1e-4 {
            out.push(outside);
        }
        rest = inside;
        if rest.len() < 3 || area(&rest) <= 1e-6 {
            return out;
        }
    }
    out
}

/// Whether edges a-b and c-d lie along each other (x, z within 2 cm, heights within 0.6 m)
/// sharing more than 2 cm.
fn shared(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> bool {
    let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
    let len = dx.hypot(dz);
    if len < 1e-4 {
        return false;
    }
    let (ux, uz) = (dx / len, dz / len);
    let off = |p: [f32; 3]| ((p[0] - a[0]) * uz - (p[2] - a[2]) * ux).abs();
    if off(c) > 0.02 || off(d) > 0.02 {
        return false;
    }
    let along = |p: [f32; 3]| (p[0] - a[0]) * ux + (p[2] - a[2]) * uz;
    let (tc, td) = (along(c), along(d));
    let (lo, hi) = (tc.min(td).max(0.0), tc.max(td).min(len));
    if hi - lo < 0.02 {
        return false;
    }
    let mid = (lo + hi) * 0.5;
    let y_ab = a[1] + (b[1] - a[1]) * mid / len;
    let s = if (td - tc).abs() > 1e-6 { (mid - tc) / (td - tc) } else { 0.0 };
    let y_cd = c[1] + (d[1] - c[1]) * s;
    (y_ab - y_cd).abs() < 0.6
}

impl NavWorld {
    /// Cutters for what `key` (a zone, a building) puts on the ground; a key already given is
    /// not cut again. The loaded ground sections they reach are cut now, the rest as they load.
    pub fn add_cutters(&mut self, key: (i64, i64), cutters: Vec<Cutter>) {
        if !self.cut_keys.insert(key) || cutters.is_empty() {
            return;
        }
        // (testing) KENSHI_NO_NAVCUT: the cutters kept but nothing cut
        if std::env::var("KENSHI_NO_NAVCUT").is_ok() {
            self.cutters.extend(cutters.iter().cloned());
            for (i, c) in cutters.iter().enumerate() {
                let i = self.cutters.len() - cutters.len() + i;
                for gx in (c.min[0] / CELL).floor() as i32..=(c.max[0] / CELL).floor() as i32 {
                    for gz in (c.min[1] / CELL).floor() as i32..=(c.max[1] / CELL).floor() as i32 {
                        self.cutter_grid.entry((gx, gz)).or_default().push(i);
                    }
                }
            }
            return;
        }
        let base = self.cutters.iter().map(|c| c.group + 1).max().unwrap_or(0);
        let mut cutters = cutters;
        for c in &mut cutters {
            c.group += base;
        }
        let mut touched = HashSet::new();
        if std::env::var("KENSHI_NAV_DEBUG").is_ok() {
            let mut big: Vec<(f32, f32, f32, [f32; 2])> = cutters.iter().map(|c| (area(&c.outline), c.lo, c.hi, c.min)).collect();
            big.sort_by(|a, b| b.0.total_cmp(&a.0));
            for b in big.iter().take(8) {
                eprintln!("  cutter {:.0} m2, heights {:.1}..{:.1} at {:?}", b.0, b.1, b.2, b.3);
            }
        }
        for c in cutters {
            let i = self.cutters.len();
            for gx in (c.min[0] / CELL).floor() as i32..=(c.max[0] / CELL).floor() as i32 {
                for gz in (c.min[1] / CELL).floor() as i32..=(c.max[1] / CELL).floor() as i32 {
                    self.cutter_grid.entry((gx, gz)).or_default().push(i);
                }
            }
            for p in [[c.min[0], 0.0, c.min[1]], [c.max[0], 0.0, c.max[1]], [c.min[0], 0.0, c.max[1]], [c.max[0], 0.0, c.min[1]]] {
                let (x, y) = tile_of(p);
                touched.insert(tile_uid(x, y));
            }
            self.cutters.push(c);
        }
        for uid in touched {
            if self.sections.contains_key(&uid) || self.pristine.contains_key(&uid) {
                self.cut_section(uid);
            }
        }
    }

    /// Whether cutter `i` stands in the way at ground height `y` (metres): reaching from below
    /// 1.5 m to above 0.6 m over it, and at least 0.4 m tall (lower things, floor plates and
    /// steps, are walked over).
    fn stands_on(&self, i: usize, y: f32) -> bool {
        let c = &self.cutters[i];
        c.lo < y + 1.5 && c.hi > y + 0.6 && c.hi - c.lo > 0.4
    }

    /// The parts (cutter groups) over section `ground` that stand on its bare ground (as loaded):
    /// at least half their cutters' middles over its faces. The others stand where the tile was
    /// baked with them (a hole in the ground, their own navmesh) and are not cut again.
    fn bare_parts(&self, ground: &Section) -> HashSet<u32> {
        // the section's faces as loaded, by 4 m cell
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for fi in 0..ground.faces.len() as u32 {
            if ground.faces[fi as usize].kind == FACE_BLOCKED {
                continue;
            }
            let c: Vec<[f32; 3]> = ground.corners(fi).collect();
            let (mut a, mut b) = ([f32::MAX; 2], [f32::MIN; 2]);
            for v in &c {
                a = [a[0].min(v[0]), a[1].min(v[2])];
                b = [b[0].max(v[0]), b[1].max(v[2])];
            }
            lo = [lo[0].min(a[0]), lo[1].min(a[1])];
            hi = [hi[0].max(b[0]), hi[1].max(b[1])];
            for gx in (a[0] / 4.0).floor() as i32..=(b[0] / 4.0).floor() as i32 {
                for gz in (a[1] / 4.0).floor() as i32..=(b[1] / 4.0).floor() as i32 {
                    cells.entry((gx, gz)).or_default().push(fi);
                }
            }
        }
        if lo[0] > hi[0] {
            return HashSet::new();
        }
        let over = |x: f32, z: f32| {
            cells.get(&((x / 4.0).floor() as i32, (z / 4.0).floor() as i32)).into_iter().flatten().any(|&fi| {
                let c: Vec<[f32; 2]> = ground.corners(fi).map(|v| [v[0], v[2]]).collect();
                let s: Vec<f32> = (0..c.len()).map(|k| cross(c[k], c[(k + 1) % c.len()], [x, z])).collect();
                s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0)
            })
        };
        let mut tally: HashMap<u32, (u32, u32)> = HashMap::new();
        for i in self.cutters_near(lo, hi) {
            let c = &self.cutters[i];
            let (x, z) = ((c.min[0] + c.max[0]) * 0.5, (c.min[1] + c.max[1]) * 0.5);
            if x < lo[0] || x > hi[0] || z < lo[1] || z > hi[1] {
                continue;
            }
            let t = tally.entry(c.group).or_default();
            t.1 += 1;
            if over(x, z) {
                t.0 += 1;
            }
        }
        // a part with a navmesh of its own (a building section over it: its floors, doors and
        // stairs, baked with it) is never cut round
        let mut boxes: HashMap<u32, ([f32; 2], [f32; 2])> = HashMap::new();
        for i in self.cutters_near(lo, hi) {
            let c = &self.cutters[i];
            let b = boxes.entry(c.group).or_insert((c.min, c.max));
            b.0 = [b.0[0].min(c.min[0]), b.0[1].min(c.min[1])];
            b.1 = [b.1[0].max(c.max[0]), b.1[1].max(c.max[1])];
        }
        let mut own: HashMap<(i32, i32), Vec<[f32; 3]>> = HashMap::new();
        for sec in self.sections.values().filter(|s| s.info.is_some()) {
            for v in &sec.vertices {
                if v[0] >= lo[0] - 50.0 && v[0] <= hi[0] + 50.0 && v[2] >= lo[1] - 50.0 && v[2] <= hi[1] + 50.0 {
                    own.entry(((v[0] / 4.0).floor() as i32, (v[2] / 4.0).floor() as i32)).or_default().push(*v);
                }
            }
        }
        let has_own = |(a, b): ([f32; 2], [f32; 2])| {
            for gx in (a[0] / 4.0).floor() as i32..=(b[0] / 4.0).floor() as i32 {
                for gz in (a[1] / 4.0).floor() as i32..=(b[1] / 4.0).floor() as i32 {
                    if own.get(&(gx, gz)).is_some_and(|vs| vs.iter().any(|v| v[0] >= a[0] && v[0] <= b[0] && v[2] >= a[1] && v[2] <= b[1])) {
                        return true;
                    }
                }
            }
            false
        };
        tally
            .into_iter()
            .filter(|(g, (on, all))| on * 2 >= *all && !boxes.get(g).is_some_and(|b| has_own(*b)))
            .map(|(g, _)| g)
            .collect()
    }

    /// Whether `key`'s cutters were given already.
    pub fn has_cutters(&self, key: (i64, i64)) -> bool {
        self.cut_keys.contains(&key)
    }

    /// The cutters a box (x, z metres) may meet.
    fn cutters_near(&self, lo: [f32; 2], hi: [f32; 2]) -> Vec<usize> {
        let mut out = Vec::new();
        for gx in (lo[0] / CELL).floor() as i32..=(hi[0] / CELL).floor() as i32 {
            for gz in (lo[1] / CELL).floor() as i32..=(hi[1] / CELL).floor() as i32 {
                for &i in self.cutter_grid.get(&(gx, gz)).into_iter().flatten() {
                    let c = &self.cutters[i];
                    if c.min[0] <= hi[0] && c.max[0] >= lo[0] && c.min[1] <= hi[1] && c.max[1] >= lo[1] {
                        out.push(i);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// A section's faces taken out of the grid.
    fn unindex(&mut self, uid: i64) {
        let Some(t) = self.sections.get(&uid) else { return };
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for v in &t.vertices {
            lo = [lo[0].min(v[0]), lo[1].min(v[2])];
            hi = [hi[0].max(v[0]), hi[1].max(v[2])];
        }
        if lo[0] > hi[0] {
            return;
        }
        for gx in grid_cell(lo[0])..=grid_cell(hi[0]) {
            for gz in grid_cell(lo[1])..=grid_cell(hi[1]) {
                if let Some(v) = self.grid.get_mut(&(gx, gz)) {
                    v.retain(|(f, _, _)| f.section != uid);
                }
            }
        }
    }

    /// Ground section `uid` cut by the cutters over it (from its faces as loaded).
    pub(crate) fn cut_section(&mut self, uid: i64) {
        let overlay = uid + CUT_UID;
        // back to the faces as loaded
        if let Some(orig) = self.pristine.get(&uid).cloned() {
            self.unindex(uid);
            self.unindex(overlay);
            self.sections.remove(&overlay);
            for s in self.sections.values_mut() {
                for v in s.links.values_mut() {
                    v.retain(|l| l.0 != overlay);
                }
                s.links.retain(|_, v| !v.is_empty());
            }
            self.index(&orig);
            self.sections.insert(uid, orig);
        } else if let Some(s) = self.sections.get(&uid) {
            if s.info.is_some() || self.cutters.is_empty() {
                return;
            }
            self.pristine.insert(uid, s.clone());
        } else {
            return;
        }
        let mut ground = self.sections[&uid].clone();
        let building = |n: i64| self.sections.get(&n).is_some_and(|s| s.info.is_some());
        let parts = self.bare_parts(&ground);
        // each face less the cutters standing on it
        let mut pieces: Vec<(u32, Vec<[f32; 3]>)> = Vec::new();
        let mut cut: Vec<u32> = Vec::new();
        let mut by_group: HashMap<u32, (u32, [f32; 2])> = HashMap::new();
        for fi in 0..ground.faces.len() as u32 {
            let face = ground.faces[fi as usize];
            if face.kind == FACE_BLOCKED {
                continue;
            }
            let edges = face.start_edge..face.start_edge + face.num_edges;
            if edges.clone().any(|e| ground.links.get(&e).is_some_and(|l| l.iter().any(|x| building(x.0)))) {
                continue;
            }
            let corners: Vec<[f32; 3]> = ground.corners(fi).collect();
            if corners.len() < 3 {
                continue;
            }
            let mut poly: Vec<[f32; 2]> = corners.iter().map(|c| [c[0], c[2]]).collect();
            if area(&poly) < 0.0 {
                poly.reverse();
            }
            let lo = poly.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
            let hi = poly.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
            let y = corners.iter().map(|c| c[1]).sum::<f32>() / corners.len() as f32;
            let near: Vec<usize> = self.cutters_near(lo, hi).into_iter().filter(|&i| parts.contains(&self.cutters[i].group) && self.stands_on(i, y)).collect();
            if near.is_empty() {
                continue;
            }
            let whole = area(&poly);
            let mut parts = vec![poly];
            for &i in &near {
                parts = parts.iter().flat_map(|p| subtract(p, &self.cutters[i].outline)).collect();
            }
            if parts.len() == 1 && (area(&parts[0]) - whole).abs() < 1e-3 {
                continue;
            }
            // heights on the face's plane (Newell's normal)
            let mut n = [0.0f32; 3];
            for i in 0..corners.len() {
                let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
                n[0] += (a[1] - b[1]) * (a[2] + b[2]);
                n[1] += (a[2] - b[2]) * (a[0] + b[0]);
                n[2] += (a[0] - b[0]) * (a[1] + b[1]);
            }
            let p0 = corners[0];
            let height = |x: f32, z: f32| if n[1].abs() > 1e-6 { p0[1] - (n[0] * (x - p0[0]) + n[2] * (z - p0[2])) / n[1] } else { y };
            cut.push(fi);
            for &i in &near {
                let e = by_group.entry(self.cutters[i].group).or_insert((0, [lo[0], lo[1]]));
                e.0 += 1;
            }
            for p in parts {
                pieces.push((fi, p.iter().map(|q| [q[0], height(q[0], q[1]), q[1]]).collect()));
            }
        }
        if cut.is_empty() {
            return;
        }
        // the ground: the cut faces shut, the edges facing them opened to links
        let cut_set: HashSet<u32> = cut.iter().copied().collect();
        let mut overlay_sec = Section { uid: overlay, tile: ground.tile, ..Default::default() };
        let cluster_of: HashMap<u32, usize> = cut.iter().enumerate().map(|(k, &f)| (f, k)).collect();
        let kinds: Vec<i32> = ground.faces.iter().map(|f| f.kind).collect();
        for &f in &cut {
            ground.faces[f as usize].kind = FACE_BLOCKED;
        }
        for e in ground.edges.iter_mut() {
            if e.opposite_face >= 0 && cut_set.contains(&(e.opposite_face as u32)) {
                e.opposite_face = -1;
                e.opposite_edge = -1;
            }
        }
        // the pieces: a section of their own, a cluster per cut face joined to the ground's
        overlay_sec.clusters.edges = vec![Vec::new(); cut.len()];
        overlay_sec.clusters.positions = cut.iter().map(|&f| {
            let c: Vec<[f32; 3]> = ground.corners(f).collect();
            let n = c.len() as f32;
            [c.iter().map(|v| v[0]).sum::<f32>() / n, c.iter().map(|v| v[1]).sum::<f32>() / n, c.iter().map(|v| v[2]).sum::<f32>() / n]
        }).collect();
        for (k, &f) in cut.iter().enumerate() {
            let gc = ground.faces[f as usize].cluster.max(0) as u32;
            overlay_sec.clusters.links.push((k as u32, uid, gc, 0.0));
            ground.clusters.links.push((gc, overlay, k as u32, 0.0));
        }
        for (f, poly) in &pieces {
            let base = overlay_sec.vertices.len() as u32;
            let start = overlay_sec.edges.len() as u32;
            let n = poly.len() as u32;
            overlay_sec.vertices.extend(poly.iter().copied());
            for i in 0..n {
                overlay_sec.edges.push(Edge { a: base + i, b: base + (i + 1) % n, opposite_face: -1, opposite_edge: -1 });
            }
            overlay_sec.faces.push(Face { start_edge: start, num_edges: n, cluster: cluster_of[f] as i32, kind: kinds[*f as usize] });
        }
        // the pieces' links: to each other, and to the faces round them in any section
        let edge_face: Vec<u32> = overlay_sec.faces.iter().enumerate().flat_map(|(k, f)| std::iter::repeat_n(k as u32, f.num_edges as usize)).collect();
        let mut by_cell: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (k, e) in overlay_sec.edges.iter().enumerate() {
            let (a, b) = (overlay_sec.vertices[e.a as usize], overlay_sec.vertices[e.b as usize]);
            for gx in grid_cell(a[0].min(b[0]))..=grid_cell(a[0].max(b[0])) {
                for gz in grid_cell(a[2].min(b[2]))..=grid_cell(a[2].max(b[2])) {
                    by_cell.entry((gx, gz)).or_default().push(k as u32);
                }
            }
        }
        // the grid: the ground without its shut faces (searched for the pieces' neighbours)
        self.unindex(uid);
        let mut shown = ground.clone();
        for f in &cut {
            shown.faces[*f as usize].num_edges = 0;
        }
        self.index(&shown);
        self.sections.insert(uid, ground);
        let mut new_links: Vec<(u32, (i64, u32, u32))> = Vec::new();
        let mut back: Vec<(i64, u32, (i64, u32, u32))> = Vec::new();
        for (k, e) in overlay_sec.edges.iter().enumerate() {
            let k = k as u32;
            let (a, b) = (overlay_sec.vertices[e.a as usize], overlay_sec.vertices[e.b as usize]);
            let mut seen = HashSet::new();
            for gx in grid_cell(a[0].min(b[0]))..=grid_cell(a[0].max(b[0])) {
                for gz in grid_cell(a[2].min(b[2]))..=grid_cell(a[2].max(b[2])) {
                    // the other pieces
                    for &o in by_cell.get(&(gx, gz)).into_iter().flatten() {
                        if edge_face[o as usize] == edge_face[k as usize] || !seen.insert((overlay, o)) {
                            continue;
                        }
                        let oe = overlay_sec.edges[o as usize];
                        if shared(a, b, overlay_sec.vertices[oe.a as usize], overlay_sec.vertices[oe.b as usize]) {
                            new_links.push((k, (overlay, edge_face[o as usize], o)));
                        }
                    }
                    // the faces of the loaded sections (the ground's uncut ones among them)
                    for &(f, lo, hi) in self.grid.get(&(gx, gz)).into_iter().flatten() {
                        if f.section == uid && cut_set.contains(&f.face) {
                            continue;
                        }
                        if lo[0] > a[0].max(b[0]) + 0.05 || hi[0] < a[0].min(b[0]) - 0.05 || lo[2] > a[2].max(b[2]) + 0.05 || hi[2] < a[2].min(b[2]) - 0.05 {
                            continue;
                        }
                        let s = &self.sections[&f.section];
                        let face = s.faces[f.face as usize];
                        for oe in face.start_edge..face.start_edge + face.num_edges {
                            if !seen.insert((f.section, oe)) {
                                continue;
                            }
                            let x = s.edges[oe as usize];
                            if shared(a, b, s.vertices[x.a as usize], s.vertices[x.b as usize]) {
                                new_links.push((k, (f.section, f.face, oe)));
                                back.push((f.section, oe, (overlay, edge_face[k as usize], k)));
                            }
                        }
                    }
                }
            }
        }
        for (k, l) in new_links {
            overlay_sec.links.entry(k).or_default().push(l);
        }
        for (sec, e, l) in back {
            if let Some(s) = self.sections.get_mut(&sec) {
                s.links.entry(e).or_default().push(l);
            }
        }
        if std::env::var("KENSHI_NAV_DEBUG").is_ok() {
            let linked = (0..overlay_sec.edges.len() as u32).filter(|e| overlay_sec.links.contains_key(e)).count();
            let g = &self.sections[&uid];
            let face_of: Vec<u32> = g.faces.iter().enumerate().flat_map(|(k, f)| std::iter::repeat_n(k as u32, f.num_edges as usize)).collect();
            let opened: Vec<u32> = (0..g.edges.len() as u32)
                .filter(|&e| g.edges[e as usize].opposite_face < 0 && self.pristine[&uid].edges[e as usize].opposite_face >= 0 && !cut_set.contains(&face_of[e as usize]))
                .collect();
            for &e in opened.iter().filter(|e| !g.links.contains_key(e)).take(3) {
                let x = g.edges[e as usize];
                eprintln!("  unlinked ground edge {e}: {:?} -> {:?}", g.vertices[x.a as usize], g.vertices[x.b as usize]);
            }
            let back = opened.iter().filter(|e| g.links.contains_key(e)).count();
            let mut top: Vec<_> = by_group.iter().collect();
            top.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
            for (g, (n, at)) in top.iter().take(6) {
                eprintln!("  group {g} cut {n} faces near {at:?}");
            }
            eprintln!("navcut: section {uid}: {} faces cut into {} pieces; piece edges linked {linked} of {}; ground edges opened {}, linked back {back}", cut.len(), overlay_sec.faces.len(), overlay_sec.edges.len(), opened.len());
        }
        self.index(&overlay_sec);
        self.sections.insert(overlay, overlay_sec);
    }

    /// The faces of a section, for drawing: (section uid, face corners), the cut-away ground
    /// faces left out.
    pub fn open_faces(&self) -> impl Iterator<Item = (FaceRef, Vec<[f32; 3]>)> + '_ {
        self.sections.values().flat_map(|s| {
            (0..s.faces.len() as u32).filter(|&f| s.faces[f as usize].kind != FACE_BLOCKED).map(move |f| (FaceRef { section: s.uid, face: f }, s.corners(f).collect()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_less_middle() {
        let p = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let q = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let parts = subtract(&p, &q);
        let total: f32 = parts.iter().map(|x| area(x)).sum();
        assert!((total - 12.0).abs() < 1e-3, "{total}");
        assert!(parts.iter().all(|x| area(x) > 0.0));
    }

    #[test]
    fn apart_is_whole() {
        let p = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        let q = vec![[5.0, 5.0], [6.0, 5.0], [6.0, 6.0]];
        let parts = subtract(&p, &q);
        assert_eq!(parts.len(), 1);
        assert!((area(&parts[0]) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn hull_of_wall_is_thin_box() {
        let c = Cutter::from_points(&[[0.0, 0.0, 0.0], [3.0, 2.0, 0.0], [3.0, 0.0, 0.0]], 0.1).unwrap();
        assert!((area(&c.outline) - 3.2 * 0.2).abs() < 1e-3);
    }
}

impl NavWorld {
    /// (debugging) The cutters over a point (metres): area, heights.
    pub fn cutters_at(&self, x: f32, z: f32) -> Vec<(f32, f32, f32, usize)> {
        self.cutters_near([x, z], [x, z])
            .into_iter()
            .filter(|&i| {
                let o = &self.cutters[i].outline;
                (0..o.len()).all(|k| cross(o[k], o[(k + 1) % o.len()], [x, z]) >= 0.0)
            })
            .map(|i| (area(&self.cutters[i].outline), self.cutters[i].lo, self.cutters[i].hi, self.cutters[i].outline.len()))
            .collect()
    }
}

impl NavWorld {
    /// (testing) Paths asked straight across the larger cutters within `reach` metres of
    /// (`x`, `z`) whose foot is on the ground (`ground`: its height in metres): how many were
    /// asked, found, and how many go through a cutter they were asked across.
    pub fn check_cutters(&self, x: f32, z: f32, reach: f32, ground: &dyn Fn(f32, f32) -> f32) -> (usize, usize, usize) {
        let (mut asked, mut found, mut through) = (0, 0, 0);
        // the parts that cut (on bare ground), by their ground section
        let mut bare: HashMap<i64, HashSet<u32>> = HashMap::new();
        for i in self.cutters_near([x - reach, z - reach], [x + reach, z + reach]) {
            let c = &self.cutters[i];
            let a = area(&c.outline);
            if !(2.0..=400.0).contains(&a) {
                continue;
            }
            let (tx, ty) = tile_of([c.min[0], 0.0, c.min[1]]);
            let uid = tile_uid(tx, ty);
            let Some(sec) = self.pristine.get(&uid).or_else(|| self.sections.get(&uid)) else { continue };
            let parts = bare.entry(uid).or_insert_with(|| self.bare_parts(sec));
            if !parts.contains(&c.group) {
                continue;
            }
            let mid = [(c.min[0] + c.max[0]) * 0.5, (c.min[1] + c.max[1]) * 0.5];
            let g = ground(mid[0], mid[1]);
            if c.lo > g + 0.3 || c.hi < g + 1.8 {
                continue;
            }
            let r = (c.max[0] - c.min[0]).max(c.max[1] - c.min[1]) * 0.5 + 4.0;
            let (sx, sz, gx, gz) = (mid[0] - r, mid[1], mid[0] + r, mid[1]);
            asked += 1;
            let res = self.find_path([sx, ground(sx, sz) + 0.5, sz], [gx, ground(gx, gz) + 0.5, gz], 0.4, 3.0);
            let p = match res {
                crate::navmesh::PathResult::Found(p) => p,
                other => {
                    if std::env::var("KENSHI_NAV_DEBUG").is_ok() {
                        let kind = match other {
                            crate::navmesh::PathResult::NoStart => "no start",
                            crate::navmesh::PathResult::NoGoal => "no goal",
                            _ => "unreachable",
                        };
                        let s = self.closest_point([sx, ground(sx, sz) + 0.5, sz], 50.0);
                        let t = self.closest_point([gx, ground(gx, gz) + 0.5, gz], 50.0);
                        let (reach, search) = match (s, t) {
                            (Some((f, p)), Some((g, q))) => (self.reachable(f, g), self.search_found(f, p, g, q)),
                            _ => (false, false),
                        };
                        let fl = s.map(|x| self.flood(x.0, 5000));
                        // the ground clusters under the start and goal pieces
                        let orig = |f: FaceRef| -> (i64, u32) {
                            let sec = &self.sections[&f.section];
                            let c = sec.faces[f.face as usize].cluster as u32;
                            sec.clusters.links.iter().find(|l| f.section >= CUT_UID && l.0 == c).map_or((f.section, c), |l| (l.1, l.2))
                        };
                        if let (Some((f, _)), Some((g, _))) = (s, t) {
                            eprintln!("    ground clusters {:?} -> {:?}", orig(f), orig(g));
                        }
                        eprintln!("  check path across cutter at {mid:?}: {kind} (faces {:?} -> {:?}, reachable {reach}, search {search}, flood {fl:?})", s.map(|x| x.0), t.map(|x| x.0));
                    }
                    continue;
                }
            };
            found += 1;
            let inside = |q: [f32; 2]| (0..c.outline.len()).all(|k| cross(c.outline[k], c.outline[(k + 1) % c.outline.len()], q) > 0.05);
            let hits = p.points.windows(2).any(|w| {
                let n = ((w[1][0] - w[0][0]).hypot(w[1][2] - w[0][2]) / 0.25).ceil().max(1.0) as usize;
                (0..=n).any(|k| {
                    let t = k as f32 / n as f32;
                    let q = [w[0][0] + (w[1][0] - w[0][0]) * t, w[0][2] + (w[1][2] - w[0][2]) * t];
                    let y = w[0][1] + (w[1][1] - w[0][1]) * t;
                    y < c.lo + 1.5 && inside(q)
                })
            });
            if hits {
                through += 1;
            }
        }
        (asked, found, through)
    }
}

impl NavWorld {
    /// (testing) Faces reached from `f` across edges (any, up to `max`), and how many of `f`'s
    /// own edges lead anywhere.
    pub fn flood(&self, f: FaceRef, max: usize) -> (usize, usize) {
        let mut seen = HashSet::from([f]);
        let mut stack = vec![f];
        while let Some(g) = stack.pop() {
            if seen.len() >= max {
                break;
            }
            let face = self.sections[&g.section].faces[g.face as usize];
            for e in face.start_edge..face.start_edge + face.num_edges {
                for (n, _) in self.across(g, e) {
                    if self.sections[&n.section].faces[n.face as usize].kind != FACE_BLOCKED && seen.insert(n) {
                        stack.push(n);
                    }
                }
            }
        }
        let face = self.sections[&f.section].faces[f.face as usize];
        let out = (face.start_edge..face.start_edge + face.num_edges).filter(|&e| !self.across(f, e).is_empty()).count();
        (seen.len(), out)
    }
}

impl NavWorld {
    /// (testing) Paths from `from` (metres) to the lowest and highest face of each building
    /// section within `reach`: (asked, found) for the low floors and for the high ones.
    pub fn check_floors(&self, from: [f32; 3], reach: f32) -> ((usize, usize), (usize, usize)) {
        let (mut low, mut high) = ((0, 0), (0, 0));
        for s in self.sections.values().filter(|s| s.info.is_some()) {
            let Some(v0) = s.vertices.first() else { continue };
            if (v0[0] - from[0]).hypot(v0[2] - from[2]) > reach || s.faces.is_empty() {
                continue;
            }
            let mid = |f: u32| {
                let c: Vec<[f32; 3]> = s.corners(f).collect();
                let n = c.len().max(1) as f32;
                [c.iter().map(|v| v[0]).sum::<f32>() / n, c.iter().map(|v| v[1]).sum::<f32>() / n, c.iter().map(|v| v[2]).sum::<f32>() / n]
            };
            let faces: Vec<u32> = (0..s.faces.len() as u32).filter(|&f| s.faces[f as usize].kind != FACE_BLOCKED && s.faces[f as usize].num_edges > 0).collect();
            let Some(&lo) = faces.iter().min_by(|a, b| mid(**a)[1].total_cmp(&mid(**b)[1])) else { continue };
            let Some(&hi) = faces.iter().max_by(|a, b| mid(**a)[1].total_cmp(&mid(**b)[1])) else { continue };
            for (k, f) in [lo, hi].into_iter().enumerate() {
                if k == 1 && mid(hi)[1] - mid(lo)[1] < 2.0 {
                    continue;
                }
                let tally = if k == 0 { &mut low } else { &mut high };
                tally.0 += 1;
                if matches!(self.find_path(from, mid(f), 0.4, 3.0), crate::navmesh::PathResult::Found(_)) {
                    tally.1 += 1;
                } else if std::env::var("KENSHI_NAV_DEBUG").is_ok() {
                    eprintln!("  no path to {} floor at {:?} of {:?}", if k == 0 { "low" } else { "high" }, mid(f), s.info);
                }
            }
        }
        (low, high)
    }
}

impl NavWorld {
    /// (testing) The middle of the highest building face within `reach` metres of `from`.
    pub fn highest_floor(&self, from: [f32; 3], reach: f32) -> Option<[f32; 3]> {
        let mut best: Option<[f32; 3]> = None;
        for s in self.sections.values().filter(|s| s.info.is_some()) {
            for f in 0..s.faces.len() as u32 {
                if s.faces[f as usize].kind == FACE_BLOCKED {
                    continue;
                }
                let c: Vec<[f32; 3]> = s.corners(f).collect();
                if c.is_empty() {
                    continue;
                }
                let n = c.len() as f32;
                let m = [c.iter().map(|v| v[0]).sum::<f32>() / n, c.iter().map(|v| v[1]).sum::<f32>() / n, c.iter().map(|v| v[2]).sum::<f32>() / n];
                if (m[0] - from[0]).hypot(m[2] - from[2]) < reach && best.is_none_or(|b| m[1] > b[1]) {
                    best = Some(m);
                }
            }
        }
        best
    }
}
