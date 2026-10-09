//! FCS game-data files (`data/*.base`, `data/*.mod`, workshop/`mods/` `.mod`).
//!
//! Format (v16 and v17) from sheet rows T00001..T00006 (`sheet/types.tsv`, `sheet/fields.tsv`), derived from the
//! player's data files. Little-endian, length-prefixed, no alignment.

use std::fmt;
use std::path::Path;

/// File type tags of the supported formats.
pub const FILETYPE_V15: i32 = 15;
pub const FILETYPE_V16: i32 = 16;
pub const FILETYPE_V17: i32 = 17;

/// v16 records carry this flag in `datatype` (T00006 CHANGED_FLAG).
pub const DATATYPE_CHANGED_FLAG: u32 = 0x8000_0000;

/// @type FcsHeader
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// 15 (zone/level files), 16 or 17.
    pub filetype: i32,
    pub version: i32,
    pub author: String,
    pub description: String,
    /// Comma-separated file names this file builds on.
    pub dependencies: String,
    /// Comma-separated file names referenced but not required.
    pub references: String,
    /// v17 only: bytes between `references` and the header end; meaning unknown, kept verbatim.
    pub tail: Vec<u8>,
    /// Next numeric id for new string ids; the loader keeps the max over all files.
    pub id_counter: i32,
    pub record_count: i32,
}

/// @type FcsReference
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// `string_id` of the referenced record.
    pub target: String,
    pub values: [i32; 3],
}

/// @type FcsInstance
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub id: String,
    /// `string_id` of the placed record.
    pub target: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub states: Vec<String>,
}

/// @type FcsRecord
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Record {
    /// v16: 0. v17: 0 or the record's byte length including this field (checked when nonzero).
    pub lead: i32,
    pub kind: i32,
    pub id: i32,
    pub name: String,
    pub string_id: String,
    /// @type FcsDataType
    pub datatype: u32,
    pub bools: Vec<(String, bool)>,
    pub floats: Vec<(String, f32)>,
    pub ints: Vec<(String, i32)>,
    pub vec3s: Vec<(String, [f32; 3])>,
    pub vec4s: Vec<(String, [f32; 4])>,
    pub strings: Vec<(String, String)>,
    pub files: Vec<(String, String)>,
    /// (category, references) in file order; category is an fcs.def field name.
    pub references: Vec<(String, Vec<Reference>)>,
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub header: Header,
    pub records: Vec<Record>,
    /// v15 zone files end with an i32 count and that many i32s (ascending numeric ids, apparently
    /// those in use); empty for other files.
    pub trailer: Vec<i32>,
    /// More such lists after it (Kenshi's quick.save has three).
    pub trailers: Vec<Vec<i32>>,
    /// The file ends with the list, even an empty one.
    pub has_trailer: bool,
    /// Strings that were stored as Latin-1 (not valid UTF-8), written back that way.
    pub latin1: std::collections::HashSet<String>,
}

thread_local! {
    /// The strings the parse under way decoded from Latin-1.
    static LATIN1_SEEN: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Ran out of bytes reading `what` at byte offset `at`.
    Truncated {
        at: usize,
        what: &'static str,
    },
    UnsupportedFiletype(i32),
    /// A count or length was negative or larger than the remaining bytes could hold.
    BadLength {
        at: usize,
        len: i32,
    },
    /// Parsing finished with bytes left over: the format hypothesis is wrong somewhere.
    TrailingBytes {
        at: usize,
        left: usize,
    },
    /// A v17 header or record length disagrees with what was parsed.
    LengthMismatch {
        at: usize,
        declared: i32,
        parsed: usize,
    },
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { at, what } => write!(f, "truncated reading {what} at byte {at}"),
            Error::UnsupportedFiletype(t) => write!(f, "unsupported FCS filetype {t}"),
            Error::BadLength { at, len } => write!(f, "bad length {len} at byte {at}"),
            Error::TrailingBytes { at, left } => write!(f, "{left} trailing bytes after byte {at}"),
            Error::LengthMismatch {
                at,
                declared,
                parsed,
            } => {
                write!(
                    f,
                    "length {declared} declared at byte {at}, parsed {parsed}"
                )
            }
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or(Error::Truncated { at: self.pos, what })?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn i32(&mut self, what: &'static str) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, Error> {
        Ok(self.i32(what)? as u32)
    }

    fn f32(&mut self, what: &'static str) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }

    fn f32s<const N: usize>(&mut self, what: &'static str) -> Result<[f32; N], Error> {
        let mut v = [0.0; N];
        for x in &mut v {
            *x = self.f32(what)?;
        }
        Ok(v)
    }

    /// A count of items each at least `min_item` bytes long; rejects counts the file can't hold.
    fn count(&mut self, min_item: usize, what: &'static str) -> Result<usize, Error> {
        let at = self.pos;
        let n = self.i32(what)?;
        let left = self.buf.len() - self.pos;
        if n < 0 || (n as usize).saturating_mul(min_item) > left {
            return Err(Error::BadLength { at, len: n });
        }
        Ok(n as usize)
    }

    /// @type FcsString
    fn string(&mut self, what: &'static str) -> Result<String, Error> {
        let n = self.count(1, what)?;
        Ok(decode(self.take(n, what)?))
    }

    fn list<T>(
        &mut self,
        min_item: usize,
        what: &'static str,
        mut item: impl FnMut(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let n = self.count(min_item, what)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(item(self)?);
        }
        Ok(v)
    }
}

/// Text is UTF-8 where valid, otherwise Latin-1 (one char per byte).
fn decode(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            let s: String = b.iter().map(|&c| c as char).collect();
            LATIN1_SEEN.with(|l| l.borrow_mut().insert(s.clone()));
            s
        }
    }
}

fn header(r: &mut Reader) -> Result<Header, Error> {
    let filetype = r.i32("filetype")?;
    let v17 = match filetype {
        FILETYPE_V15 => {
            // v15 (zone/level files): no version or description strings, straight to the counts
            return Ok(Header {
                filetype,
                version: 0,
                author: String::new(),
                description: String::new(),
                dependencies: String::new(),
                references: String::new(),
                tail: Vec::new(),
                id_counter: r.i32("id_counter")?,
                record_count: r.i32("record_count")?,
            });
        }
        FILETYPE_V16 => false,
        FILETYPE_V17 => true,
        t => return Err(Error::UnsupportedFiletype(t)),
    };
    let size_at = r.pos;
    let size = if v17 { r.count(1, "header_size")? } else { 0 };
    let start = r.pos;
    let version = r.i32("version")?;
    let author = r.string("author")?;
    let description = r.string("description")?;
    let dependencies = r.string("dependencies")?;
    let references = r.string("references")?;
    let mut tail = Vec::new();
    if v17 {
        let used = r.pos - start;
        if used > size {
            return Err(Error::LengthMismatch {
                at: size_at,
                declared: size as i32,
                parsed: used,
            });
        }
        tail = r.take(size - used, "header tail")?.to_vec();
    }
    Ok(Header {
        filetype,
        version,
        author,
        description,
        dependencies,
        references,
        tail,
        id_counter: r.i32("id_counter")?,
        record_count: r.i32("record_count")?,
    })
}

fn record(r: &mut Reader, filetype: i32) -> Result<Record, Error> {
    let start = r.pos;
    let mut rec = Record {
        lead: r.i32("record lead")?,
        kind: r.i32("record type")?,
        id: r.i32("record id")?,
        name: r.string("record name")?,
        string_id: r.string("record string_id")?,
        datatype: r.u32("record datatype")?,
        ..Default::default()
    };
    rec.bools = r.list(5, "bools", |r| {
        Ok((r.string("bool key")?, r.take(1, "bool")?[0] != 0))
    })?;
    rec.floats = r.list(8, "floats", |r| {
        Ok((r.string("float key")?, r.f32("float")?))
    })?;
    rec.ints = r.list(8, "ints", |r| Ok((r.string("int key")?, r.i32("int")?)))?;
    rec.vec3s = r.list(16, "vec3s", |r| {
        Ok((r.string("vec3 key")?, r.f32s("vec3")?))
    })?;
    rec.vec4s = r.list(20, "vec4s", |r| {
        Ok((r.string("vec4 key")?, r.f32s("vec4")?))
    })?;
    rec.strings = r.list(8, "strings", |r| {
        Ok((r.string("string key")?, r.string("string")?))
    })?;
    rec.files = r.list(8, "files", |r| {
        Ok((r.string("file key")?, r.string("file")?))
    })?;
    rec.references = r.list(8, "reference categories", |r| {
        let cat = r.string("reference category")?;
        let refs = r.list(16, "references", |r| {
            Ok(Reference {
                target: r.string("reference target")?,
                values: [r.i32("ref val0")?, r.i32("ref val1")?, r.i32("ref val2")?],
            })
        })?;
        Ok((cat, refs))
    })?;
    rec.instances = r.list(40, "instances", |r| {
        Ok(Instance {
            id: r.string("instance id")?,
            target: r.string("instance target")?,
            position: r.f32s("instance position")?,
            rotation: r.f32s("instance rotation")?,
            states: r.list(4, "instance states", |r| r.string("instance state"))?,
        })
    })?;
    let parsed = r.pos - start;
    // only v17 stores the record length in the lead (v15 zone files keep something else there)
    if filetype == FILETYPE_V17 && rec.lead != 0 && rec.lead as usize != parsed {
        return Err(Error::LengthMismatch {
            at: start,
            declared: rec.lead,
            parsed,
        });
    }
    Ok(rec)
}

/// Parse a whole FCS file. Fails on any leftover bytes, so a successful parse of every shipped
/// file is the check that the format rows are right.
pub fn parse(buf: &[u8]) -> Result<File, Error> {
    let mut r = Reader { buf, pos: 0 };
    LATIN1_SEEN.with(|l| l.borrow_mut().clear());
    let header = header(&mut r)?;
    let mut records = Vec::with_capacity(header.record_count.clamp(0, 1 << 16) as usize);
    for _ in 0..header.record_count.max(0) {
        records.push(record(&mut r, header.filetype)?);
    }
    let mut trailer = Vec::new();
    let mut trailers = Vec::new();
    let has_trailer = header.filetype == FILETYPE_V15 && r.pos < buf.len();
    if has_trailer {
        trailer = r.list(4, "zone trailer", |r| r.i32("zone trailer id"))?;
        while r.pos < buf.len() {
            trailers.push(r.list(4, "save trailer", |r| r.i32("save trailer id"))?);
        }
    }
    if r.pos != buf.len() {
        return Err(Error::TrailingBytes {
            at: r.pos,
            left: buf.len() - r.pos,
        });
    }
    Ok(File {
        header,
        records,
        trailer,
        trailers,
        has_trailer,
        latin1: LATIN1_SEEN.with(|l| std::mem::take(&mut *l.borrow_mut())),
    })
}

/// Write an FCS file: the exact inverse of [`parse`] (header for its filetype, the records, a
/// v15 file's trailing id lists). v17 header and record lengths are written as parsed (a
/// record's `lead`, the header's size from what it holds).
pub fn write(f: &File) -> Vec<u8> {
    let mut w: Vec<u8> = Vec::new();
    let i32_ = |w: &mut Vec<u8>, v: i32| w.extend_from_slice(&v.to_le_bytes());
    let f32_ = |w: &mut Vec<u8>, v: f32| w.extend_from_slice(&v.to_le_bytes());
    let latin1 = &f.latin1;
    let string = |w: &mut Vec<u8>, s: &str| {
        // text read as Latin-1 goes back one byte a char; the rest as UTF-8
        let bytes: Vec<u8> = if latin1.contains(s) { s.chars().map(|c| c as u32 as u8).collect() } else { s.as_bytes().to_vec() };
        w.extend_from_slice(&(bytes.len() as i32).to_le_bytes());
        w.extend_from_slice(&bytes);
    };
    let h = &f.header;
    i32_(&mut w, h.filetype);
    if h.filetype != FILETYPE_V15 {
        let mut body = Vec::new();
        body.extend_from_slice(&h.version.to_le_bytes());
        string(&mut body, &h.author);
        string(&mut body, &h.description);
        string(&mut body, &h.dependencies);
        string(&mut body, &h.references);
        if h.filetype == FILETYPE_V17 {
            body.extend_from_slice(&h.tail);
            i32_(&mut w, body.len() as i32);
        }
        w.extend_from_slice(&body);
    }
    i32_(&mut w, h.id_counter);
    i32_(&mut w, f.records.len() as i32);
    for r in &f.records {
        let start = w.len();
        i32_(&mut w, r.lead);
        i32_(&mut w, r.kind);
        i32_(&mut w, r.id);
        string(&mut w, &r.name);
        string(&mut w, &r.string_id);
        i32_(&mut w, r.datatype as i32);
        i32_(&mut w, r.bools.len() as i32);
        for (k, v) in &r.bools {
            string(&mut w, k);
            w.push(u8::from(*v));
        }
        i32_(&mut w, r.floats.len() as i32);
        for (k, v) in &r.floats {
            string(&mut w, k);
            f32_(&mut w, *v);
        }
        i32_(&mut w, r.ints.len() as i32);
        for (k, v) in &r.ints {
            string(&mut w, k);
            i32_(&mut w, *v);
        }
        i32_(&mut w, r.vec3s.len() as i32);
        for (k, v) in &r.vec3s {
            string(&mut w, k);
            v.iter().for_each(|x| f32_(&mut w, *x));
        }
        i32_(&mut w, r.vec4s.len() as i32);
        for (k, v) in &r.vec4s {
            string(&mut w, k);
            v.iter().for_each(|x| f32_(&mut w, *x));
        }
        for list in [&r.strings, &r.files] {
            i32_(&mut w, list.len() as i32);
            for (k, v) in list {
                string(&mut w, k);
                string(&mut w, v);
            }
        }
        i32_(&mut w, r.references.len() as i32);
        for (cat, refs) in &r.references {
            string(&mut w, cat);
            i32_(&mut w, refs.len() as i32);
            for x in refs {
                string(&mut w, &x.target);
                x.values.iter().for_each(|v| i32_(&mut w, *v));
            }
        }
        i32_(&mut w, r.instances.len() as i32);
        for i in &r.instances {
            string(&mut w, &i.id);
            string(&mut w, &i.target);
            i.position.iter().for_each(|x| f32_(&mut w, *x));
            i.rotation.iter().for_each(|x| f32_(&mut w, *x));
            i32_(&mut w, i.states.len() as i32);
            for st in &i.states {
                string(&mut w, st);
            }
        }
        let _ = start;
    }
    if h.filetype == FILETYPE_V15 && (f.has_trailer || !f.trailer.is_empty() || !f.trailers.is_empty()) {
        for list in std::iter::once(&f.trailer).chain(&f.trailers) {
            i32_(&mut w, list.len() as i32);
            list.iter().for_each(|v| i32_(&mut w, *v));
        }
    }
    w
}

/// Write an FCS file to `path`.
pub fn save(f: &File, path: &Path) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, write(f))
}

pub fn read(path: &Path) -> Result<File, Error> {
    parse(&read_bytes(path)?)
}

/// Only the header (dependencies, record count): for ordering files without parsing records.
pub fn read_header(path: &Path) -> Result<Header, Error> {
    header(&mut Reader {
        buf: &read_bytes(path)?,
        pos: 0,
    })
}

/// Split a header's comma-separated file list (`dependencies`, `references`).
pub fn file_list(s: &str) -> impl Iterator<Item = &str> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(out: &mut Vec<u8>, t: &str) {
        out.extend((t.len() as i32).to_le_bytes());
        out.extend(t.as_bytes());
    }
    fn i(out: &mut Vec<u8>, v: i32) {
        out.extend(v.to_le_bytes());
    }

    fn sample() -> Vec<u8> {
        let mut b = Vec::new();
        i(&mut b, 16);
        i(&mut b, 1);
        for t in ["", "", "gamedata.base", ""] {
            s(&mut b, t);
        }
        i(&mut b, 0x4c67be);
        i(&mut b, 1);
        // record
        i(&mut b, 0);
        i(&mut b, 0);
        i(&mut b, 0);
        s(&mut b, "sign");
        s(&mut b, "997-gamedata.base");
        b.extend(0x8000_0001u32.to_le_bytes());
        i(&mut b, 1);
        s(&mut b, "is sign");
        b.push(1);
        i(&mut b, 1);
        s(&mut b, "scale");
        b.extend(2.5f32.to_le_bytes());
        for _ in 0..5 {
            i(&mut b, 0); // ints, vec3s, vec4s, strings, files
        }
        i(&mut b, 1);
        s(&mut b, "parts");
        i(&mut b, 1);
        s(&mut b, "12-gamedata.base");
        for v in [1, 2, 3] {
            i(&mut b, v);
        }
        i(&mut b, 0); // instances
        b
    }

    #[test]
    fn parses_minimal_file() {
        let f = parse(&sample()).unwrap();
        assert_eq!(f.header.dependencies, "gamedata.base");
        let r = &f.records[0];
        assert_eq!((r.name.as_str(), r.datatype), ("sign", 0x8000_0001));
        assert_eq!(r.bools, vec![("is sign".into(), true)]);
        assert_eq!(r.floats, vec![("scale".into(), 2.5)]);
        assert_eq!(r.references[0].1[0].values, [1, 2, 3]);
    }

    /// The v16 sample re-encoded as v17: header length, tail bytes, record length lead.
    fn sample_v17() -> Vec<u8> {
        let v16 = sample();
        let hdr_end = 8 + 4 * 4 + "gamedata.base".len(); // filetype..references
        let tail = [9u8, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut b = Vec::new();
        i(&mut b, 17);
        i(&mut b, (hdr_end - 4 + tail.len()) as i32);
        b.extend(&v16[4..hdr_end]);
        b.extend(tail);
        b.extend(&v16[hdr_end..hdr_end + 8]); // id_counter, record_count
        let rec = &v16[hdr_end + 8..];
        i(&mut b, rec.len() as i32);
        b.extend(&rec[4..]);
        b
    }

    #[test]
    fn parses_v17_and_checks_lengths() {
        let f = parse(&sample_v17()).unwrap();
        assert_eq!(f.header.tail.len(), 10);
        assert_eq!(f.records[0].floats, vec![("scale".into(), 2.5)]);
        let mut b = sample_v17();
        let lead = b.len() - f.records[0].lead as usize;
        b[lead] += 1;
        assert!(matches!(parse(&b), Err(Error::LengthMismatch { .. })));
    }

    #[test]
    fn rejects_trailing_and_truncated() {
        let mut b = sample();
        b.push(0);
        assert!(matches!(parse(&b), Err(Error::TrailingBytes { .. })));
        let b = sample();
        assert!(matches!(
            parse(&b[..b.len() - 3]),
            Err(Error::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_absurd_counts() {
        let mut b = sample();
        let at = b.len() - 4; // instance count
        b[at..].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(matches!(parse(&b), Err(Error::BadLength { .. })));
    }
}
