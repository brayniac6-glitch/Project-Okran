//! The terrain's looks (port_map `terrain`): BIOMES records as the terrain shader takes them, and
//! which of them cover each part of the world.
//!
//! Spec: F051603 (a BIOMES record's terrain fields), F051631 ZoneMap::getTerrainMaterial_DX11 (the
//! shader parameters: scalesA = tiling 1, 2; scalesB = tiling 0, grass; scalesC = dirt, road;
//! slopeMin / slopeMax / slopeBlend = the slope fields x 0.01; overlayMult; textureFade = the
//! "ground colour" with 1 / "fade distance"; absorbance; brightnessFix; per blended biome k the
//! cliff distortion (1 / "distort wavelength", "distort amplitude" x 0.01) at distortion[2k],
//! [2k + 1]), F051621 (data/newland/land/blendinfo.dat: "KBI1", a 32 x 32 grid of world cells with
//! five BIOMES indices each, then each cell's blend block; blendmap.png carries slots 0..3 of a cell
//! in R, G, B, A, slot 4 the rest).

use crate::WORLD_SIZE;
use kenshi_data::{GameData, GameDataContainer, ItemType};
use std::path::Path;

/// The six texture layers a terrain biome draws with (the shader's array order).
pub const LAYERS: [&str; 6] = ["base", "slope", "vertical", "grass", "dirt", "road"];

/// One BIOMES record as the terrain shader takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainBiome {
    pub record: String,
    pub name: String,
    pub index: i32,
    /// Per layer (`LAYERS`): the diffuse and normal map (asset paths under the install).
    pub textures: [(String, String); 6],
    pub scales_a: [f32; 4],
    pub scales_b: [f32; 4],
    pub scales_c: [f32; 4],
    pub slope_min: [f32; 4],
    pub slope_max: [f32; 4],
    pub slope_blend: [f32; 4],
    pub overlay_mult: [f32; 4],
    pub texture_fade: [f32; 4],
    pub absorbance: [[f32; 4]; 2],
    pub brightness_fix: f32,
    /// (1 / wavelength, amplitude x 0.01) of the wavy cliff lines.
    pub distortion: [f32; 2],
    /// The biome's "ambient light" colour (0xRRGGBB) and "sun brightness" (F051616).
    pub ambient_light: u32,
    pub sun_brightness: f32,
    pub water: WaterBiome,
}

/// A BIOMES record's water (F050962, the water shader's parameters for the biome's pass).
#[derive(Debug, Clone, PartialEq)]
pub struct WaterBiome {
    /// "water normal", "texture scum", "texture scum normal", "turbulence map" (asset paths;
    /// empty: none).
    pub normal: String,
    pub scum: String,
    pub scum_normal: String,
    pub turbulence: String,
    /// The vertex shader's `scale`: ("water scale X", "water scale Y", "scum scale X",
    /// "scum scale Y") / 5000.
    pub scale: [f32; 4],
    /// invStrength = 1 / "water strength" (at least 0.0001).
    pub inv_strength: f32,
    /// invOpacity = 1 / max("water visibility" x 10, 1).
    pub inv_opacity: f32,
    pub gloss: f32,
    pub glow: f32,
    /// ("water distortion", "scum distortion" / ((scum scale X + Y) x 0.5)).
    pub distortion: [f32; 2],
}

/// A data path as an asset path: `.\data\x\y.dds` -> `data/x/y.dds`.
fn asset_path(s: &str) -> String {
    s.trim_start_matches(".\\")
        .trim_start_matches("./")
        .replace('\\', "/")
}

impl TerrainBiome {
    /// @fn F051603
    pub fn from_record(r: &GameData) -> TerrainBiome {
        let f = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
        let file = |k: &str| r.files.get(k).map(|s| asset_path(s)).unwrap_or_default();
        let layer = |l: &str| {
            (
                file(&format!("texture {l}")),
                file(&format!("texture {l} normal")),
            )
        };
        let rgb = |k: &str| {
            let v = *r.ints.get(k).unwrap_or(&0) as u32;
            [
                ((v >> 16) & 255) as f32 / 255.0,
                ((v >> 8) & 255) as f32 / 255.0,
                (v & 255) as f32 / 255.0,
            ]
        };
        let fade = f("fade distance");
        let g = rgb("ground colour");
        let wl = f("distort wavelength");
        TerrainBiome {
            record: r.string_id.clone(),
            name: r.name.clone(),
            index: *r.ints.get("index").unwrap_or(&0),
            textures: LAYERS.map(layer),
            scales_a: [
                f("tiling X 1"),
                f("tiling Y 1"),
                f("tiling X 2"),
                f("tiling Y 2"),
            ],
            scales_b: [
                f("tiling X 0"),
                f("tiling Y 0"),
                f("tiling X grass"),
                f("tiling Y grass"),
            ],
            scales_c: [
                f("tiling X dirt"),
                f("tiling Y dirt"),
                f("tiling X road"),
                f("tiling Y road"),
            ],
            slope_min: [
                f("slope min 1"),
                f("slope min 2"),
                f("slope min 3"),
                f("slope min grass"),
            ]
            .map(|v| v * 0.01),
            slope_max: [
                f("slope max 1"),
                f("slope max 2"),
                f("slope max 3"),
                f("slope max grass"),
            ]
            .map(|v| v * 0.01),
            slope_blend: [
                f("slope fade 1"),
                f("slope fade 2"),
                f("slope fade 3"),
                f("slope fade grass"),
            ]
            .map(|v| v * 0.01),
            overlay_mult: [
                f("overlay mult vertical"),
                f("overlay mult grass"),
                f("overlay mult dirt"),
                f("overlay mult road"),
            ],
            texture_fade: [g[0], g[1], g[2], if fade > 0.0 { 1.0 / fade } else { 0.0 }],
            absorbance: [
                [
                    f("absorbance 0"),
                    f("absorbance 1"),
                    f("absorbance 2"),
                    f("absorbance grass"),
                ],
                [f("absorbance dirt"), f("absorbance road"), 0.0, 0.0],
            ],
            brightness_fix: r.floats.get("brightness fix").copied().unwrap_or(1.0),
            distortion: [
                if wl > 0.0 { 1.0 / wl } else { 0.0 },
                f("distort amplitude") * 0.01,
            ],
            ambient_light: *r.ints.get("ambient light").unwrap_or(&0xffffff) as u32,
            sun_brightness: r.floats.get("sun brightness").copied().unwrap_or(1.0),
            water: WaterBiome::from_record(r),
        }
    }
}

/// Every BIOMES record with an index.
pub fn terrain_biomes(data: &GameDataContainer) -> Vec<TerrainBiome> {
    data.of_type(ItemType::Biomes)
        .filter(|r| r.ints.get("index").is_some_and(|&i| i != 0))
        .map(TerrainBiome::from_record)
        .collect()
}

/// data/newland/land/blendinfo.dat (F051621): the biomes of each world cell.
#[derive(Debug, Clone)]
pub struct BlendInfo {
    /// Cells per side.
    pub n: usize,
    /// Per cell (row-major, rows along z): the BIOMES index of each of its five slots (0: none).
    pub cells: Vec<[i32; 5]>,
}

impl BlendInfo {
    pub fn load(install: &Path) -> Option<BlendInfo> {
        let d = std::fs::read(install.join("data/newland/land/blendinfo.dat")).ok()?;
        if d.get(0..4)? != b"KBI1" {
            return None;
        }
        let int = |p: usize| -> Option<i32> {
            Some(i32::from_le_bytes(d.get(p..p + 4)?.try_into().ok()?))
        };
        let n = int(4)? as usize;
        let mut cells = Vec::with_capacity(n * n);
        for i in 0..n * n {
            let p = 12 + i * 20;
            cells.push([
                int(p)?,
                int(p + 4)?,
                int(p + 8)?,
                int(p + 12)?,
                int(p + 16)?,
            ]);
        }
        Some(BlendInfo { n, cells })
    }

    /// The world size of one cell (units).
    pub fn cell_size(&self) -> f32 {
        WORLD_SIZE / self.n as f32
    }

    /// The cell (x, y) a world point lies in.
    pub fn cell_at(&self, x: f32, z: f32) -> (usize, usize) {
        let k = |v: f32| {
            (((v + WORLD_SIZE / 2.0) / self.cell_size()) as i64).clamp(0, self.n as i64 - 1)
                as usize
        };
        (k(x), k(z))
    }

    pub fn slots(&self, cx: usize, cy: usize) -> [i32; 5] {
        self.cells[cy * self.n + cx]
    }
}

impl WaterBiome {
    /// @fn F050962
    pub fn from_record(r: &GameData) -> WaterBiome {
        let f = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
        let file = |k: &str| r.files.get(k).map(|s| asset_path(s)).unwrap_or_default();
        let scale = [
            f("water scale X"),
            f("water scale Y"),
            f("scum scale X"),
            f("scum scale Y"),
        ];
        let strength = f("water strength");
        let scum_scale = (scale[2] + scale[3]) * 0.5;
        WaterBiome {
            normal: file("water normal"),
            scum: file("texture scum"),
            scum_normal: file("texture scum normal"),
            turbulence: file("turbulence map"),
            scale: scale.map(|v| v / 5000.0),
            inv_strength: 1.0 / if strength < 0.0001 { 0.0001 } else { strength },
            inv_opacity: 1.0 / (f("water visibility") * 10.0).max(1.0),
            gloss: f("water gloss"),
            glow: f("water glow"),
            distortion: [f("water distortion"), f("scum distortion") / scum_scale],
        }
    }
}

/// data/newland/land/blendmap.png: per pixel the weights of a blend cell's slots 0..3 (R, G, B,
/// A; slot 4 the rest), rows along z as the terrain shader samples it.
#[derive(Debug, Clone)]
pub struct BlendMap {
    pub size: u32,
    pub rgba: Vec<[u8; 4]>,
}

impl BlendMap {
    pub fn load(install: &Path) -> Option<BlendMap> {
        let img = image::open(install.join("data/newland/land/blendmap.png"))
            .ok()?
            .to_rgba8();
        Some(BlendMap {
            size: img.width(),
            rgba: img.pixels().map(|p| p.0).collect(),
        })
    }

    /// The slot weighing most at world (x, z).
    pub fn dominant_slot(&self, x: f32, z: f32) -> usize {
        let k = |v: f32| {
            (((v + WORLD_SIZE / 2.0) / WORLD_SIZE * self.size as f32) as i64)
                .clamp(0, self.size as i64 - 1) as usize
        };
        let p = self.rgba[k(z) * self.size as usize + k(x)];
        let w = [p[0] as i32, p[1] as i32, p[2] as i32, p[3] as i32];
        let rest = 255 - w.iter().sum::<i32>();
        let mut best = (4, rest);
        for (s, &v) in w.iter().enumerate() {
            if v > best.1 {
                best = (s, v);
            }
        }
        best.0
    }
}
