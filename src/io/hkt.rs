//! Havok binary tagfiles (`.hkt`, hk_2014.2.0-r1, tagfile version 5), the format Kenshi ships its
//! navmesh tiles in (`data/newland/land/navtiles/tileX.Y.hkt`).
//!
//! Layout, as read from the shipped files: 8-byte magic `1e0db0ca cefa11d0`, then a stream of
//! variable-length integers (7 bits per byte, low group first; bit 0 of the result is the sign,
//! the rest the magnitude). Each record starts with a tag:
//! - 1 file info: version, SDK string, then 6 fixed bytes;
//! - 2 class: name, version, parent class index, member count, then per member a name and type;
//! - 4 object: class index, a presence bit per member (parents' members first), then the
//!   present members' values; objects are numbered from 1 in file order;
//! - 7 end.
//!
//! Strings go through a table that starts as `["", null]`: a positive length is followed by the
//! bytes and appends to the table, zero or a negative length names an earlier entry. Object
//! members hold the referenced object's number (0 = none). Arrays of structs are stored member by
//! member, each member a presence bit then all elements' values. Arrays of integers carry a
//! leading width (always 4 in the shipped files); arrays of vec4 lead with the number of floats
//! stored per element (4, or 3 with w dropped); arrays of reals are plain floats.

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    NotTagfile,
    Truncated,
    Unsupported(String),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotTagfile => write!(f, "not a Havok binary tagfile"),
            Error::Truncated => write!(f, "truncated tagfile"),
            Error::Unsupported(s) => write!(f, "unsupported tagfile content: {s}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

const MAGIC: [u8; 8] = [0x1e, 0x0d, 0xb0, 0xca, 0xce, 0xfa, 0x11, 0xd0];

const VOID: i64 = 0;
const BYTE: i64 = 1;
const INT: i64 = 2;
const REAL: i64 = 3;
const VEC4: i64 = 4;
const VEC16: i64 = 7;
const OBJECT: i64 = 8;
const STRUCT: i64 = 9;
const CSTRING: i64 = 10;
const ARRAY: i64 = 0x10;
const TUPLE: i64 = 0x20;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Byte(u8),
    Int(i64),
    Real(f32),
    /// vec4, vec8, vec12 or vec16.
    Vector(Vec<f32>),
    Str(Option<String>),
    /// Number of the referenced object (`Tagfile::object`), 0 for none.
    Ref(u32),
    Struct(Object),
    Array(Vec<Value>),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Byte(b) => Some(*b as i64),
            _ => None,
        }
    }
    pub fn real(&self) -> Option<f32> {
        match self {
            Value::Real(r) => Some(*r),
            _ => None,
        }
    }
    pub fn vector(&self) -> Option<&[f32]> {
        match self {
            Value::Vector(v) => Some(v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => s.as_deref(),
            _ => None,
        }
    }
    pub fn object_ref(&self) -> Option<u32> {
        match self {
            Value::Ref(r) => Some(*r),
            _ => None,
        }
    }
    pub fn array(&self) -> &[Value] {
        match self {
            Value::Array(a) => a,
            _ => &[],
        }
    }
    pub fn object(&self) -> Option<&Object> {
        match self {
            Value::Struct(o) => Some(o),
            _ => None,
        }
    }
}

/// An object or struct: its class and the members present in the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Object {
    pub class: String,
    pub fields: Vec<(String, Value)>,
}

impl Object {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    /// An integer member, 0 when absent (absent members hold their default).
    pub fn int(&self, name: &str) -> i64 {
        self.get(name).and_then(Value::int).unwrap_or(0)
    }
    pub fn real(&self, name: &str) -> f32 {
        self.get(name).and_then(Value::real).unwrap_or(0.0)
    }
    pub fn array(&self, name: &str) -> &[Value] {
        self.get(name).map_or(&[], Value::array)
    }
}

#[derive(Debug, Clone)]
struct Type {
    kind: i64,
    tuple: usize,
    class_name: Option<String>,
}

#[derive(Debug, Clone)]
struct Class {
    name: String,
    parent: usize,
    members: Vec<(String, Type)>,
}

/// @type HavokTagfile
#[derive(Debug, Clone, Default)]
pub struct Tagfile {
    pub version: i64,
    pub sdk: String,
    /// Objects in file order; object number n is `objects[n - 1]`.
    pub objects: Vec<Object>,
}

impl Tagfile {
    pub fn object(&self, number: u32) -> Option<&Object> {
        (number as usize)
            .checked_sub(1)
            .and_then(|i| self.objects.get(i))
    }
    /// The first object of a class.
    pub fn find(&self, class: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.class == class)
    }

    pub fn open(path: &Path) -> Result<Self, Error> {
        let bytes =
            std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 8 || bytes[..8] != MAGIC {
            return Err(Error::NotTagfile);
        }
        let mut r = Reader {
            b: bytes,
            at: 8,
            strings: vec![Some(String::new()), None],
            classes: vec![],
        };
        let mut file = Tagfile::default();
        loop {
            match r.int()? {
                1 => {
                    file.version = r.int()?;
                    file.sdk = r.string()?.unwrap_or_default();
                    r.take(6)?;
                }
                2 => r.class()?,
                4 => {
                    let ci = r.class_index()?;
                    let o = r.body(ci)?;
                    file.objects.push(o);
                }
                7 => return Ok(file),
                t => return Err(Error::Unsupported(format!("tag {t} at {}", r.at))),
            }
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
    strings: Vec<Option<String>>,
    /// Class 1 is `classes[0]`; 0 means none.
    classes: Vec<Class>,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let s = self.b.get(self.at..self.at + n).ok_or(Error::Truncated)?;
        self.at += n;
        Ok(s)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn int(&mut self) -> Result<i64, Error> {
        let mut v: u64 = 0;
        let mut shift = 0;
        loop {
            let c = self.byte()?;
            v |= ((c & 0x7f) as u64) << shift;
            shift += 7;
            if c & 0x80 == 0 {
                break;
            }
        }
        let m = (v >> 1) as i64;
        Ok(if v & 1 != 0 { -m } else { m })
    }
    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<Option<String>, Error> {
        let n = self.int()?;
        if n > 0 {
            let s = String::from_utf8_lossy(self.take(n as usize)?).into_owned();
            self.strings.push(Some(s.clone()));
            Ok(Some(s))
        } else {
            self.strings
                .get((-n) as usize)
                .cloned()
                .ok_or_else(|| Error::Unsupported(format!("string {n}")))
        }
    }
    fn class_index(&mut self) -> Result<usize, Error> {
        let i = self.int()? as usize;
        if i == 0 || i > self.classes.len() {
            return Err(Error::Unsupported(format!("class {i}")));
        }
        Ok(i)
    }
    fn class(&mut self) -> Result<(), Error> {
        let name = self.string()?.unwrap_or_default();
        let _version = self.int()?;
        let parent = self.int()? as usize;
        let n = self.int()?;
        let mut members = Vec::new();
        for _ in 0..n {
            let m = self.string()?.unwrap_or_default();
            let kind = self.int()?;
            let tuple = if kind & TUPLE != 0 {
                self.int()? as usize
            } else {
                0
            };
            let class_name = if matches!(kind & 0xf, OBJECT | STRUCT) {
                self.string()?
            } else {
                None
            };
            members.push((
                m,
                Type {
                    kind,
                    tuple,
                    class_name,
                },
            ));
        }
        self.classes.push(Class {
            name,
            parent,
            members,
        });
        Ok(())
    }
    fn class_named(&self, name: &str) -> Result<usize, Error> {
        self.classes
            .iter()
            .position(|c| c.name == name)
            .map(|i| i + 1)
            .ok_or_else(|| Error::Unsupported(format!("undeclared class {name}")))
    }
    /// All members of a class, parents' first.
    fn members(&self, ci: usize) -> Vec<(String, Type)> {
        let c = &self.classes[ci - 1];
        let mut out = if c.parent != 0 {
            self.members(c.parent)
        } else {
            vec![]
        };
        out.extend(c.members.iter().cloned());
        out
    }
    fn bits(&mut self, n: usize) -> Result<Vec<bool>, Error> {
        let bytes = self.take(n.div_ceil(8))?;
        Ok((0..n).map(|i| bytes[i / 8] >> (i % 8) & 1 != 0).collect())
    }
    fn body(&mut self, ci: usize) -> Result<Object, Error> {
        let members = self.members(ci);
        let present = self.bits(members.len())?;
        let mut o = Object {
            class: self.classes[ci - 1].name.clone(),
            fields: vec![],
        };
        for ((name, ty), p) in members.into_iter().zip(present) {
            if p {
                let v = self.value(&ty)?;
                o.fields.push((name, v));
            }
        }
        Ok(o)
    }
    fn struct_class(&self, ty: &Type) -> Result<usize, Error> {
        self.class_named(ty.class_name.as_deref().unwrap_or(""))
    }
    fn value(&mut self, ty: &Type) -> Result<Value, Error> {
        let base = Type {
            kind: ty.kind & 0xf,
            ..ty.clone()
        };
        if ty.kind & TUPLE != 0 {
            self.array(&base, ty.tuple)
        } else if ty.kind & ARRAY != 0 {
            let n = self.int()? as usize;
            self.array(&base, n)
        } else {
            self.scalar(&base)
        }
    }
    fn scalar(&mut self, ty: &Type) -> Result<Value, Error> {
        Ok(match ty.kind {
            BYTE => Value::Byte(self.byte()?),
            INT => Value::Int(self.int()?),
            REAL => Value::Real(self.f32()?),
            VEC4..=VEC16 => {
                let n = 4 * (ty.kind - VEC4 + 1) as usize;
                Value::Vector((0..n).map(|_| self.f32()).collect::<Result<_, _>>()?)
            }
            CSTRING => Value::Str(self.string()?),
            OBJECT => Value::Ref(self.int()? as u32),
            STRUCT => {
                let ci = self.struct_class(ty)?;
                Value::Struct(self.body(ci)?)
            }
            VOID => return Err(Error::Unsupported("void value".into())),
            k => return Err(Error::Unsupported(format!("type {k}"))),
        })
    }
    fn width(&mut self) -> Result<(), Error> {
        match self.int()? {
            4 => Ok(()),
            w => Err(Error::Unsupported(format!(
                "array width {w} at {}",
                self.at
            ))),
        }
    }
    fn array(&mut self, ty: &Type, n: usize) -> Result<Value, Error> {
        match ty.kind {
            BYTE => Ok(Value::Bytes(self.take(n)?.to_vec())),
            INT => {
                self.width()?;
                (0..n)
                    .map(|_| self.scalar(ty))
                    .collect::<Result<_, _>>()
                    .map(Value::Array)
            }
            VEC4 => {
                let stored = self.int()?;
                if !(1..=4).contains(&stored) {
                    return Err(Error::Unsupported(format!(
                        "vec4 width {stored} at {}",
                        self.at
                    )));
                }
                (0..n)
                    .map(|_| {
                        let mut v = vec![0.0; 4];
                        for x in v.iter_mut().take(stored as usize) {
                            *x = self.f32()?;
                        }
                        Ok(Value::Vector(v))
                    })
                    .collect::<Result<_, _>>()
                    .map(Value::Array)
            }
            STRUCT => {
                let ci = self.struct_class(ty)?;
                let members = self.members(ci);
                let present = self.bits(members.len())?;
                let class = self.classes[ci - 1].name.clone();
                let mut out = vec![
                    Object {
                        class,
                        fields: vec![]
                    };
                    n
                ];
                for ((name, mty), p) in members.into_iter().zip(present) {
                    if !p {
                        continue;
                    }
                    let column = if mty.kind & (ARRAY | TUPLE) != 0 {
                        (0..n)
                            .map(|_| self.value(&mty))
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        match self.array(&mty, n)? {
                            Value::Array(a) => a,
                            Value::Bytes(b) => b.into_iter().map(Value::Byte).collect(),
                            _ => unreachable!(),
                        }
                    };
                    for (o, v) in out.iter_mut().zip(column) {
                        o.fields.push((name.clone(), v));
                    }
                }
                Ok(Value::Array(out.into_iter().map(Value::Struct).collect()))
            }
            REAL | OBJECT | CSTRING => (0..n)
                .map(|_| self.scalar(ty))
                .collect::<Result<_, _>>()
                .map(Value::Array),
            k => Err(Error::Unsupported(format!("array of type {k}"))),
        }
    }
}
