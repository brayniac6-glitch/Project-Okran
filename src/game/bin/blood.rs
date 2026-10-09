//! Blood on the ground (port_map `render`, Kenshi's TerrainDecalsManager and TerrainBlood): images
//! of data/materials/blood ("pool", "splash", "line", "smear", F047742's kinds 0..3) painted on the
//! CPU into a blood map over the ground (F045917 paintImageOnTexture: each texel blended by the
//! image's alpha, the map's alpha the greater of the two, the image flipped at random), which the
//! terrain draws over its albedo (deferred/blood.hlsl terrain; kenshi_terrain.wgsl).
//!
//! (stand-in) When and how big Kenshi paints which image is not decompiled: here a hit that lands
//! splashes its victim's feet (10 units + 0.4 per point of damage, at most 40), a bolt in flesh
//! 12; someone down and bleeding grows a pool under them every 2 seconds, the dead a last large
//! one; someone bleeding on their feet drips a line every second. The images (greyscale with
//! alpha) are tinted by the race's "blood colour" (its colour x the image's grey). The map covers
//! 2048 units (2048 texels) round
//! where the first blood fell and starts afresh when blood falls beyond it.

use super::Game;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use kenshi_combat::arena::Cue;
use kenshi_render::terrain_material::{KenshiTerrainMaterial, TerrainLook};
use std::collections::HashSet;

const TEXELS: usize = 2048;
const WIDTH: f32 = 2048.0;
const KINDS: [&str; 4] = ["pool", "splash", "line", "smear"];

#[derive(Resource, Default)]
pub struct Blood {
    image: Option<Handle<Image>>,
    pixels: Vec<[u8; 4]>,
    /// The map's corner (world x, z).
    corner: Vec2,
    /// The images of each kind (KINDS order), loaded once.
    images: Option<Vec<Vec<kenshi_io::dds::Rgba>>>,
    dirty: bool,
    since_upload: f32,
    timer: f32,
    dead: HashSet<usize>,
    roll: u64,
}

impl Blood {
    fn next(&mut self) -> f32 {
        self.roll = self.roll.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.roll >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Paint an image of `kind` (KINDS index) `size` units across centred at world (x, z),
    /// `strength` its alpha's share, tinted `colour`.
    fn paint(&mut self, kind: usize, at: Vec2, size: f32, strength: f32, colour: [f32; 3]) {
        let count = self.images.as_ref().map_or(0, |i| i[kind].len());
        if count == 0 {
            return;
        }
        let pick = (self.next() * count as f32) as usize % count;
        let (flip_x, flip_y) = (self.next() < 0.5, self.next() < 0.5);
        let images = self.images.take().expect("loaded");
        let img = &images[kind][pick];
        let texel = WIDTH / TEXELS as f32;
        let n = (size / texel).max(2.0) as i64;
        let aspect = img.height as f32 / img.width as f32;
        let (w, h) = (n, ((n as f32) * aspect).max(1.0) as i64);
        let cx = ((at.x - self.corner.x) / texel) as i64;
        let cy = ((at.y - self.corner.y) / texel) as i64;
        for j in 0..h {
            let y = cy - h / 2 + j;
            if y < 0 || y >= TEXELS as i64 {
                continue;
            }
            for i in 0..w {
                let x = cx - w / 2 + i;
                if x < 0 || x >= TEXELS as i64 {
                    continue;
                }
                let mut sx = (i as f32 / w as f32 * img.width as f32) as u32;
                let mut sy = (j as f32 / h as f32 * img.height as f32) as u32;
                if flip_x {
                    sx = img.width - 1 - sx.min(img.width - 1);
                }
                if flip_y {
                    sy = img.height - 1 - sy.min(img.height - 1);
                }
                let s = img.pixels[(sy.min(img.height - 1) * img.width + sx.min(img.width - 1)) as usize];
                let a = s[3] as f32 / 255.0 * strength;
                if a <= 0.0 {
                    continue;
                }
                let grey = s[0] as f32 / 255.0;
                let d = &mut self.pixels[y as usize * TEXELS + x as usize];
                for k in 0..3 {
                    let src = (colour[k] * grey * 255.0).min(255.0);
                    d[k] = (src * a + d[k] as f32 * (1.0 - a)) as u8;
                }
                d[3] = d[3].max((a * 255.0) as u8);
            }
        }
        self.images = Some(images);
        self.dirty = true;
    }
}

fn load_images(game: &Game) -> Vec<Vec<kenshi_io::dds::Rgba>> {
    KINDS
        .iter()
        .map(|k| {
            let dir = game.root.join("data/materials/blood").join(k);
            let mut files: Vec<_> = std::fs::read_dir(&dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
            files.sort();
            files.iter().filter_map(|f| kenshi_io::dds::read(f).ok()).collect()
        })
        .collect()
}

/// The race's "blood colour" (0xRRGGBB) of person `id`.
fn colour_of(game: &Game, id: usize) -> [f32; 3] {
    let race = game.sim.folk.get(id).and_then(|p| p.race.clone()).unwrap_or_else(|| game.sim.race_record.clone());
    let c = game.sim.data.get(&race).and_then(|r| r.ints.get("blood colour").copied()).unwrap_or(0x80_0000) as u32;
    [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0]
}

#[allow(clippy::too_many_arguments)]
pub fn paint(
    game: Res<Game>,
    time: Res<Time>,
    mut blood: ResMut<Blood>,
    mut images: ResMut<Assets<Image>>,
    mut look: ResMut<TerrainLook>,
    mut materials: ResMut<Assets<KenshiTerrainMaterial>>,
) {
    let dt = time.delta_secs();
    let sim = &game.sim;
    let pos = |id: usize| sim.folk.get(id).map(|p| {
        let q = p.mover.position_units();
        Vec2::new(q[0], q[2])
    });
    // what to paint: (kind, where, size, strength, colour)
    let mut marks: Vec<(usize, Vec2, f32, f32, [f32; 3])> = Vec::new();
    for cue in &sim.arena.sh.cues {
        match *cue {
            Cue::Hit(_, v, damage) if damage > 0.0 => {
                if let Some(at) = pos(v) {
                    marks.push((1, at, (10.0 + damage * 0.4).min(40.0), 0.9, colour_of(&game, v)));
                }
            }
            Cue::Bolt(_, v, -1) => {
                if let Some(at) = pos(v) {
                    marks.push((1, at, 12.0, 0.8, colour_of(&game, v)));
                }
            }
            _ => {}
        }
    }
    // (testing) KENSHI_BLOOD_TEST: 15 seconds in, a splash, a pool and a drip line beside the
    // first selected character
    if std::env::var("KENSHI_BLOOD_TEST").is_ok()
        && blood.timer < 15.0
        && 15.0 <= blood.timer + dt
        && let Some(at) = game.selected.first().and_then(|&i| pos(i))
    {
        let c = colour_of(&game, game.selected[0]);
        marks.push((1, at + Vec2::new(20.0, 0.0), 35.0, 0.9, c));
        marks.push((0, at + Vec2::new(-20.0, 10.0), 30.0, 0.8, c));
        marks.push((2, at + Vec2::new(0.0, 25.0), 20.0, 0.8, c));
    }
    blood.timer += dt;
    let (drip, pool) = (blood.timer % 1.0 < dt, blood.timer % 2.0 < dt);
    for (id, f) in sim.arena.f.iter().enumerate() {
        let Some(p) = sim.folk.get(id) else { continue };
        let bleeding = 0.0 < f.medical.bleed_rate;
        if p.dead && !blood.dead.contains(&id) {
            blood.dead.insert(id);
            if let Some(at) = pos(id) {
                marks.push((0, at, 30.0, 0.8, colour_of(&game, id)));
            }
        } else if p.down && !p.dead && bleeding && pool {
            if let Some(at) = pos(id) {
                marks.push((0, at, 18.0, 0.35, colour_of(&game, id)));
            }
        } else if !p.down && 0.05 < f.medical.bleed_rate && drip {
            if let Some(at) = pos(id) {
                marks.push((2, at, 10.0, 0.6, colour_of(&game, id)));
            }
        }
    }
    if !marks.is_empty() {
        if blood.images.is_none() {
            blood.images = Some(load_images(&game));
        }
        // a map round the first mark, made afresh when one falls beyond it
        let inside = |b: &Blood, at: Vec2| b.image.is_some() && at.x > b.corner.x + 100.0 && at.y > b.corner.y + 100.0 && at.x < b.corner.x + WIDTH - 100.0 && at.y < b.corner.y + WIDTH - 100.0;
        if !inside(&blood, marks[0].1) {
            blood.corner = marks[0].1 - Vec2::splat(WIDTH * 0.5);
            blood.pixels = vec![[0; 4]; TEXELS * TEXELS];
            let mut img = Image::new(
                Extent3d { width: TEXELS as u32, height: TEXELS as u32, depth_or_array_layers: 1 },
                TextureDimension::D2,
                vec![0; TEXELS * TEXELS * 4],
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::default(),
            );
            img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
            let h = match blood.image.clone() {
                Some(h) => {
                    let _ = images.insert(&h, img);
                    h
                }
                None => images.add(img),
            };
            blood.image = Some(h.clone());
            let area = Vec4::new(blood.corner.x, blood.corner.y, 1.0 / WIDTH, 1.0);
            look.set_blood(h, area, &mut materials);
        }
        for (kind, at, size, strength, colour) in marks {
            blood.paint(kind, at, size, strength, colour);
        }
    }
    // the map sent to the GPU at most four times a second
    blood.since_upload += dt;
    if blood.dirty && blood.since_upload > 0.25 {
        if let Some(h) = blood.image.clone()
            && let Some(mut img) = images.get_mut(&h)
        {
            let bytes: Vec<u8> = blood.pixels.iter().flatten().copied().collect();
            img.data = Some(bytes);
        }
        blood.dirty = false;
        blood.since_upload = 0.0;
    }
}
