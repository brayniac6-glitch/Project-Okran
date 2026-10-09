//! Ogre binary `.skeleton` files (port_map `ogre_assets`).
//!
//! Format from sheet rows T00016..T00019 (Ogre's public, MIT-licensed format description). Parent
//! chunks (ANIMATION, ANIMATION_TRACK) are read by content like Ogre's own reader; stored lengths are
//! only used to skip unknown chunks and for Ogre's "is the optional scale present" rule.

use crate::ogre::{Chunk, Error, Reader};
use std::path::Path;

/// T00016 OgreSkeletonChunk.
pub mod chunk {
    pub const HEADER: u16 = 0x1000;
    pub const BLENDMODE: u16 = 0x1010;
    pub const BONE: u16 = 0x2000;
    pub const BONE_PARENT: u16 = 0x3000;
    pub const ANIMATION: u16 = 0x4000;
    pub const ANIMATION_BASEINFO: u16 = 0x4010;
    pub const ANIMATION_TRACK: u16 = 0x4100;
    pub const ANIMATION_TRACK_KEYFRAME: u16 = 0x4110;
    pub const ANIMATION_LINK: u16 = 0x5000;
}

const CHUNK_HEADER: usize = 6;

/// @type OgreBone
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub handle: u16,
    pub position: [f32; 3],
    /// Quaternion as stored: x, y, z, w.
    pub orientation: [f32; 4],
    pub scale: Option<[f32; 3]>,
    pub parent: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    pub time: f32,
    /// Quaternion as stored: x, y, z, w.
    pub rotation: [f32; 4],
    pub translate: [f32; 3],
    pub scale: Option<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub bone: u16,
    pub keyframes: Vec<Keyframe>,
}

/// @type OgreAnimation
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub length: f32,
    /// (base animation name, base key time) for additive animations.
    pub base: Option<(String, f32)>,
    pub tracks: Vec<Track>,
}

/// @type OgreSkeleton
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skeleton {
    pub version: String,
    pub blend_mode: Option<u16>,
    /// In file order; look up by `handle` with `bone()`.
    pub bones: Vec<Bone>,
    pub animations: Vec<Animation>,
    /// (skeleton file, scale): animations borrowed from another skeleton.
    pub links: Vec<(String, f32)>,
}

impl Skeleton {
    pub fn bone(&self, handle: u16) -> Option<&Bone> {
        self.bones.iter().find(|b| b.handle == handle)
    }
}

fn f32x4(r: &mut Reader, what: &'static str) -> Result<[f32; 4], Error> {
    Ok([r.f32(what)?, r.f32(what)?, r.f32(what)?, r.f32(what)?])
}

/// Optional trailing scale is present only if the stored chunk length exceeds the size of the
/// chunk without it.
fn has_scale(c: &Chunk, size_without_scale: usize) -> bool {
    c.len as usize > size_without_scale
}

fn bone(r: &mut Reader, c: &Chunk) -> Result<Bone, Error> {
    let name = r.line("bone name")?;
    let handle = r.u16("bone handle")?;
    let position = r.f32x3("bone position")?;
    let orientation = f32x4(r, "bone orientation")?;
    // the shipped files' writer leaves the name out of the stored length (36 = no scale, 48 = scale),
    // so compare against the size without the name
    let without_scale = CHUNK_HEADER + 2 + 12 + 16;
    let scale = if has_scale(c, without_scale) {
        Some(r.f32x3("bone scale")?)
    } else {
        None
    };
    Ok(Bone {
        name,
        handle,
        position,
        orientation,
        scale,
        parent: None,
    })
}

fn animation(r: &mut Reader) -> Result<Animation, Error> {
    let mut a = Animation {
        name: r.line("animation name")?,
        length: r.f32("animation length")?,
        base: None,
        tracks: Vec::new(),
    };
    if r.child(&[chunk::ANIMATION_BASEINFO])?.is_some() {
        a.base = Some((r.line("base animation")?, r.f32("base key time")?));
    }
    while r.child(&[chunk::ANIMATION_TRACK])?.is_some() {
        let mut t = Track {
            bone: r.u16("track bone")?,
            keyframes: Vec::new(),
        };
        while let Some(k) = r.child(&[chunk::ANIMATION_TRACK_KEYFRAME])? {
            let time = r.f32("key time")?;
            let rotation = f32x4(r, "key rotation")?;
            let translate = r.f32x3("key translate")?;
            let scale = if has_scale(&k, CHUNK_HEADER + 4 + 16 + 12) {
                Some(r.f32x3("key scale")?)
            } else {
                None
            };
            t.keyframes.push(Keyframe {
                time,
                rotation,
                translate,
                scale,
            });
        }
        a.tracks.push(t);
    }
    Ok(a)
}

/// Parse a whole `.skeleton`; every byte must belong to a chunk.
pub fn parse(buf: &[u8]) -> Result<Skeleton, Error> {
    let mut r = Reader { buf, pos: 0 };
    let id = r.u16("header")?;
    if id != chunk::HEADER {
        return Err(Error::BadHeader(id));
    }
    let mut s = Skeleton {
        version: r.line("version")?,
        ..Default::default()
    };
    while r.pos < buf.len() {
        let c = r.header()?;
        match c.id {
            chunk::BLENDMODE => s.blend_mode = Some(r.u16("blend mode")?),
            chunk::BONE => s.bones.push(bone(&mut r, &c)?),
            chunk::BONE_PARENT => {
                let child = r.u16("parent child")?;
                let parent = r.u16("parent parent")?;
                match s.bones.iter_mut().find(|b| b.handle == child) {
                    Some(b) => b.parent = Some(parent),
                    None => {
                        return Err(crate::ogre::invalid(
                            c.start,
                            format!("parent link for unknown bone {child}"),
                        ));
                    }
                }
            }
            chunk::ANIMATION => s.animations.push(animation(&mut r)?),
            chunk::ANIMATION_LINK => s
                .links
                .push((r.line("linked skeleton")?, r.f32("link scale")?)),
            _ => r.skip(&c)?,
        }
    }
    // tracks must animate bones that exist
    for a in &s.animations {
        if let Some(t) = a.tracks.iter().find(|t| s.bone(t.bone).is_none()) {
            return Err(crate::ogre::invalid(
                0,
                format!("animation '{}' tracks unknown bone {}", a.name, t.bone),
            ));
        }
    }
    Ok(s)
}

pub fn read(path: &Path) -> Result<Skeleton, Error> {
    let buf = std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    parse(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: u16, len: u32, body: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend(len.to_le_bytes());
        v.extend(body);
        v
    }
    fn floats(xs: &[f32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    /// Two bones, a parent link and a one-track animation, with bone lengths written the way the
    /// shipped files do (name not counted, so no scale).
    #[test]
    fn parses_bones_and_animation() {
        let mut f = chunk::HEADER.to_le_bytes().to_vec();
        f.extend(b"[Serializer_v1.10]\n");
        for (name, h) in [("Root", 0u16), ("Arm", 1)] {
            let mut b = format!("{name}\n").into_bytes();
            b.extend(h.to_le_bytes());
            b.extend(floats(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
            f.extend(ch(chunk::BONE, 36, &b));
        }
        let mut p = 1u16.to_le_bytes().to_vec();
        p.extend(0u16.to_le_bytes());
        f.extend(ch(chunk::BONE_PARENT, 10, &p));
        let mut key = floats(&[0.5, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0]);
        key = ch(chunk::ANIMATION_TRACK_KEYFRAME, 38, &key);
        let mut track = 1u16.to_le_bytes().to_vec();
        track.extend(key);
        let mut anim = b"wave\n".to_vec();
        anim.extend(1.5f32.to_le_bytes());
        anim.extend(ch(chunk::ANIMATION_TRACK, 8, &track)); // wrong lengths are fine
        f.extend(ch(chunk::ANIMATION, 10, &anim));
        let s = parse(&f).unwrap();
        assert_eq!(s.bones.len(), 2);
        assert_eq!(s.bone(1).unwrap().parent, Some(0));
        assert_eq!(s.bone(1).unwrap().scale, None);
        let a = &s.animations[0];
        assert_eq!((a.name.as_str(), a.length), ("wave", 1.5));
        assert_eq!(a.tracks[0].keyframes[0].translate, [1.0, 2.0, 3.0]);
    }
}
