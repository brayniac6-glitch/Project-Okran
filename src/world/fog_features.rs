//! The map's fog features (data/newland/land/fogfeatures.dat; port_map `effects`): the swamps'
//! mists, the Fog Islands' banks and the other standing fogs, read by F023589 when the world
//! loads (F046692) and drawn with fog.material's FogSphere, FogBeam and FogPlaneVolume.
//!
//! Spec: F023589 (the reader; F023576 the editor's writer): "FF01" or "FF02", the count (i32),
//! then each feature: (FF02) its name (a length byte and the bytes), its type byte (2 sphere, 3
//! cylinder, 4 planes), its colour (r, g, b, a f32), its density distance (f32, F023565: density
//! 1 / it, 1 when it is 0 or less), then by type: a sphere's centre (3 x f32) and radius; a
//! cylinder's ends (2 x 3 x f32), radius and edge distance; a plane volume's edge distance and
//! seven planes, each a normal (3 x f32) and a value v built as Ogre's Plane(normal, -v) (the
//! inside: normal . p < -v). The edge distance sets edgeBlur 1 / it (F023566).

use std::path::Path;

/// A fog feature's shape (world units).
#[derive(Debug, Clone, PartialEq)]
pub enum FogShape {
    Sphere { centre: [f32; 3], radius: f32 },
    Cylinder { start: [f32; 3], end: [f32; 3], radius: f32, edge: f32 },
    /// Seven planes (normal, w), the inside where normal . p < w.
    Planes { edge: f32, planes: [[f32; 4]; 7] },
}

/// One fog feature of the map.
#[derive(Debug, Clone, PartialEq)]
pub struct FogFeature {
    pub name: String,
    pub colour: [f32; 4],
    /// The density distance ("density" 1 / it).
    pub distance: f32,
    pub shape: FogShape,
}

/// @fn F023589
///
/// The fog features of `install` (none when the file is missing or unreadable).
pub fn load(install: &Path) -> Vec<FogFeature> {
    std::fs::read(install.join("data/newland/land/fogfeatures.dat")).map(|d| parse(&d)).unwrap_or_default()
}

/// The features in a fogfeatures.dat file's bytes (as many as read whole).
pub fn parse(d: &[u8]) -> Vec<FogFeature> {
    let mut out = Vec::new();
    let version = match d.get(..4) {
        Some(b"FF01") => 1,
        Some(b"FF02") => 2,
        _ => return out,
    };
    let mut o = 4;
    let f32_at = |o: &mut usize| -> Option<f32> {
        let v = f32::from_le_bytes(d.get(*o..*o + 4)?.try_into().ok()?);
        *o += 4;
        Some(v)
    };
    let v3 = |o: &mut usize| -> Option<[f32; 3]> { Some([f32_at(o)?, f32_at(o)?, f32_at(o)?]) };
    let Some(count) = d.get(4..8).map(|b| i32::from_le_bytes(b.try_into().unwrap())) else { return out };
    o += 4;
    for _ in 0..count.max(0) {
        let mut read = || -> Option<FogFeature> {
            let mut name = String::new();
            if version > 1 {
                let n = *d.get(o)? as usize;
                name = String::from_utf8_lossy(d.get(o + 1..o + 1 + n)?).into_owned();
                o += 1 + n;
            }
            let ty = *d.get(o)?;
            o += 1;
            let colour = [f32_at(&mut o)?, f32_at(&mut o)?, f32_at(&mut o)?, f32_at(&mut o)?];
            let distance = f32_at(&mut o)?;
            let shape = match ty {
                2 => FogShape::Sphere { centre: v3(&mut o)?, radius: f32_at(&mut o)? },
                3 => FogShape::Cylinder { start: v3(&mut o)?, end: v3(&mut o)?, radius: f32_at(&mut o)?, edge: f32_at(&mut o)? },
                4 => {
                    let edge = f32_at(&mut o)?;
                    let mut planes = [[0.0; 4]; 7];
                    for p in &mut planes {
                        let n = v3(&mut o)?;
                        let v = f32_at(&mut o)?;
                        *p = [n[0], n[1], n[2], -v];
                    }
                    FogShape::Planes { edge, planes }
                }
                _ => return None,
            };
            Some(FogFeature { name, colour, distance, shape })
        };
        match read() {
            Some(f) => out.push(f),
            None => break,
        }
    }
    out
}

/// The corners of a plane volume (the points where three planes meet inside all the rest), and
/// per plane the corners on it in order round it, facing out of the volume. The hull is made
/// of eight planes: F023581 starts the volume with eight Plane(-Y, 0) and the file sets the
/// first seven, so the eighth, the ground at y 0 (inside above it), closes it below (the shader
/// reads the seven).
pub fn plane_hull(planes7: &[[f32; 4]; 7]) -> (Vec<[f32; 3]>, Vec<Vec<usize>>) {
    let mut planes = [[0.0f32, -1.0, 0.0, 0.0]; 8];
    planes[..7].copy_from_slice(planes7);
    let planes = &planes;
    let n = |p: &[f32; 4]| [p[0] as f64, p[1] as f64, p[2] as f64];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let mut corners: Vec<[f64; 3]> = Vec::new();
    for i in 0..8 {
        for j in i + 1..8 {
            for k in j + 1..8 {
                let (a, b, c) = (n(&planes[i]), n(&planes[j]), n(&planes[k]));
                let det = dot(a, cross(b, c));
                if det.abs() < 1e-9 {
                    continue;
                }
                let (da, db, dc) = (planes[i][3] as f64, planes[j][3] as f64, planes[k][3] as f64);
                let bc = cross(b, c);
                let ca = cross(c, a);
                let ab = cross(a, b);
                let p = [0, 1, 2].map(|q| (da * bc[q] + db * ca[q] + dc * ab[q]) / det);
                let inside = planes.iter().all(|pl| dot(n(pl), p) <= pl[3] as f64 + 0.5);
                if inside && !corners.iter().any(|q| (0..3).all(|r| (q[r] - p[r]).abs() < 0.5)) {
                    corners.push(p);
                }
            }
        }
    }
    let mut faces = Vec::new();
    for pl in planes {
        let normal = n(pl);
        let on: Vec<usize> = (0..corners.len()).filter(|&c| (dot(normal, corners[c]) - pl[3] as f64).abs() < 1.0).collect();
        if on.len() < 3 {
            faces.push(Vec::new());
            continue;
        }
        let centre = on.iter().fold([0.0; 3], |m, &c| [0, 1, 2].map(|q| m[q] + corners[c][q] / on.len() as f64));
        // a frame on the plane, and the corners by their angle round its centre
        let u = {
            let d = [0, 1, 2].map(|q| corners[on[0]][q] - centre[q]);
            let l = dot(d, d).sqrt().max(1e-9);
            d.map(|v| v / l)
        };
        let v = cross(normal, u);
        let mut sorted = on.clone();
        sorted.sort_by(|&a, &b| {
            let ang = |c: usize| {
                let d = [0, 1, 2].map(|q| corners[c][q] - centre[q]);
                dot(d, v).atan2(dot(d, u))
            };
            ang(a).total_cmp(&ang(b))
        });
        faces.push(sorted);
    }
    (corners.into_iter().map(|p| p.map(|v| v as f32)).collect(), faces)
}
