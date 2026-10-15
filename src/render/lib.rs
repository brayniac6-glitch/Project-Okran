//! Rendering of Kenshi's assets with Bevy (port_map `rendering`; replaces Ogre3D).

pub mod body_shape;
pub mod character;
pub mod character_material;
pub mod fog_volume;
pub mod hdr;
pub mod interior_mask;
pub mod local_lights;
pub mod mipmaps;
pub mod texture_quality;
pub mod moons;
pub mod object_material;
pub mod particles;
pub mod prof;
pub mod ragdoll;
pub mod reflection;
pub mod shadows;
pub mod sky;
pub mod sky_dome;
pub mod terrain_lod;
pub mod terrain_material;
pub mod water_material;
pub mod zone;

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::dds;
use kenshi_io::ogre_mesh::{self, OP_TRIANGLE_LIST, semantic, vtype};
use kenshi_world::Patch;

/// One Bevy mesh per Ogre submesh (each submesh has its own material), with the submesh's
/// material name. Submeshes that are not triangle lists or lack positions are skipped.
pub fn to_bevy(mesh: &ogre_mesh::Mesh) -> Vec<(String, Mesh)> {
    to_bevy_skinned(mesh, None)
}

/// Like `to_bevy`, adding joint indices/weights from the bone assignments when `joint_of` maps
/// an Ogre bone handle to a joint index (its position in the `SkinnedMesh::joints` list).
pub fn to_bevy_skinned(
    mesh: &ogre_mesh::Mesh,
    joint_of: Option<&dyn Fn(u16) -> Option<u16>>,
) -> Vec<(String, Mesh)> {
    let mut out = Vec::new();
    for s in &mesh.submeshes {
        let geo = if s.use_shared {
            mesh.shared.as_ref()
        } else {
            s.geometry.as_ref()
        };
        let (Some(geo), true) = (geo, s.operation == OP_TRIANGLE_LIST) else {
            continue;
        };
        let Some(positions) = geo.positions() else {
            continue;
        };
        let n = positions.len();
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        if let Some(normals) = geo.normals() {
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        }
        // Ogre stores tangents as FLOAT4 (xyz + handedness) or FLOAT3; Bevy wants 4 components
        if let Some(t) = geo.floats::<4>(semantic::TANGENT, 0) {
            let has_w = geo
                .element(semantic::TANGENT, 0)
                .is_some_and(|e| e.kind == vtype::FLOAT4);
            let t: Vec<[f32; 4]> = t
                .into_iter()
                .map(|[x, y, z, w]| [x, y, z, if has_w { w } else { 1.0 }])
                .collect();
            m.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t);
        }
        m.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            geo.uvs(0).unwrap_or_else(|| vec![[0.0; 2]; n]),
        );
        if let Some(c) = geo.colours() {
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, c);
        }
        m.insert_indices(Indices::U32(s.indices.clone()));
        if let Some(joint_of) = joint_of {
            let assigned = if s.use_shared {
                &mesh.bone_assignments
            } else {
                &s.bone_assignments
            };
            if !assigned.is_empty() {
                let (idx, w) = skin_weights(assigned, n, joint_of);
                m.insert_attribute(
                    Mesh::ATTRIBUTE_JOINT_INDEX,
                    VertexAttributeValues::Uint16x4(idx),
                );
                m.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, w);
            }
        }
        if geo.normals().is_none() {
            m.compute_smooth_normals();
        }
        out.push((s.material.clone(), m));
    }
    out
}

/// The first weapon model of the manufacturers (by name) that make weapon `rec`.
fn maker_model<'a>(data: &'a GameDataContainer, rec: &GameData) -> Option<&'a GameData> {
    let mut makers: Vec<&GameData> = data
        .of_type(ItemType::WeaponManufacturer)
        .filter(|m| m.refs("weapon types").iter().any(|(sid, _)| *sid == rec.string_id))
        .collect();
    makers.sort_by(|a, b| a.name.cmp(&b.name));
    makers.iter().flat_map(|m| m.refs("weapon models")).find_map(|(sid, _)| data.get(sid))
}

/// The record whose `texture map`/`normal map` dress `rec`'s mesh:
/// - an item's `material` reference (an ITEM -> MATERIAL_SPECS_* record);
/// - a weapon type's first model from a manufacturer that makes it (WEAPON_MANUFACTURER
///   `weapon types` -> `weapon models`, a MATERIAL_SPECS_WEAPON); manufacturers are tried by name;
/// - otherwise the record itself, when it carries its own maps.
pub fn appearance_material<'a>(
    data: &'a GameDataContainer,
    rec: &'a GameData,
) -> Option<&'a GameData> {
    if let Some(m) = rec
        .refs("material")
        .first()
        .and_then(|(sid, _)| data.get(sid))
    {
        return Some(m);
    }
    if rec.item_type() == Some(ItemType::Weapon) {
        if let Some(model) = maker_model(data, rec) {
            return Some(model);
        }
        // (assumption) a weapon no manufacturer makes (unique ones: Bull Horn Axe, Exile Plank)
        // takes the model of a made weapon drawn with the same mesh; its mesh's own Ogre
        // material names an exporter texture (mainweapons2.psd) the install does not have
        let mesh = rec.files.get("mesh").filter(|m| !m.is_empty());
        let mut twins: Vec<&GameData> = data
            .of_type(ItemType::Weapon)
            .filter(|w| w.string_id != rec.string_id && mesh.is_some() && w.files.get("mesh") == mesh)
            .collect();
        twins.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(model) = twins.into_iter().find_map(|w| maker_model(data, w)) {
            return Some(model);
        }
    }
    rec.files.contains_key("texture map").then_some(rec)
}

/// The submeshes `to_bevy_skinned` converts, in its order (their numbers in the mesh).
pub fn converted_submeshes(mesh: &ogre_mesh::Mesh) -> Vec<usize> {
    mesh.submeshes
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            let geo = if s.use_shared { mesh.shared.as_ref() } else { s.geometry.as_ref() };
            s.operation == OP_TRIANGLE_LIST && geo.and_then(|g| g.positions()).is_some()
        })
        .map(|(k, _)| k)
        .collect()
}

/// A vertex attribute's values at the given vertices (the formats meshes here carry).
fn gather(v: &VertexAttributeValues, at: &[u32]) -> Option<VertexAttributeValues> {
    use VertexAttributeValues as V;
    fn pick<T: Copy>(a: &[T], at: &[u32]) -> Vec<T> {
        at.iter().map(|&i| a[i as usize]).collect()
    }
    Some(match v {
        V::Float32(a) => V::Float32(pick(a, at)),
        V::Float32x2(a) => V::Float32x2(pick(a, at)),
        V::Float32x3(a) => V::Float32x3(pick(a, at)),
        V::Float32x4(a) => V::Float32x4(pick(a, at)),
        V::Uint16x4(a) => V::Uint16x4(pick(a, at)),
        V::Unorm8x4(a) => V::Unorm8x4(pick(a, at)),
        V::Uint8x4(a) => V::Uint8x4(pick(a, at)),
        _ => return None,
    })
}

/// Mesh `m` (converted from submesh `submesh` of `ogre`) cut down to the vertices its triangles
/// use, with the Ogre mesh's poses on that geometry (a pose's target: 0 the shared geometry, n
/// submesh n - 1) as its morph targets, by pose name (Kenshi's face morphs, F021065: drawn by
/// each character's weights).
pub fn with_poses(m: &Mesh, ogre: &ogre_mesh::Mesh, submesh: usize) -> Mesh {
    use bevy::mesh::morph::MorphAttributes;
    let Some(Indices::U32(idx)) = m.indices() else { return m.clone() };
    let shared = ogre.submeshes.get(submesh).is_some_and(|s| s.use_shared);
    let target = if shared { 0 } else { submesh.wrapping_add(1) as u16 };
    // (submesh usize::MAX: the vertices cut down, no poses)
    let poses: Vec<&ogre_mesh::Pose> = if submesh == usize::MAX { Vec::new() } else { ogre.poses.iter().filter(|p| p.target == target).collect() };
    let n = m.count_vertices();
    // the vertices used, in first-use order, and where each old one went
    let mut new_of = vec![u32::MAX; n];
    let mut used: Vec<u32> = Vec::new();
    for &i in idx {
        if new_of[i as usize] == u32::MAX {
            new_of[i as usize] = used.len() as u32;
            used.push(i);
        }
    }
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    for (attr, values) in m.attributes() {
        if let Some(v) = gather(values, &used) {
            out.insert_attribute(attr.clone(), v);
        }
    }
    out.insert_indices(Indices::U32(idx.iter().map(|&i| new_of[i as usize]).collect()));
    if poses.is_empty() || poses.len() > bevy::mesh::morph::MAX_MORPH_WEIGHTS {
        return out;
    }
    let count = used.len();
    let mut targets = vec![MorphAttributes::new(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO); count * poses.len()];
    for (k, p) in poses.iter().enumerate() {
        for &(i, d) in &p.offsets {
            if let Some(&j) = new_of.get(i as usize).filter(|&&j| j != u32::MAX) {
                targets[k * count + j as usize].position = Vec3::from(d);
            }
        }
    }
    out.set_morph_targets(targets);
    out.set_morph_target_names(poses.iter().map(|p| p.name.clone()).collect());
    out
}

/// Mesh `m` without its morph targets (its attributes and triangles only).
pub fn without_morphs(m: &Mesh) -> Mesh {
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    for (attr, values) in m.attributes() {
        out.insert_attribute(attr.clone(), values.clone());
    }
    if let Some(i) = m.indices() {
        out.insert_indices(i.clone());
    }
    out
}

/// Mesh `base`'s morph targets baked in at the weights `weight` gives each by name (Kenshi's
/// face poses, Ogre's software pose blend: each vertex moved by the weighted sum of its pose
/// offsets), a mesh of its own without morph targets; None if it has none.
pub fn baked(base: &Mesh, weight: impl Fn(&str) -> f32) -> Option<Mesh> {
    let names = base.morph_target_names()?;
    let targets = base.morph_targets()?;
    let Some(VertexAttributeValues::Float32x3(pos)) = base.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let n = pos.len();
    let mut moved = pos.clone();
    for (k, name) in names.iter().enumerate() {
        let w = weight(name);
        if w == 0.0 {
            continue;
        }
        for (v, p) in moved.iter_mut().enumerate() {
            let d = targets.get(k * n + v)?.position * w;
            p[0] += d.x;
            p[1] += d.y;
            p[2] += d.z;
        }
    }
    let mut out = without_morphs(base);
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, moved);
    Some(out)
}

/// Split a character body mesh into (body, head): Kenshi body meshes put the head's UVs in the tile
/// above the body's (v < 0), textured from the HEAD record instead of the race body texture. Head
/// triangles move to their own mesh with v shifted into 0..1. `None` if the mesh has no head tile.
pub fn split_head_tile(mesh: &Mesh) -> Option<(Mesh, Mesh)> {
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        return None;
    };
    let Some(Indices::U32(idx)) = mesh.indices() else {
        return None;
    };
    let (mut body, mut head) = (Vec::new(), Vec::new());
    for tri in idx.as_chunks::<3>().0 {
        // a triangle belongs to the head tile if its centroid does
        let v = tri.iter().map(|&i| uvs[i as usize][1]).sum::<f32>() / 3.0;
        if v < 0.0 {
            head.extend_from_slice(tri)
        } else {
            body.extend_from_slice(tri)
        }
    }
    if head.is_empty() {
        return None;
    }
    let mut b = mesh.clone();
    b.insert_indices(Indices::U32(body));
    let mut h = mesh.clone();
    h.insert_indices(Indices::U32(head));
    let shifted: Vec<[f32; 2]> = uvs.iter().map(|[u, v]| [*u, v + 1.0]).collect();
    h.insert_attribute(Mesh::ATTRIBUTE_UV_0, shifted);
    Some((b, h))
}

/// A decoded texture as an sRGB image.
pub fn rgba_image(tex: &dds::Rgba) -> Image {
    Image::new(
        Extent3d {
            width: tex.width,
            height: tex.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        tex.pixels.iter().flatten().copied().collect(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Kenshi's hair/beard "Mod" textures pack the strands rather than colour them (inferred from the
/// shipped files, not yet confirmed against the shader): G = strand brightness, A = coverage, R/B =
/// mid-grey (likely normal xy). The character's hair colour tints G. Returns an sRGB RGBA image to be
/// drawn alpha-masked.
pub fn hair_image(tex: &dds::Rgba, colour: [f32; 3]) -> Image {
    let data: Vec<u8> = tex
        .pixels
        .iter()
        .flat_map(|[_, g, _, a]| {
            let l = *g as f32 / 255.0;
            let c = colour.map(|c| (c * l * 255.0).round().clamp(0.0, 255.0) as u8);
            [c[0], c[1], c[2], *a]
        })
        .collect();
    Image::new(
        Extent3d {
            width: tex.width,
            height: tex.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Per-vertex 4 joint indices and weights: the 4 heaviest assignments, normalised to sum 1.
/// Vertices without assignments are bound fully to joint 0.
pub fn skin_weights(
    assigned: &[ogre_mesh::BoneAssignment],
    vertex_count: usize,
    joint_of: &dyn Fn(u16) -> Option<u16>,
) -> (Vec<[u16; 4]>, Vec<[f32; 4]>) {
    let mut per: Vec<Vec<(u16, f32)>> = vec![Vec::new(); vertex_count];
    for a in assigned {
        if let (Some(v), Some(j)) = (per.get_mut(a.vertex as usize), joint_of(a.bone)) {
            v.push((j, a.weight));
        }
    }
    let mut idx = Vec::with_capacity(vertex_count);
    let mut wts = Vec::with_capacity(vertex_count);
    for mut v in per {
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.truncate(4);
        let total: f32 = v.iter().map(|x| x.1).sum();
        let (mut i4, mut w4) = ([0u16; 4], [0f32; 4]);
        if total > 0.0 {
            for (k, (j, w)) in v.into_iter().enumerate() {
                i4[k] = j;
                w4[k] = w / total;
            }
        } else {
            w4[0] = 1.0;
        }
        idx.push(i4);
        wts.push(w4);
    }
    (idx, wts)
}

/// A path as stored in game data (`.\data\items\x.mesh`) -> relative to the install root
/// (`data/items/x.mesh`).
pub fn data_path(stored: &str) -> String {
    stored
        .trim_start_matches(".\\")
        .trim_start_matches("./")
        .replace('\\', "/")
}

/// Axis-aligned bounds of all positions, for framing a camera.
pub fn bounds(mesh: &ogre_mesh::Mesh) -> Option<([f32; 3], [f32; 3])> {
    let mut it = mesh
        .shared
        .iter()
        .chain(mesh.submeshes.iter().filter_map(|s| s.geometry.as_ref()))
        .filter_map(|g| g.positions())
        .flatten();
    let first = it.next()?;
    Some(it.fold((first, first), |(mut lo, mut hi), p| {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
        (lo, hi)
    }))
}

/// Terrain patch split by colour tile: one mesh per `colour.X.Y.png` tile the patch touches, with
/// UVs into that tile (heightmap sample / 2048 within the tile) and normals; positions relative to
/// `origin` (`None`: centred on the patch). Returns ((X, Y), mesh).
pub fn terrain_tiles(p: &Patch, origin: Option<Vec3>) -> Vec<((u32, u32), Mesh)> {
    terrain_pieces(p, origin, kenshi_world::COLOUR_TILE_PX)
        .into_iter()
        .map(|(t, _, m)| (t, m))
        .collect()
}

/// A terrain piece: (colour tile X, Y), (blend cell X, Y), its mesh.
pub type TerrainPiece = ((u32, u32), (u32, u32), Mesh);

/// As [`terrain_tiles`], split further into cells of `cell_px` heightmap samples (the terrain
/// blend cells): ((tile X, Y), (cell X, Y), mesh), UVs into the colour tile.
pub fn terrain_pieces(p: &Patch, origin: Option<Vec3>, cell_px: u32) -> Vec<TerrainPiece> {
    terrain_pieces_where(p, origin, cell_px, 0.0, |_, _| true)
}

/// As [`terrain_pieces`], keeping only the quads whose centre (world x, z) `keep` accepts, and
/// with a skirt `skirt` units deep hanging from every edge of the kept quads that borders no kept
/// quad (the patch's rim and the holes; 0: none), closing cracks against terrain of another
/// resolution next to it.
pub fn terrain_pieces_where(
    p: &Patch,
    origin: Option<Vec3>,
    cell_px: u32,
    skirt: f32,
    keep: impl Fn(f32, f32) -> bool,
) -> Vec<TerrainPiece> {
    let full = terrain_mesh(p, origin);
    let Some(VertexAttributeValues::Float32x3(pos)) = full.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Vec::new();
    };
    let Some(VertexAttributeValues::Float32x3(nrm)) = full.attribute(Mesh::ATTRIBUTE_NORMAL) else {
        return Vec::new();
    };
    let (w, h) = (p.w as usize, p.h as usize);
    let tile_px = kenshi_world::COLOUR_TILE_PX;
    let sample = |i: usize, j: usize| (p.sx + i as u32 * p.step, p.sy + j as u32 * p.step);
    // quads go to the tile of their first corner
    type Key = ((u32, u32), (u32, u32));
    let mut by_tile: std::collections::BTreeMap<Key, Vec<(usize, usize)>> = Default::default();
    let centre = |k: usize, o: f32| o + (k as f32 + 0.5) * p.spacing;
    let kept: Vec<bool> = (0..(h - 1) * (w - 1))
        .map(|q| keep(centre(q % (w - 1), p.x0), centre(q / (w - 1), p.z0)))
        .collect();
    let is_kept = |i: i64, j: i64| {
        i >= 0
            && j >= 0
            && (i as usize) < w - 1
            && (j as usize) < h - 1
            && kept[j as usize * (w - 1) + i as usize]
    };
    for j in 0..h - 1 {
        for i in 0..w - 1 {
            if !is_kept(i as i64, j as i64) {
                continue;
            }
            let (c, r) = sample(i, j);
            let tile = ((c / tile_px).min(7), (r / tile_px).min(7));
            let cell = (c / cell_px, r / cell_px);
            by_tile.entry((tile, cell)).or_default().push((i, j));
        }
    }
    let mut out = Vec::new();
    for (((tx, ty), cell), quads) in by_tile {
        let mut remap: std::collections::HashMap<usize, u32> = Default::default();
        let (mut tp, mut tn, mut tuv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut vert = |i: usize,
                        j: usize,
                        tp: &mut Vec<[f32; 3]>,
                        tn: &mut Vec<[f32; 3]>,
                        tuv: &mut Vec<[f32; 2]>| {
            let k = j * w + i;
            *remap.entry(k).or_insert_with(|| {
                let (c, r) = sample(i, j);
                tp.push(pos[k]);
                tn.push(nrm[k]);
                tuv.push([
                    (c as f32 - (tx * tile_px) as f32) / tile_px as f32,
                    (r as f32 - (ty * tile_px) as f32) / tile_px as f32,
                ]);
                (tp.len() - 1) as u32
            })
        };
        for (i, j) in quads {
            let a = vert(i, j, &mut tp, &mut tn, &mut tuv);
            let b = vert(i + 1, j, &mut tp, &mut tn, &mut tuv);
            let c = vert(i, j + 1, &mut tp, &mut tn, &mut tuv);
            let d = vert(i + 1, j + 1, &mut tp, &mut tn, &mut tuv);
            idx.extend([a, c, b, b, c, d]);
            if skirt > 0.0 {
                // the edges on the patch's border, each hung with a quad `skirt` deep
                let mut hang = |u: u32, v: u32| {
                    let n = tp.len() as u32;
                    for k in [u, v] {
                        let q = tp[k as usize];
                        tp.push([q[0], q[1] - skirt, q[2]]);
                        tn.push(tn[k as usize]);
                        tuv.push(tuv[k as usize]);
                    }
                    idx.extend([u, v, n, n, v, n + 1, u, n, v, v, n, n + 1]);
                };
                let (si, sj) = (i as i64, j as i64);
                if !is_kept(si, sj - 1) {
                    hang(a, b);
                }
                if !is_kept(si, sj + 1) {
                    hang(c, d);
                }
                if !is_kept(si - 1, sj) {
                    hang(a, c);
                }
                if !is_kept(si + 1, sj) {
                    hang(b, d);
                }
            }
        }
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, tp);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, tn);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, tuv);
        m.insert_indices(Indices::U32(idx));
        out.push(((tx, ty), cell, m));
    }
    out
}

/// Grid mesh of a terrain patch: in world units relative to `origin` (`None`: centred on the
/// patch), coloured by slope (flat = sand, steep = rock)
/// and darkened at low altitude.
pub fn terrain_mesh(p: &Patch, origin: Option<Vec3>) -> Mesh {
    let (w, h) = (p.w as usize, p.h as usize);
    // (cx, cz): offset of the origin from the patch's first sample; oy: origin height
    let (cx, cz) = match origin {
        Some(o) => (o.x - p.x0, o.z - p.z0),
        None => (
            (w - 1) as f32 * p.spacing / 2.0,
            (h - 1) as f32 * p.spacing / 2.0,
        ),
    };
    let oy = origin.map_or(0.0, |o| o.y);
    let mut pos = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            pos.push([
                i as f32 * p.spacing - cx,
                p.at(i as u32, j as u32) - oy,
                j as f32 * p.spacing - cz,
            ]);
        }
    }
    let mut idx = Vec::with_capacity((w - 1) * (h - 1) * 6);
    for j in 0..h - 1 {
        for i in 0..w - 1 {
            let a = (j * w + i) as u32;
            let (b, c, d) = (a + 1, a + w as u32, a + w as u32 + 1);
            idx.extend([a, c, b, b, c, d]);
        }
    }
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos.clone());
    m.insert_indices(Indices::U32(idx));
    m.compute_smooth_normals();
    let normals: Vec<[f32; 3]> = match m.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(n)) => n.clone(),
        _ => vec![[0.0, 1.0, 0.0]; pos.len()],
    };
    let (sand, rock) = (Vec3::new(0.55, 0.42, 0.26), Vec3::new(0.30, 0.26, 0.23));
    let colours: Vec<[f32; 4]> = pos
        .iter()
        .zip(&normals)
        .map(|(p, n)| {
            let steep = ((1.0 - n[1]) * 4.0).clamp(0.0, 1.0);
            let low = ((p[1] + oy) / 1500.0).clamp(0.55, 1.0);
            let c = sand.lerp(rock, steep) * low;
            [c.x, c.y, c.z, 1.0]
        })
        .collect();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_paths() {
        assert_eq!(
            data_path(".\\data\\items\\meshes\\A.mesh"),
            "data/items/meshes/A.mesh"
        );
        assert_eq!(data_path("data/x.dds"), "data/x.dds");
    }

    #[test]
    fn skin_weights_keep_top_four_normalised() {
        let a = |vertex, bone, weight| ogre_mesh::BoneAssignment {
            vertex,
            bone,
            weight,
        };
        let assigned = [
            a(0, 7, 0.1),
            a(0, 8, 0.4),
            a(0, 9, 0.2),
            a(0, 10, 0.2),
            a(0, 11, 0.1),
            a(1, 7, 1.0),
        ];
        let (idx, w) = skin_weights(&assigned, 3, &|h| Some(h - 7));
        assert_eq!(idx[0], [1, 2, 3, 0]);
        assert!((w[0].iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!((idx[1], w[1]), ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        assert_eq!(w[2], [1.0, 0.0, 0.0, 0.0]); // unassigned
    }
}
