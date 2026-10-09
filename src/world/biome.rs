//! Biomes (port_map `zones`): which biome each world zone belongs to and what roams it.
//!
//! Spec: F048162 Biome::Biome (a BIOME_GROUP record: "index" - the colour the biome map marks it
//! with -, "homeless spawn amount", "homeless spawns" as a SpawnInfoList, "num nests"; the zones
//! of the 64 x 64 grid whose biome-map colour is the index belong to it, the unmarked ones to
//! the default biome 18003-gamedata.base), F048100 the biome-map colour at a zone's centre (the
//! map spans the world; R << 16 | G << 8 | B), F048166 SpawnInfoList::load (each reference a
//! squad template with its weight, the first value, kept when positive; the squad's faction, else
//! 3943-gamedata.base).

use crate::{WORLD_SIZE, ZONE_SIZE};
use kenshi_data::{GameData, GameDataContainer, ItemType};
use std::path::Path;

/// The biome a zone whose map colour no biome claims falls in (F048162).
pub const DEFAULT_BIOME: &str = "18003-gamedata.base";
/// A spawn entry's faction when its squad names none (F048166).
pub const DEFAULT_SPAWN_FACTION: &str = "3943-gamedata.base";

/// The biome map (data/newland/land/biomemap.png): one colour per pixel over the whole world.
pub struct BiomeMap {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<[u8; 3]>,
}

impl BiomeMap {
    pub fn load(install: &Path) -> Option<BiomeMap> {
        let img = image::open(install.join("data/newland/land/biomemap.png"))
            .ok()?
            .to_rgb8();
        Some(BiomeMap {
            width: img.width(),
            height: img.height(),
            rgb: img.pixels().map(|p| p.0).collect(),
        })
    }

    /// @fn F048100
    ///
    /// The map's colour at world (x, z), clamped to the map, as R << 16 | G << 8 | B. The image's
    /// rows run against z: (assumption, checked) of the four ways to lay the map on the world
    /// only this one puts Squin in Okran's Gulf and Admag in Vain (the loader setting the image's
    /// extent, F051626 -> F025f13, is not read yet).
    pub fn colour_at(&self, x: f32, z: f32) -> i32 {
        let col = ((x + WORLD_SIZE / 2.0) / (WORLD_SIZE / self.width as f32)) as i32;
        let row = ((z + WORLD_SIZE / 2.0) / (WORLD_SIZE / self.height as f32)) as i32;
        let col = col.clamp(0, self.width as i32 - 1) as u32;
        let row = self.height - 1 - row.clamp(0, self.height as i32 - 1) as u32;
        let [r, g, b] = self.rgb[(row * self.width + col) as usize];
        (r as i32) << 16 | (g as i32) << 8 | b as i32
    }
}

/// The centre of zone (x, y) (world units).
pub fn zone_centre(x: i32, y: i32) -> (f32, f32) {
    let c = |i: i32| i as f32 * ZONE_SIZE - WORLD_SIZE / 2.0 + ZONE_SIZE / 2.0;
    (c(x), c(y))
}

/// The zone (x, y) a world point is in.
pub fn zone_at(x: f32, z: f32) -> (i32, i32) {
    let k = |v: f32| ((v + WORLD_SIZE / 2.0) / ZONE_SIZE).floor() as i32;
    (k(x), k(z))
}

/// One squad a biome (or town) may spawn.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnInfo {
    /// The SQUAD_TEMPLATE record.
    pub template: String,
    pub faction: String,
    pub weight: i32,
}

/// @fn F048166
///
/// A record's spawn list in a reference category: the squad templates with a positive weight.
pub fn spawn_list(data: &GameDataContainer, r: &GameData, category: &str) -> Vec<SpawnInfo> {
    r.refs(category)
        .iter()
        .filter_map(|(sid, v)| {
            let t = data.get(sid)?;
            if t.item_type() != Some(ItemType::SquadTemplate) || v[0] <= 0 {
                return None;
            }
            let faction = t
                .refs("faction")
                .first()
                .map_or(DEFAULT_SPAWN_FACTION.to_string(), |(f, _)| f.clone());
            Some(SpawnInfo {
                template: sid.clone(),
                faction,
                weight: v[0],
            })
        })
        .collect()
}

/// A BIOME_GROUP as the population reads it.
#[derive(Debug, Clone)]
pub struct Biome {
    pub record: String,
    pub name: String,
    /// Its biome-map colour.
    pub index: i32,
    pub homeless_spawn_amount: f32,
    pub homeless: Vec<SpawnInfo>,
    /// "num nests" (F048141) and "nests at fixed markers only".
    pub num_nests: i32,
    pub nests_at_markers: bool,
}

/// @fn F048162
///
/// Every biome of the data.
pub fn biomes(data: &GameDataContainer) -> Vec<Biome> {
    data.of_type(ItemType::BiomeGroup)
        .map(|b| Biome {
            record: b.string_id.clone(),
            name: b.name.clone(),
            index: *b.ints.get("index").unwrap_or(&-1),
            homeless_spawn_amount: b
                .floats
                .get("homeless spawn amount")
                .copied()
                .unwrap_or(0.0),
            homeless: spawn_list(data, b, "homeless spawns"),
            num_nests: *b.ints.get("num nests").unwrap_or(&0),
            nests_at_markers: b.bools.get("nests at fixed markers only").copied().unwrap_or(false),
        })
        .collect()
}

/// The biome of zone (x, y): the one whose index is the map's colour at its centre, else the
/// default biome.
pub fn biome_of_zone(map: &BiomeMap, biomes: &[Biome], x: i32, y: i32) -> Option<usize> {
    let (cx, cz) = zone_centre(x, y);
    let c = map.colour_at(cx, cz);
    biomes
        .iter()
        .position(|b| b.index == c)
        .or_else(|| biomes.iter().position(|b| b.record == DEFAULT_BIOME))
}
