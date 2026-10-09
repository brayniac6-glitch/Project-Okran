//! A Wwise Vorbis sound (RIFF "WAVE", codec 0xFFFF) rebuilt as an Ogg Vorbis stream.
//!
//! Ported from ww2ogg (Copyright (c) 2002 Xiph.org Foundation, (c) 2009-2016 Adam Gashlin; BSD,
//! data/ww2ogg_COPYING): Wwise keeps Vorbis packets without Ogg pages, a 2 or 6 byte header
//! each, the identification and comment headers left out (their fields in the "fmt" / "vorb"
//! chunk) and the setup header stripped (its codebooks by number into a shared library, its
//! fixed fields left out); with "modified packets" an audio packet's first byte lacks its
//! packet type and window flags. The stream is rebuilt: the three headers made again, each
//! audio packet on its own page.
//!
//! Kenshi's sounds (every bank): "fmt " of 0x42 bytes holding the vorb data, 2 byte packet
//! headers without granules, stripped setup, external codebooks.

use std::fmt;

/// ww2ogg's packed codebooks (aoTuV 6.03), the library Wwise's codebook numbers index.
const CODEBOOKS: &[u8] = include_bytes!("data/packed_codebooks_aoTuV_603.bin");

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err<T>(s: impl Into<String>) -> Result<T, Error> {
    Err(Error(s.into()))
}

/// Bits read least significant first.
struct BitReader<'a> {
    d: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(d: &'a [u8]) -> Self {
        BitReader { d, bit: 0 }
    }

    fn read(&mut self, n: u32) -> Result<u32, Error> {
        let mut v = 0u32;
        for i in 0..n {
            let byte = *self.d.get(self.bit / 8).ok_or(Error("out of bits".into()))?;
            if byte & (1 << (self.bit % 8)) != 0 {
                v |= 1 << i;
            }
            self.bit += 1;
        }
        Ok(v)
    }

    fn total(&self) -> usize {
        self.bit
    }
}

/// An Ogg stream written a packet at a time (bits least significant first), each packet on
/// its own page (ww2ogg's Bit_oggstream).
struct OggWriter {
    out: Vec<u8>,
    payload: Vec<u8>,
    byte: u8,
    bits: u32,
    first: bool,
    continued: bool,
    granule: u64,
    seq: u32,
}

const SEGMENT: usize = 255;
const MAX_SEGMENTS: usize = 255;

impl OggWriter {
    fn new() -> Self {
        OggWriter { out: Vec::new(), payload: Vec::new(), byte: 0, bits: 0, first: true, continued: false, granule: 0, seq: 0 }
    }

    fn put_bit(&mut self, b: bool) {
        if b {
            self.byte |= 1 << self.bits;
        }
        self.bits += 1;
        if self.bits == 8 {
            self.flush_bits();
        }
    }

    fn write(&mut self, v: u32, n: u32) {
        for i in 0..n {
            self.put_bit(v & (1 << i) != 0);
        }
    }

    fn flush_bits(&mut self) {
        if self.bits != 0 {
            self.payload.push(self.byte);
            self.byte = 0;
            self.bits = 0;
        }
    }

    /// The packet so far onto pages (a packet longer than one page goes on as continued pages).
    fn flush_page(&mut self, last: bool) {
        self.flush_bits();
        let payload = std::mem::take(&mut self.payload);
        let mut chunks: Vec<&[u8]> = payload.chunks(SEGMENT * MAX_SEGMENTS).collect();
        if chunks.is_empty() {
            return;
        }
        let n = chunks.len();
        for (k, chunk) in chunks.drain(..).enumerate() {
            let more = k + 1 < n;
            // lacing: full segments, then the remainder (a 0 when the packet ends on a full one,
            // unless the page is full and the packet goes on)
            let mut lacing = vec![SEGMENT as u8; chunk.len() / SEGMENT];
            if !(more && lacing.len() == MAX_SEGMENTS) {
                lacing.push((chunk.len() % SEGMENT) as u8);
            }
            let mut page = Vec::with_capacity(27 + lacing.len() + chunk.len());
            page.extend_from_slice(b"OggS");
            page.push(0);
            page.push((self.continued as u8) | ((self.first as u8) << 1) | (((last && !more) as u8) << 2));
            let granule = if more { u64::MAX } else { self.granule };
            page.extend_from_slice(&granule.to_le_bytes());
            page.extend_from_slice(&1u32.to_le_bytes());
            page.extend_from_slice(&self.seq.to_le_bytes());
            page.extend_from_slice(&0u32.to_le_bytes());
            page.push(lacing.len() as u8);
            page.extend_from_slice(&lacing);
            page.extend_from_slice(chunk);
            let crc = ogg_crc(&page);
            page[22..26].copy_from_slice(&crc.to_le_bytes());
            self.out.extend_from_slice(&page);
            self.seq += 1;
            self.first = false;
            self.continued = more;
        }
    }

    fn vorbis_header(&mut self, kind: u32) {
        self.write(kind, 8);
        for c in b"vorbis" {
            self.write(*c as u32, 8);
        }
    }
}

/// The Ogg page checksum (CRC-32, polynomial 0x04c11db7, not reflected).
fn ogg_crc(d: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { (r << 1) ^ 0x04c1_1db7 } else { r << 1 };
            }
            *e = r;
        }
        t
    });
    let mut crc = 0u32;
    for &b in d {
        crc = (crc << 8) ^ t[((crc >> 24) as u8 ^ b) as usize];
    }
    crc
}

fn ilog(mut v: u32) -> u32 {
    let mut r = 0;
    while v != 0 {
        r += 1;
        v >>= 1;
    }
    r
}

fn quantvals(entries: u32, dimensions: u32) -> u32 {
    let bits = ilog(entries) as i64;
    let mut vals = (entries as i64) >> ((bits - 1) * (dimensions as i64 - 1) / dimensions as i64);
    loop {
        let (mut acc, mut acc1) = (1i64, 1i64);
        for _ in 0..dimensions {
            acc *= vals;
            acc1 *= vals + 1;
        }
        if acc <= entries as i64 && acc1 > entries as i64 {
            return vals as u32;
        }
        if acc > entries as i64 {
            vals -= 1;
        } else {
            vals += 1;
        }
    }
}

/// Codebook `i` of the library, rebuilt into the setup packet.
fn rebuild_codebook(i: u32, os: &mut OggWriter) -> Result<(), Error> {
    let lib = CODEBOOKS;
    let n = lib.len();
    let table = u32::from_le_bytes(lib[n - 4..].try_into().unwrap()) as usize;
    let count = (n - table) / 4;
    let off = |k: usize| u32::from_le_bytes(lib[table + 4 * k..table + 4 * k + 4].try_into().unwrap()) as usize;
    let i = i as usize;
    if i + 1 >= count {
        return err(format!("invalid codebook id {i}"));
    }
    let (a, b) = (off(i), off(i + 1));
    let mut bis = BitReader::new(&lib[a..b]);
    rebuild_stripped(&mut bis, os)
}

/// A stripped codebook (4 bit dimensions, 14 bit entries, short lengths) as a Vorbis one.
fn rebuild_stripped(bis: &mut BitReader, os: &mut OggWriter) -> Result<(), Error> {
    let dimensions = bis.read(4)?;
    let entries = bis.read(14)?;
    os.write(0x564342, 24);
    os.write(dimensions, 16);
    os.write(entries, 24);
    let ordered = bis.read(1)?;
    os.write(ordered, 1);
    if ordered != 0 {
        let initial = bis.read(5)?;
        os.write(initial, 5);
        let mut current = 0;
        while current < entries {
            let bits = ilog(entries - current);
            let number = bis.read(bits)?;
            os.write(number, bits);
            current += number;
        }
        if current > entries {
            return err("codebook entry out of range");
        }
    } else {
        let length_bits = bis.read(3)?;
        let sparse = bis.read(1)?;
        if length_bits == 0 || length_bits > 5 {
            return err("nonsense codeword length");
        }
        os.write(sparse, 1);
        for _ in 0..entries {
            let present = if sparse != 0 {
                let p = bis.read(1)?;
                os.write(p, 1);
                p != 0
            } else {
                true
            };
            if present {
                let len = bis.read(length_bits)?;
                os.write(len, 5);
            }
        }
    }
    let lookup = bis.read(1)?;
    os.write(lookup, 4);
    if lookup == 1 {
        let min = bis.read(32)?;
        let max = bis.read(32)?;
        let value_length = bis.read(4)?;
        let sequence = bis.read(1)?;
        os.write(min, 32);
        os.write(max, 32);
        os.write(value_length, 4);
        os.write(sequence, 1);
        for _ in 0..quantvals(entries, dimensions) {
            let v = bis.read(value_length + 1)?;
            os.write(v, value_length + 1);
        }
    }
    Ok(())
}

fn u16le(d: &[u8], o: usize) -> Result<u16, Error> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or(Error("truncated".into()))
}

fn u32le(d: &[u8], o: usize) -> Result<u32, Error> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(Error("truncated".into()))
}

/// A sound's format: channels, sample rate and length (samples).
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: u32,
    /// The sound's loop (start, end samples), if it has one.
    pub looped: Option<(u32, u32)>,
}

/// The sound's format (its "fmt " / "smpl" chunks).
pub fn info(wem: &[u8]) -> Result<Info, Error> {
    let r = Riff::parse(wem)?;
    Ok(r.info)
}

struct Riff {
    info: Info,
    avg_bytes: u32,
    data: (usize, usize),
    setup: usize,
    first_audio: usize,
    mod_packets: bool,
    no_granule: bool,
    blocksize: (u8, u8),
}

impl Riff {
    fn parse(d: &[u8]) -> Result<Riff, Error> {
        if d.get(0..4) != Some(b"RIFF") || d.get(8..12) != Some(b"WAVE") {
            return err("not a RIFF WAVE");
        }
        let riff_size = (u32le(d, 4)? as usize + 8).min(d.len());
        let (mut fmt, mut data, mut vorb, mut smpl) = (None, None, None, None);
        let mut o = 12;
        while o + 8 <= riff_size {
            let size = u32le(d, o + 4)? as usize;
            let chunk = (o + 8, size);
            match &d[o..o + 4] {
                b"fmt " => fmt = Some(chunk),
                b"data" => data = Some(chunk),
                b"vorb" => vorb = Some(chunk),
                b"smpl" => smpl = Some(chunk),
                _ => {}
            }
            o += 8 + size;
        }
        let (fo, fs) = fmt.ok_or(Error("no fmt".into()))?;
        let data = data.ok_or(Error("no data".into()))?;
        if u16le(d, fo)? != 0xFFFF {
            return err("not Wwise Vorbis");
        }
        let channels = u16le(d, fo + 2)?;
        let sample_rate = u32le(d, fo + 4)?;
        let avg_bytes = u32le(d, fo + 8)?;
        // the vorb data: its own chunk, else inside a 0x42 fmt
        let (vo, vs) = match vorb {
            Some(v) => v,
            None if fs == 0x42 => (fo + 0x18, 0x2A),
            None => return err("no vorb data"),
        };
        let samples = u32le(d, vo)?;
        let (no_granule, mod_packets, offsets_at, uid_at) = match vs {
            0x2A => {
                let signal = u32le(d, vo + 4)?;
                (true, !matches!(signal, 0x4A | 0x4B | 0x69 | 0x70), vo + 0x10, vo + 0x24)
            }
            0x32 | 0x34 => (false, false, vo + 0x18, vo + 0x2C),
            _ => return err(format!("unsupported vorb size {vs:#x}")),
        };
        let setup = u32le(d, offsets_at)? as usize;
        let first_audio = u32le(d, offsets_at + 4)? as usize;
        let blocksize = (*d.get(uid_at + 4).ok_or(Error("truncated".into()))?, *d.get(uid_at + 5).ok_or(Error("truncated".into()))?);
        let looped = match smpl {
            Some((so, _)) if u32le(d, so + 0x1C)? == 1 => {
                let start = u32le(d, so + 0x2C)?;
                let end = u32le(d, so + 0x30)?;
                Some((start, if end == 0 { samples } else { end + 1 }))
            }
            _ => None,
        };
        Ok(Riff {
            info: Info { channels, sample_rate, samples, looped },
            avg_bytes,
            data,
            setup,
            first_audio,
            mod_packets,
            no_granule,
            blocksize,
        })
    }
}

/// The sound as an Ogg Vorbis stream.
pub fn to_ogg(d: &[u8]) -> Result<Vec<u8>, Error> {
    let r = Riff::parse(d)?;
    let (data, data_size) = r.data;
    let data_end = (data + data_size).min(d.len());
    let header = if r.no_granule { 2 } else { 6 };
    let packet = |o: usize| -> Result<(usize, usize, u32), Error> {
        let size = u16le(d, o)? as usize;
        let granule = if r.no_granule { 0 } else { u32le(d, o + 2)? };
        Ok((o + header, size, granule))
    };
    let mut os = OggWriter::new();
    let channels = r.info.channels as u32;
    // identification
    os.vorbis_header(1);
    os.write(0, 32);
    os.write(channels, 8);
    os.write(r.info.sample_rate, 32);
    os.write(0, 32);
    os.write(r.avg_bytes * 8, 32);
    os.write(0, 32);
    os.write(r.blocksize.0 as u32, 4);
    os.write(r.blocksize.1 as u32, 4);
    os.write(1, 1);
    os.flush_page(false);
    // comment
    os.vorbis_header(3);
    let vendor = b"converted from Audiokinetic Wwise (ww2ogg's method)";
    os.write(vendor.len() as u32, 32);
    for c in vendor {
        os.write(*c as u32, 8);
    }
    match r.info.looped {
        None => os.write(0, 32),
        Some((a, b)) => {
            os.write(2, 32);
            for s in [format!("LoopStart={a}"), format!("LoopEnd={b}")] {
                os.write(s.len() as u32, 32);
                for c in s.bytes() {
                    os.write(c as u32, 8);
                }
            }
        }
    }
    os.write(1, 1);
    os.flush_page(false);
    // setup
    os.vorbis_header(5);
    let (so, ssize, sgranule) = packet(data + r.setup)?;
    if sgranule != 0 {
        return err("setup packet granule != 0");
    }
    let setup_bytes = d.get(so..so + ssize).ok_or(Error("setup truncated".into()))?;
    let mut ss = BitReader::new(setup_bytes);
    let codebooks = ss.read(8)? + 1;
    os.write(codebooks - 1, 8);
    for _ in 0..codebooks {
        let id = ss.read(10)?;
        rebuild_codebook(id, &mut os)?;
    }
    // time domain transforms (placeholder)
    os.write(0, 6);
    os.write(0, 16);
    // floors
    let floors = ss.read(6)? + 1;
    os.write(floors - 1, 6);
    for _ in 0..floors {
        os.write(1, 16);
        let partitions = ss.read(5)?;
        os.write(partitions, 5);
        let mut classes = Vec::new();
        let mut max_class = 0;
        for _ in 0..partitions {
            let c = ss.read(4)?;
            os.write(c, 4);
            classes.push(c);
            max_class = max_class.max(c);
        }
        let mut dims = Vec::new();
        for _ in 0..=max_class {
            let dl = ss.read(3)?;
            os.write(dl, 3);
            dims.push(dl + 1);
            let sub = ss.read(2)?;
            os.write(sub, 2);
            if sub != 0 {
                let master = ss.read(8)?;
                os.write(master, 8);
                if master >= codebooks {
                    return err("invalid floor1 masterbook");
                }
            }
            for _ in 0..(1 << sub) {
                let b = ss.read(8)?;
                os.write(b, 8);
                if b > 0 && b - 1 >= codebooks {
                    return err("invalid floor1 subclass book");
                }
            }
        }
        let mult = ss.read(2)?;
        os.write(mult, 2);
        let rangebits = ss.read(4)?;
        os.write(rangebits, 4);
        for &c in &classes {
            for _ in 0..dims[c as usize] {
                let x = ss.read(rangebits)?;
                os.write(x, rangebits);
            }
        }
    }
    // residues
    let residues = ss.read(6)? + 1;
    os.write(residues - 1, 6);
    for _ in 0..residues {
        let kind = ss.read(2)?;
        os.write(kind, 16);
        if kind > 2 {
            return err("invalid residue type");
        }
        let begin = ss.read(24)?;
        let end = ss.read(24)?;
        let psize = ss.read(24)?;
        let classifications = ss.read(6)? + 1;
        let classbook = ss.read(8)?;
        os.write(begin, 24);
        os.write(end, 24);
        os.write(psize, 24);
        os.write(classifications - 1, 6);
        os.write(classbook, 8);
        if classbook >= codebooks {
            return err("invalid residue classbook");
        }
        let mut cascade = Vec::new();
        for _ in 0..classifications {
            let low = ss.read(3)?;
            os.write(low, 3);
            let flag = ss.read(1)?;
            os.write(flag, 1);
            let high = if flag != 0 {
                let h = ss.read(5)?;
                os.write(h, 5);
                h
            } else {
                0
            };
            cascade.push(high * 8 + low);
        }
        for c in cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 {
                    let b = ss.read(8)?;
                    os.write(b, 8);
                    if b >= codebooks {
                        return err("invalid residue book");
                    }
                }
            }
        }
    }
    // mappings
    let mappings = ss.read(6)? + 1;
    os.write(mappings - 1, 6);
    for _ in 0..mappings {
        os.write(0, 16);
        let submaps_flag = ss.read(1)?;
        os.write(submaps_flag, 1);
        let submaps = if submaps_flag != 0 {
            let s = ss.read(4)?;
            os.write(s, 4);
            s + 1
        } else {
            1
        };
        let polar = ss.read(1)?;
        os.write(polar, 1);
        if polar != 0 {
            let steps = ss.read(8)? + 1;
            os.write(steps - 1, 8);
            let bits = ilog(channels - 1);
            for _ in 0..steps {
                let m = ss.read(bits)?;
                let a = ss.read(bits)?;
                os.write(m, bits);
                os.write(a, bits);
                if a == m || m >= channels || a >= channels {
                    return err("invalid coupling");
                }
            }
        }
        let reserved = ss.read(2)?;
        os.write(reserved, 2);
        if reserved != 0 {
            return err("mapping reserved field nonzero");
        }
        if submaps > 1 {
            for _ in 0..channels {
                let mux = ss.read(4)?;
                os.write(mux, 4);
                if mux >= submaps {
                    return err("mapping_mux >= submaps");
                }
            }
        }
        for _ in 0..submaps {
            let time = ss.read(8)?;
            os.write(time, 8);
            let floor = ss.read(8)?;
            os.write(floor, 8);
            if floor >= floors {
                return err("invalid floor mapping");
            }
            let residue = ss.read(8)?;
            os.write(residue, 8);
            if residue >= residues {
                return err("invalid residue mapping");
            }
        }
    }
    // modes
    let modes = ss.read(6)? + 1;
    os.write(modes - 1, 6);
    let mode_bits = ilog(modes - 1);
    let mut blockflag = Vec::new();
    for _ in 0..modes {
        let f = ss.read(1)?;
        os.write(f, 1);
        blockflag.push(f != 0);
        os.write(0, 16);
        os.write(0, 16);
        let m = ss.read(8)?;
        os.write(m, 8);
        if m >= mappings {
            return err("invalid mode mapping");
        }
    }
    os.write(1, 1);
    os.flush_page(false);
    if ss.total().div_ceil(8) != ssize {
        return err("didn't read exactly the setup packet");
    }
    if so + ssize != data + r.first_audio {
        return err("first audio packet doesn't follow setup packet");
    }
    // audio packets
    let mut o = data + r.first_audio;
    let mut prev_long = false;
    // (the Wwise packets carry no granules here: each page's is the samples decoded so far,
    // from the windows; ww2ogg leaves them 0)
    let (short, long) = (1u64 << r.blocksize.0, 1u64 << r.blocksize.1);
    let mut granule = 0u64;
    let mut last_block: Option<u64> = None;
    while o < data_end {
        let (po, size, g) = packet(o)?;
        let next = po + size;
        if next > data_end {
            return err("packet truncated");
        }
        let bytes = &d[po..next];
        let mut this_long = false;
        if r.mod_packets {
            os.write(0, 1);
            let mut bs = BitReader::new(bytes);
            let mode = bs.read(mode_bits)?;
            os.write(mode, mode_bits);
            let rest = bs.read(8 - mode_bits)?;
            this_long = *blockflag.get(mode as usize).ok_or(Error("bad mode".into()))?;
            if this_long {
                // the next packet's window
                let mut next_long = false;
                if next + header <= data_end {
                    let (no, nsize, _) = packet(next)?;
                    if nsize > 0
                        && let Some(nb) = d.get(no..no + nsize)
                    {
                        let nm = BitReader::new(nb).read(mode_bits)?;
                        next_long = *blockflag.get(nm as usize).unwrap_or(&false);
                    }
                }
                os.write(prev_long as u32, 1);
                os.write(next_long as u32, 1);
            }
            prev_long = this_long;
            os.write(rest, 8 - mode_bits);
            for &b in &bytes[1..] {
                os.write(b as u32, 8);
            }
        } else {
            // the mode after the packet type bit
            if let Ok(m) = BitReader::new(bytes).read(1 + mode_bits) {
                this_long = *blockflag.get((m >> 1) as usize).unwrap_or(&false);
            }
            for &b in bytes {
                os.write(b as u32, 8);
            }
        }
        if !r.no_granule {
            granule = g as u64;
        } else {
            let block = if this_long { long } else { short };
            if let Some(prev) = last_block {
                granule += prev / 4 + block / 4;
            }
            last_block = Some(block);
        }
        o = next;
        // the last page ends the stream at its length
        os.granule = if o >= data_end { r.info.samples as u64 } else { granule.min(r.info.samples as u64) };
        os.flush_page(o >= data_end);
    }
    Ok(os.out)
}
