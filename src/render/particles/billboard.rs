//! Ogre's BillboardSet geometry as ParticleUniverse's Billboard renderer feeds it (world-space
//! billboards, external data): each particle a quad from the billboard axes (genBillboardAxes),
//! its origin and size (genVertexOffsets), its rotation turning the corners (BBR_VERTEX) or the
//! texture coordinates (BBR_TEXCOORD) (genVertices), its colour as Ogre packs it; and PU's
//! SphereSet spheres.

use super::math::normalise;
use super::particle::Particle;
use super::system::{Billboard, BillboardType, Origin};
use bevy::math::{Mat3, Quat, Vec2, Vec3};

/// The camera: where it is and how it is turned (world space).
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub position: Vec3,
    pub orientation: Quat,
}

/// Geometry for one draw.
#[derive(Debug, Default)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub colours: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl Geometry {
    pub fn clear(&mut self) {
        self.positions.clear();
        self.colours.clear();
        self.uvs.clear();
        self.indices.clear();
    }
}

/// The vertex colour Ogre writes (ColourValue::getAsRGBA: (uint8)(v * 255), no clamping) as the
/// shader reads it (normalised).
fn colour(c: bevy::math::Vec4) -> [f32; 4] {
    let b = |v: f32| ((v * 255.0) as i32 as u8) as f32 / 255.0;
    [b(c.x), b(c.y), b(c.z), b(c.w)]
}

fn origin_offsets(o: Origin) -> (f32, f32, f32, f32) {
    // (left, right, top, bottom)
    match o {
        Origin::TopLeft => (0.0, 1.0, 0.0, -1.0),
        Origin::TopCenter => (-0.5, 0.5, 0.0, -1.0),
        Origin::TopRight => (-1.0, 0.0, 0.0, -1.0),
        Origin::CenterLeft => (0.0, 1.0, 0.5, -0.5),
        Origin::Center => (-0.5, 0.5, 0.5, -0.5),
        Origin::CenterRight => (-1.0, 0.0, 0.5, -0.5),
        Origin::BottomLeft => (0.0, 1.0, 1.0, 0.0),
        Origin::BottomCenter => (-0.5, 0.5, 1.0, 0.0),
        Origin::BottomRight => (-1.0, 0.0, 1.0, 0.0),
    }
}

/// The texture rectangles (left, top, right, bottom): the defined ones, a grid of rows x
/// columns (setTextureStacksAndSlices), or the whole texture.
fn texture_rects(b: &Billboard) -> Vec<[f32; 4]> {
    if !b.texture_coords.is_empty() {
        return b.texture_coords.clone();
    }
    if let Some((stacks, slices)) = b.rows_columns {
        let (stacks, slices) = (stacks.max(1) as f32, slices.max(1) as f32);
        let mut v = Vec::new();
        for s in 0..stacks as u32 {
            for u in 0..slices as u32 {
                v.push([
                    u as f32 / slices,
                    s as f32 / stacks,
                    (u + 1) as f32 / slices,
                    (s + 1) as f32 / stacks,
                ]);
            }
        }
        return v;
    }
    vec![[0.0, 0.0, 1.0, 1.0]]
}

/// A technique's billboards, in `order`, into `g`; positions relative to `relative_to`.
pub fn billboards(
    b: &Billboard,
    default_size: Vec2,
    particles: &[Particle],
    order: &[u32],
    cam: &Camera,
    relative_to: Vec3,
    g: &mut Geometry,
) {
    let cam_x = cam.orientation * Vec3::X;
    let cam_y = cam.orientation * Vec3::Y;
    let view_dir = cam.orientation * Vec3::NEG_Z;
    let rects = texture_rects(b);
    let (left, right, top, bottom) = origin_offsets(b.origin);
    // Ogre's BillboardSet types (PU's oriented_shape is oriented_self with the orientation's xyz)
    let ty = if b.ty == BillboardType::OrientedShape { BillboardType::OrientedSelf } else { b.ty };
    let per_billboard = matches!(ty, BillboardType::OrientedSelf | BillboardType::PerpendicularSelf)
        || (b.accurate_facing && ty != BillboardType::PerpendicularCommon);
    let mut cam_dir = view_dir;
    // genBillboardAxes for one billboard (`dir` its direction)
    let mut axes = |pos: Vec3, dir: Vec3| -> (Vec3, Vec3) {
        if b.accurate_facing
            && matches!(ty, BillboardType::Point | BillboardType::OrientedCommon | BillboardType::OrientedSelf)
        {
            cam_dir = normalise(pos - cam.position);
        }
        match ty {
            BillboardType::Point => {
                if b.accurate_facing {
                    let y = cam_y;
                    let x = normalise(cam_dir.cross(y));
                    (x, x.cross(cam_dir))
                } else {
                    (cam_x, cam_y)
                }
            }
            BillboardType::OrientedCommon => {
                let y = b.common_direction;
                (normalise(cam_dir.cross(y)), y)
            }
            BillboardType::OrientedSelf | BillboardType::OrientedShape => {
                let y = dir;
                (normalise(cam_dir.cross(y)), y)
            }
            BillboardType::PerpendicularCommon => {
                let x = b.common_up.cross(b.common_direction);
                (x, b.common_direction.cross(x))
            }
            BillboardType::PerpendicularSelf => {
                let x = normalise(b.common_up.cross(dir));
                (x, dir.cross(x))
            }
        }
    };
    let (mut ax, mut ay) = if per_billboard { (Vec3::ZERO, Vec3::ZERO) } else { axes(Vec3::ZERO, Vec3::ZERO) };
    for &i in order {
        let p = &particles[i as usize];
        if per_billboard {
            let dir = match b.ty {
                BillboardType::OrientedShape => normalise(Vec3::new(p.orientation.x, p.orientation.y, p.orientation.z)),
                _ => normalise(p.direction),
            };
            (ax, ay) = axes(p.position, dir);
        }
        let (w, h) = if p.own_dimensions { (p.width, p.height) } else { (default_size.x, default_size.y) };
        // genVertexOffsets
        let lo = ax * (left * w);
        let ro = ax * (right * w);
        let to = ay * (top * h);
        let bo = ay * (bottom * h);
        let off = [lo + to, ro + to, lo + bo, ro + bo];
        let r = rects[(p.texture_coords_current as usize).min(rects.len() - 1)];
        let base = g.positions.len() as u32;
        let c = colour(p.colour);
        let pos = p.position - relative_to;
        let rot = p.z_rotation;
        if rot == 0.0 {
            let uv = [[r[0], r[1]], [r[2], r[1]], [r[0], r[3]], [r[2], r[3]]];
            for k in 0..4 {
                g.positions.push((pos + off[k]).to_array());
                g.uvs.push(uv[k]);
            }
        } else if b.rotate_vertices {
            let axis = normalise((off[3] - off[0]).cross(off[2] - off[1]));
            let m = Mat3::from_axis_angle(axis, rot);
            let uv = [[r[0], r[1]], [r[2], r[1]], [r[0], r[3]], [r[2], r[3]]];
            for k in 0..4 {
                g.positions.push((pos + m * off[k]).to_array());
                g.uvs.push(uv[k]);
            }
        } else {
            let (cr, sr) = (rot.cos(), rot.sin());
            let (hw, hh) = ((r[2] - r[0]) / 2.0, (r[3] - r[1]) / 2.0);
            let (mu, mv) = (r[0] + hw, r[1] + hh);
            let (crw, crh, srw, srh) = (cr * hw, cr * hh, sr * hw, sr * hh);
            let uv = [
                [mu - crw + srh, mv - srw - crh],
                [mu + crw + srh, mv + srw - crh],
                [mu - crw - srh, mv - srw + crh],
                [mu + crw - srh, mv + srw + crh],
            ];
            for k in 0..4 {
                g.positions.push((pos + off[k]).to_array());
                g.uvs.push(uv[k]);
            }
        }
        for _ in 0..4 {
            g.colours.push(c);
        }
        g.indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }
}

/// PU's SphereSet: 16 rings x 16 segments a sphere (radius half the particle's width, else the
/// default), turned by the particle's orientation; the indices as PU writes them (counted from 1
/// across the whole set, so each sphere's triangles are one vertex off; those past the end are
/// dropped).
pub fn spheres(default_radius: f32, particles: &[Particle], order: &[u32], relative_to: Vec3, g: &mut Geometry) {
    const RINGS: u32 = 16;
    const SEGMENTS: u32 = 16;
    let first = g.positions.len() as u32;
    let mut vertex_index = 1u32;
    let mut indices = Vec::new();
    for &i in order {
        let p = &particles[i as usize];
        let radius = if p.own_dimensions { 0.5 * p.width } else { default_radius };
        let c = colour(p.colour);
        let ring_step = std::f32::consts::PI / RINGS as f32;
        let seg_step = 2.0 * std::f32::consts::PI / SEGMENTS as f32;
        for ring in 0..=RINGS {
            let r0 = radius * (ring as f32 * ring_step).sin();
            let y0 = radius * (ring as f32 * ring_step).cos();
            for seg in 0..=SEGMENTS {
                let x0 = r0 * (seg as f32 * seg_step).sin();
                let z0 = r0 * (seg as f32 * seg_step).cos();
                let v = p.orientation.rotate(Vec3::new(x0, y0, z0));
                g.positions.push((v + p.position - relative_to).to_array());
                g.colours.push(c);
                g.uvs.push([seg as f32 / SEGMENTS as f32, ring as f32 / RINGS as f32]);
                if ring != RINGS {
                    indices.extend_from_slice(&[
                        vertex_index + SEGMENTS + 1,
                        vertex_index,
                        vertex_index + SEGMENTS,
                        vertex_index + SEGMENTS + 1,
                        vertex_index + 1,
                        vertex_index,
                    ]);
                }
                vertex_index += 1;
            }
        }
    }
    let count = g.positions.len() as u32 - first;
    for tri in indices.chunks(3) {
        if tri.iter().all(|&k| k < count) {
            g.indices.extend(tri.iter().map(|&k| first + k));
        }
    }
}
