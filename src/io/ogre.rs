//! Shared reading of Ogre's binary chunk streams (`.mesh`, `.skeleton`): little-endian values,
//! newline-terminated strings, `u16 id + u32 length` chunk headers.

use crate::ogre_mesh::BoneAssignment;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Truncated {
        at: usize,
        what: &'static str,
    },
    BadHeader(u16),
    /// A chunk's length runs past its parent (or the file), or is shorter than its header.
    BadChunk {
        at: usize,
        id: u16,
        len: u32,
    },
    /// Data that contradicts itself: buffer size, element offsets, index range.
    Invalid {
        at: usize,
        what: String,
    },
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { at, what } => write!(f, "truncated reading {what} at byte {at}"),
            Error::BadHeader(id) => write!(f, "not an Ogre mesh (header id {id:#x})"),
            Error::BadChunk { at, id, len } => {
                write!(f, "bad chunk {id:#x} length {len} at byte {at}")
            }
            Error::Invalid { at, what } => write!(f, "{what} (chunk at byte {at})"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) struct Reader<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

pub(crate) struct Chunk {
    pub id: u16,
    pub start: usize,
    pub len: u32,
}

impl<'a> Reader<'a> {
    pub fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or(Error::Truncated { at: self.pos, what })?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self, what: &'static str) -> Result<u8, Error> {
        Ok(self.take(1, what)?[0])
    }
    pub fn u16(&mut self, what: &'static str) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2, what)?.try_into().unwrap()))
    }
    pub fn u32(&mut self, what: &'static str) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    pub fn f32(&mut self, what: &'static str) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    pub fn f32x3(&mut self, what: &'static str) -> Result<[f32; 3], Error> {
        Ok([self.f32(what)?, self.f32(what)?, self.f32(what)?])
    }
    /// `\n`-terminated string (the terminator is consumed, not returned).
    pub fn line(&mut self, what: &'static str) -> Result<String, Error> {
        let rest = &self.buf[self.pos..];
        let n = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or(Error::Truncated { at: self.pos, what })?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.pos += n + 1;
        Ok(s)
    }

    /// Read a chunk header. The stored length is trusted only by `skip`: older serializers
    /// (v1.8, v1.41) write wrong lengths for parent chunks, so known chunks are read by content.
    pub fn header(&mut self) -> Result<Chunk, Error> {
        let start = self.pos;
        let id = self.u16("chunk id")?;
        let len = self.u32("chunk length")?;
        Ok(Chunk { id, start, len })
    }

    /// Jump past a chunk we do not decode, by its stored length.
    pub fn skip(&mut self, c: &Chunk) -> Result<(), Error> {
        let end = c.start.checked_add(c.len as usize);
        let end = end.filter(|&e| c.len >= 6 && e <= self.buf.len());
        self.pos = end.ok_or(Error::BadChunk {
            at: c.start,
            id: c.id,
            len: c.len,
        })?;
        Ok(())
    }

    /// Next child chunk if its id is one the parent can contain; otherwise `None`, consuming
    /// nothing (this is how Ogre's own reader finds the end of a parent).
    pub fn child(&mut self, allowed: &[u16]) -> Result<Option<Chunk>, Error> {
        if self.buf.len() - self.pos < 6 {
            return Ok(None);
        }
        let id = u16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        if !allowed.contains(&id) {
            return Ok(None);
        }
        self.header().map(Some)
    }

    pub fn bone_assignment(&mut self) -> Result<BoneAssignment, Error> {
        Ok(BoneAssignment {
            vertex: self.u32("bone assignment vertex")?,
            bone: self.u16("bone assignment bone")?,
            weight: self.f32("bone assignment weight")?,
        })
    }
}

pub(crate) fn invalid(at: usize, what: String) -> Error {
    Error::Invalid { at, what }
}
