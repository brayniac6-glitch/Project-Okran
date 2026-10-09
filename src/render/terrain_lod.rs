//! Kenshi's terrain (Plugin_Terrain_x64.dll): one quadtree over the whole map, each node a patch
//! of 17 x 17 vertices, split where its error on screen is over the detail threshold and
//! geomorphed between levels; as the game sets it up (F046700): terrain scale 294912 x 9800,
//! setDetailThreshold(8, 8, 0), patch size 16 (levels 4 to 10: the finest patch has the
//! heightmap's own spacing).
//!
//! Spec (the plugin): the node update (0x180003660), the error (0x180003b30: the viewport height
//! in pixels x the node's error / the distance from the camera, the camera's height taken 40% of
//! the way to the node's centre), the patch build (0x1800047a0: heights of the heightmap's samples,
//! Terrain::getHeight 0x180014430; each vertex's normal from its six neighbours; the morph targets
//! of the odd vertices the mean of their neighbours across; the node's error the larger of a tenth
//! of the vertex spacing and the largest morph), the split (0x180003e00) and the merge
//! (0x1800045b0); children unused for 500 updates are dropped.
//!
//! Materials (the plugin's updateMaterial 0x180013070, each update): a patch whose bounds are
//! within the material distance of the camera (setMaterialDistance stores it squared,
//! 0x180013c80; F046700 sets 30000) takes the game's material for it (the callback), the rest the
//! default material, "DistantTerrain" (F046700: setDefaultMaterialName).
//!
//! The geomorph runs on the GPU as in the plugin: a patch's meshes hold the morph targets and
//! the change to its own heights and normals, and the node's morph is the mesh tag (x 65535)
//! kenshi_terrain_vertex.wgsl and kenshi_terrain_prepass.wgsl apply; a morph change touches no
//! mesh.
//!
//! (stand-in) The plugin builds patches on worker threads and stitches neighbours of different
//! levels with matching index buffers; here patches are built on the frame (a budget per frame)
//! and the cracks are hidden with skirts. Its merge check against neighbours two levels finer
//! is not ported. Patch materials are the blend cells' (one mesh per cell a patch covers), not
//! a material per patch box (F051639).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use kenshi_world::{Terrain, WORLD_SIZE};

/// Vertices per patch side (setPatchSize(16) + 1).
const VERTS: usize = 17;
/// setPatchSize: levels 4 .. 15 - log2(32) = 10.
const MIN_LEVEL: u8 = 4;
const MAX_LEVEL: u8 = 10;
/// setDetailThreshold(8, 8, 0): the threshold; 8 / 8 = 1 (no distance falloff).
const THRESHOLD: f32 = 8.0;
/// F046700: setMaterialDistance(30000).
const MATERIAL_DISTANCE: f32 = 30000.0;
/// Children unused this many updates are dropped.
const KEEP_FRAMES: u32 = 500;
/// Heightmap samples per blend cell and per colour tile.
const CELL_PX: u32 = 512;
const TILE_PX: u32 = kenshi_world::COLOUR_TILE_PX;

/// (colour tile, blend cell): what a terrain piece's material is chosen by.
type Piece = ((u32, u32), (u32, u32));

/// A built patch: per vertex (row-major, x fastest) the height, normal, morph height and normal.
struct PatchData {
    h: Vec<f32>,
    n: Vec<Vec3>,
    mh: Vec<f32>,
    mn: Vec<Vec3>,
}

struct Node {
    level: u8,
    /// World x, z of its first corner, and its side.
    x0: f32,
    z0: f32,
    size: f32,
    parent: Option<usize>,
    children: Option<[usize; 4]>,
    /// Drawing its children (else itself).
    split: bool,
    patch: Option<PatchData>,
    /// +0x84: the node's error (world units).
    error: f32,
    /// Its bounds (world).
    lo: Vec3,
    hi: Vec3,
    /// +0x7c: how far toward its own heights (1) from its morph targets (0).
    morph: f32,
    /// The update it was last visited in (+0xc0).
    visited: u32,
    /// Its meshes in the scene (entity, mesh, piece) and the morph they were built with.
    drawn: Vec<(Entity, Handle<Mesh>, Piece)>,
    drawn_morph: f32,
    /// Drawn with the distant material.
    far: bool,
}

/// The terrain quadtree and what it has in the scene.
#[derive(Resource)]
pub struct LodTerrain {
    nodes: Vec<Option<Node>>,
    free: Vec<usize>,
    root: usize,
    frame: u32,
    terrain: Terrain,
    /// Patches built this frame, and the most per frame.
    built: usize,
    pub budget: usize,
    /// Materials by (tile, cell).
    materials: std::collections::HashMap<Piece, Handle<crate::terrain_material::KenshiTerrainMaterial>>,
    /// Entities to despawn.
    dead: Vec<Entity>,
    /// "DistantTerrain".
    distant: Option<Handle<crate::terrain_material::KenshiTerrainMaterial>>,
}

impl LodTerrain {
    pub fn new(terrain: Terrain) -> LodTerrain {
        let mut t = LodTerrain {
            nodes: Vec::new(),
            free: Vec::new(),
            root: 0,
            frame: 0,
            terrain,
            built: 0,
            budget: 96,
            materials: Default::default(),
            dead: Vec::new(),
            distant: None,
        };
        t.root = t.add(0, -WORLD_SIZE / 2.0, -WORLD_SIZE / 2.0, WORLD_SIZE, None);
        t
    }

    fn add(&mut self, level: u8, x0: f32, z0: f32, size: f32, parent: Option<usize>) -> usize {
        let n = Node {
            level,
            x0,
            z0,
            size,
            parent,
            children: None,
            split: false,
            patch: None,
            error: size / 16.0 * 0.1,
            lo: Vec3::new(x0, 0.0, z0),
            hi: Vec3::new(x0 + size, 0.0, z0 + size),
            morph: 1.0,
            visited: 0,
            drawn: Vec::new(),
            drawn_morph: f32::NAN,
            far: false,
        };
        match self.free.pop() {
            Some(i) => {
                self.nodes[i] = Some(n);
                i
            }
            None => {
                self.nodes.push(Some(n));
                self.nodes.len() - 1
            }
        }
    }

    fn node(&self, i: usize) -> &Node {
        self.nodes[i].as_ref().expect("node")
    }

    fn node_mut(&mut self, i: usize) -> &mut Node {
        self.nodes[i].as_mut().expect("node")
    }

    /// A node and its subtree let go.
    fn remove(&mut self, i: usize) {
        if let Some(n) = self.nodes[i].take() {
            self.dead.extend(n.drawn.iter().map(|(e, _, _)| *e));
            if let Some(c) = n.children {
                for k in c {
                    self.remove(k);
                }
            }
            self.free.push(i);
        }
    }

    /// @fn 0x180003b30 (Plugin_Terrain)
    ///
    /// A node's error on screen from `cam`, `k` the viewport's height in pixels.
    fn error(&self, i: usize, cam: Vec3, k: f32) -> f32 {
        let n = self.node(i);
        let centre_y = (n.lo.y + n.hi.y) * 0.5;
        let c = Vec3::new(cam.x, cam.y + (centre_y - cam.y) * 0.4, cam.z);
        let d = (c - c.clamp(n.lo, n.hi)).length().max(1.0);
        k / d * n.error
    }

    /// @fn 0x180003660 (Plugin_Terrain)
    fn update_node(&mut self, i: usize, cam: Vec3, k: f32) {
        let e = self.error(i, cam, k);
        let frame = self.frame;
        self.node_mut(i).visited = frame;
        let n = self.node(i);
        if !n.split {
            let ep_raw = match n.parent {
                Some(p) => self.error(p, cam, k),
                None => e,
            };
            let ep = if ep_raw <= e { e * 1.001 } else { ep_raw };
            let mut morph = if e == ep { 1.0 } else { (THRESHOLD - ep) / (e - ep) };
            let level = n.level;
            if level < MIN_LEVEL {
                morph = 1.0;
            }
            if (level >= MIN_LEVEL && e <= THRESHOLD) || level >= MAX_LEVEL {
                // children unused for a while are let go
                if let Some(c) = n.children
                    && c.iter().all(|&j| self.node(j).visited + KEEP_FRAMES < frame)
                {
                    for j in c {
                        self.remove(j);
                    }
                    self.node_mut(i).children = None;
                }
            } else if self.built < self.budget {
                self.split(i);
            }
            self.node_mut(i).morph = morph;
        } else {
            let children = n.children.expect("split node has children");
            for c in children {
                self.update_node(c, cam, k);
            }
            let n = self.node_mut(i);
            n.morph = 1.0;
            if e < THRESHOLD && n.level > MIN_LEVEL {
                self.merge(i);
            }
        }
    }

    /// @fn 0x180003e00 (Plugin_Terrain)
    ///
    /// The node draws its four children (made and built if new).
    fn split(&mut self, i: usize) {
        let n = self.node(i);
        let (level, x0, z0, half) = (n.level + 1, n.x0, n.z0, n.size * 0.5);
        let children = match n.children {
            Some(c) => c,
            None => {
                let mut c = [0; 4];
                for (q, slot) in c.iter_mut().enumerate() {
                    let x = x0 + if q & 1 != 0 { half } else { 0.0 };
                    let z = z0 + if q & 2 != 0 { half } else { 0.0 };
                    *slot = self.add(level, x, z, half, Some(i));
                }
                self.node_mut(i).children = Some(c);
                c
            }
        };
        for c in children {
            if self.node(c).patch.is_none() {
                self.build(c);
            }
        }
        let frame = self.frame;
        for c in children {
            self.node_mut(c).visited = frame;
        }
        self.node_mut(i).split = true;
    }

    /// @fn 0x1800045b0 (Plugin_Terrain)
    fn merge(&mut self, i: usize) {
        if let Some(c) = self.node(i).children {
            for k in c {
                if self.node(k).split {
                    self.merge(k);
                }
            }
        }
        if self.node(i).patch.is_none() {
            self.build(i);
        }
        let n = self.node_mut(i);
        n.split = false;
        n.morph = 1.0;
    }

    /// @fn 0x1800047a0 (Plugin_Terrain)
    ///
    /// A node's patch: heights at the heightmap's samples, normals, morph targets, bounds and
    /// error.
    fn build(&mut self, i: usize) {
        self.built += 1;
        let n = self.node(i);
        let samples = self.terrain.samples();
        let step = ((samples - 1) as f32 / WORLD_SIZE * n.size / 16.0).round().max(1.0) as i64;
        let sx = ((n.x0 + WORLD_SIZE / 2.0) / WORLD_SIZE * (samples - 1) as f32).round() as i64;
        let sz = ((n.z0 + WORLD_SIZE / 2.0) / WORLD_SIZE * (samples - 1) as f32).round() as i64;
        let spacing = n.size / 16.0;
        // the patch with a ring round it (the normals' neighbours), clamped to the map
        let side = VERTS as i64 + 2;
        let first = |s: i64| (s - step).clamp(0, samples as i64 - 1 - (side - 1) * step).max(0);
        let (fx, fz) = (first(sx), first(sz));
        let grid = self.terrain.patch(fx as u32, fz as u32, side as u32, side as u32, step as u32).ok();
        let height = |gx: i64, gz: i64| -> f32 {
            // sample (gx, gz) of the patch's own grid (-1 ..= 17)
            let Some(g) = &grid else { return 0.0 };
            let ax = (sx + gx * step - fx) / step;
            let az = (sz + gz * step - fz) / step;
            g.at(ax.clamp(0, side - 1) as u32, az.clamp(0, side - 1) as u32)
        };
        let v = VERTS;
        let mut h = vec![0.0f32; v * v];
        for z in 0..v {
            for x in 0..v {
                h[z * v + x] = height(x as i64, z as i64);
            }
        }
        // normals: the six neighbours (0,-1) (1,-1) (1,0) (0,1) (-1,1) (-1,0)
        const ROUND: [(i64, i64); 6] = [(0, -1), (1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0)];
        let mut nrm = vec![Vec3::Y; v * v];
        for z in 0..v as i64 {
            for x in 0..v as i64 {
                let h0 = h[z as usize * v + x as usize];
                let r: Vec<Vec3> = ROUND
                    .iter()
                    .map(|&(dx, dz)| Vec3::new(dx as f32 * spacing, height(x + dx, z + dz) - h0, dz as f32 * spacing))
                    .collect();
                let mut s = Vec3::ZERO;
                for k in 0..6 {
                    s += r[k].cross(r[(k + 1) % 6]);
                }
                nrm[z as usize * v + x as usize] = -s.normalize_or(Vec3::NEG_Y);
            }
        }
        // morph targets: odd vertices the mean of their neighbours across
        let mut mh = h.clone();
        let mut mn = nrm.clone();
        let mut error = spacing * 0.1;
        for z in 0..v {
            for x in 0..v {
                let (a, b) = match (x & 1, z & 1) {
                    (1, 1) => ((x - 1, z - 1), (x + 1, z + 1)),
                    (1, 0) => ((x - 1, z), (x + 1, z)),
                    (0, 1) => ((x, z - 1), (x, z + 1)),
                    _ => continue,
                };
                let (ia, ib) = (a.1 * v + a.0, b.1 * v + b.0);
                let k = z * v + x;
                mn[k] = (nrm[ia] + nrm[ib]).normalize_or(Vec3::Y);
                mh[k] = (h[ia] + h[ib]) * 0.5;
                error = error.max((mh[k] - h[k]).abs());
            }
        }
        let (lo, hi) = h.iter().fold((f32::MAX, f32::MIN), |(l, u), &y| (l.min(y), u.max(y)));
        let n = self.node_mut(i);
        n.lo.y = lo;
        n.hi.y = hi;
        n.error = error;
        n.patch = Some(PatchData { h, n: nrm, mh, mn });
        n.drawn_morph = f32::NAN;
    }

    /// The frame: the tree updated from the camera (`cam` a world position, `k` the viewport's
    /// height in pixels), then the drawn patches' meshes made or remade.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        cam: Vec3,
        k: f32,
        origin: Vec3,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<crate::terrain_material::KenshiTerrainMaterial>,
        look: &mut crate::terrain_material::TerrainLook,
        assets: &AssetServer,
    ) {
        self.frame += 1;
        self.built = 0;
        let root = self.root;
        let mut prof = crate::prof::Marks::start();
        self.update_node(root, cam, k);
        prof.mark("terrain: node update");
        // the leaves drawn, the rest not
        let mut leaves = Vec::new();
        let mut stack = vec![root];
        let mut hidden = Vec::new();
        while let Some(i) = stack.pop() {
            let n = self.node(i);
            if n.split {
                stack.extend(n.children.expect("children"));
                if !n.drawn.is_empty() {
                    hidden.push(i);
                }
            } else {
                leaves.push(i);
            }
            if let Some(c) = n.children
                && !n.split
            {
                // children kept but not drawn
                let mut s = c.to_vec();
                while let Some(j) = s.pop() {
                    if !self.node(j).drawn.is_empty() {
                        hidden.push(j);
                    }
                    if let Some(cc) = self.node(j).children {
                        s.extend(cc);
                    }
                }
            }
        }
        for i in hidden {
            let n = self.nodes[i].as_mut().expect("node");
            self.dead.extend(n.drawn.drain(..).map(|(e, _, _)| e));
            n.drawn_morph = f32::NAN;
        }
        if self.distant.is_none() {
            self.distant = Some(materials.add(look.distant_material(assets, origin)));
        }
        for i in leaves {
            // updateMaterial: the squared distance from the camera to the patch's bounds against
            // the material distance squared
            let n = self.node(i);
            let far = (cam - cam.clamp(n.lo, n.hi)).length_squared() > MATERIAL_DISTANCE * MATERIAL_DISTANCE;
            if !n.drawn.is_empty() && n.far != far {
                for k in 0..self.node(i).drawn.len() {
                    let (e, _, (tile, cell)) = self.node(i).drawn[k].clone();
                    let mat = if far {
                        self.distant.clone().expect("distant")
                    } else {
                        self.material(tile, cell, origin, materials, look, assets)
                    };
                    commands.entity(e).insert(MeshMaterial3d(mat));
                }
                self.node_mut(i).far = far;
            }
            if self.node(i).patch.is_none() {
                if self.built >= self.budget {
                    continue;
                }
                prof.mark("terrain: leaves");
                self.build(i);
                prof.mark("terrain: patch builds");
            }
            let morph = self.node(i).morph.clamp(0.0, 1.0);
            let n = self.node(i);
            if !n.drawn.is_empty() && (n.drawn_morph - morph).abs() < 0.01 {
                continue;
            }
            prof.mark("terrain: leaves");
            let tag = (morph * 65535.0).round() as u32;
            let fresh = self.node(i).drawn.is_empty();
            if fresh {
                let pieces = self.meshes(i, origin);
                crate::prof::add("terrain: meshes built (count x 1s)", 1.0);
                let mut drawn = Vec::new();
                for ((tile, cell), mesh) in pieces {
                    let mat = if far {
                        self.distant.clone().expect("distant")
                    } else {
                        self.material(tile, cell, origin, materials, look, assets)
                    };
                    let h = meshes.add(mesh);
                    // render queue 25: in the water reflection
                    let mut e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(mat), bevy::mesh::MeshTag(tag), crate::reflection::reflected()));
                    if self.node(i).size > 2000.0 {
                        e.insert(bevy::light::NotShadowCaster);
                    }
                    drawn.push((e.id(), h, (tile, cell)));
                }
                self.node_mut(i).drawn = drawn;
                self.node_mut(i).far = far;
            } else {
                for (e, _, _) in &self.node(i).drawn {
                    commands.entity(*e).insert(bevy::mesh::MeshTag(tag));
                }
            }
            self.node_mut(i).drawn_morph = morph;
            prof.mark("terrain: mesh rebuilds");
        }
        for e in self.dead.drain(..) {
            commands.entity(e).despawn();
        }
    }

    fn material(
        &mut self,
        tile: (u32, u32),
        cell: (u32, u32),
        origin: Vec3,
        materials: &mut Assets<crate::terrain_material::KenshiTerrainMaterial>,
        look: &mut crate::terrain_material::TerrainLook,
        assets: &AssetServer,
    ) -> Handle<crate::terrain_material::KenshiTerrainMaterial> {
        self.materials
            .entry((tile, cell))
            .or_insert_with(|| materials.add(look.material(assets, tile, cell, origin)))
            .clone()
    }

    /// A drawn node's meshes, one per colour tile and blend cell it covers: positions relative
    /// to `origin` at the morph targets' heights and normals, the change to its own in UV_1.x
    /// and the colour (the geomorph the vertex shaders apply), UVs into the colour tile, a skirt
    /// round the rim.
    fn meshes(&self, i: usize, origin: Vec3) -> Vec<(Piece, Mesh)> {
        let n = self.node(i);
        let p = n.patch.as_ref().expect("patch");
        let v = VERTS;
        let spacing = n.size / 16.0;
        let samples = self.terrain.samples() as f32 - 1.0;
        let skirt = spacing * 4.0;
        let sample_of = |w: f32| (w + WORLD_SIZE / 2.0) / WORLD_SIZE * samples;
        let pos = |x: usize, z: usize| -> (Vec3, Vec3, f32, Vec3) {
            let k = z * v + x;
            (Vec3::new(n.x0 + x as f32 * spacing, p.mh[k], n.z0 + z as f32 * spacing), p.mn[k], p.h[k] - p.mh[k], p.n[k] - p.mn[k])
        };
        let mut by: std::collections::BTreeMap<Piece, Vec<(usize, usize)>> = Default::default();
        for z in 0..v - 1 {
            for x in 0..v - 1 {
                let c = sample_of(n.x0 + x as f32 * spacing).max(0.0) as u32;
                let r = sample_of(n.z0 + z as f32 * spacing).max(0.0) as u32;
                let tile = ((c / TILE_PX).min(7), (r / TILE_PX).min(7));
                let cell = (c / CELL_PX, r / CELL_PX);
                by.entry((tile, cell)).or_default().push((x, z));
            }
        }
        let mut out = Vec::new();
        for ((tile, cell), quads) in by {
            let (mut tp, mut tn, mut tuv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let (mut tdy, mut tdn): (Vec<[f32; 2]>, Vec<[f32; 4]>) = (Vec::new(), Vec::new());
            let mut remap: std::collections::HashMap<usize, u32> = Default::default();
            let uv = |w: Vec3| {
                [
                    (sample_of(w.x) - (tile.0 * TILE_PX) as f32) / TILE_PX as f32,
                    (sample_of(w.z) - (tile.1 * TILE_PX) as f32) / TILE_PX as f32,
                ]
            };
            let mut vert = |x: usize, z: usize, tp: &mut Vec<[f32; 3]>, tn: &mut Vec<[f32; 3]>, tuv: &mut Vec<[f32; 2]>, tdy: &mut Vec<[f32; 2]>, tdn: &mut Vec<[f32; 4]>| {
                *remap.entry(z * v + x).or_insert_with(|| {
                    let (w, nn, dy, dn) = pos(x, z);
                    tp.push((w - origin).to_array());
                    tn.push(nn.to_array());
                    tuv.push(uv(w));
                    tdy.push([dy, 0.0]);
                    tdn.push([dn.x, dn.y, dn.z, 0.0]);
                    (tp.len() - 1) as u32
                })
            };
            for &(x, z) in &quads {
                let a = vert(x, z, &mut tp, &mut tn, &mut tuv, &mut tdy, &mut tdn);
                let b = vert(x + 1, z, &mut tp, &mut tn, &mut tuv, &mut tdy, &mut tdn);
                let c = vert(x, z + 1, &mut tp, &mut tn, &mut tuv, &mut tdy, &mut tdn);
                let d = vert(x + 1, z + 1, &mut tp, &mut tn, &mut tuv, &mut tdy, &mut tdn);
                idx.extend([a, c, b, b, c, d]);
                // the skirt's own vertices, their UVs flagged (-1 - uv: kenshi_terrain.wgsl leaves
                // the skirts out of the water reflection, Kenshi has none)
                let mut hang = |u: u32, w: u32| {
                    let base = tp.len() as u32;
                    for drop in [0.0, skirt] {
                        for k in [u, w] {
                            let q = tp[k as usize];
                            let t = tuv[k as usize];
                            tp.push([q[0], q[1] - drop, q[2]]);
                            tn.push(tn[k as usize]);
                            tuv.push([-1.0 - t[0], -1.0 - t[1]]);
                            tdy.push(tdy[k as usize]);
                            tdn.push(tdn[k as usize]);
                        }
                    }
                    let (t0, t1, b0, b1) = (base, base + 1, base + 2, base + 3);
                    idx.extend([t0, t1, b0, b0, t1, b1, t0, b0, t1, t1, b0, b1]);
                };
                if z == 0 {
                    hang(a, b);
                }
                if z == v - 2 {
                    hang(c, d);
                }
                if x == 0 {
                    hang(a, c);
                }
                if x == v - 2 {
                    hang(b, d);
                }
            }
            let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, tp);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, tn);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, tuv);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_1, tdy);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, tdn);
            m.insert_indices(Indices::U32(idx));
            out.push(((tile, cell), m));
        }
        out
    }

    /// Nodes held and drawn (for the log).
    pub fn stats(&self) -> (usize, usize) {
        let held = self.nodes.iter().filter(|n| n.is_some()).count();
        let drawn = self.nodes.iter().flatten().filter(|n| !n.drawn.is_empty()).count();
        (held, drawn)
    }
}
