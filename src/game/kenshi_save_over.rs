//! A Kenshi save written over a real one (port_map `saves`): Kenshi's quick.save and platoon files
//! hold far more than the rewrite models (towns, wars, biomes, trust tables, the handles that link
//! records, missions, jobs, tags), so a save written from scratch (`kenshi_save::write`) lacks
//! what Kenshi's loader may need. Written over a base save instead, everything the rewrite does
//! not model is the base's, verbatim, and only what it does is replaced:
//! - quick.save: the camera record's time, money, camera, zones, squad and member counts (its
//!   selection let go); the research done; each faction's relations (`relationSID<n>` /
//!   `relation<n>`, new pairs added); the player's PLATOONs: one per squad, the base's kept
//!   (handles, towns, tags) and its count and position set, a squad more than the base had made
//!   from the base's first with a fresh handle and no tags, the base's squads beyond the
//!   rewrite's dropped;
//! - each squad's platoon file: per character, its records (GAMESTATE_CHARACTER, STATS,
//!   MEDICAL_STATE, CHARACTER_APPEARANCE, INVENTORY_STATE and its items, GAMESTATE_AI) made from
//!   the base's character of the same name (else the base's first), so the fields not set here
//!   are a real character's: its name, faction, personality and slave state, its stats, its
//!   parts' flesh, stun and bandages, blood, hunger, dead and knocked out, its looks, its items;
//!   its handle the squad's (Kenshi's TYPE 1, the squad's C and CS, its index, its own serial)
//!   and the handles naming other objects (carried, carrying, inside of, slaver) let go;
//! - everything else (the zone files, the other squads' platoon files, the portraits) copied.
//!
//! The base is read only: the save goes in its own folder. (not written: the player's
//! buildings, which live in the base's zone files as the base had them; the dead uniques)

use crate::kenshi_save::{KChar, KItem, KenshiSave};
use kenshi_io::fcs::{File, Instance, Record, Reference};
use std::collections::HashMap;
use std::path::Path;

const STATS: i32 = 25;
const INSTANCES: i32 = 30;
const PLATOON: i32 = 34;
const CHARACTER_STATE: i32 = 36;
const INVENTORY: i32 = 41;
const ITEM_STATE: i32 = 42;
const MEDICAL: i32 = 57;
const APPEARANCE: i32 = 66;
const AI: i32 = 67;
const RESEARCH: i32 = 21;
const CAMERA: i32 = 56;
const FACTION_STATE: i32 = 37;
/// Kenshi's handle type of no object.
const NO_HANDLE: i32 = 11;

fn set_int(r: &mut Record, k: &str, v: i32) {
    match r.ints.iter_mut().find(|(n, _)| n == k) {
        Some(e) => e.1 = v,
        None => r.ints.push((k.to_string(), v)),
    }
}
fn set_float(r: &mut Record, k: &str, v: f32) {
    match r.floats.iter_mut().find(|(n, _)| n == k) {
        Some(e) => e.1 = v,
        None => r.floats.push((k.to_string(), v)),
    }
}
fn set_string(r: &mut Record, k: &str, v: &str) {
    match r.strings.iter_mut().find(|(n, _)| n == k) {
        Some(e) => e.1 = v.to_string(),
        None => r.strings.push((k.to_string(), v.to_string())),
    }
}
fn set_bool(r: &mut Record, k: &str, v: bool) {
    match r.bools.iter_mut().find(|(n, _)| n == k) {
        Some(e) => e.1 = v,
        None => r.bools.push((k.to_string(), v)),
    }
}
fn set_vec3(r: &mut Record, k: &str, v: [f32; 3]) {
    match r.vec3s.iter_mut().find(|(n, _)| n == k) {
        Some(e) => e.1 = v,
        None => r.vec3s.push((k.to_string(), v)),
    }
}
fn get_int(r: &Record, k: &str) -> Option<i32> {
    r.ints.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
}
fn get_string<'a>(r: &'a Record, k: &str) -> Option<&'a str> {
    r.strings.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

/// A handle field group (`<prefix>TYPE`, `C`, `CS`, `I`, `S`) set.
fn set_handle(r: &mut Record, prefix: &str, h: [i32; 5]) {
    for (suffix, v) in ["TYPE", "C", "CS", "I", "S"].iter().zip(h) {
        set_int(r, &format!("{prefix}{suffix}"), v);
    }
}
/// A handle naming nothing.
fn null_handle(r: &mut Record, prefix: &str) {
    set_handle(r, prefix, [NO_HANDLE, 0, 0, 0, 0]);
}

/// A stable, nonzero serial for a new object (Kenshi's own are random).
fn serial(seed: &str) -> i32 {
    let mut h: u32 = 0x811c9dc5;
    for b in seed.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    (h | 1) as i32
}

/// A record renumbered into file `file` as id `id`.
fn renumber(mut r: Record, id: i32, file: &str) -> Record {
    r.id = id;
    r.string_id = format!("{id}-{file}-INGAME");
    r
}

/// One base character: its instance and its records by kind (the items apart).
struct BaseChar {
    instance: Instance,
    by_kind: HashMap<i32, Record>,
    items: Vec<Record>,
    name: String,
}

fn base_characters(f: &File) -> Vec<BaseChar> {
    let by_id: HashMap<&str, &Record> = f.records.iter().map(|r| (r.string_id.as_str(), r)).collect();
    let mut out = Vec::new();
    for coll in f.records.iter().filter(|r| r.kind == INSTANCES) {
        for inst in &coll.instances {
            let mut by_kind = HashMap::new();
            let mut items = Vec::new();
            for s in &inst.states {
                if let Some(r) = by_id.get(s.as_str()) {
                    if r.kind == INVENTORY {
                        // its items, and what its bags hold (a bag's INVENTORY_STATE and items)
                        let mut stack: Vec<&str> = r.instances.iter().map(|i| i.target.as_str()).collect();
                        while let Some(t) = stack.pop() {
                            if let Some(x) = by_id.get(t) {
                                stack.extend(x.instances.iter().map(|i| i.target.as_str()));
                                if x.kind == ITEM_STATE {
                                    items.push((*x).clone());
                                }
                            }
                        }
                    }
                    by_kind.insert(r.kind, (*r).clone());
                }
            }
            let name = by_kind.get(&CHARACTER_STATE).and_then(|g| get_string(g, "name")).unwrap_or_default().to_string();
            out.push(BaseChar { instance: inst.clone(), by_kind, items, name });
        }
    }
    out
}

/// A save in Kenshi's layout written to `dir` over the Kenshi save at `base` (read only).
pub fn write_over(base: &Path, dir: &Path, s: &KenshiSave) -> std::io::Result<()> {
    if base.canonicalize().ok() == dir.canonicalize().ok() {
        return Err(std::io::Error::other("the base save is not written over"));
    }
    std::fs::create_dir_all(dir.join("platoon"))?;
    let read = |p: &Path| kenshi_io::fcs::read(p).map_err(|e| std::io::Error::other(e.to_string()));
    let mut quick = read(&base.join("quick.save"))?;
    let pfaction = s.squads.first().map(|q| q.faction.clone()).unwrap_or_else(|| "204-gamedata.base".to_string());
    // the base's squads of the player's faction, in their squad order
    let mut base_squads: Vec<usize> = (0..quick.records.len())
        .filter(|&i| quick.records[i].kind == PLATOON && get_string(&quick.records[i], "faction stringID") == Some(pfaction.as_str()))
        .collect();
    base_squads.sort_by_key(|&i| get_int(&quick.records[i], "squad index").unwrap_or(i32::MAX));
    // the base's characters of those squads (templates by name)
    let mut templates: Vec<BaseChar> = Vec::new();
    let mut base_file: Option<File> = None;
    for &i in &base_squads {
        let name = quick.records[i].name.clone();
        if let Ok(f) = read(&base.join("platoon").join(format!("{name}.platoon"))) {
            templates.extend(base_characters(&f));
            base_file.get_or_insert(f);
        }
    }
    // everything else copied as it is
    copy_tree(base, dir, &base_squads.iter().map(|&i| format!("{}.platoon", quick.records[i].name)).collect::<Vec<_>>())?;
    // the camera record
    let members: usize = s.squads.iter().map(|q| q.characters.len()).sum();
    if let Some(cam) = quick.records.iter_mut().find(|r| r.kind == CAMERA) {
        set_int(cam, "time day", s.time.0);
        set_int(cam, "time hour", s.time.1);
        set_int(cam, "time minute", s.time.2);
        set_int(cam, "player money", s.money);
        set_int(cam, "squads", s.squads.len() as i32);
        set_int(cam, "members", members as i32);
        set_vec3(cam, "pos", s.camera);
        set_string(cam, "pfaction name", &s.faction_name);
        for h in ["selected_character", "selected_characters0", "selected_gui_object", "tracking"] {
            null_handle(cam, h);
        }
        let zones = s.zones.iter().enumerate().map(|(k, z)| Reference { target: k.to_string(), values: [z.0, z.1, 1] }).collect();
        match cam.references.iter_mut().find(|(n, _)| n == "zones") {
            Some(e) => e.1 = zones,
            None => cam.references.push(("zones".into(), zones)),
        }
    }
    // the research done
    if let Some(r) = quick.records.iter_mut().find(|r| r.kind == RESEARCH) {
        r.strings.retain(|(k, _)| !k.starts_with("finished"));
        for (k, name) in s.research.iter().enumerate() {
            r.strings.push((format!("finished{k}"), name.clone()));
        }
        set_float(r, "num finished", s.research.len() as f32);
    }
    // the factions' relations, by the faction's name
    for (name, rels) in &s.relations {
        let Some(f) = quick.records.iter_mut().find(|r| r.kind == FACTION_STATE && &r.name == name) else { continue };
        let mut next = (0..).find(|k| !f.strings.iter().any(|(n, _)| n == &format!("relationSID{k}"))).unwrap_or(0);
        for (sid, v) in rels {
            let at = f.strings.iter().find(|(n, x)| n.starts_with("relationSID") && x == sid).map(|(n, _)| n["relationSID".len()..].to_string());
            match at {
                Some(k) => set_float(f, &format!("relation{k}"), *v),
                None => {
                    f.strings.push((format!("relationSID{next}"), sid.clone()));
                    f.floats.push((format!("relation{next}"), *v));
                    next += 1;
                }
            }
        }
    }
    // the player's squads: the base's kept as they are, more made from the first, the rest dropped
    let max_c = quick.records.iter().filter(|r| r.kind == PLATOON).filter_map(|r| get_int(r, "handleC")).max().unwrap_or(0);
    let mut next_id = quick.header.id_counter.max(quick.records.iter().map(|r| r.id).max().unwrap_or(0) + 1);
    let mut squad_records: Vec<Record> = Vec::new();
    for (k, sq) in s.squads.iter().enumerate() {
        let mut p = match base_squads.get(k) {
            Some(&i) => quick.records[i].clone(),
            None => {
                let mut p = base_squads.first().map(|&i| quick.records[i].clone()).unwrap_or_default();
                p.kind = PLATOON;
                p.id = next_id;
                next_id += 1;
                set_handle(&mut p, "handle", [PLATOON, max_c + 1 + k as i32, serial(&sq.id), 0, 0]);
                // its tags name the base squad's dealings: none
                p.ints.retain(|(n, _)| !n.starts_with("tag"));
                p.floats.retain(|(n, _)| !n.starts_with("tagtime"));
                p
            }
        };
        p.name = sq.id.clone();
        p.string_id = sq.id.clone();
        set_string(&mut p, "platoon stringID", &sq.id);
        set_string(&mut p, "faction stringID", &sq.faction);
        set_string(&mut p, "faction name", &s.faction_name);
        if !sq.template.is_empty() {
            set_string(&mut p, "squad template", &sq.template);
        }
        set_int(&mut p, "char count", sq.characters.len() as i32);
        set_int(&mut p, "squad index", k as i32);
        set_vec3(&mut p, "position", sq.position);
        set_bool(&mut p, "dead", false);
        squad_records.push(p);
    }
    // the base's player squads out, ours in where the first was
    let at = base_squads.iter().copied().min().unwrap_or(quick.records.len());
    let drop: std::collections::HashSet<usize> = base_squads.iter().copied().collect();
    let mut records: Vec<Record> = Vec::new();
    for (i, r) in std::mem::take(&mut quick.records).into_iter().enumerate() {
        if i == at {
            records.extend(squad_records.iter().cloned());
        }
        if !drop.contains(&i) {
            records.push(r);
        }
    }
    if at >= records.len() + drop.len() {
        records.extend(squad_records.iter().cloned());
    }
    quick.records = records;
    quick.header.record_count = quick.records.len() as i32;
    quick.header.id_counter = next_id;
    kenshi_io::fcs::save(&quick, &dir.join("quick.save"))?;
    // each squad's file
    for (k, sq) in s.squads.iter().enumerate() {
        let p = &squad_records[k];
        let handle = [get_int(p, "handleC").unwrap_or(0), get_int(p, "handleCS").unwrap_or(0)];
        let file = squad_file(sq, &templates, base_file.as_ref(), handle);
        kenshi_io::fcs::save(&file, &dir.join("platoon").join(format!("{}.platoon", sq.id)))?;
    }
    Ok(())
}

/// Copy the base save folder into `dir`, leaving out the player's platoon files `skip`.
fn copy_tree(base: &Path, dir: &Path, skip: &[String]) -> std::io::Result<()> {
    for e in std::fs::read_dir(base)? {
        let e = e?;
        let to = dir.join(e.file_name());
        if e.file_type()?.is_dir() {
            std::fs::create_dir_all(&to)?;
            for f in std::fs::read_dir(e.path())? {
                let f = f?;
                let name = f.file_name().to_string_lossy().into_owned();
                if f.file_type()?.is_file() && !skip.contains(&name) {
                    std::fs::copy(f.path(), to.join(&name))?;
                }
            }
        } else if e.file_name() != "quick.save" {
            std::fs::copy(e.path(), to)?;
        }
    }
    Ok(())
}

/// A squad's platoon file from the base's characters.
fn squad_file(sq: &crate::kenshi_save::KSquad, templates: &[BaseChar], base: Option<&File>, squad: [i32; 2]) -> File {
    let file = format!("{}.platoon", sq.id);
    let mut records: Vec<Record> = Vec::new();
    let mut id = 1;
    let mut take = |r: Record, records: &mut Vec<Record>| {
        id += 1;
        let r = renumber(r, id, &file);
        let sid = r.string_id.clone();
        records.push(r);
        sid
    };
    let mut coll = base.and_then(|f| f.records.iter().find(|r| r.kind == INSTANCES).cloned()).unwrap_or_default();
    coll.kind = INSTANCES;
    coll.instances.clear();
    set_int(&mut coll, "char count", sq.characters.len() as i32);
    let coll_sid = take(coll, &mut records);
    let any_item = templates.iter().flat_map(|t| t.items.iter()).next().cloned();
    let mut instances = Vec::new();
    for (k, c) in sq.characters.iter().enumerate() {
        let tpl = templates.iter().find(|t| t.name == c.name && !c.name.is_empty()).or(templates.first());
        let rec_of = |kind: i32| tpl.and_then(|t| t.by_kind.get(&kind).cloned()).unwrap_or_else(|| Record { kind, name: "0".into(), ..Default::default() });
        let matched = tpl.is_some_and(|t| t.name == c.name);
        let g = character_state(rec_of(CHARACTER_STATE), c, k, squad, matched, &sq.id);
        let st = stats(rec_of(STATS), c);
        let m = medical(rec_of(MEDICAL), c);
        let a = appearance(rec_of(APPEARANCE), c);
        let mut ai = rec_of(AI);
        if !matched {
            set_bool(&mut ai, "jobs", false);
            ai.ints.retain(|(n, _)| !n.starts_with("tjob"));
        }
        // the items, each made from the base's item of the same record (else any)
        let mut inv = rec_of(INVENTORY);
        inv.instances.clear();
        let mut item_sids = Vec::new();
        // an item and what it holds, each made from the base's item of the same record (else any)
        fn put(it: &KItem, tpl: Option<&BaseChar>, any_item: &Option<Record>, take: &mut dyn FnMut(Record, &mut Vec<Record>) -> String, records: &mut Vec<Record>) -> String {
            let t = tpl.and_then(|t| t.items.iter().find(|r| get_string(r, "base data sid") == Some(it.record.as_str())).cloned()).or_else(|| any_item.clone());
            let mut r = item_state(t.unwrap_or_else(|| Record { kind: ITEM_STATE, name: "0".into(), ..Default::default() }), it);
            // a bag: its own INVENTORY_STATE holding its items
            if !it.inside.is_empty() {
                let mut bag = tpl.and_then(|t| t.by_kind.get(&INVENTORY).cloned()).unwrap_or_else(|| Record { kind: INVENTORY, name: "0".into(), ..Default::default() });
                bag.instances = it
                    .inside
                    .iter()
                    .enumerate()
                    .map(|(k, inner)| Instance { id: (k + 1).to_string(), target: put(inner, tpl, any_item, take, records), position: [0.0; 3], rotation: [1.0, 0.0, 0.0, 0.0], states: Vec::new() })
                    .collect();
                let sid = take(bag, records);
                r.instances.push(Instance { id: "1".into(), target: sid, position: [0.0; 3], rotation: [1.0, 0.0, 0.0, 0.0], states: Vec::new() });
            }
            take(r, records)
        }
        for (j, it) in c.items.iter().enumerate() {
            let sid = put(it, tpl, &any_item, &mut take, &mut records);
            item_sids.push((j, sid));
        }
        inv.instances = item_sids.iter().map(|(j, sid)| Instance { id: (j + 1).to_string(), target: sid.clone(), position: [0.0; 3], rotation: [1.0, 0.0, 0.0, 0.0], states: Vec::new() }).collect();
        // the states in the base's order (by kind)
        let order: Vec<i32> = tpl
            .map(|t| t.instance.states.iter().filter_map(|s| t.by_kind.iter().find(|(_, r)| &r.string_id == s).map(|(k, _)| *k)).collect())
            .filter(|o: &Vec<i32>| o.len() == 6)
            .unwrap_or_else(|| vec![CHARACTER_STATE, AI, INVENTORY, MEDICAL, STATS, APPEARANCE]);
        let mut made: HashMap<i32, Record> = [(CHARACTER_STATE, g), (STATS, st), (MEDICAL, m), (APPEARANCE, a), (AI, ai), (INVENTORY, inv)].into_iter().collect();
        let mut states = Vec::new();
        for kind in order {
            if let Some(r) = made.remove(&kind) {
                states.push(take(r, &mut records));
            }
        }
        instances.push(Instance { id: states.first().cloned().unwrap_or_default(), target: c.template.clone(), position: c.position, rotation: c.rotation, states });
    }
    if let Some(coll) = records.iter_mut().find(|r| r.string_id == coll_sid) {
        coll.instances = instances;
    }
    let mut f = base.cloned().unwrap_or_else(|| crate::kenshi_save::v15(Vec::new(), 1));
    f.records = records;
    f.header.record_count = f.records.len() as i32;
    f.header.id_counter = id + 1;
    f
}

fn character_state(mut g: Record, c: &KChar, k: usize, squad: [i32; 2], matched: bool, squad_id: &str) -> Record {
    g.kind = CHARACTER_STATE;
    set_string(&mut g, "name", &c.name);
    set_string(&mut g, "owner faction ID", &c.faction);
    set_int(&mut g, "personality", c.personality);
    set_int(&mut g, "slavestate", c.slave_state);
    set_bool(&mut g, "shaved", c.shaved);
    // its handle: the squad's, its place in it, its own serial (a matched one keeps its own)
    let own = if matched { get_int(&g, "handleS").filter(|s| *s != 0) } else { None };
    set_handle(&mut g, "handle", [1, squad[0], squad[1], k as i32, own.unwrap_or_else(|| serial(&format!("{squad_id}/{}/{k}", c.name)))]);
    // what it carried, was inside, was enslaved by: other objects of the base's, let go
    for h in ["carrying", "in what", "isindoors", "slaver"] {
        null_handle(&mut g, h);
    }
    set_int(&mut g, "in something", 0);
    set_bool(&mut g, "carrying", false);
    set_bool(&mut g, "is leader", k == 0);
    g
}

fn stats(mut st: Record, c: &KChar) -> Record {
    st.kind = STATS;
    st.name = c.name.clone();
    for (k, v) in &c.stats {
        set_float(&mut st, k, *v);
    }
    st
}

fn medical(mut m: Record, c: &KChar) -> Record {
    m.kind = MEDICAL;
    set_bool(&mut m, "dead", c.dead);
    set_bool(&mut m, "unconcious", c.unconscious);
    if !c.unconscious {
        set_bool(&mut m, "coma", false);
        set_bool(&mut m, "incapacitated", false);
    }
    set_float(&mut m, "blood", c.blood);
    set_float(&mut m, "hung", c.hunger);
    set_float(&mut m, "fed", c.fed);
    for (sid, flesh, stun, bandage) in &c.parts {
        let k = match m.strings.iter().find(|(n, v)| n.starts_with("sid") && v == sid) {
            Some((n, _)) => n[3..].to_string(),
            None => {
                let k = (0..).find(|k| !m.strings.iter().any(|(n, _)| n == &format!("sid{k}"))).unwrap_or(0).to_string();
                m.strings.push((format!("sid{k}"), sid.clone()));
                k
            }
        };
        set_float(&mut m, &format!("flesh{k}"), *flesh);
        set_float(&mut m, &format!("stun{k}"), *stun);
        set_float(&mut m, &format!("bandage{k}"), *bandage);
    }
    m
}

fn appearance(mut a: Record, c: &KChar) -> Record {
    a.kind = APPEARANCE;
    let ap = &c.appearance;
    set_bool(&mut a, "sex female", ap.female);
    for (k, v) in &ap.values {
        set_float(&mut a, k, *v);
    }
    for (k, v) in [("head", &ap.head), ("hair style", &ap.hair), ("beard", &ap.beard), ("idle stance", &ap.idle_stance)] {
        set_string(&mut a, k, v.as_deref().unwrap_or(""));
    }
    set_vec3(&mut a, "Skin Tone", ap.skin_tone);
    if let Some(r) = &ap.race {
        let race = vec![Reference { target: r.clone(), values: [0, 0, 0] }];
        match a.references.iter_mut().find(|(n, _)| n == "race") {
            Some(e) => e.1 = race,
            None => a.references.push(("race".into(), race)),
        }
    }
    a
}

fn item_state(mut r: Record, it: &KItem) -> Record {
    r.kind = ITEM_STATE;
    set_bool(&mut r, "in inventory", true);
    set_bool(&mut r, "death", false);
    set_int(&mut r, "inventory x", it.x);
    set_int(&mut r, "inventory y", it.y);
    set_int(&mut r, "quantity", it.quantity);
    set_float(&mut r, "quality", it.quality);
    set_float(&mut r, "charges", it.charges);
    for (k, v) in [("base data sid", &it.record), ("section", &it.section), ("company sid", &it.maker), ("material sid", &it.material), ("uniform", &it.uniform)] {
        set_string(&mut r, k, v);
    }
    // its owner and the building it lay in: the base's, let go; nothing inside it
    null_handle(&mut r, "ownedby");
    null_handle(&mut r, "insideBuilding");
    r.instances.clear();
    r
}
