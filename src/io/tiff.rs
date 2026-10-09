//! Uncompressed greyscale TIFF (public format), as used for Kenshi's terrain heightmap
//! (`data/newland/land/fullmap.tif`, sheet row T00022). Only what that file needs: little-endian,
//! one sample per pixel, 8 or 16 bits, no compression, strips. Pixels are read by seeking, so a
//! window of a 537 MB map costs only that window.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NotTiff,
    Unsupported(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::NotTiff => write!(f, "not a little-endian TIFF"),
            Error::Unsupported(s) => write!(f, "unsupported TIFF: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub struct Tiff {
    file: File,
    pub width: u32,
    pub height: u32,
    pub bits: u16,
    /// Byte offset of each strip and rows per strip.
    strips: Vec<u64>,
    rows_per_strip: u32,
}

const WIDTH: u16 = 256;
const HEIGHT: u16 = 257;
const BITS: u16 = 258;
const COMPRESSION: u16 = 259;
const STRIP_OFFSETS: u16 = 273;
const SAMPLES: u16 = 277;
const ROWS_PER_STRIP: u16 = 278;

impl Tiff {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let mut file = File::open(path)?;
        let mut head = [0u8; 8];
        file.read_exact(&mut head)?;
        if &head[0..4] != b"II*\0" {
            return Err(Error::NotTiff);
        }
        let ifd = u32::from_le_bytes(head[4..8].try_into().unwrap()) as u64;
        file.seek(SeekFrom::Start(ifd))?;
        let mut n = [0u8; 2];
        file.read_exact(&mut n)?;
        let mut entries = vec![0u8; u16::from_le_bytes(n) as usize * 12];
        file.read_exact(&mut entries)?;

        let (mut width, mut height, mut bits, mut compression, mut samples) = (0, 0, 0, 1, 1);
        let mut rows_per_strip = u32::MAX;
        let mut strips_at = None;
        for e in entries.as_chunks::<12>().0 {
            let tag = u16::from_le_bytes([e[0], e[1]]);
            let typ = u16::from_le_bytes([e[2], e[3]]);
            let count = u32::from_le_bytes(e[4..8].try_into().unwrap());
            let raw = u32::from_le_bytes(e[8..12].try_into().unwrap());
            // SHORT values sit in the low half of the value field
            let val = if typ == 3 { raw & 0xffff } else { raw };
            match tag {
                WIDTH => width = val,
                HEIGHT => height = val,
                BITS => bits = val as u16,
                COMPRESSION => compression = val,
                SAMPLES => samples = val,
                ROWS_PER_STRIP => rows_per_strip = val,
                STRIP_OFFSETS => strips_at = Some((typ, count, raw)),
                _ => {}
            }
        }
        if compression != 1 || samples != 1 || !(bits == 8 || bits == 16) {
            return Err(Error::Unsupported(format!(
                "compression {compression}, {samples} samples, {bits} bits"
            )));
        }
        let (typ, count, raw) = strips_at.ok_or_else(|| Error::Unsupported("no strips".into()))?;
        let strips = if count == 1 {
            vec![raw as u64]
        } else {
            // the field holds the offset of an array of LONG (4) or SHORT (3) values
            let size = if typ == 3 { 2 } else { 4 };
            let mut arr = vec![0u8; count as usize * size];
            file.seek(SeekFrom::Start(raw as u64))?;
            file.read_exact(&mut arr)?;
            arr.chunks_exact(size)
                .map(|c| {
                    if size == 2 {
                        u16::from_le_bytes([c[0], c[1]]) as u64
                    } else {
                        u32::from_le_bytes(c.try_into().unwrap()) as u64
                    }
                })
                .collect()
        };
        Ok(Tiff {
            file,
            width,
            height,
            bits,
            strips,
            rows_per_strip: rows_per_strip.min(height.max(1)),
        })
    }

    fn row_offset(&self, y: u32) -> u64 {
        let strip = (y / self.rows_per_strip) as usize;
        let row_bytes = self.width as u64 * (self.bits as u64 / 8);
        self.strips[strip] + (y % self.rows_per_strip) as u64 * row_bytes
    }

    /// Pixels of the window starting at (x, y), `w` x `h` samples taken every `step` pixels
    /// (row-major, clamped to the image). 8-bit values are widened to 16 bits (v * 257).
    pub fn window(&mut self, x: u32, y: u32, w: u32, h: u32, step: u32) -> Result<Vec<u16>, Error> {
        let step = step.max(1);
        let bpp = (self.bits / 8) as u64;
        let mut out = Vec::with_capacity((w * h) as usize);
        let span = ((w.saturating_sub(1)) * step + 1).min(self.width.saturating_sub(x));
        let mut row = vec![0u8; span as usize * bpp as usize];
        for j in 0..h {
            let yy = (y + j * step).min(self.height - 1);
            self.file
                .seek(SeekFrom::Start(self.row_offset(yy) + x as u64 * bpp))?;
            self.file.read_exact(&mut row)?;
            for i in 0..w {
                let xx = ((i * step) as usize).min(span as usize - 1);
                out.push(if bpp == 2 {
                    u16::from_le_bytes([row[xx * 2], row[xx * 2 + 1]])
                } else {
                    row[xx] as u16 * 257
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A 4x3 16-bit image whose pixel (x, y) holds 100*y + x.
    fn write_test_tiff(path: &Path) {
        let (w, h) = (4u32, 3u32);
        let data_at = 8u32;
        let pixels: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| ((100 * y + x) as u16).to_le_bytes()))
            .collect();
        let ifd_at = data_at + pixels.len() as u32;
        let mut b = Vec::new();
        b.extend(b"II*\0");
        b.extend(ifd_at.to_le_bytes());
        b.extend(&pixels);
        let tags: [(u16, u16, u32); 6] = [
            (WIDTH, 3, w),
            (HEIGHT, 3, h),
            (BITS, 3, 16),
            (COMPRESSION, 3, 1),
            (STRIP_OFFSETS, 4, data_at),
            (ROWS_PER_STRIP, 3, h),
        ];
        b.extend((tags.len() as u16).to_le_bytes());
        for (tag, typ, val) in tags {
            b.extend(tag.to_le_bytes());
            b.extend(typ.to_le_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend(val.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
        std::fs::File::create(path).unwrap().write_all(&b).unwrap();
    }

    #[test]
    fn windows_and_steps() {
        let p = std::env::temp_dir().join(format!("kenshi-tiff-{}.tif", std::process::id()));
        write_test_tiff(&p);
        let mut t = Tiff::open(&p).unwrap();
        assert_eq!((t.width, t.height, t.bits), (4, 3, 16));
        assert_eq!(t.window(1, 1, 2, 2, 1).unwrap(), [101, 102, 201, 202]);
        assert_eq!(t.window(0, 0, 2, 2, 2).unwrap(), [0, 2, 200, 202]);
        std::fs::remove_file(&p).unwrap();
    }
}
