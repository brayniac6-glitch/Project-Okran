//! Kenshi's world (port_map `terrain`): the terrain heightmap in world units.
//!
//! Spec: sheet row T00022 TerrainHeightmap and initTerrain (F046700): the heightmap
//! `data/newland/land/fullmap.tif` is a 16385 x 16385 grid of 16-bit heights spanning
//! `WORLD_SIZE` units, values scaled to `HEIGHT_RANGE`.

pub mod foliage;
pub mod biome;
pub mod nest;
pub mod furniture;
pub mod interior;
pub mod navmesh;
pub mod navcut;
pub mod nodes;
pub mod longpath;
pub mod resources;
pub mod terrain_biome;
pub mod town;
pub mod weather;

use kenshi_io::tiff::{Error, Tiff};
use std::path::Path;

/// World width and depth in units (`setTerrainScale` first argument).
pub const WORLD_SIZE: f32 = 294_912.0;
/// Height of the largest heightmap value in units (`setTerrainScale` second argument).
pub const HEIGHT_RANGE: f32 = 9_800.0;
/// The heightmap, relative to the install root.
pub const HEIGHTMAP: &str = "data/newland/land/fullmap.tif";
/// Painted terrain colour, 8 x 8 tiles of 2048 x 2048 covering the heightmap one-to-one:
/// `colour.X.Y.png`, X = tile column (world x), Y = tile row (world z).
pub const COLOUR_TILE_PX: u32 = 2048;
pub fn colour_tile(x: u32, y: u32) -> String {
    format!("data/newland/land/overlaymaps/colour.{x}.{y}.png")
}

/// Samples per side of a cached heightmap tile.
const TILE: u32 = 64;

pub struct Terrain {
    map: Tiff,
    /// Heightmap tiles read so far ([`Terrain::height_at`]).
    tiles: std::collections::HashMap<(u32, u32), Vec<u16>>,
}

/// A rectangular patch of heights, `w` x `h` samples spaced `spacing` units apart, starting at
/// world (`x0`, `z0`). Heights are in units, row-major (z rows, x columns).
#[derive(Debug, Clone)]
pub struct Patch {
    /// First heightmap sample (column, row) and the sample step between vertices.
    pub sx: u32,
    pub sy: u32,
    pub step: u32,
    pub x0: f32,
    pub z0: f32,
    pub spacing: f32,
    pub w: u32,
    pub h: u32,
    pub heights: Vec<f32>,
}

impl Patch {
    pub fn at(&self, i: u32, j: u32) -> f32 {
        self.heights[(j * self.w + i) as usize]
    }
}

/// Side of one zone cell in units (64 x 64 zones cover the world).
pub const ZONE_SIZE: f32 = WORLD_SIZE / 64.0;

/// Zone (X, Y) of a world position; zones are named `zone.X.Y.zone` (sheet row T00023).
pub fn zone_of(x: f32, z: f32) -> (i32, i32) {
    (
        ((x + WORLD_SIZE / 2.0) / ZONE_SIZE).floor() as i32,
        ((z + WORLD_SIZE / 2.0) / ZONE_SIZE).floor() as i32,
    )
}

/// Folders under data/newland/leveldata holding zone files, base game first (later ones win).
pub const ZONE_FOLDERS: [&str; 3] = ["Newwworld", "Dialogue", "rebirth"];

/// The zone file for zone X.Y: the last folder that has one.
pub fn zone_file(install: &Path, x: i32, y: i32) -> Option<std::path::PathBuf> {
    let name = format!("zone.{x}.{y}.zone");
    ZONE_FOLDERS
        .iter()
        .rev()
        .map(|f| install.join("data/newland/leveldata").join(f).join(&name))
        .find(|p| p.is_file())
}

/// Zone X.Y as the game has it: its file in every leveldata folder that has one, applied in
/// folder order (Newwworld, Dialogue, rebirth), each on the ones before as the game data's mod
/// files are (GameDataContainer::apply_file): a record already there takes the later file's
/// fields over its own and its instances by id (an instance with no target removed), a record
/// marked "REMOVED" goes, others are added.
pub fn read_zone(install: &Path, x: i32, y: i32) -> Option<kenshi_io::fcs::File> {
    let name = format!("zone.{x}.{y}.zone");
    let mut out: Option<kenshi_io::fcs::File> = None;
    for f in ZONE_FOLDERS {
        let p = install.join("data/newland/leveldata").join(f).join(&name);
        if !p.is_file() {
            continue;
        }
        let Ok(file) = kenshi_io::fcs::read(&p) else { continue };
        match &mut out {
            None => out = Some(file),
            Some(base) => merge_zone(base, file),
        }
    }
    out
}

fn merge_zone(base: &mut kenshi_io::fcs::File, later: kenshi_io::fcs::File) {
    fn set<T: Clone>(list: &mut Vec<(String, T)>, add: &[(String, T)]) {
        for (k, v) in add {
            match list.iter_mut().find(|(n, _)| n == k) {
                Some(e) => e.1 = v.clone(),
                None => list.push((k.clone(), v.clone())),
            }
        }
    }
    for r in later.records {
        let removed = r.bools.iter().any(|(k, v)| k == "REMOVED" && *v);
        let Some(i) = base.records.iter().position(|b| b.string_id == r.string_id) else {
            if !removed {
                base.records.push(r);
            }
            continue;
        };
        if removed {
            base.records.remove(i);
            continue;
        }
        let b = &mut base.records[i];
        set(&mut b.bools, &r.bools);
        set(&mut b.floats, &r.floats);
        set(&mut b.ints, &r.ints);
        set(&mut b.vec3s, &r.vec3s);
        set(&mut b.vec4s, &r.vec4s);
        set(&mut b.strings, &r.strings);
        set(&mut b.files, &r.files);
        for (cat, refs) in r.references {
            match b.references.iter_mut().find(|(c, _)| *c == cat) {
                Some((_, list)) => {
                    for rf in refs {
                        match list.iter().position(|x| x.target == rf.target) {
                            Some(k) => list[k] = rf,
                            None => list.push(rf),
                        }
                    }
                }
                None => b.references.push((cat, refs)),
            }
        }
        for inst in r.instances {
            let at = b.instances.iter().position(|i| i.id == inst.id);
            match (at, inst.target.is_empty()) {
                (Some(k), true) => {
                    b.instances.remove(k);
                }
                (None, true) => {}
                (Some(k), false) => b.instances[k] = inst,
                (None, false) => b.instances.push(inst),
            }
        }
    }
}

/// A building placed in the world by a zone file.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// Game-data BUILDING record string id.
    pub building: String,
    pub position: [f32; 3],
    /// Quaternion as stored: w, x, y, z.
    pub rotation: [f32; 4],
    /// Ground height from the placement's state record ("world Y pos"), when it has one.
    pub world_y: Option<f32>,
    /// The state record's "interior layout name" / "exterior layout name": which furnishing
    /// layout (`nodes::Layouts`) the building has.
    pub interior: Option<String>,
    pub exterior: Option<String>,
    /// The town handle's index ("townI") and serial ("townS"), when the building belongs to one.
    pub town: Option<(i32, i32)>,
    /// The state record's "squad template": the squad living here.
    pub squad: Option<String>,
    /// The state record's "residents template": the town's resident squad for the building,
    /// whose "building designation" (BuildingDesignation: 1 shop, 3 bar, 9 residential, ...)
    /// is the building's (F048905).
    pub residents: Option<String>,
    /// Per door (the building's "doors" in order): locked ("locked{n}") and lock level
    /// ("lock lvl{n}").
    pub locks: Vec<(bool, i32)>,
    /// The state record's "destroyed" (the building is a ruin: its parts draw their
    /// "destroyed mesh", F037846 / F037849 via the building's +0x1a1).
    pub destroyed: bool,
}

/// Building placements of one zone file: `buildinglist` instances that target game data (not empty,
/// which marks a removal, and not
/// another `-INGAME` record), with the height from their type-35 state record.
pub fn placements(zone: &kenshi_io::fcs::File) -> Vec<Placement> {
    let state = |i: &kenshi_io::fcs::Instance| {
        i.states
            .iter()
            .find_map(|s| zone.records.iter().find(|r| r.string_id == *s))
    };
    let float = |r: Option<&kenshi_io::fcs::Record>, k: &str| {
        r.and_then(|r| r.floats.iter().find(|(n, _)| n == k).map(|(_, v)| *v))
    };
    let string = |r: Option<&kenshi_io::fcs::Record>, k: &str| {
        r.and_then(|r| {
            r.strings
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
        })
        .filter(|v| !v.is_empty())
    };
    let int = |r: Option<&kenshi_io::fcs::Record>, k: &str| {
        r.and_then(|r| r.ints.iter().find(|(n, _)| n == k).map(|(_, v)| *v))
    };
    zone.records
        .iter()
        .flat_map(|r| &r.instances)
        .filter(|i| !i.target.is_empty() && !i.target.contains("-INGAME"))
        .map(|i| {
            let st = state(i);
            Placement {
                building: i.target.clone(),
                position: i.position,
                rotation: i.rotation,
                world_y: float(st, "world Y pos"),
                interior: string(st, "interior layout name"),
                exterior: string(st, "exterior layout name"),
                town: int(st, "townI")
                    .zip(int(st, "townS"))
                    .filter(|t| t.0 != 0 || t.1 != 0),
                squad: string(st, "squad template"),
                residents: string(st, "residents template"),
                destroyed: st
                    .and_then(|r| r.bools.iter().find(|(k, _)| k == "destroyed").map(|(_, v)| *v))
                    .unwrap_or(false),
                locks: (0..16)
                    .map_while(|n| {
                        let lvl = int(st, &format!("lock lvl{n}"))?;
                        let locked = st
                            .and_then(|r| {
                                r.bools
                                    .iter()
                                    .find(|(k, _)| *k == format!("locked{n}"))
                                    .map(|(_, v)| *v)
                            })
                            .unwrap_or(false);
                        Some((locked, lvl))
                    })
                    .collect(),
            }
        })
        .collect()
}

impl Terrain {
    /// Heightmap sample (column, row) at a world position: world origin is the map centre,
    /// columns follow x and rows follow z (verified against building heights, T00023).
    pub fn sample_of(&self, x: f32, z: f32) -> (f32, f32) {
        let s = self.spacing();
        ((x + WORLD_SIZE / 2.0) / s, (z + WORLD_SIZE / 2.0) / s)
    }

    /// Terrain height at a world position, bilinearly interpolated.
    pub fn height_at(&mut self, x: f32, z: f32) -> Result<f32, Error> {
        let (c, r) = self.sample_of(x, z);
        let max = (self.samples() - 2) as f32;
        let (c, r) = (c.clamp(0.0, max), r.clamp(0.0, max));
        let (ci, ri) = (c as u32, r as u32);
        let (fc, fr) = (c.fract(), r.fract());
        let h00 = self.sample(ci, ri)?;
        let h10 = self.sample(ci + 1, ri)?;
        let h01 = self.sample(ci, ri + 1)?;
        let h11 = self.sample(ci + 1, ri + 1)?;
        let top = h00 * (1.0 - fc) + h10 * fc;
        let bottom = h01 * (1.0 - fc) + h11 * fc;
        Ok(top * (1.0 - fr) + bottom * fr)
    }

    /// One heightmap sample (units), read through 64 x 64 sample tiles kept in memory (at most
    /// 1024 of them, 8 MB), so characters standing on the ground every tick don't go to the file.
    fn sample(&mut self, c: u32, r: u32) -> Result<f32, Error> {
        let key = (c / TILE, r / TILE);
        if !self.tiles.contains_key(&key) {
            if self.tiles.len() >= 1024 {
                self.tiles.clear();
            }
            let tile = self.map.window(key.0 * TILE, key.1 * TILE, TILE, TILE, 1)?;
            self.tiles.insert(key, tile);
        }
        let v = self.tiles[&key][((r % TILE) * TILE + c % TILE) as usize];
        Ok(v as f32 / u16::MAX as f32 * HEIGHT_RANGE)
    }

    pub fn open(install: &Path) -> Result<Self, Error> {
        Ok(Terrain {
            map: Tiff::open(&install.join(HEIGHTMAP))?,
            tiles: std::collections::HashMap::new(),
        })
    }

    /// Samples per side (16385 for the shipped map).
    pub fn samples(&self) -> u32 {
        self.map.width
    }

    /// Units between neighbouring samples (18 for the shipped map).
    pub fn spacing(&self) -> f32 {
        WORLD_SIZE / (self.map.width - 1) as f32
    }

    /// A patch of `w` x `h` samples taken every `step` heightmap samples, starting at sample
    /// (`sx`, `sy`). `x0`/`z0` are world coordinates (origin at the map centre, T00023).
    pub fn patch(&mut self, sx: u32, sy: u32, w: u32, h: u32, step: u32) -> Result<Patch, Error> {
        let raw = self.map.window(sx, sy, w, h, step)?;
        let s = self.spacing();
        Ok(Patch {
            sx,
            sy,
            step: step.max(1),
            x0: sx as f32 * s - WORLD_SIZE / 2.0,
            z0: sy as f32 * s - WORLD_SIZE / 2.0,
            spacing: s * step as f32,
            w,
            h,
            heights: raw
                .into_iter()
                .map(|v| v as f32 / u16::MAX as f32 * HEIGHT_RANGE)
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_cover_the_world_from_its_centre() {
        assert_eq!(zone_of(-WORLD_SIZE / 2.0, -WORLD_SIZE / 2.0), (0, 0));
        assert_eq!(zone_of(0.0, 0.0), (32, 32));
        // a building seen in zone.7.21.zone
        assert_eq!(zone_of(-112_529.0, -47_779.0), (7, 21));
    }
}
