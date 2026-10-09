//! Merged game data: every FCS file in load order applied onto one record set.
//!
//! Spec (sheet row F041671 GameDataContainer::load, fields of T00002/T00003):
//! - records merge by `string_id`; an unknown id creates a record tagged with the file's index
//!   (with a warning when its datatype says it modifies an existing record, bit 0);
//! - datatype bit 1 on an existing record replaces its name;
//! - bool/float/int/vec3/vec4/string/file fields overwrite per key;
//! - a reference whose three values are all `i32::MAX` removes `category -> target`, any other sets it;
//! - an instance with an empty target removes the instance with that id, any other sets it;
//! - after a record is applied, a true bool field `REMOVED` deletes it;
//! - the id counter is the max over all files.

use crate::ItemType;
use kenshi_io::fcs;
use kenshi_io::mods::LoadOrder;
use std::collections::HashMap;

/// T00003 datatype bit 0: the record modifies an existing one.
pub const DATATYPE_MODIFIES: u32 = 0x1;
/// T00003 datatype bit 1: the record sets the name of an existing one.
pub const DATATYPE_SETS_NAME: u32 = 0x2;
/// Reference values meaning "remove this reference".
pub const REF_REMOVE: [i32; 3] = [i32::MAX; 3];
/// Bool field that deletes a record.
pub const FIELD_REMOVED: &str = "REMOVED";

/// A reference inside a category: target string_id and its three values (T00004).
pub type RefEntry = (String, [i32; 3]);

/// @type GameData
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GameData {
    pub kind: i32,
    pub name: String,
    pub string_id: String,
    /// Index into `GameDataContainer::files` of the file that created the record.
    pub source: usize,
    pub bools: HashMap<String, bool>,
    pub floats: HashMap<String, f32>,
    pub ints: HashMap<String, i32>,
    pub vec3s: HashMap<String, [f32; 3]>,
    pub vec4s: HashMap<String, [f32; 4]>,
    pub strings: HashMap<String, String>,
    pub files: HashMap<String, String>,
    /// category -> (target string_id, values), in the order they were first set.
    pub references: Vec<(String, Vec<RefEntry>)>,
    /// Placed instances by id, in the order they were first set.
    pub instances: Vec<fcs::Instance>,
}

impl GameData {
    pub fn item_type(&self) -> Option<ItemType> {
        ItemType::from_i32(self.kind)
    }

    pub fn refs(&self, category: &str) -> &[RefEntry] {
        self.references
            .iter()
            .find(|(c, _)| c == category)
            .map_or(&[], |(_, v)| v.as_slice())
    }

    fn apply(&mut self, r: &fcs::Record) {
        self.bools.extend(r.bools.iter().cloned());
        self.floats.extend(r.floats.iter().cloned());
        self.ints.extend(r.ints.iter().cloned());
        self.vec3s.extend(r.vec3s.iter().cloned());
        self.vec4s.extend(r.vec4s.iter().cloned());
        self.strings.extend(r.strings.iter().cloned());
        self.files.extend(r.files.iter().cloned());
        for (cat, refs) in &r.references {
            for rf in refs {
                self.set_ref(cat, &rf.target, rf.values);
            }
        }
        for inst in &r.instances {
            let at = self.instances.iter().position(|i| i.id == inst.id);
            match (at, inst.target.is_empty()) {
                (Some(i), true) => {
                    self.instances.remove(i);
                }
                (None, true) => {}
                (Some(i), false) => self.instances[i] = inst.clone(),
                (None, false) => self.instances.push(inst.clone()),
            }
        }
    }

    fn set_ref(&mut self, category: &str, target: &str, values: [i32; 3]) {
        let ci = match self.references.iter().position(|(c, _)| c == category) {
            Some(i) => i,
            None if values == REF_REMOVE => return,
            None => {
                self.references.push((category.to_owned(), Vec::new()));
                self.references.len() - 1
            }
        };
        let list = &mut self.references[ci].1;
        let at = list.iter().position(|(t, _)| t == target);
        match (at, values == REF_REMOVE) {
            (Some(i), true) => {
                list.remove(i);
            }
            (None, true) => {}
            (Some(i), false) => list[i].1 = values,
            (None, false) => list.push((target.to_owned(), values)),
        }
    }
}

/// @type GameDataContainer
#[derive(Debug, Clone, Default)]
pub struct GameDataContainer {
    pub records: HashMap<String, GameData>,
    /// Record ids in the order first loaded: `of_type` walks them in this order, so a listing is
    /// the same on every run (the original's own map order is not reproduced).
    pub order: Vec<String>,
    pub id_counter: i32,
    /// File names in the order applied; `GameData::source` indexes this.
    pub files: Vec<String>,
    /// Non-fatal problems, like the original's `[Mods]` log lines.
    pub warnings: Vec<String>,
}

impl GameDataContainer {
    /// Apply one parsed file on top of what is loaded.
    pub fn apply_file(&mut self, name: &str, file: &fcs::File) {
        let source = self.files.len();
        self.files.push(name.to_owned());
        self.id_counter = self.id_counter.max(file.header.id_counter);
        for r in &file.records {
            let flags = r.datatype & 0x7fff_ffff;
            let rec = match self.records.get_mut(&r.string_id) {
                Some(rec) => {
                    if flags & DATATYPE_SETS_NAME != 0 {
                        rec.name.clone_from(&r.name);
                    }
                    rec
                }
                None => {
                    if flags & DATATYPE_MODIFIES != 0 {
                        self.warnings.push(format!(
                            "[Mods] Item {} ({}) modified by {name} does not exist",
                            r.name, r.string_id
                        ));
                    }
                    self.order.push(r.string_id.clone());
                    self.records.entry(r.string_id.clone()).or_insert(GameData {
                        kind: r.kind,
                        name: r.name.clone(),
                        string_id: r.string_id.clone(),
                        source,
                        ..Default::default()
                    })
                }
            };
            rec.apply(r);
            if rec.bools.get(FIELD_REMOVED) == Some(&true) {
                self.records.remove(&r.string_id);
                self.order.retain(|s| s != &r.string_id);
            }
        }
    }

    /// Read and apply every file of a load order. A core file that fails is an error; a failing mod
    /// is skipped with a warning (the original collects them into one dialog).
    pub fn load(order: &LoadOrder) -> Result<Self, fcs::Error> {
        let mut c = GameDataContainer::default();
        for name in &order.missing {
            c.warnings.push(format!("[Mods] Mod '{name}' not found"));
        }
        for f in &order.files {
            match fcs::read(&f.path) {
                Ok(file) => c.apply_file(&f.name, &file),
                Err(e) if f.core => return Err(e),
                Err(e) => c
                    .warnings
                    .push(format!("[Mods] Failed to load mod: {} ({e})", f.name)),
            }
        }
        Ok(c)
    }

    pub fn get(&self, string_id: &str) -> Option<&GameData> {
        self.records.get(string_id)
    }

    pub fn of_type(&self, t: ItemType) -> impl Iterator<Item = &GameData> {
        self.order
            .iter()
            .filter_map(|s| self.records.get(s))
            .filter(move |r| r.kind == t as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(sid: &str, name: &str, datatype: u32) -> fcs::Record {
        fcs::Record {
            kind: ItemType::Item as i32,
            name: name.into(),
            string_id: sid.into(),
            datatype,
            ..Default::default()
        }
    }

    fn file(id_counter: i32, records: Vec<fcs::Record>) -> fcs::File {
        fcs::File {
            header: fcs::Header {
                filetype: 16,
                version: 1,
                author: String::new(),
                description: String::new(),
                dependencies: String::new(),
                references: String::new(),
                tail: Vec::new(),
                id_counter,
                record_count: records.len() as i32,
            },
            records,
            trailer: Vec::new(),
            trailers: Vec::new(),
            has_trailer: false,
            latin1: Default::default(),
        }
    }

    fn inst(id: &str, target: &str) -> fcs::Instance {
        fcs::Instance {
            id: id.into(),
            target: target.into(),
            position: [0.0; 3],
            rotation: [1.0, 0.0, 0.0, 0.0],
            states: vec![],
        }
    }

    #[test]
    fn merge_rules() {
        let mut base = rec("1-base", "Sword", 0x8000_0002);
        base.floats = vec![("weight".into(), 2.0), ("value".into(), 10.0)];
        base.references = vec![(
            "parts".into(),
            vec![
                fcs::Reference {
                    target: "a".into(),
                    values: [1, 0, 0],
                },
                fcs::Reference {
                    target: "b".into(),
                    values: [2, 0, 0],
                },
            ],
        )];
        base.instances = vec![inst("i1", "x"), inst("i2", "y")];
        let mut c = GameDataContainer::default();
        c.apply_file("base", &file(5, vec![base]));

        let mut m = rec("1-base", "Better Sword", 0x1); // modifies, keeps name
        m.floats = vec![("weight".into(), 3.0)];
        m.references = vec![(
            "parts".into(),
            vec![
                fcs::Reference {
                    target: "a".into(),
                    values: REF_REMOVE,
                },
                fcs::Reference {
                    target: "b".into(),
                    values: [7, 0, 0],
                },
                fcs::Reference {
                    target: "c".into(),
                    values: [3, 0, 0],
                },
            ],
        )];
        m.instances = vec![inst("i1", ""), inst("i3", "z")];
        c.apply_file("mod", &file(3, vec![m, rec("9-mod", "Ghost", 0x1)]));

        let r = c.get("1-base").unwrap();
        assert_eq!(r.name, "Sword");
        assert_eq!((r.floats["weight"], r.floats["value"]), (3.0, 10.0));
        assert_eq!(
            r.refs("parts"),
            [("b".to_string(), [7, 0, 0]), ("c".to_string(), [3, 0, 0])]
        );
        let ids: Vec<_> = r.instances.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["i2", "i3"]);
        assert_eq!(r.source, 0);
        assert_eq!(c.id_counter, 5);
        // unknown record that claims to modify: created, with a warning
        assert_eq!(c.get("9-mod").unwrap().source, 1);
        assert_eq!(c.warnings.len(), 1);

        let mut ren = rec("1-base", "Renamed", 0x3);
        ren.bools = vec![];
        c.apply_file("mod2", &file(0, vec![ren]));
        assert_eq!(c.get("1-base").unwrap().name, "Renamed");

        let mut del = rec("1-base", "", 0x1);
        del.bools = vec![(FIELD_REMOVED.into(), true)];
        c.apply_file("mod3", &file(0, vec![del]));
        assert!(c.get("1-base").is_none());
    }
}
