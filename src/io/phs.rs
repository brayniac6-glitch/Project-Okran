//! Scythe physics files (`.phs`): the rigid bodies, their collision shapes and the joints between
//! them that Kenshi builds PhysX ragdolls from (RACE "male ragdoll" / "female ragdoll").
//!
//! Units and axes are the model's (decimetres, Y up, a character facing +Z). Poses are in the
//! model's bind space.
use std::io::{self, Read};
use std::path::Path;

/// A position and a rotation (three rows of a 3x3 matrix).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub pos: [f32; 3],
    pub rows: [[f32; 3]; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Box,
    Sphere,
    Capsule,
    Other(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// World (bind) pose of the shape.
    pub frame: Frame,
    pub kind: ShapeKind,
    /// Full extents: a box's sides; a sphere's diameter (x); a capsule's diameter (x) and
    /// length along its local Y (y).
    pub size: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub name: String,
    /// World (bind) position of the body; its rotation is the identity.
    pub pos: [f32; 3],
    /// Linear and angular damping.
    pub damping: [f32; 2],
    /// Density (mass from the shapes' volumes).
    pub density: f32,
    /// The skeleton bone the body drives, and that bone's bind frame.
    pub bone: String,
    pub bone_frame: Frame,
    pub shapes: Vec<Shape>,
    /// Model offsets: meshes shown on the body (an attachment point's example weapon), each its
    /// frame on the body, mesh file and scale.
    pub offsets: Vec<(Frame, String, [f32; 3])>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointKind {
    /// Turns about its frame's second row, between `low` and `high`.
    Hinge,
    /// Twists about its frame's first row between `low` and `high`, swings within `swing`.
    Ball,
    Other(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    /// World (bind) anchor and axes.
    pub frame: Frame,
    pub kind: JointKind,
    /// Limits in degrees.
    pub high: f32,
    pub low: f32,
    pub swing: f32,
    /// The two actors joined (indices into `Phs::actors`, -1 for none); limits measure the
    /// second against the first.
    pub actors: [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Phs {
    pub version: f32,
    pub actors: Vec<Actor>,
    pub joints: Vec<Joint>,
}

struct R<'a> {
    b: &'a [u8],
    o: usize,
}

impl R<'_> {
    fn bytes(&mut self, n: usize) -> io::Result<&[u8]> {
        let s = self
            .b
            .get(self.o..self.o + n)
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "phs truncated"))?;
        self.o += n;
        Ok(s)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn v3(&mut self) -> io::Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    fn count(&mut self) -> io::Result<usize> {
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "negative count"))
    }
    fn string(&mut self) -> io::Result<String> {
        let n = self.count()?;
        let s = self.bytes(n)?;
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        Ok(String::from_utf8_lossy(&s[..end]).into_owned())
    }
    /// Three rows, then the position.
    fn frame(&mut self) -> io::Result<Frame> {
        let rows = [self.v3()?, self.v3()?, self.v3()?];
        Ok(Frame { pos: self.v3()?, rows })
    }
    fn skip(&mut self, n: usize) -> io::Result<()> {
        self.bytes(n).map(|_| ())
    }
}

fn f(v: f64) -> f32 {
    v as f32
}

/// @fn F025940
///
/// A shape's description: its frame, type and type data (0x20 bytes).
fn shape_head(r: &mut R) -> io::Result<(Frame, i32)> {
    let frame = r.frame()?;
    let kind = r.i32()?;
    r.skip(0x20)?;
    Ok((frame, kind))
}

/// @fn F025941
///
/// A collision shape: the description, then (int, size, four floats, a name and from 1.2 a
/// second name, two flags); a convex mesh (type 9, or flagged before 1.2) carries its vertices,
/// normals and 16-bit triangles; four trailing flags.
fn shape(r: &mut R, ver: f32) -> io::Result<Shape> {
    let (frame, kind) = shape_head(r)?;
    r.i32()?;
    let size = r.v3()?;
    r.skip(16)?;
    r.string()?;
    if ver >= f(1.2) {
        r.string()?;
    }
    r.u8()?;
    let flagged = r.u8()? != 0;
    let mesh = if ver < f(1.2) { flagged } else { kind == 9 };
    if mesh {
        r.u8()?;
        let nv = r.count()?;
        let nt = r.count()?;
        r.skip(nv * 12 * 2 + nt * 6)?;
        if ver >= f(1.2) {
            r.skip(2)?;
        }
    }
    r.skip(4)?;
    let kind = match kind {
        3 => ShapeKind::Box,
        4 => ShapeKind::Sphere,
        5 => ShapeKind::Capsule,
        k => ShapeKind::Other(k),
    };
    Ok(Shape { frame, kind, size })
}

/// A model offset record: description, name, scale.
fn offset(r: &mut R) -> io::Result<(Frame, String, [f32; 3])> {
    let (frame, _) = shape_head(r)?;
    let mesh = r.string()?;
    let scale = r.v3()?;
    Ok((frame, mesh, scale))
}

/// @fn F025944
///
/// Read a `.phs` file: version (float), actors (counts of shapes, offsets and 0x7c records;
/// position; damping, density, sleep speeds, name from 1.2; from 1.4 the driven bone and its
/// frame), from 1.4 model offsets, joints (raw 0x264-byte records, 0x138 before 1.2), then
/// further lists Kenshi's ragdolls leave empty.
pub fn parse(b: &[u8]) -> io::Result<Phs> {
    let mut r = R { b, o: 0 };
    let version = r.f32()?;
    let n = r.count()?;
    let mut actors = Vec::with_capacity(n);
    for _ in 0..n {
        let shapes = r.count()?;
        let offsets = r.count()?;
        let extra = r.count()?;
        r.i32()?;
        let pos = r.v3()?;
        r.f32()?;
        r.f32()?;
        r.u8()?;
        let lin = r.f32()?;
        let ang = r.f32()?;
        r.f32()?;
        r.skip(0x20)?;
        let density = r.f32()?;
        r.skip(8 + 12 + 0x24)?;
        if version >= f(1.1) {
            r.u8()?;
        }
        let mut name = String::from("Actor");
        if version >= f(1.2) {
            name = r.string()?;
            r.skip(2)?;
        }
        if version >= f(1.3) {
            r.i32()?;
        }
        let mut bone = String::new();
        let mut bone_frame = Frame { pos, rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] };
        if version >= f(1.4) {
            r.i32()?;
            bone = r.string()?;
            let p = r.v3()?;
            bone_frame = Frame { pos: p, rows: [r.v3()?, r.v3()?, r.v3()?] };
            r.u8()?;
        }
        let mut list = Vec::with_capacity(shapes);
        for _ in 0..shapes {
            list.push(shape(&mut r, version)?);
        }
        let mut models = Vec::with_capacity(offsets);
        for _ in 0..offsets {
            models.push(offset(&mut r)?);
        }
        r.skip(extra * 0x7c)?;
        actors.push(Actor { name, pos, damping: [lin, ang], density, bone, bone_frame, shapes: list, offsets: models });
    }
    if version >= f(1.4) {
        for _ in 0..r.count()? {
            offset(&mut r)?;
        }
    }
    let nj = r.count()?;
    let size = if version >= f(1.2) { 0x264 } else { 0x138 };
    let mut joints = Vec::with_capacity(nj);
    for _ in 0..nj {
        let j = r.bytes(size)?;
        let mut q = R { b: j, o: 0 };
        let pos = q.v3()?;
        let rows = [q.v3()?, q.v3()?, q.v3()?];
        let kind = q.i32()?;
        q.o = 0x58;
        let (high, low, swing) = (q.f32()?, q.f32()?, q.f32()?);
        q.o = 0x6c;
        let a = q.i32()?;
        let b2 = q.i32()?;
        let kind = match kind {
            1 => JointKind::Hinge,
            2 => JointKind::Ball,
            k => JointKind::Other(k),
        };
        joints.push(Joint { frame: Frame { pos, rows }, kind, high, low, swing, actors: [a, b2] });
    }
    Ok(Phs { version, actors, joints })
}

pub fn read(path: &Path) -> io::Result<Phs> {
    let mut b = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut b)?;
    parse(&b)
}
