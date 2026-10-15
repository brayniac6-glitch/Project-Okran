//! The texture quality option (settings.cfg "texture resolution gimping": 0 Maximum, 1 Medium, 2
//! Fugly; 1 when missing, F032944): Kenshi's resource loading listener (F046209) drops the top mip
//! levels of DXT textures as they load, so the GPU keeps them at half or a quarter of their size.
//!
//! Spec: F046209 for a resource whose name has "dds" and whose group is not "Overlaymaps", "GUI",
//! "PLSM2" or none: levels = the option, one fewer for a name with "_LO.", then (still above 0)
//! one fewer again unless the name has "_HI." or the group is "Landscape". F046205 then cuts that
//! many levels off a DXT1/3/5 file (not a cube or volume map), stopping at the last level or
//! once the next would be under 5 pixels a side; the header gets the new size and level count.
//!
//! The groups by folder as Kenshi's resources.cfg lays them out (data/gui GUI, data/land
//! Landscape) and as it adds data/newland/land/overlaymaps to Overlaymaps (F033099).
//! (assumption) data/newland/land/textures, the ground textures the terrain material loads by
//! name, is not in resources.cfg: taken as General.

use bevy::asset::AssetEvent;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureDimension, TextureFormat};

/// settings.cfg "texture resolution gimping".
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureGimping(pub i32);

impl Default for TextureGimping {
    fn default() -> Self {
        TextureGimping(1)
    }
}

/// @fn F046209
///
/// The levels a texture at `path` loses at gimping `option`.
pub fn levels_dropped(path: &str, option: i32) -> u32 {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    if !p.contains("dds") || p.contains("/gui/") || p.starts_with("data/gui") || p.contains("overlaymaps") {
        return 0;
    }
    let landscape = p.starts_with("data/land/");
    let mut n = option;
    if n > 0 {
        if p.contains("_lo.") {
            n -= 1;
        }
        if n > 0 && !p.contains("_hi.") && !landscape {
            n -= 1;
        }
    }
    n.max(0) as u32
}

/// The bytes of one level of a block-compressed format (4 x 4 blocks).
fn level_bytes(format: TextureFormat, w: u32, h: u32) -> Option<usize> {
    let block = match format {
        TextureFormat::Bc1RgbaUnorm | TextureFormat::Bc1RgbaUnormSrgb => 8,
        TextureFormat::Bc2RgbaUnorm | TextureFormat::Bc2RgbaUnormSrgb | TextureFormat::Bc3RgbaUnorm | TextureFormat::Bc3RgbaUnormSrgb => 16,
        _ => return None,
    };
    Some((w.div_ceil(4) * h.div_ceil(4)) as usize * block)
}

/// @fn F046205
///
/// Cut `n` top levels off a DXT image's data: (data offset, width, height, levels) after.
pub fn cut_levels(format: TextureFormat, mut w: u32, mut h: u32, mut levels: u32, n: u32) -> Option<(usize, u32, u32, u32)> {
    let mut skip = 0usize;
    for _ in 0..n {
        if levels < 2 || h < 5 || w < 5 {
            break;
        }
        skip += level_bytes(format, w, h)?;
        w >>= 1;
        h >>= 1;
        levels -= 1;
    }
    Some((skip, w, h, levels))
}

fn gimp(
    mut events: MessageReader<AssetEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
    assets: Res<AssetServer>,
    option: Option<Res<TextureGimping>>,
    mut done: Local<std::collections::HashSet<AssetId<Image>>>,
) {
    let option = option.map_or(1, |o| o.0);
    for e in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = e else {
            continue;
        };
        // (each image once: the event can come again for the same image)
        if !done.insert(*id) {
            continue;
        }
        let Some(path) = assets.get_path(*id) else { continue };
        let n = levels_dropped(&path.path().to_string_lossy(), option);
        if n == 0 {
            continue;
        }
        let Some(image) = images.get(*id) else { continue };
        let d = &image.texture_descriptor;
        if d.dimension != TextureDimension::D2 || d.size.depth_or_array_layers != 1 {
            continue;
        }
        let Some((skip, w, h, levels)) = cut_levels(d.format, d.size.width, d.size.height, d.mip_level_count, n) else {
            continue;
        };
        if skip == 0 || image.data.as_ref().is_none_or(|v| v.len() <= skip) {
            continue;
        }
        if std::env::var("KENSHI_GIMP_DEBUG").is_ok() {
            eprintln!("gimp: {} {}x{} -{n} -> {w}x{h}", path, d.size.width, d.size.height);
        }
        if let Some(mut image) = images.get_mut(*id) {
            if let Some(data) = image.data.as_mut() {
                data.drain(..skip);
            }
            image.texture_descriptor.size.width = w;
            image.texture_descriptor.size.height = h;
            image.texture_descriptor.mip_level_count = levels;
        }
    }
}

pub struct TextureQualityPlugin;

impl Plugin for TextureQualityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextureGimping>().add_systems(PostUpdate, gimp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_by_option_and_name() {
        // Medium: general textures whole, _HI and the landscape's one level less
        assert_eq!(levels_dropped("data/items/armour/coat.dds", 1), 0);
        assert_eq!(levels_dropped("data/materials/moon_HI.dds", 1), 1);
        assert_eq!(levels_dropped("data/land/textures/Sand01bc.dds", 1), 1);
        assert_eq!(levels_dropped("data/newland/land/textures/RubbleLoop_NML.dds", 1), 0);
        // Fugly: general one, _HI and landscape two, _LO one less
        assert_eq!(levels_dropped("data/items/armour/coat.dds", 2), 1);
        assert_eq!(levels_dropped("data/items/armour/coat_LO.dds", 2), 0);
        assert_eq!(levels_dropped("data/materials/moon_HI.dds", 2), 2);
        // never the GUI, the overlay maps, or a PNG
        assert_eq!(levels_dropped("data/gui/skins/a.dds", 2), 0);
        assert_eq!(levels_dropped("data/newland/land/overlaymaps/colour.1.1.png", 2), 0);
        assert_eq!(levels_dropped("data/x.png", 2), 0);
    }

    #[test]
    fn cuts_whole_levels_and_stops_small() {
        // DXT5 2048: one level off is 2048 * 2048 bytes
        assert_eq!(cut_levels(TextureFormat::Bc3RgbaUnorm, 2048, 2048, 12, 1), Some((2048 * 2048, 1024, 1024, 11)));
        // DXT1 half that
        assert_eq!(cut_levels(TextureFormat::Bc1RgbaUnorm, 2048, 2048, 12, 1), Some((2048 * 1024, 1024, 1024, 11)));
        // no levels to spare
        assert_eq!(cut_levels(TextureFormat::Bc3RgbaUnorm, 256, 256, 1, 2), Some((0, 256, 256, 1)));
        // not under 5 pixels
        assert_eq!(cut_levels(TextureFormat::Bc1RgbaUnorm, 8, 8, 4, 2).map(|c| c.1), Some(4));
        // not a DXT format
        assert_eq!(cut_levels(TextureFormat::Rgba8Unorm, 64, 64, 7, 1), None);
    }
}
