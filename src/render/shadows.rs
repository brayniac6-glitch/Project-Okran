//! Kenshi's sun shadows, the CSM mode (F030147: "shadow mode" 1, Main_Lighting_CSM): four
//! cascades split over the "Shadow Range", each a quarter of a square map whose side is
//! [1024, 2048, 4096] by the "shadow quality" (F032951). The lookup is kenshi::lighting's
//! sun_shadow.

use bevy::light::CascadeShadowConfig;

/// The cascades (F046653: four).
pub const CASCADES: usize = 4;

/// @fn F046650
///
/// The split distances, `n` + 1 of them: the first `near`, the last `far`, between them half
/// of a blend (`lambda`) of the logarithmic split and the even one.
pub fn csm_splits(n: usize, near: f32, far: f32, lambda: f32) -> Vec<f32> {
    let mut s = vec![near; n + 1];
    for (i, v) in s.iter_mut().enumerate().take(n).skip(1) {
        let t = i as f32 / n as f32;
        *v = ((far / near).powf(t) * near * lambda + ((far - near) * t + near) * (1.0 - lambda)) * 0.5;
    }
    s[n] = far;
    s
}

/// The shadow map's side by the "shadow quality" option (F032951).
pub fn map_size(quality: i32) -> u32 {
    [1024, 2048, 4096][quality.clamp(0, 2) as usize]
}

/// A cascade's side: a quarter of the map (Kenshi's 2 x 2 atlas), (enhancement) at least 2048
/// (Kenshi's "shadow quality" 2), so near shadows are not blocky.
pub fn cascade_size(quality: i32) -> u32 {
    (map_size(quality) / 2).max(2048)
}

/// The sun's cascades for the "Shadow Range" (F046653 and F046654: near 1, lambda 0.95), as
/// Bevy's cascade bounds (each cascade from the previous split, no overlap).
pub fn cascade_config(shadow_range: f32) -> CascadeShadowConfig {
    let s = csm_splits(CASCADES, 1.0, shadow_range, 0.95);
    CascadeShadowConfig {
        bounds: s[1..].to_vec(),
        overlap_proportion: 0.0,
        minimum_distance: s[0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_the_default_range() {
        let s = csm_splits(4, 1.0, 1500.0, 0.95);
        assert_eq!(s.len(), 5);
        assert_eq!((s[0], s[4]), (1.0, 1500.0));
        // pow(1500, 1/4) x 0.95 + 375.75 x 0.05, halved
        assert!((s[1] - 12.35).abs() < 0.01, "{s:?}");
        assert!(s[1] < s[2] && s[2] < s[3] && s[3] < s[4]);
    }
}
