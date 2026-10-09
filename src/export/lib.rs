//! Export Kenshi's character models from the user's own install to glTF 2.0 binary (`.glb`):
//! static meshes in their bind pose, each Ogre submesh a primitive with its texture embedded as
//! PNG. Nothing of the game's is bundled with the rewrite; this only converts the files of the
//! install it is pointed at.

use kenshi_io::dds::Rgba;
use kenshi_io::ogre_mesh::{self, OP_TRIANGLE_LIST};

/// One primitive: triangles with positions, normals and UVs, and its material.
pub struct Part {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material: Material,
}

/// A texture: embedded, or a PNG file the model refers to (`uri`, relative to the model).
#[derive(Clone)]
pub enum Texture {
    Embedded(Rgba),
    File(String),
}

/// A material: base colour texture (and normal map), alpha handling.
#[derive(Clone, Default)]
pub struct Material {
    pub name: String,
    pub base: Option<Texture>,
    pub normal: Option<Texture>,
    /// Cut out by alpha (hair cards) and drawn on both sides.
    pub mask: bool,
}

/// The triangle-list submeshes of an Ogre mesh as parts (all with `material`), positions scaled
/// by `scale`.
pub fn parts(mesh: &ogre_mesh::Mesh, material: &Material, scale: f32) -> Vec<Part> {
    let mut out = vec![];
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
        out.push(Part {
            positions: positions.iter().map(|p| p.map(|v| v * scale)).collect(),
            normals: geo.normals(),
            uvs: geo.uvs(0).unwrap_or_else(|| vec![[0.0; 2]; n]),
            indices: s.indices.clone(),
            material: material.clone(),
        });
    }
    out
}

/// Split parts by UV tile: triangles whose centroid has v < 0 (Kenshi body meshes keep the head
/// there) and the rest. The head part's UVs are shifted into 0..1.
pub fn split_head(parts: Vec<Part>) -> (Vec<Part>, Vec<Part>) {
    let (mut body, mut head) = (vec![], vec![]);
    for p in parts {
        let (mut b, mut h) = (vec![], vec![]);
        for t in p.indices.as_chunks::<3>().0 {
            let v = t.iter().map(|&i| p.uvs[i as usize][1]).sum::<f32>() / 3.0;
            if v < 0.0 {
                h.extend_from_slice(t)
            } else {
                b.extend_from_slice(t)
            }
        }
        if !h.is_empty() {
            head.push(Part {
                positions: p.positions.clone(),
                normals: p.normals.clone(),
                uvs: p.uvs.iter().map(|[u, v]| [*u, v + 1.0]).collect(),
                indices: h,
                material: p.material.clone(),
            });
        }
        if !b.is_empty() {
            body.push(Part { indices: b, ..p });
        }
    }
    (body, head)
}

/// Drop vertices no triangle uses (a head cut from a body keeps only its own).
pub fn compact(p: &mut Part) {
    let mut map = vec![u32::MAX; p.positions.len()];
    let mut pos = vec![];
    let mut nor = vec![];
    let mut uv = vec![];
    for i in p.indices.iter_mut() {
        let k = *i as usize;
        if map[k] == u32::MAX {
            map[k] = pos.len() as u32;
            pos.push(p.positions[k]);
            if let Some(n) = &p.normals {
                nor.push(n[k]);
            }
            uv.push(p.uvs[k]);
        }
        *i = map[k];
    }
    p.positions = pos;
    p.normals = p.normals.as_ref().map(|_| nor);
    p.uvs = uv;
}

/// A texture as PNG bytes.
pub fn png(t: &Rgba) -> Vec<u8> {
    let img = image::RgbaImage::from_raw(
        t.width,
        t.height,
        t.pixels.iter().flatten().copied().collect(),
    )
    .expect("texture size");
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Appends bytes to the binary chunk as a buffer view; returns its index.
type PushView<'a> = dyn FnMut(&mut Vec<u8>, &[u8], Option<u32>) -> usize + 'a;

/// Write parts as one glTF 2.0 binary with one mesh (a primitive per part).
pub fn glb(name: &str, parts: &[Part]) -> Vec<u8> {
    let mut bin: Vec<u8> = vec![];
    let mut views = vec![];
    let mut accessors = vec![];
    let mut images = vec![];
    let mut materials = vec![];
    let mut prims = vec![];
    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: Option<u32>| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let off = bin.len();
        bin.extend_from_slice(bytes);
        let t = target.map_or(String::new(), |t| format!(",\"target\":{t}"));
        views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}{t}}}",
            bytes.len()
        ));
        views.len() - 1
    };
    let f32s = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let tex_index =
        |bin: &mut Vec<u8>, images: &mut Vec<String>, push_view: &mut PushView, t: &Texture| {
            match t {
                Texture::Embedded(t) => {
                    let v = push_view(bin, &png(t), None);
                    images.push(format!("{{\"bufferView\":{v},\"mimeType\":\"image/png\"}}"));
                }
                Texture::File(uri) => images.push(format!("{{\"uri\":\"{}\"}}", esc(uri))),
            }
            images.len() - 1
        };
    for p in parts {
        if p.indices.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for q in &p.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        let n = p.positions.len();
        let flat: Vec<f32> = p.positions.iter().flatten().copied().collect();
        let v = push_view(&mut bin, &f32s(&flat), Some(34962));
        accessors.push(format!(
            "{{\"bufferView\":{v},\"componentType\":5126,\"count\":{n},\"type\":\"VEC3\",\"min\":[{},{},{}],\"max\":[{},{},{}]}}",
            lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
        ));
        let pos = accessors.len() - 1;
        let mut attrs = format!("\"POSITION\":{pos}");
        if let Some(nm) = &p.normals {
            let flat: Vec<f32> = nm.iter().flatten().copied().collect();
            let v = push_view(&mut bin, &f32s(&flat), Some(34962));
            accessors.push(format!(
                "{{\"bufferView\":{v},\"componentType\":5126,\"count\":{n},\"type\":\"VEC3\"}}"
            ));
            attrs += &format!(",\"NORMAL\":{}", accessors.len() - 1);
        }
        let flat: Vec<f32> = p.uvs.iter().flatten().copied().collect();
        let v = push_view(&mut bin, &f32s(&flat), Some(34962));
        accessors.push(format!(
            "{{\"bufferView\":{v},\"componentType\":5126,\"count\":{n},\"type\":\"VEC2\"}}"
        ));
        attrs += &format!(",\"TEXCOORD_0\":{}", accessors.len() - 1);
        let ib: Vec<u8> = p.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let v = push_view(&mut bin, &ib, Some(34963));
        accessors.push(format!(
            "{{\"bufferView\":{v},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
            p.indices.len()
        ));
        let idx = accessors.len() - 1;
        // material
        let m = &p.material;
        let mut pbr = String::from("\"metallicFactor\":0,\"roughnessFactor\":1");
        if let Some(t) = &m.base {
            let i = tex_index(&mut bin, &mut images, &mut push_view, t);
            pbr += &format!(",\"baseColorTexture\":{{\"index\":{i}}}");
        }
        let mut extra = String::new();
        if let Some(t) = &m.normal {
            let i = tex_index(&mut bin, &mut images, &mut push_view, t);
            extra += &format!(",\"normalTexture\":{{\"index\":{i}}}");
        }
        if m.mask {
            extra += ",\"alphaMode\":\"MASK\",\"alphaCutoff\":0.5,\"doubleSided\":true";
        }
        materials.push(format!(
            "{{\"name\":\"{}\",\"pbrMetallicRoughness\":{{{pbr}}}{extra}}}",
            esc(&m.name)
        ));
        prims.push(format!(
            "{{\"attributes\":{{{attrs}}},\"indices\":{idx},\"material\":{}}}",
            materials.len() - 1
        ));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    // textures: one per image, sharing a sampler
    let textures: Vec<String> = (0..images.len())
        .map(|i| format!("{{\"sampler\":0,\"source\":{i}}}"))
        .collect();
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"kenshi-rewrite export-models\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[0]}}],\"nodes\":[{{\"name\":\"{n}\",\"mesh\":0}}],\"meshes\":[{{\"name\":\"{n}\",\"primitives\":[{}]}}],\"materials\":[{}],\"textures\":[{}],\"images\":[{}],\"samplers\":[{{\"magFilter\":9729,\"minFilter\":9987}}],\"accessors\":[{}],\"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}]}}",
        prims.join(","),
        materials.join(","),
        textures.join(","),
        images.join(","),
        accessors.join(","),
        views.join(","),
        bin.len(),
        n = esc(name)
    );
    let mut js = json.into_bytes();
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// Whether a normal map uses the swizzled DXT5nm layout Kenshi ships (X in alpha, Y copied into
/// red, green and blue): most pixels grey with a varying alpha.
pub fn is_swizzled_normal(t: &Rgba) -> bool {
    let n = t.pixels.len().max(1);
    let grey = t
        .pixels
        .iter()
        .step_by(97)
        .filter(|[r, g, b, _]| r.abs_diff(*g) < 4 && g.abs_diff(*b) < 4)
        .count();
    let (lo, hi) = t
        .pixels
        .iter()
        .step_by(97)
        .fold((255u8, 0u8), |(l, h), p| (l.min(p[3]), h.max(p[3])));
    grey * 97 * 10 >= n * 9 && hi - lo > 64
}

/// A swizzled normal map as a standard tangent-space one (X red, Y green, Z blue rebuilt), with Y
/// flipped from Kenshi's DirectX convention to glTF's (Y up).
pub fn unswizzle_normal(t: &Rgba) -> Rgba {
    Rgba {
        width: t.width,
        height: t.height,
        pixels: t
            .pixels
            .iter()
            .map(|[_, g, _, a]| {
                let x = *a as f32 / 255.0 * 2.0 - 1.0;
                let y = -(*g as f32 / 255.0 * 2.0 - 1.0);
                let z = (1.0 - x * x - y * y).max(0.0).sqrt();
                let c = |v: f32| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
                [c(x), c(y), c(z), 255]
            })
            .collect(),
    }
}

/// An opaque texture's alpha set solid (Kenshi keeps masks there that viewers would take for
/// transparency).
pub fn opaque(t: &Rgba) -> Rgba {
    Rgba {
        width: t.width,
        height: t.height,
        pixels: t
            .pixels
            .iter()
            .map(|[r, g, b, _]| [*r, *g, *b, 255])
            .collect(),
    }
}

/// Kenshi's hair "Mod" textures keep strand brightness in green and coverage in alpha; tint the
/// strands with a hair colour for a plain colour texture.
pub fn tint_hair(t: &Rgba, colour: [f32; 3]) -> Rgba {
    Rgba {
        width: t.width,
        height: t.height,
        pixels: t
            .pixels
            .iter()
            .map(|[_, g, _, a]| {
                let l = *g as f32 / 255.0;
                let c = colour.map(|c| (c * l * 255.0).round().clamp(0.0, 255.0) as u8);
                [c[0], c[1], c[2], *a]
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glb_header_and_chunks() {
        let p = Part {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: None,
            uvs: vec![[0.0; 2]; 3],
            indices: vec![0, 1, 2],
            material: Material::default(),
        };
        let g = glb("tri", &[p]);
        assert_eq!(&g[0..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(g[8..12].try_into().unwrap()) as usize,
            g.len()
        );
        let jl = u32::from_le_bytes(g[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&g[20..20 + jl]).unwrap();
        assert!(json.contains("\"POSITION\":0"));
        assert_eq!(jl % 4, 0);
    }
}
