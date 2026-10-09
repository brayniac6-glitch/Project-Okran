//! A Wwise sound bank (data/audio/*.bnk): its chunks ("BKHD" header and version, "DIDX" the
//! index of the sounds it holds, "DATA" those sounds, "HIRC" its hierarchy objects, "STID" the
//! names of banks it refers to).

use std::collections::HashMap;
use std::ops::Range;

/// A hierarchy object: its kind (2 sound, 3 action, 4 event, 5 random / sequence container, 6
/// switch container, 7 actor-mixer, 9 layer container, 10 music segment, 11 music track, 12
/// music switch, 13 music random / sequence), its id and its bytes after the id.
#[derive(Clone, Debug)]
pub struct Object {
    pub kind: u8,
    pub id: u32,
    pub body: Range<usize>,
}

#[derive(Default)]
pub struct Bank {
    pub version: u32,
    pub id: u32,
    pub bytes: Vec<u8>,
    /// Sound id -> its bytes in the bank.
    pub sounds: HashMap<u32, Range<usize>>,
    pub objects: Vec<Object>,
    /// Object id -> its index in `objects`.
    pub by_id: HashMap<u32, usize>,
}

fn u32le(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl Bank {
    pub fn parse(bytes: Vec<u8>) -> Option<Bank> {
        let mut b = Bank::default();
        let d = &bytes;
        let mut o = 0;
        let (mut didx, mut data, mut hirc) = (None, None, None);
        while o + 8 <= d.len() {
            let size = u32le(d, o + 4)? as usize;
            let body = o + 8..(o + 8 + size).min(d.len());
            match &d[o..o + 4] {
                b"BKHD" => {
                    b.version = u32le(d, body.start)?;
                    b.id = u32le(d, body.start + 4)?;
                }
                b"DIDX" => didx = Some(body),
                b"DATA" => data = Some(body),
                b"HIRC" => hirc = Some(body),
                _ => {}
            }
            o += 8 + size;
        }
        if let (Some(didx), Some(data)) = (didx, data) {
            for k in 0..(didx.end - didx.start) / 12 {
                let e = didx.start + 12 * k;
                let (id, off, size) = (u32le(d, e)?, u32le(d, e + 4)? as usize, u32le(d, e + 8)? as usize);
                b.sounds.insert(id, data.start + off..data.start + off + size);
            }
        }
        if let Some(h) = hirc {
            let count = u32le(d, h.start)? as usize;
            let mut o = h.start + 4;
            for _ in 0..count {
                let kind = *d.get(o)?;
                let size = u32le(d, o + 1)? as usize;
                let id = u32le(d, o + 5)?;
                let body = o + 9..(o + 5 + size).min(d.len());
                b.by_id.insert(id, b.objects.len());
                b.objects.push(Object { kind, id, body });
                o += 5 + size;
            }
        }
        b.bytes = bytes;
        Some(b)
    }

    /// A sound the bank holds (a WEM).
    pub fn sound(&self, id: u32) -> Option<&[u8]> {
        self.sounds.get(&id).map(|r| &self.bytes[r.clone()])
    }

    pub fn object(&self, id: u32) -> Option<&Object> {
        self.by_id.get(&id).map(|&i| &self.objects[i])
    }

    pub fn body(&self, o: &Object) -> &[u8] {
        &self.bytes[o.body.clone()]
    }
}

/// Wwise's id of a name: FNV-1 (32 bit) of its lower case.
pub fn name_id(name: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for b in name.to_lowercase().bytes() {
        h = h.wrapping_mul(16_777_619);
        h ^= b as u32;
    }
    h
}
