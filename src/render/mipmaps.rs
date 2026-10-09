//! Mipmaps for textures loaded without them (port_map `render`): Ogre gives every texture a full
//! mip chain on load (its default number of mipmaps is unlimited), which Kenshi's PNG textures
//! (colour, overlay, blend and water maps) rely on far away; Bevy loads PNGs with one level. Each
//! 8-bit RGBA image is given its chain once loaded, each level a 2 x 2 box filter of the last.

use bevy::asset::AssetEvent;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

/// The levels below one 8-bit RGBA level of `w` x `h`, box filtered.
fn chain(top: &[u8], mut w: u32, mut h: u32) -> (Vec<u8>, u32) {
    let mut out = top.to_vec();
    let mut prev = top.to_vec();
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += prev[((sy * w + sx) * 4 + c) as usize] as u32;
                    }
                    next[((y * nw + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        out.extend_from_slice(&next);
        prev = next;
        w = nw;
        h = nh;
        levels += 1;
    }
    (out, levels)
}

fn give_mipmaps(mut events: MessageReader<AssetEvent<Image>>, mut images: ResMut<Assets<Image>>) {
    for e in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = e else {
            continue;
        };
        let Some(image) = images.get(*id) else {
            continue;
        };
        let d = &image.texture_descriptor;
        let flat = matches!(
            d.format,
            TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
        );
        if d.mip_level_count != 1
            || !flat
            || d.size.depth_or_array_layers != 1
            || d.dimension != bevy::render::render_resource::TextureDimension::D2
        {
            continue;
        }
        let (w, h) = (d.size.width, d.size.height);
        let Some(data) = image.data.as_ref() else {
            continue;
        };
        if data.len() != (w * h * 4) as usize {
            continue;
        }
        let (all, levels) = chain(data, w, h);
        if let Some(mut image) = images.get_mut(*id) {
            image.data = Some(all);
            image.texture_descriptor.mip_level_count = levels;
        }
    }
}

pub struct MipmapPlugin;

impl Plugin for MipmapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, give_mipmaps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_four_by_two_has_three_levels() {
        let top: Vec<u8> = (0..4 * 2 * 4).map(|i| (i * 8) as u8).collect();
        let (all, levels) = chain(&top, 4, 2);
        assert_eq!(levels, 3);
        assert_eq!(all.len(), (4 * 2 + 2 + 1) * 4);
    }
}
