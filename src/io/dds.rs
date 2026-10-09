//! DirectDraw Surface textures (public format): header parsing and CPU decoding of the top mip level
//! for DXT1/DXT3/DXT5 (BC1/2/3) and uncompressed 32-bit RGBA/BGRA. Used where a texture's channels
//! must be repacked before rendering (e.g. Kenshi's hair "Mod" maps); plain textures go to the GPU
//! compressed through the renderer's own loader.

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    NotDds,
    Truncated,
    Unsupported(String),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotDds => write!(f, "not a DDS file"),
            Error::Truncated => write!(f, "truncated DDS data"),
            Error::Unsupported(s) => write!(f, "unsupported DDS format {s}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Top mip level as RGBA8, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn rgb565(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [
        (r * 255 / 31) as u8,
        (g * 255 / 63) as u8,
        (b * 255 / 31) as u8,
    ]
}

/// BC1 colour block -> 16 RGB(A) texels. `four_colour` forces 4-colour mode (BC2/BC3).
fn colour_block(b: &[u8], four_colour: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u16, wb: u16| ((a as u16 * wa + b as u16 * wb) / (wa + wb)) as u8;
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if four_colour || c0 > c1 {
        pal[2] = [
            mix(p0[0], p1[0], 2, 1),
            mix(p0[1], p1[1], 2, 1),
            mix(p0[2], p1[2], 2, 1),
            255,
        ];
        pal[3] = [
            mix(p0[0], p1[0], 1, 2),
            mix(p0[1], p1[1], 1, 2),
            mix(p0[2], p1[2], 1, 2),
            255,
        ];
    } else {
        pal[2] = [
            mix(p0[0], p1[0], 1, 1),
            mix(p0[1], p1[1], 1, 1),
            mix(p0[2], p1[2], 1, 1),
            255,
        ];
        pal[3] = [0, 0, 0, 0];
    }
    let idx = u32_at(b, 4);
    std::array::from_fn(|i| pal[(idx >> (2 * i) & 3) as usize])
}

/// BC3 interpolated alpha block -> 16 alphas.
fn bc3_alpha(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (b[0] as u16, b[1] as u16);
    let mut pal = [0u8; 8];
    pal[0] = a0 as u8;
    pal[1] = a1 as u8;
    if a0 > a1 {
        for i in 0..6 {
            pal[i + 2] = (((6 - i as u16) * a0 + (i as u16 + 1) * a1) / 7) as u8;
        }
    } else {
        for i in 0..4 {
            pal[i + 2] = (((4 - i as u16) * a0 + (i as u16 + 1) * a1) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits = 0u64;
    for (k, &x) in b[2..8].iter().enumerate() {
        bits |= (x as u64) << (8 * k);
    }
    std::array::from_fn(|i| pal[(bits >> (3 * i) & 7) as usize])
}

/// Decode the top mip level.
pub fn decode(buf: &[u8]) -> Result<Rgba, Error> {
    if buf.len() < 128 || &buf[0..4] != b"DDS " {
        return Err(Error::NotDds);
    }
    let (height, width) = (u32_at(buf, 12), u32_at(buf, 16));
    let pf_flags = u32_at(buf, 80);
    let fourcc = &buf[84..88];
    let data = &buf[128..];
    let mut pixels = vec![[0u8; 4]; (width * height) as usize];
    let (w, h) = (width as usize, height as usize);
    if pf_flags & 0x4 != 0 {
        let block_size = match fourcc {
            b"DXT1" => 8,
            b"DXT3" | b"DXT5" => 16,
            other => {
                return Err(Error::Unsupported(
                    String::from_utf8_lossy(other).into_owned(),
                ));
            }
        };
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        if data.len() < bw * bh * block_size {
            return Err(Error::Truncated);
        }
        for by in 0..bh {
            for bx in 0..bw {
                let blk = &data[(by * bw + bx) * block_size..][..block_size];
                let texels = match fourcc {
                    b"DXT1" => colour_block(blk, false),
                    b"DXT3" => {
                        let mut t = colour_block(&blk[8..], true);
                        for (i, px) in t.iter_mut().enumerate() {
                            px[3] = ((blk[i / 2] >> (4 * (i % 2))) & 15) * 17;
                        }
                        t
                    }
                    _ => {
                        let a = bc3_alpha(blk);
                        let mut t = colour_block(&blk[8..], true);
                        for (px, a) in t.iter_mut().zip(a) {
                            px[3] = a;
                        }
                        t
                    }
                };
                for (i, px) in texels.into_iter().enumerate() {
                    let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                    if x < w && y < h {
                        pixels[y * w + x] = px;
                    }
                }
            }
        }
    } else if pf_flags & 0x40 != 0 && u32_at(buf, 88) == 32 {
        // uncompressed 32-bit, channel order from the masks
        let masks = [
            u32_at(buf, 92),
            u32_at(buf, 96),
            u32_at(buf, 100),
            u32_at(buf, 104),
        ];
        if data.len() < w * h * 4 {
            return Err(Error::Truncated);
        }
        for (px, raw) in pixels.iter_mut().zip(data.as_chunks::<4>().0) {
            let v = u32::from_le_bytes(*raw);
            for (c, m) in masks.iter().enumerate() {
                px[c] = if *m == 0 {
                    255
                } else {
                    ((v & m) >> m.trailing_zeros()) as u8
                };
            }
        }
    } else {
        return Err(Error::Unsupported(format!(
            "pixel format flags {pf_flags:#x}"
        )));
    }
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

pub fn read(path: &Path) -> Result<Rgba, Error> {
    decode(&std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(w: u32, h: u32, fourcc: &[u8; 4]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[0..4].copy_from_slice(b"DDS ");
        b[12..16].copy_from_slice(&h.to_le_bytes());
        b[16..20].copy_from_slice(&w.to_le_bytes());
        b[80..84].copy_from_slice(&4u32.to_le_bytes());
        b[84..88].copy_from_slice(fourcc);
        b
    }

    #[test]
    fn dxt1_solid_colours() {
        let mut b = header(4, 4, b"DXT1");
        // c0 = pure red, c1 = pure blue, all indices 0 except texel 1 = 1
        b.extend(0xf800u16.to_le_bytes());
        b.extend(0x001fu16.to_le_bytes());
        b.extend(0b0100u32.to_le_bytes());
        let img = decode(&b).unwrap();
        assert_eq!(img.pixels[0], [255, 0, 0, 255]);
        assert_eq!(img.pixels[1], [0, 0, 255, 255]);
    }

    #[test]
    fn dxt3_explicit_alpha_and_dxt5_interpolated_alpha() {
        let mut b = header(4, 4, b"DXT3");
        b.extend([0x0f, 0, 0, 0, 0, 0, 0, 0xf0]); // texel 0 alpha 15, texel 15 alpha 15
        b.extend([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]); // white
        let img = decode(&b).unwrap();
        assert_eq!(
            (img.pixels[0][3], img.pixels[1][3], img.pixels[15][3]),
            (255, 0, 255)
        );

        let mut b = header(4, 4, b"DXT5");
        b.extend([200, 100, 0b001, 0, 0, 0, 0, 0]); // texel 0 index 1 -> 100, rest index 0 -> 200
        b.extend([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
        let img = decode(&b).unwrap();
        assert_eq!((img.pixels[0][3], img.pixels[1][3]), (100, 200));
    }
}
