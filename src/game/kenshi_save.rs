//! Kenshi's own save format (port_map `saves`): a save folder holding `quick.save` (the world:
//! the camera record with the game time, "player money", the zones loaded and the player
//! faction's name; research; every squad's PLATOON record; factions, towns, wars) and
//! `platoon/<squad>.platoon` per squad (an INSTANCE_COLLECTION of the squad's characters, each
//! instance the character's CHARACTER template, world position and rotation and its state
//! records: GAMESTATE_CHARACTER, STATS, MEDICAL_STATE, CHARACTER_APPEARANCE, INVENTORY_STATE
//! with its INVENTORY_ITEM_STATEs, GAMESTATE_AI), plus `zone/zone.X.Y.zone` per zone loaded.
//! All are FCS v15 files (kenshi_io::fcs).
//!
//! Read: the player's squads (the platoons of the camera record's "pfaction name" faction) with
//! their characters, the money, the time, the research done, the zones, each faction's relations
//! (GamestateFaction: "relationSID<n>" / "relation<n>"), everyone dead in any squad, and the
//! player's buildings (the zones' "buildinglist" instances whose BUILDING state's "owner faction
//! ID" is the player's). Written: the same records for the player's squads, money, time,
//! research, zones and relations (the world's other squads, towns and zone files are not: the
//! rewrite keeps its whole world in its own file beside them, crate::save).
//!
//! Importing (`Sim::apply_kenshi`, Kenshi_ImportGamePanel's ticks) puts the save on a world
//! built fresh at its squad's zone: the characters as they were, the money, the time, and as
//! ticked the research, the relations, the buildings and the dead unique characters (kept dead).
//! (stand-in) The buildings come without their storage's contents or their interiors. (assumption) Kenshi's handle fields (the "...CS"/"...S"/"...I" ints linking
//! records at run time) are written as 0: whether Kenshi itself would load such a save is not
//! known.

use kenshi_io::fcs::{File, Header, Instance, Record, Reference};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct KItem {
    /// The item record (string id), its inventory section ("main", "hip", "armour"...) and cell.
    pub record: String,
    pub section: String,
    pub x: i32,
    pub y: i32,
    pub quantity: i32,
    pub quality: f32,
    pub charges: f32,
    /// A weapon's maker ("company sid"), armour's material ("material sid"), a uniform's faction.
    pub maker: String,
    pub material: String,
    pub uniform: String,
}

#[derive(Debug, Clone, Default)]
pub struct KChar {
    /// The CHARACTER template, its name, world position (units) and rotation (w, x, y, z).
    pub template: String,
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub faction: String,
    /// STATS by key ("strength", "attack", ... as kenshi_char::stats::stat_of_key reads them).
    pub stats: Vec<(String, f32)>,
    /// MEDICAL_STATE: each part (LOCATIONAL_DAMAGE id) with its flesh, stun and bandage; blood,
    /// the hunger ("hung") and stomach ("fed"), knocked out / dead.
    pub parts: Vec<(String, f32, f32, f32)>,
    pub blood: f32,
    pub hunger: f32,
    pub fed: f32,
    pub unconscious: bool,
    pub dead: bool,
    /// CHARACTER_APPEARANCE: race, sex, head, hair, beard, sliders, idle stance.
    pub appearance: crate::body_file::BodyFile,
    pub personality: i32,
    pub slave_state: i32,
    pub shaved: bool,
    pub items: Vec<KItem>,
}

#[derive(Debug, Clone, Default)]
pub struct KSquad {
    pub id: String,
    pub faction: String,
    pub template: String,
    pub position: [f32; 3],
    pub characters: Vec<KChar>,
}

/// A building of the player's (a zone's "buildinglist" instance whose state's "owner faction
/// ID" is the player's): its BUILDING record, place, rotation (w, x, y, z), and how far built.
#[derive(Debug, Clone, Default)]
pub struct KBuilding {
    pub record: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub progress: f32,
    pub complete: bool,
}

#[derive(Debug, Clone, Default)]
pub struct KenshiSave {
    /// Game day, hour, minute.
    pub time: (i32, i32, i32),
    pub money: i32,
    pub camera: [f32; 3],
    pub zones: Vec<(i32, i32)>,
    pub faction_name: String,
    pub version: String,
    pub research: Vec<String>,
    pub squads: Vec<KSquad>,
    /// GamestateFaction: each faction (by name) and its relations to others (string id, value).
    pub relations: Vec<(String, Vec<(String, f32)>)>,
    /// The CHARACTER templates of everyone dead in any squad.
    pub dead: Vec<String>,
    pub buildings: Vec<KBuilding>,
}

fn int(r: &Record, k: &str) -> Option<i32> {
    r.ints.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
}
fn float(r: &Record, k: &str) -> Option<f32> {
    r.floats.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
}
fn string(r: &Record, k: &str) -> Option<String> {
    r.strings.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}
fn boolean(r: &Record, k: &str) -> bool {
    r.bools.iter().any(|(n, v)| n == k && *v)
}

/// Record kinds (itemType) the save uses.
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
const BUILDING_STATE: i32 = 35;

/// A Kenshi save folder read (the player's squads only).
pub fn read(dir: &Path) -> Result<KenshiSave, String> {
    let quick = kenshi_io::fcs::read(&dir.join("quick.save")).map_err(|e| format!("quick.save: {e}"))?;
    let mut out = KenshiSave::default();
    if let Some(c) = quick.records.iter().find(|r| r.kind == CAMERA) {
        out.time = (int(c, "time day").unwrap_or(1), int(c, "time hour").unwrap_or(12), int(c, "time minute").unwrap_or(0));
        out.money = int(c, "player money").unwrap_or(0);
        out.camera = c.vec3s.iter().find(|(n, _)| n == "pos").map_or([0.0; 3], |(_, v)| *v);
        out.faction_name = string(c, "pfaction name").unwrap_or_default();
        out.version = string(c, "version").unwrap_or_default();
        out.zones = c.references.iter().filter(|(cat, _)| cat == "zones").flat_map(|(_, v)| v.iter().map(|x| (x.values[0], x.values[1]))).collect();
    }
    if let Some(r) = quick.records.iter().find(|r| r.kind == RESEARCH) {
        out.research = r.strings.iter().filter(|(n, _)| n.starts_with("finished")).map(|(_, v)| v.clone()).collect();
    }
    for f in quick.records.iter().filter(|r| r.kind == FACTION_STATE) {
        let rel = f
            .strings
            .iter()
            .filter_map(|(k, sid)| Some((sid.clone(), float(f, &format!("relation{}", k.strip_prefix("relationSID")?))?)))
            .collect();
        out.relations.push((f.name.clone(), rel));
    }
    for p in quick.records.iter().filter(|r| r.kind == PLATOON) {
        let id = string(p, "platoon stringID").unwrap_or_else(|| p.name.clone());
        if string(p, "faction name").as_deref() != Some(out.faction_name.as_str()) {
            // another squad: only its dead
            let Ok(f) = kenshi_io::fcs::read(&dir.join("platoon").join(format!("{id}.platoon"))) else { continue };
            let by_id: std::collections::HashMap<&str, &Record> = f.records.iter().map(|r| (r.string_id.as_str(), r)).collect();
            for coll in f.records.iter().filter(|r| r.kind == INSTANCES) {
                for inst in &coll.instances {
                    if inst.states.iter().filter_map(|s| by_id.get(s.as_str())).any(|r| r.kind == MEDICAL && boolean(r, "dead")) {
                        out.dead.push(inst.target.clone());
                    }
                }
            }
            continue;
        }
        let mut squad = KSquad {
            id: id.clone(),
            faction: string(p, "faction stringID").unwrap_or_default(),
            template: string(p, "squad template").unwrap_or_default(),
            position: p.vec3s.iter().find(|(n, _)| n == "position").map_or([0.0; 3], |(_, v)| *v),
            characters: Vec::new(),
        };
        let Ok(f) = kenshi_io::fcs::read(&dir.join("platoon").join(format!("{id}.platoon"))) else { continue };
        let by_id: std::collections::HashMap<&str, &Record> = f.records.iter().map(|r| (r.string_id.as_str(), r)).collect();
        let Some(coll) = f.records.iter().find(|r| r.kind == INSTANCES) else { continue };
        for inst in &coll.instances {
            let states: Vec<&Record> = inst.states.iter().filter_map(|s| by_id.get(s.as_str()).copied()).collect();
            let of = |k: i32| states.iter().find(|r| r.kind == k).copied();
            let mut c = KChar { template: inst.target.clone(), position: inst.position, rotation: inst.rotation, faction: squad.faction.clone(), ..Default::default() };
            if let Some(g) = of(CHARACTER_STATE) {
                c.name = string(g, "name").unwrap_or_default();
                c.personality = int(g, "personality").unwrap_or(0);
                c.slave_state = int(g, "slavestate").unwrap_or(0);
                c.shaved = boolean(g, "shaved");
            }
            if let Some(s) = of(STATS) {
                c.stats = s.floats.clone();
            }
            if let Some(m) = of(MEDICAL) {
                let parts = (0..32).filter_map(|i| {
                    let sid = string(m, &format!("sid{i}"))?;
                    Some((sid, float(m, &format!("flesh{i}")).unwrap_or(0.0), float(m, &format!("stun{i}")).unwrap_or(0.0), float(m, &format!("bandage{i}")).unwrap_or(0.0)))
                });
                c.parts = parts.collect();
                c.blood = float(m, "blood").unwrap_or(100.0);
                c.hunger = float(m, "hung").unwrap_or(2.0);
                c.fed = float(m, "fed").unwrap_or(0.0);
                c.unconscious = boolean(m, "unconcious");
                c.dead = boolean(m, "dead");
            }
            if let Some(a) = of(APPEARANCE) {
                let st = |k: &str| string(a, k).filter(|v| !v.is_empty());
                c.appearance = crate::body_file::BodyFile {
                    race: a.references.iter().find(|(cat, _)| cat == "race").and_then(|(_, v)| v.first()).map(|x| x.target.clone()),
                    female: boolean(a, "sex female"),
                    head: st("head"),
                    hair: st("hair style"),
                    beard: st("beard"),
                    values: a.floats.clone(),
                    skin_tone: a.vec3s.iter().find(|(n, _)| n == "Skin Tone").map_or([1.0; 3], |(_, v)| *v),
                    idle_stance: st("idle stance"),
                };
            }
            if let Some(inv) = of(INVENTORY) {
                for i in &inv.instances {
                    let Some(it) = by_id.get(i.target.as_str()) else { continue };
                    c.items.push(KItem {
                        record: string(it, "base data sid").unwrap_or_default(),
                        section: string(it, "section").unwrap_or_else(|| "main".into()),
                        x: int(it, "inventory x").unwrap_or(0),
                        y: int(it, "inventory y").unwrap_or(0),
                        quantity: int(it, "quantity").unwrap_or(1),
                        quality: float(it, "quality").unwrap_or(0.0),
                        charges: float(it, "charges").unwrap_or(1.0),
                        maker: string(it, "company sid").unwrap_or_default(),
                        material: string(it, "material sid").unwrap_or_default(),
                        uniform: string(it, "uniform").unwrap_or_default(),
                    });
                }
            }
            squad.characters.push(c);
        }
        out.dead.extend(squad.characters.iter().filter(|c| c.dead).map(|c| c.template.clone()));
        out.squads.push(squad);
    }
    // the player's buildings, zone by zone
    let player = out.squads.first().map_or(crate::sim::PLAYER_FACTION.to_string(), |s| s.faction.clone());
    if let Ok(zones) = std::fs::read_dir(dir.join("zone")) {
        for z in zones.flatten() {
            let Ok(f) = kenshi_io::fcs::read(&z.path()) else { continue };
            let by_id: std::collections::HashMap<&str, &Record> = f.records.iter().map(|r| (r.string_id.as_str(), r)).collect();
            for coll in f.records.iter().filter(|r| r.kind == INSTANCES) {
                for inst in &coll.instances {
                    let Some(st) = inst.states.iter().filter_map(|s| by_id.get(s.as_str())).find(|r| r.kind == BUILDING_STATE) else { continue };
                    if string(st, "owner faction ID").as_deref() != Some(player.as_str()) || boolean(st, "destroyed") {
                        continue;
                    }
                    out.buildings.push(KBuilding {
                        record: inst.target.clone(),
                        position: inst.position,
                        rotation: inst.rotation,
                        progress: float(st, "construction progress").unwrap_or(1.0),
                        complete: boolean(st, "is complete"),
                    });
                }
            }
        }
    }
    Ok(out)
}

/// A record of `kind` with numeric id `id` in file `file` (its string id "id-file-INGAME").
fn rec(kind: i32, id: i32, file: &str, name: &str) -> Record {
    Record { lead: 0, kind, id, name: name.to_string(), string_id: format!("{id}-{file}-INGAME"), datatype: 0, ..Default::default() }
}

fn v15(records: Vec<Record>, next: i32) -> File {
    File {
        header: Header { filetype: 15, version: 0, author: String::new(), description: String::new(), dependencies: String::new(), references: String::new(), tail: Vec::new(), id_counter: next, record_count: records.len() as i32 },
        records,
        trailer: Vec::new(),
        trailers: Vec::new(),
        has_trailer: false,
        latin1: Default::default(),
    }
}

/// A save written as Kenshi lays one out (see the module notes).
pub fn write(dir: &Path, s: &KenshiSave) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("platoon"))?;
    let mut next = 1;
    let mut quick = Vec::new();
    // the camera record: time, money, camera, zones, the player faction
    let mut cam = rec(CAMERA, next, "quick.save", "0");
    next += 1;
    cam.ints = vec![("time day".into(), s.time.0), ("time hour".into(), s.time.1), ("time minute".into(), s.time.2), ("player money".into(), s.money)];
    cam.strings = vec![("version".into(), s.version.clone()), ("pfaction name".into(), s.faction_name.clone())];
    cam.vec3s = vec![("pos".into(), s.camera)];
    cam.references = vec![("zones".into(), s.zones.iter().enumerate().map(|(k, z)| Reference { target: k.to_string(), values: [z.0, z.1, 1] }).collect())];
    quick.push(cam);
    let mut research = rec(RESEARCH, next, "quick.save", "0");
    next += 1;
    research.floats = vec![("num finished".into(), s.research.len() as f32), ("num currents".into(), 0.0)];
    research.strings = s.research.iter().enumerate().map(|(k, r)| (format!("finished{k}"), r.clone())).collect();
    quick.push(research);
    // the factions' relations (GamestateFaction, by faction name)
    for (name, rels) in &s.relations {
        let mut f = rec(FACTION_STATE, next, "quick.save", name);
        next += 1;
        f.ints = vec![("platoonIDs".into(), 0), ("rank".into(), 0)];
        for (k, (sid, v)) in rels.iter().enumerate() {
            f.strings.push((format!("relationSID{k}"), sid.clone()));
            f.floats.push((format!("relation{k}"), *v));
        }
        quick.push(f);
    }
    for sq in &s.squads {
        let mut p = rec(PLATOON, next, "quick.save", &sq.id);
        p.string_id = sq.id.clone();
        next += 1;
        p.bools = vec![("persistent".into(), true), ("dead".into(), false), ("intact".into(), true)];
        p.ints = vec![("char count".into(), sq.characters.len() as i32)];
        p.strings = vec![("faction name".into(), s.faction_name.clone()), ("faction stringID".into(), sq.faction.clone()), ("squad template".into(), sq.template.clone()), ("platoon stringID".into(), sq.id.clone())];
        p.vec3s = vec![("position".into(), sq.position)];
        quick.push(p);
        // the squad's file
        let file = format!("{}.platoon", sq.id);
        let mut recs = Vec::new();
        let mut n = 1;
        let mut take = |kind: i32, name: &str| {
            n += 1;
            rec(kind, n, &file, name)
        };
        let mut coll = take(INSTANCES, "0");
        coll.ints = vec![("char count".into(), sq.characters.len() as i32)];
        for c in &sq.characters {
            let mut g = take(CHARACTER_STATE, "0");
            g.bools = vec![("shaved".into(), c.shaved)];
            g.ints = vec![("personality".into(), c.personality), ("slavestate".into(), c.slave_state)];
            g.strings = vec![("name".into(), c.name.clone()), ("owner faction ID".into(), c.faction.clone())];
            let mut st = take(STATS, &c.name);
            st.floats = c.stats.clone();
            let mut m = take(MEDICAL, "0");
            m.bools = vec![("dead".into(), c.dead), ("unconcious".into(), c.unconscious)];
            for (k, (sid, flesh, stun, bandage)) in c.parts.iter().enumerate() {
                m.strings.push((format!("sid{k}"), sid.clone()));
                m.floats.push((format!("flesh{k}"), *flesh));
                m.floats.push((format!("stun{k}"), *stun));
                m.floats.push((format!("bandage{k}"), *bandage));
            }
            m.floats.extend([("blood".into(), c.blood), ("hung".into(), c.hunger), ("fed".into(), c.fed)]);
            let mut a = take(APPEARANCE, "0");
            let ap = &c.appearance;
            a.bools = vec![("sex female".into(), ap.female)];
            a.floats = ap.values.clone();
            a.strings = [("head", &ap.head), ("hair style", &ap.hair), ("beard", &ap.beard), ("idle stance", &ap.idle_stance)]
                .iter()
                .map(|(k, v)| (k.to_string(), (*v).clone().unwrap_or_default()))
                .collect();
            a.vec3s = vec![("Skin Tone".into(), ap.skin_tone)];
            if let Some(r) = &ap.race {
                a.references = vec![("race".into(), vec![Reference { target: r.clone(), values: [0, 0, 0] }])];
            }
            let mut inv = take(INVENTORY, "0");
            let mut items = Vec::new();
            for (k, it) in c.items.iter().enumerate() {
                let mut r = take(ITEM_STATE, "0");
                r.bools = vec![("in inventory".into(), true)];
                r.ints = vec![("inventory x".into(), it.x), ("inventory y".into(), it.y), ("quantity".into(), it.quantity)];
                r.floats = vec![("quality".into(), it.quality), ("charges".into(), it.charges)];
                r.strings = vec![
                    ("base data sid".into(), it.record.clone()),
                    ("section".into(), it.section.clone()),
                    ("company sid".into(), it.maker.clone()),
                    ("material sid".into(), it.material.clone()),
                    ("uniform".into(), it.uniform.clone()),
                ];
                inv.instances.push(Instance { id: (k + 1).to_string(), target: r.string_id.clone(), position: [0.0; 3], rotation: [1.0, 0.0, 0.0, 0.0], states: Vec::new() });
                items.push(r);
            }
            let ai = take(AI, "ai");
            coll.instances.push(Instance {
                id: g.string_id.clone(),
                target: c.template.clone(),
                position: c.position,
                rotation: c.rotation,
                states: vec![g.string_id.clone(), ai.string_id.clone(), inv.string_id.clone(), m.string_id.clone(), st.string_id.clone(), a.string_id.clone()],
            });
            recs.extend([g, st, m, a, inv, ai]);
            recs.extend(items);
        }
        recs.insert(0, coll);
        let f = v15(recs, n + 1);
        kenshi_io::fcs::save(&f, &dir.join("platoon").join(&file))?;
    }
    kenshi_io::fcs::save(&v15(quick, next), &dir.join("quick.save"))
}

/// Kenshi's import options (Kenshi_ImportGamePanel: "Reset squad positions", "Import
/// Buildings", "Import Research", "Import Dead NPCs", "Import Relations").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImportOptions {
    pub reset_positions: bool,
    pub buildings: bool,
    pub research: bool,
    pub dead_npcs: bool,
    pub relations: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions { reset_positions: false, buildings: true, research: true, dead_npcs: true, relations: true }
    }
}

/// The STATS keys a save holds (kenshi_char::stats::stat_of_key reads them).
const STAT_KEYS: [&str; 35] = [
    "strength", "attack", "labouring", "science", "engineer", "robotics", "weapon smith", "armour smith", "medic", "thievery", "turrets", "farming",
    "cooking", "stealth", "athletics", "dexterity", "defence", "toughness2", "assassin", "swimming", "perception", "katana", "sabres", "hackers",
    "heavy weapons", "blunt", "unarmed", "mass combat", "dodge", "survival", "poles", "bow", "ff", "lockpicking", "bow smith",
];

impl crate::sim::Sim {
    /// "Reset squad positions": the player's characters gathered round the first of them.
    /// (assumption) Kenshi's own gathering place is not decompiled: four abreast, 8 units
    /// apart.
    pub fn gather_squad(&mut self) {
        let ids: Vec<usize> = self.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
        let Some(&first) = ids.first() else { return };
        let at = self.folk[first].mover.position;
        let step = 8.0 * kenshi_world::navmesh::METRES;
        for (n, id) in ids.into_iter().enumerate() {
            let to = [at[0] + (n % 4) as f32 * step, at[1], at[2] + (n / 4) as f32 * step];
            self.folk[id].mover.position = to;
            self.arena.f[id].mover.position = to;
        }
    }

    /// The player squad, money, time, research and zones as a Kenshi save. `look` gives each
    /// player character its appearance (race, sex, head, hair, beard, sliders).
    pub fn kenshi_snapshot(&self, look: impl Fn(usize) -> Option<crate::body_file::BodyFile>) -> KenshiSave {
        let hours = self.start_hour() + self.hours_gone();
        // to the nearest minute (the game's clock in f32 seconds drifts under it)
        let minutes = (hours * 60.0).round() as i64;
        let day = (minutes / (24 * 60)) as i32 + 1;
        let hour = (minutes / 60 % 24) as i32;
        let minute = (minutes % 60) as i32;
        let players: Vec<&crate::sim::Person> = self.folk.iter().filter(|p| p.player).collect();
        let first = players.first().map_or([0.0; 3], |p| p.mover.position_units());
        let faction_name = self.data.get(crate::sim::PLAYER_FACTION).map_or("Nameless".to_string(), |f| f.name.clone());
        let characters = players
            .iter()
            .map(|p| {
                let f = &self.arena.f[p.id];
                let yaw = p.facing[0].atan2(p.facing[2]);
                let m = &f.medical;
                let items = p
                    .inventory
                    .sections
                    .iter()
                    .flat_map(|s| {
                        s.items.iter().map(move |it| KItem {
                            record: it.item.record.clone(),
                            section: s.name.to_string(),
                            x: it.x as i32,
                            y: it.y as i32,
                            quantity: it.item.quantity as i32,
                            quality: it.item.quality,
                            charges: it.item.food.or(it.item.ammo).map_or(1.0, |(_, l)| l),
                            maker: it.item.maker.clone().unwrap_or_default(),
                            material: it.item.model.clone().unwrap_or_default(),
                            uniform: it.item.uniform.clone().unwrap_or_default(),
                        })
                    })
                    .collect();
                KChar {
                    template: p.character.clone().unwrap_or_default(),
                    name: p.name.clone(),
                    position: p.mover.position_units(),
                    rotation: [(yaw * 0.5).cos(), 0.0, (yaw * 0.5).sin(), 0.0],
                    faction: crate::sim::PLAYER_FACTION.to_string(),
                    stats: STAT_KEYS
                        .iter()
                        .filter_map(|k| Some((k.to_string(), *f.skills.levels.get(&kenshi_char::stats::stat_of_key(k)?)?)))
                        .collect(),
                    parts: m.parts.iter().map(|q| (q.id.clone(), q.flesh, q.stun, q.treatment)).collect(),
                    blood: m.blood,
                    hunger: m.hunger.level,
                    fed: m.hunger.stomach,
                    unconscious: p.down,
                    dead: p.dead,
                    appearance: look(p.id).unwrap_or_default(),
                    personality: p.personality,
                    // (assumption) 1 a slave, 2 one that has escaped
                    slave_state: if p.slave.is_some() {
                        1
                    } else if p.escaped.is_some() {
                        2
                    } else {
                        0
                    },
                    shaved: p.shaved,
                    items,
                }
            })
            .collect();
        KenshiSave {
            time: (day, hour, minute),
            money: self.money,
            camera: first,
            zones: self.zone_order.clone(),
            squads: vec![KSquad {
                id: format!("{faction_name}_0"),
                faction: crate::sim::PLAYER_FACTION.to_string(),
                template: String::new(),
                position: first,
                characters,
            }],
            faction_name,
            version: "1.0.68".into(),
            research: self.research_done.clone(),
            relations: {
                let mut v: Vec<(String, Vec<(String, f32)>)> = self
                    .relations
                    .values()
                    .filter_map(|r| {
                        let mut rels: Vec<(String, f32)> = r.relations.iter().map(|(o, x)| (o.clone(), x.relation)).collect();
                        rels.sort_by(|a, b| a.0.cmp(&b.0));
                        Some((self.data.get(&r.faction)?.name.clone(), rels))
                    })
                    .collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                v
            },
            dead: {
                let mut d: Vec<String> = self.world.dead_uniques.iter().cloned().collect();
                d.sort();
                d
            },
            buildings: self
                .buildings
                .iter()
                .filter(|b| b.confirmed)
                .map(|b| {
                    let total: u32 = b.materials.iter().map(|(_, n)| n).sum();
                    KBuilding {
                        record: b.record.clone(),
                        position: b.at,
                        rotation: [(b.yaw * 0.5).cos(), 0.0, (b.yaw * 0.5).sin(), 0.0],
                        progress: if b.done || total == 0 { 1.0 } else { b.progress / total as f32 },
                        complete: b.done,
                    }
                })
                .collect(),
        }
    }

    /// A Kenshi save's player squads put on this world's player characters (as many as both
    /// have, in order): names, templates, places (gathered at the first one's with
    /// `reset_positions`), stats, wounds, hunger, kit, sex, race, personality; the money, the
    /// time, and the research (with `research`). Returns each character's appearance.
    pub fn apply_kenshi(&mut self, k: &KenshiSave, opt: &ImportOptions) -> Vec<(usize, crate::body_file::BodyFile)> {
        use kenshi_world::navmesh::METRES;
        let data = self.data.clone();
        let ids: Vec<usize> = self.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
        let chars: Vec<&KChar> = k.squads.iter().flat_map(|s| &s.characters).collect();
        let first = chars.first().map_or([0.0; 3], |c| c.position);
        let mut out = Vec::new();
        for (n, (&id, c)) in ids.iter().zip(&chars).enumerate() {
            let _ = (n, first);
            let at = c.position;
            let p = &mut self.folk[id];
            p.name = c.name.clone();
            if data.get(&c.template).is_some() {
                p.character = Some(c.template.clone());
            }
            p.mover.position = at.map(|v| v * METRES);
            // the rotation (w, x, y, z) about y as a facing
            let yaw = 2.0 * c.rotation[2].atan2(c.rotation[0]);
            p.facing = [yaw.sin(), 0.0, yaw.cos()];
            p.down = c.unconscious;
            p.dead = c.dead;
            p.personality = c.personality;
            p.shaved = c.shaved;
            p.female = Some(c.appearance.female);
            p.look_race = c.appearance.race.clone();
            p.ai.queue.clear();
            // the kit, each item in its section and cell (else anywhere it fits)
            let mut inv = kenshi_char::inventory::Inventory::character();
            for it in &c.items {
                let Some(r) = data.get(&it.record) else { continue };
                let mut item = kenshi_char::inventory::Item::from_record(r, it.quantity.max(1) as u32);
                if let Some((full, _)) = item.food {
                    item.food = Some((full, it.charges));
                }
                if let Some((full, _)) = item.ammo {
                    item.ammo = Some((full, it.charges));
                }
                item.maker = (!it.maker.is_empty()).then(|| it.maker.clone());
                item.model = (!it.material.is_empty()).then(|| it.material.clone());
                item.uniform = (!it.uniform.is_empty()).then(|| it.uniform.clone());
                let item = item.with_quality(&data, it.quality);
                let left = match inv.section(&it.section) {
                    Some(s) => inv.put(s, item, it.x.max(0) as u32, it.y.max(0) as u32).err(),
                    None => Some(item),
                };
                if let Some(item) = left {
                    inv.add(item, false);
                }
            }
            p.inventory = inv;
            // the body: wounds, blood, hunger, stats
            let f = &mut self.arena.f[id];
            f.mover.position = at.map(|v| v * METRES);
            f.down = c.unconscious;
            for (sid, flesh, stun, bandage) in &c.parts {
                if let Some(part) = f.medical.parts.iter_mut().find(|q| &q.id == sid) {
                    part.flesh = *flesh;
                    part.stun = *stun;
                    part.treatment = *bandage;
                }
            }
            f.medical.blood = c.blood;
            f.medical.hunger.level = c.hunger;
            f.medical.hunger.stomach = c.fed;
            let race = c.appearance.race.as_deref().and_then(|r| data.get(r)).or_else(|| data.get(&self.race_record)).cloned();
            let mut skills = f.skills.clone();
            skills.levels = c.stats.iter().filter_map(|(key, v)| Some((kenshi_char::stats::stat_of_key(key)?, *v))).collect();
            if let Some(race) = &race {
                skills.race_xp = kenshi_char::stats::race_multipliers(race);
                f.set_skills(&data, race, skills);
            }
            self.equip(id);
            out.push((id, c.appearance.clone()));
        }
        if opt.reset_positions {
            self.gather_squad();
        }
        self.money = k.money;
        let hours = ((k.time.0 - 1).max(0) * 24 + k.time.1) as f64 + k.time.2 as f64 / 60.0;
        self.t = ((hours - self.start_hour()).max(0.0) * self.hour_secs()) as f32;
        if opt.research {
            self.research_done = k.research.iter().filter(|r| data.get(r).is_some()).cloned().collect();
        }
        if opt.relations {
            for (name, rels) in &k.relations {
                let Some(fr) = data.of_type(kenshi_data::ItemType::Faction).find(|f| &f.name == name) else { continue };
                let r = self.relations.entry(fr.string_id.clone()).or_insert_with(|| kenshi_ai::faction::FactionRelations::from_data(fr, &data));
                for (other, v) in rels {
                    if data.get(other).is_some() {
                        let forced_ally = r.relations.get(other).is_some_and(|x| x.forced_ally);
                        r.relations.insert(other.clone(), kenshi_ai::faction::Relation { forced_ally, relation: *v });
                    }
                }
            }
        }
        if opt.dead_npcs {
            for c in &k.dead {
                if data.get(c).is_some_and(|r| r.bools.get("unique").copied().unwrap_or(false)) {
                    self.world.dead_uniques.insert(c.clone());
                }
            }
        }
        if opt.buildings {
            for b in &k.buildings {
                let Some(rec) = data.get(&b.record) else { continue };
                // the rotation (w, x, y, z) about y
                let yaw = 2.0 * b.rotation[2].atan2(b.rotation[0]);
                let mut pb = crate::base::PlayerBuilding::new(rec, b.position, yaw);
                let total: u32 = pb.materials.iter().map(|(_, n)| n).sum();
                pb.confirmed = true;
                pb.done = b.complete || total == 0;
                let p = if pb.done { 1.0 } else { b.progress.clamp(0.0, 1.0) };
                pb.progress = p * total as f32;
                pb.used = (p * total as f32).ceil() as u32;
                self.buildings.push(pb);
            }
        }
        out
    }
}
