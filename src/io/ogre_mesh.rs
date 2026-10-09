//! Ogre binary `.mesh` files (port_map `ogre_assets`), as shipped in Kenshi's `data/`.
//!
//! Format from sheet rows T00010..T00015 (Ogre's public, MIT-licensed format description). Every
//! chunk is `u16 id, u32 length` with the length counting the 6-byte header, so chunks this reader
//! does not need (LOD, edge lists, poses, animations, extremes) are skipped whole. Strings end in `\n`.

pub use crate::ogre::Error;
use crate::ogre::{Chunk, Reader, invalid};
use std::path::Path;

/// T00010 OgreMeshChunk.
pub mod chunk {
    pub const HEADER: u16 = 0x1000;
    pub const MESH: u16 = 0x3000;
    pub const SUBMESH: u16 = 0x4000;
    pub const SUBMESH_OPERATION: u16 = 0x4010;
    pub const SUBMESH_BONE_ASSIGNMENT: u16 = 0x4100;
    pub const SUBMESH_TEXTURE_ALIAS: u16 = 0x4200;
    pub const GEOMETRY: u16 = 0x5000;
    pub const GEOMETRY_VERTEX_DECLARATION: u16 = 0x5100;
    pub const GEOMETRY_VERTEX_ELEMENT: u16 = 0x5110;
    pub const GEOMETRY_VERTEX_BUFFER: u16 = 0x5200;
    pub const GEOMETRY_VERTEX_BUFFER_DATA: u16 = 0x5210;
    pub const MESH_SKELETON_LINK: u16 = 0x6000;
    pub const MESH_BONE_ASSIGNMENT: u16 = 0x7000;
    pub const MESH_LOD: u16 = 0x8000;
    pub const MESH_LOD_USAGE: u16 = 0x8100;
    pub const MESH_BOUNDS: u16 = 0x9000;
    pub const SUBMESH_NAME_TABLE: u16 = 0xa000;
    pub const SUBMESH_NAME_TABLE_ELEMENT: u16 = 0xa100;
    pub const MESH_POSES: u16 = 0xc000;
    pub const MESH_POSE: u16 = 0xc100;
    pub const MESH_POSE_VERTEX: u16 = 0xc111;
}

/// Chunks a MESH may contain (anything else ends the mesh).
const MESH_CHILDREN: &[u16] = &[
    chunk::GEOMETRY,
    chunk::SUBMESH,
    chunk::MESH_SKELETON_LINK,
    chunk::MESH_BONE_ASSIGNMENT,
    chunk::MESH_LOD,
    chunk::MESH_BOUNDS,
    chunk::SUBMESH_NAME_TABLE,
    0xb000, // EDGE_LISTS
    chunk::MESH_POSES,
    0xd000, // ANIMATIONS
    0xe000, // TABLE_EXTREMES
];

/// Chunks a SUBMESH may contain.
const SUBMESH_CHILDREN: &[u16] = &[
    chunk::GEOMETRY,
    chunk::SUBMESH_OPERATION,
    chunk::SUBMESH_BONE_ASSIGNMENT,
    chunk::SUBMESH_TEXTURE_ALIAS,
];

/// T00013 OgreVertexElementSemantic.
pub mod semantic {
    pub const POSITION: u16 = 1;
    pub const BLEND_WEIGHTS: u16 = 2;
    pub const BLEND_INDICES: u16 = 3;
    pub const NORMAL: u16 = 4;
    pub const DIFFUSE: u16 = 5;
    pub const SPECULAR: u16 = 6;
    pub const TEXTURE_COORDINATES: u16 = 7;
    pub const BINORMAL: u16 = 8;
    pub const TANGENT: u16 = 9;
}

/// T00012 OgreVertexElementType.
pub mod vtype {
    pub const FLOAT1: u16 = 0;
    pub const FLOAT2: u16 = 1;
    pub const FLOAT3: u16 = 2;
    pub const FLOAT4: u16 = 3;
    pub const COLOUR: u16 = 4;
    pub const SHORT1: u16 = 5;
    pub const SHORT2: u16 = 6;
    pub const SHORT3: u16 = 7;
    pub const SHORT4: u16 = 8;
    pub const UBYTE4: u16 = 9;
    pub const COLOUR_ARGB: u16 = 10;
    pub const COLOUR_ABGR: u16 = 11;

    /// Byte size of an element of this type, if known.
    pub fn size(t: u16) -> Option<usize> {
        Some(match t {
            FLOAT1 | COLOUR | UBYTE4 | COLOUR_ARGB | COLOUR_ABGR | SHORT2 => 4,
            FLOAT2 | SHORT4 => 8,
            FLOAT3 => 12,
            FLOAT4 => 16,
            SHORT1 => 2,
            SHORT3 => 6,
            _ => return None,
        })
    }
}

/// Submesh operation type for a plain triangle list (the default when no operation chunk exists).
pub const OP_TRIANGLE_LIST: u16 = 4;

/// @type OgreVertexElement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexElement {
    pub source: u16,
    pub kind: u16,
    pub semantic: u16,
    pub offset: u16,
    pub index: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VertexBuffer {
    pub bind: u16,
    pub stride: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    pub vertex_count: u32,
    pub elements: Vec<VertexElement>,
    pub buffers: Vec<VertexBuffer>,
}

impl Geometry {
    pub fn element(&self, semantic: u16, index: u16) -> Option<&VertexElement> {
        self.elements
            .iter()
            .find(|e| e.semantic == semantic && e.index == index)
    }

    /// One float-typed element per vertex, as `N` floats (missing components are 0).
    /// `None` if the element is absent or not a float type.
    pub fn floats<const N: usize>(&self, semantic: u16, index: u16) -> Option<Vec<[f32; N]>> {
        let e = self.element(semantic, index)?;
        let comps = match e.kind {
            vtype::FLOAT1 => 1,
            vtype::FLOAT2 => 2,
            vtype::FLOAT3 => 3,
            vtype::FLOAT4 => 4,
            _ => return None,
        };
        let buf = self.buffers.iter().find(|b| b.bind == e.source)?;
        let mut out = Vec::with_capacity(self.vertex_count as usize);
        for v in 0..self.vertex_count as usize {
            let at = v * buf.stride as usize + e.offset as usize;
            let mut x = [0.0; N];
            for (c, slot) in x.iter_mut().enumerate().take(comps.min(N)) {
                let b = &buf.data[at + 4 * c..at + 4 * c + 4];
                *slot = f32::from_le_bytes(b.try_into().unwrap());
            }
            out.push(x);
        }
        Some(out)
    }

    pub fn positions(&self) -> Option<Vec<[f32; 3]>> {
        self.floats(semantic::POSITION, 0)
    }

    pub fn normals(&self) -> Option<Vec<[f32; 3]>> {
        self.floats(semantic::NORMAL, 0)
    }

    pub fn uvs(&self, set: u16) -> Option<Vec<[f32; 2]>> {
        self.floats(semantic::TEXTURE_COORDINATES, set)
    }

    /// The diffuse vertex colours as RGBA in 0..1: COLOUR_ARGB (and the generic COLOUR, D3D's
    /// packing) stores B, G, R, A bytes; COLOUR_ABGR stores R, G, B, A.
    pub fn colours(&self) -> Option<Vec<[f32; 4]>> {
        let e = self.element(semantic::DIFFUSE, 0)?;
        let bgra = match e.kind {
            vtype::COLOUR_ARGB | vtype::COLOUR => true,
            vtype::COLOUR_ABGR => false,
            _ => return None,
        };
        let buf = self.buffers.iter().find(|b| b.bind == e.source)?;
        let mut out = Vec::with_capacity(self.vertex_count as usize);
        for v in 0..self.vertex_count as usize {
            let at = v * buf.stride as usize + e.offset as usize;
            let b = buf.data.get(at..at + 4)?;
            let c = |i: usize| b[i] as f32 / 255.0;
            out.push(if bgra {
                [c(2), c(1), c(0), c(3)]
            } else {
                [c(0), c(1), c(2), c(3)]
            });
        }
        Some(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneAssignment {
    pub vertex: u32,
    pub bone: u16,
    pub weight: f32,
}

/// @type OgreSubMesh
#[derive(Debug, Clone, PartialEq)]
pub struct SubMesh {
    pub material: String,
    pub use_shared: bool,
    pub indices: Vec<u32>,
    /// Own vertices; `None` when `use_shared`.
    pub geometry: Option<Geometry>,
    pub operation: u16,
    pub bone_assignments: Vec<BoneAssignment>,
    pub texture_aliases: Vec<(String, String)>,
}

/// @type OgreMesh
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    /// e.g. `MeshSerializer_v1.8`.
    pub version: String,
    pub skeletally_animated: bool,
    pub shared: Option<Geometry>,
    pub submeshes: Vec<SubMesh>,
    pub skeleton: Option<String>,
    /// (min, max, radius)
    pub bounds: Option<([f32; 3], [f32; 3], f32)>,
    /// Bone assignments for the shared geometry.
    pub bone_assignments: Vec<BoneAssignment>,
    pub submesh_names: Vec<(u16, String)>,
    /// Morph targets (e.g. a face's "big eyes").
    pub poses: Vec<Pose>,
}

/// A morph target: vertex offsets for one geometry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pose {
    pub name: String,
    /// 0 = the shared geometry, n = submesh n - 1.
    pub target: u16,
    /// (vertex index, position offset)
    pub offsets: Vec<(u32, [f32; 3])>,
}

impl Mesh {
    /// Move the vertices of pose `name` by `weight` times its offsets (positions only).
    pub fn apply_pose(&mut self, name: &str, weight: f32) -> bool {
        let Some(pose) = self.poses.iter().find(|p| p.name == name).cloned() else {
            return false;
        };
        let g = if pose.target == 0 {
            self.shared.as_mut()
        } else {
            self.submeshes
                .get_mut(pose.target as usize - 1)
                .and_then(|s| s.geometry.as_mut())
        };
        let Some(g) = g else { return false };
        let Some(e) = g.element(semantic::POSITION, 0).cloned() else {
            return false;
        };
        if e.kind != vtype::FLOAT3 {
            return false;
        }
        let Some(b) = g.buffers.iter_mut().find(|b| b.bind == e.source) else {
            return false;
        };
        for (i, d) in &pose.offsets {
            let at = *i as usize * b.stride as usize + e.offset as usize;
            for (k, dk) in d.iter().enumerate() {
                let o = at + k * 4;
                let Some(bytes) = b.data.get_mut(o..o + 4) else {
                    continue;
                };
                let v = f32::from_le_bytes(bytes.try_into().unwrap()) + dk * weight;
                bytes.copy_from_slice(&v.to_le_bytes());
            }
        }
        true
    }
}

fn geometry(r: &mut Reader, c: &Chunk) -> Result<Geometry, Error> {
    let mut g = Geometry {
        vertex_count: r.u32("vertex count")?,
        ..Default::default()
    };
    let parts = [
        chunk::GEOMETRY_VERTEX_DECLARATION,
        chunk::GEOMETRY_VERTEX_BUFFER,
    ];
    while let Some(sub) = r.child(&parts)? {
        if sub.id == chunk::GEOMETRY_VERTEX_DECLARATION {
            while r.child(&[chunk::GEOMETRY_VERTEX_ELEMENT])?.is_some() {
                g.elements.push(VertexElement {
                    source: r.u16("element source")?,
                    kind: r.u16("element type")?,
                    semantic: r.u16("element semantic")?,
                    offset: r.u16("element offset")?,
                    index: r.u16("element index")?,
                });
            }
            continue;
        }
        let bind = r.u16("buffer bind")?;
        let stride = r.u16("buffer vertex size")?;
        if r.child(&[chunk::GEOMETRY_VERTEX_BUFFER_DATA])?.is_none() {
            return Err(invalid(sub.start, "vertex buffer without data".into()));
        }
        let n = g.vertex_count as usize * stride as usize;
        let data = r.take(n, "vertex buffer data")?.to_vec();
        g.buffers.push(VertexBuffer { bind, stride, data });
    }
    for e in &g.elements {
        let Some(b) = g.buffers.iter().find(|b| b.bind == e.source) else {
            return Err(invalid(
                c.start,
                format!("element uses missing buffer {}", e.source),
            ));
        };
        if let Some(sz) = vtype::size(e.kind)
            && e.offset as usize + sz > b.stride as usize
        {
            return Err(invalid(
                c.start,
                format!("element at {} overruns stride {}", e.offset, b.stride),
            ));
        }
    }
    Ok(g)
}

fn submesh(r: &mut Reader) -> Result<SubMesh, Error> {
    let material = r.line("submesh material")?;
    let use_shared = r.u8("submesh use shared")? != 0;
    let count = r.u32("index count")? as usize;
    let idx32 = r.u8("index 32-bit")? != 0;
    let indices = if idx32 {
        let b = r.take(count.saturating_mul(4), "indices")?;
        b.as_chunks::<4>()
            .0
            .iter()
            .map(|x| u32::from_le_bytes(*x))
            .collect()
    } else {
        let b = r.take(count.saturating_mul(2), "indices")?;
        b.as_chunks::<2>()
            .0
            .iter()
            .map(|x| u16::from_le_bytes(*x) as u32)
            .collect()
    };
    let mut s = SubMesh {
        material,
        use_shared,
        indices,
        geometry: None,
        operation: OP_TRIANGLE_LIST,
        bone_assignments: Vec::new(),
        texture_aliases: Vec::new(),
    };
    while let Some(sub) = r.child(SUBMESH_CHILDREN)? {
        match sub.id {
            chunk::GEOMETRY => s.geometry = Some(geometry(r, &sub)?),
            chunk::SUBMESH_OPERATION => s.operation = r.u16("operation type")?,
            chunk::SUBMESH_BONE_ASSIGNMENT => s.bone_assignments.push(r.bone_assignment()?),
            chunk::SUBMESH_TEXTURE_ALIAS => {
                s.texture_aliases
                    .push((r.line("alias name")?, r.line("alias texture")?));
            }
            _ => r.skip(&sub)?,
        }
    }
    Ok(s)
}

fn mesh(r: &mut Reader, c: &Chunk, version: String) -> Result<Mesh, Error> {
    let mut m = Mesh {
        version,
        skeletally_animated: r.u8("skeletally animated")? != 0,
        ..Default::default()
    };
    while let Some(sub) = r.child(MESH_CHILDREN)? {
        match sub.id {
            chunk::GEOMETRY => m.shared = Some(geometry(r, &sub)?),
            chunk::SUBMESH => m.submeshes.push(submesh(r)?),
            chunk::MESH_SKELETON_LINK => m.skeleton = Some(r.line("skeleton name")?),
            chunk::MESH_BONE_ASSIGNMENT => m.bone_assignments.push(r.bone_assignment()?),
            chunk::MESH_BOUNDS => {
                m.bounds = Some((r.f32x3("bounds")?, r.f32x3("bounds")?, r.f32("radius")?))
            }
            // v1.41/v1.8 store a wrong MESH_LOD length; read its header fields and skip each
            // LOD_USAGE child by its (correct) length. v1.100's layout differs but its lengths hold.
            chunk::MESH_LOD if !m.version.contains("v1.100") => {
                r.line("lod strategy")?;
                r.u16("lod level count")?;
                r.u8("lod manual")?;
                while let Some(u) = r.child(&[chunk::MESH_LOD_USAGE])? {
                    r.skip(&u)?;
                }
            }
            chunk::SUBMESH_NAME_TABLE => {
                while r.child(&[chunk::SUBMESH_NAME_TABLE_ELEMENT])?.is_some() {
                    m.submesh_names
                        .push((r.u16("submesh index")?, r.line("submesh name")?));
                }
            }
            chunk::MESH_POSES => {
                // v1.8+ poses say whether their vertices carry normal offsets too
                let has_normal_flag = !m.version.contains("v1.41");
                while r.child(&[chunk::MESH_POSE])?.is_some() {
                    let mut pose = Pose {
                        name: r.line("pose name")?,
                        target: r.u16("pose target")?,
                        offsets: Vec::new(),
                    };
                    let normals = has_normal_flag && r.u8("pose includes normals")? != 0;
                    while r.child(&[chunk::MESH_POSE_VERTEX])?.is_some() {
                        let i = r.u32("pose vertex index")?;
                        let d = r.f32x3("pose vertex offset")?;
                        if normals {
                            r.f32x3("pose vertex normal")?;
                        }
                        pose.offsets.push((i, d));
                    }
                    m.poses.push(pose);
                }
            }
            _ => r.skip(&sub)?,
        }
    }
    // every index must address a vertex of the geometry it draws from
    for s in &m.submeshes {
        let g = if s.use_shared {
            m.shared.as_ref()
        } else {
            s.geometry.as_ref()
        };
        let n = g.map_or(0, |g| g.vertex_count);
        if let Some(&bad) = s.indices.iter().find(|&&i| i >= n) {
            return Err(invalid(
                c.start,
                format!("submesh '{}' index {bad} >= {n} vertices", s.material),
            ));
        }
    }
    Ok(m)
}

/// Parse a whole `.mesh`; every byte must belong to a chunk.
pub fn parse(buf: &[u8]) -> Result<Mesh, Error> {
    let mut r = Reader { buf, pos: 0 };
    let id = r.u16("header")?;
    if id != chunk::HEADER {
        return Err(Error::BadHeader(id));
    }
    let version = r.line("version")?;
    let mut out = None;
    while r.pos < buf.len() {
        let c = r.header()?;
        if c.id == chunk::MESH && out.is_none() {
            out = Some(mesh(&mut r, &c, version.clone())?);
        } else {
            r.skip(&c)?;
        }
    }
    out.ok_or(Error::Invalid {
        at: 0,
        what: "no mesh chunk".into(),
    })
}

pub fn read(path: &Path) -> Result<Mesh, Error> {
    let buf = std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    parse(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal writer for tests: chunk = id, len (incl. 6-byte header), body.
    fn ch(id: u16, body: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend(((body.len() + 6) as u32).to_le_bytes());
        v.extend(body);
        v
    }

    fn triangle(index: u32) -> Vec<u8> {
        build(index, false)
    }

    /// `old`: a v1.8 file with the real format's wrong MESH/SUBMESH/GEOMETRY/MESH_LOD lengths.
    fn build(index: u32, old: bool) -> Vec<u8> {
        // parent chunk, with the stored length replaced by a wrong one in old files
        let parent = |id: u16, body: &[u8]| {
            let mut c = ch(id, body);
            if old {
                c[2..6].copy_from_slice(&7u32.to_le_bytes());
            }
            c
        };
        let el = |src: u16, t: u16, sem: u16, off: u16| {
            let mut b = Vec::new();
            for x in [src, t, sem, off, 0] {
                b.extend(x.to_le_bytes());
            }
            ch(chunk::GEOMETRY_VERTEX_ELEMENT, &b)
        };
        let mut decl = el(0, vtype::FLOAT3, semantic::POSITION, 0);
        decl.extend(el(0, vtype::FLOAT2, semantic::TEXTURE_COORDINATES, 12));
        let mut data = Vec::new();
        for v in [
            [0.0f32, 0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 1.0],
        ] {
            for x in v {
                data.extend(x.to_le_bytes());
            }
        }
        let mut vb = 0u16.to_le_bytes().to_vec();
        vb.extend(20u16.to_le_bytes());
        vb.extend(ch(chunk::GEOMETRY_VERTEX_BUFFER_DATA, &data));
        let mut geo = 3u32.to_le_bytes().to_vec();
        geo.extend(ch(chunk::GEOMETRY_VERTEX_DECLARATION, &decl));
        geo.extend(ch(chunk::GEOMETRY_VERTEX_BUFFER, &vb));
        let mut sm = b"Mat/Rock\n".to_vec();
        sm.push(0);
        sm.extend(3u32.to_le_bytes());
        sm.push(0);
        for i in [0u16, 1, index as u16] {
            sm.extend(i.to_le_bytes());
        }
        sm.extend(parent(chunk::GEOMETRY, &geo));
        sm.extend(ch(
            chunk::SUBMESH_OPERATION,
            &OP_TRIANGLE_LIST.to_le_bytes(),
        ));
        let mut m = vec![0u8];
        m.extend(parent(chunk::SUBMESH, &sm));
        if old {
            let mut lod = chunk::MESH_LOD.to_le_bytes().to_vec();
            lod.extend(9u32.to_le_bytes()); // as written by the old serializer
            lod.extend(b"Distance\n");
            lod.extend(2u16.to_le_bytes());
            lod.push(0);
            lod.extend(ch(chunk::MESH_LOD_USAGE, &1000f32.to_le_bytes()));
            m.extend(lod);
        } else {
            m.extend(ch(chunk::MESH_LOD, b"lod data we skip"));
        }
        m.extend(ch(chunk::MESH_SKELETON_LINK, b"rock.skeleton\n"));
        let mut f = chunk::HEADER.to_le_bytes().to_vec();
        let version: &[u8] = if old {
            b"[MeshSerializer_v1.8]\n"
        } else {
            b"[MeshSerializer_v1.100]\n"
        };
        f.extend(version);
        f.extend(parent(chunk::MESH, &m));
        f
    }

    #[test]
    fn parses_triangle() {
        let m = parse(&triangle(2)).unwrap();
        assert_eq!(m.version, "[MeshSerializer_v1.100]");
        assert_eq!(m.skeleton.as_deref(), Some("rock.skeleton"));
        let s = &m.submeshes[0];
        assert_eq!(
            (s.material.as_str(), s.indices.as_slice()),
            ("Mat/Rock", &[0, 1, 2][..])
        );
        let g = s.geometry.as_ref().unwrap();
        assert_eq!(g.positions().unwrap()[1], [1.0, 0.0, 0.0]);
        assert_eq!(g.uvs(0).unwrap()[2], [0.0, 1.0]);
    }

    #[test]
    fn rejects_bad_index_and_chunk_overrun() {
        assert!(matches!(parse(&triangle(3)), Err(Error::Invalid { .. })));
        let mut b = triangle(2);
        b.truncate(b.len() - 1);
        assert!(matches!(parse(&b), Err(Error::Truncated { .. })));
    }

    /// v1.8/v1.41 files: MESH, SUBMESH, GEOMETRY and MESH_LOD lengths are wrong in the file; the
    /// reader must not depend on them.
    #[test]
    fn old_versions_ignore_parent_lengths() {
        let m = parse(&build(2, true)).unwrap();
        assert_eq!(m.version, "[MeshSerializer_v1.8]");
        assert_eq!(m.skeleton.as_deref(), Some("rock.skeleton"));
        assert_eq!(m.submeshes[0].geometry.as_ref().unwrap().vertex_count, 3);
    }
}
