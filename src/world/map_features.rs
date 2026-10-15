//! The map's features (data/newland/land/features.dat; port_map `world`): the canyons' cliff
//! blocks, the great rock slabs and cogs, the statues, wrecks and other MAP_FEATURES placed
//! over the terrain outside the zone files.
//!
//! Spec: F032046 (MapFeatureSource, created by F032049 with "features.dat" in the Landscape
//! group): "MF01", the count of zones (0x1000: 64 x 64), then per zone its block's offset and
//! size in the file (two i32); a zone's block is the count of its features (i32), then each:
//! the MAP_FEATURES record's string id (a length byte and the bytes), its position (3 x f32,
//! world units), scale (3 x f32) and rotation (Ogre's quaternion: w, x, y, z). Zone (X, Y) is
//! entry Y x 64 + X.

use std::collections::HashMap;
use std::path::Path;

/// One placed map feature.
#[derive(Debug, Clone, PartialEq)]
pub struct MapFeature {
    /// The MAP_FEATURES record's string id.
    pub record: String,
    /// World units.
    pub position: [f32; 3],
    pub scale: [f32; 3],
    /// w, x, y, z.
    pub rotation: [f32; 4],
}

/// @fn F032046
///
/// The features of `install`, by zone (none when the file is missing or unreadable).
pub fn load(install: &Path) -> HashMap<(i32, i32), Vec<MapFeature>> {
    std::fs::read(install.join("data/newland/land/features.dat")).map(|d| parse(&d)).unwrap_or_default()
}

/// The features in a features.dat file's bytes, by zone (as many as read whole).
pub fn parse(d: &[u8]) -> HashMap<(i32, i32), Vec<MapFeature>> {
    let mut out = HashMap::new();
    if d.get(..4) != Some(b"MF01") {
        return out;
    }
    let int = |o: usize| -> Option<i32> { Some(i32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?)) };
    let float = |o: usize| -> Option<f32> { Some(f32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?)) };
    let Some(zones) = int(4) else { return out };
    for z in 0..zones.max(0) {
        let (Some(off), Some(size)) = (int(8 + z as usize * 8), int(12 + z as usize * 8)) else { break };
        if size <= 0 || off <= 0 {
            continue;
        }
        let mut o = off as usize;
        let Some(count) = int(o) else { continue };
        o += 4;
        let mut list = Vec::new();
        for _ in 0..count.max(0) {
            let read = |o: &mut usize| -> Option<MapFeature> {
                let n = *d.get(*o)? as usize;
                let record = String::from_utf8_lossy(d.get(*o + 1..*o + 1 + n)?).into_owned();
                *o += 1 + n;
                let mut v = [0.0f32; 10];
                for (k, x) in v.iter_mut().enumerate() {
                    *x = float(*o + k * 4)?;
                }
                *o += 40;
                Some(MapFeature { record, position: [v[0], v[1], v[2]], scale: [v[3], v[4], v[5]], rotation: [v[6], v[7], v[8], v[9]] })
            };
            match read(&mut o) {
                Some(f) => list.push(f),
                None => break,
            }
        }
        if !list.is_empty() {
            out.insert((z % 64, z / 64), list);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_zone_block() {
        let mut d = b"MF01".to_vec();
        d.extend(4096i32.to_le_bytes());
        let table = d.len();
        d.extend(vec![0u8; 4096 * 8]);
        let block = d.len();
        d.extend(1i32.to_le_bytes());
        d.push(3);
        d.extend(b"abc");
        for v in [100.0f32, 5.0, -200.0, 1.0, 2.0, 1.0, 1.0, 0.0, 0.0, 0.0] {
            d.extend(v.to_le_bytes());
        }
        // zone (19, 2): entry 2 x 64 + 19
        let k = table + (2 * 64 + 19) * 8;
        d[k..k + 4].copy_from_slice(&(block as i32).to_le_bytes());
        let size = (d.len() - block) as i32;
        d[k + 4..k + 8].copy_from_slice(&size.to_le_bytes());
        let f = parse(&d);
        let z = &f[&(19, 2)];
        assert_eq!(z.len(), 1);
        assert_eq!(z[0].record, "abc");
        assert_eq!(z[0].position, [100.0, 5.0, -200.0]);
        assert_eq!(z[0].scale, [1.0, 2.0, 1.0]);
        assert_eq!(z[0].rotation, [1.0, 0.0, 0.0, 0.0]);
    }
}
