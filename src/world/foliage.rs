//! Foliage meshes (rocks, trees, junk) scattered over a zone as Kenshi's FoliageSystem does when
//! a zone loads (ZoneMapContent::_activate F051437 -> F041804): each biome of the zone, each of its
//! FOLIAGE_LAYERs with meshes (F041803), each FOLIAGE_MESH of the layer a number of objects (the
//! layer's reference value), alone or in clusters, at random places of the zone, each kept or not
//! by the ground there (F041802).
//!
//! The random numbers are a Mersenne twister (MT19937) seeded per layer and zone (F041803); a
//! float in [a, b) is a + (b - a) x the tempered output / 2^32 (F041815), an integer in
//! [0, n] the tempered outputs masked to n's bits until one is no larger (F041775).
//!
//! The grass (F041797, PagedGeometry's GrassLoader) is `paint_grass` and `grass_blades` here, drawn
//! by kenshi_render::zone. Not ported: FOLIAGE_BUILDINGs ("building type"),
//! collision, sounds, navmesh cutting, "use accurate trace" (the terrain's height is taken), the
//! zone's own ground (F050506's zone lookup: the terrain's normal is taken), F041804's clearing of
//! the overlay's grass for flagged zones.

use crate::town::{Town, nearest};
use kenshi_data::{GameData, GameDataContainer, ItemType};

/// A FOLIAGE_MESH (F041792).
#[derive(Debug, Clone)]
pub struct FoliageMesh {
    pub record: String,
    pub mesh: String,
    pub max_slope: f32,
    pub min_slope: f32,
    pub min_altitude: f32,
    pub max_altitude: f32,
    pub floating: bool,
    pub clustered: bool,
    pub cluster_num: (f32, f32),
    pub cluster_radius: (f32, f32),
    pub limit_to_grass: bool,
    pub child_cluster_radius: f32,
    pub road_avoidance: f32,
    pub grass_spot: f32,
    pub avoid_towns: bool,
    pub building_type: bool,
    /// (min, max) height: the scale is drawn from twice these, within 0.01 .. 10.
    pub height: (f32, f32),
    pub vertical_offset: (f32, f32),
    /// The sub "meshes": child clusters round each object.
    pub children: Vec<usize>,
}

/// A FOLIAGE_LAYER's meshes (the layer's "meshes" references: mesh, count per zone).
#[derive(Debug, Clone)]
pub struct FoliageLayer {
    pub record: String,
    /// parseInt of the string id (the seed's base).
    pub seed_base: i32,
    pub meshes: Vec<(usize, f32)>,
    pub visibility_range: i32,
    pub wind: bool,
}

/// One object placed.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub mesh: usize,
    pub layer: usize,
    pub position: [f32; 3],
    /// Degrees about y.
    pub yaw: f32,
    pub scale: f32,
}

/// A GRASS record as F041797 and F041791 take it.
#[derive(Debug, Clone)]
pub struct Grass {
    pub record: String,
    pub sprite: String,
    /// setMinimumSize / setMaximumSize: (width, height) x 9 and x 17.
    pub min_size: (f32, f32),
    pub max_size: (f32, f32),
    /// x 0.005 (the caller multiplies in the "grass density" setting).
    pub density: f32,
    pub altitude: (f32, f32),
    pub max_slope: f32,
    pub cross_quads: bool,
    pub wind_factor: f32,
    pub noise_scale: f32,
    pub zero_cutoff: f32,
    pub blackout: bool,
    pub blackout_noise_scale: f32,
    pub blackout_zero_cutoff: f32,
    pub cap: f32,
    pub brightness_boost: f32,
}

/// A layer's grass: the GRASS, the overlay channel holding its density (2, red: painted with
/// its noise; 1, green: when the layer's reference value is set), the layer's visibility range.
#[derive(Debug, Clone, Copy)]
pub struct GrassUse {
    pub grass: usize,
    pub channel: usize,
    pub visibility_range: i32,
}

/// The foliage records.
#[derive(Debug, Clone, Default)]
pub struct Foliage {
    pub meshes: Vec<FoliageMesh>,
    pub layers: Vec<FoliageLayer>,
    /// Per BIOMES record: its layers with meshes, in reference order.
    pub biome_layers: std::collections::HashMap<String, Vec<usize>>,
    pub grass: Vec<Grass>,
    /// Per BIOMES record: its layers' grass, in reference order.
    pub biome_grass: std::collections::HashMap<String, Vec<GrassUse>>,
}

/// Ogre StringConverter::parseInt: the leading integer (0 when none).
fn parse_int(s: &str) -> i32 {
    let t = s.trim_start();
    let end = t
        .char_indices()
        .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    t[..end].parse().unwrap_or(0)
}

impl Foliage {
    pub fn load(data: &GameDataContainer) -> Foliage {
        let mut f = Foliage::default();
        let mut mesh_index: std::collections::HashMap<String, usize> = Default::default();
        fn mesh_of(
            data: &GameDataContainer,
            r: &GameData,
            f: &mut Foliage,
            index: &mut std::collections::HashMap<String, usize>,
            depth: u32,
        ) -> usize {
            if let Some(&i) = index.get(&r.string_id) {
                return i;
            }
            let fl = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
            let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
            let i = f.meshes.len();
            index.insert(r.string_id.clone(), i);
            f.meshes.push(FoliageMesh {
                record: r.string_id.clone(),
                mesh: r.files.get("mesh").cloned().unwrap_or_default(),
                max_slope: fl("max slope"),
                min_slope: fl("min slope"),
                min_altitude: fl("min altitude"),
                max_altitude: fl("max altitude"),
                floating: b("floating"),
                clustered: b("clustered"),
                cluster_num: (fl("cluster num min").trunc(), fl("cluster num max").trunc()),
                cluster_radius: (fl("cluster radius min"), fl("cluster radius max")),
                limit_to_grass: b("limit to grass areas"),
                child_cluster_radius: fl("child cluster radius"),
                road_avoidance: fl("road avoidance"),
                grass_spot: fl("grass spot"),
                avoid_towns: b("avoid towns"),
                building_type: !r.refs("building type").is_empty(),
                height: (fl("min height"), fl("max height")),
                vertical_offset: (fl("vertical offset min"), fl("vertical offset max")),
                children: Vec::new(),
            });
            if depth < 4 {
                let kids: Vec<usize> = r
                    .refs("meshes")
                    .iter()
                    .filter_map(|(id, _)| data.get(id))
                    .filter(|c| c.item_type() == Some(ItemType::FoliageMesh))
                    .map(|c| mesh_of(data, c, f, index, depth + 1))
                    .collect();
                f.meshes[i].children = kids;
            }
            i
        }
        let mut layer_index: std::collections::HashMap<String, usize> = Default::default();
        let mut grass_index: std::collections::HashMap<String, usize> = Default::default();
        for biome in data.of_type(ItemType::Biomes) {
            let mut list = Vec::new();
            let mut grass = Vec::new();
            for (id, _) in biome.refs("foliage") {
                let Some(l) = data.get(id) else { continue };
                for (gid, v) in l.refs("grass") {
                    let Some(g) = data.get(gid) else { continue };
                    let gi = *grass_index.entry(gid.clone()).or_insert_with(|| {
                        let fl = |k: &str| g.floats.get(k).copied().or_else(|| g.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
                        let b = |k: &str| g.bools.get(k).copied().unwrap_or(false);
                        f.grass.push(Grass {
                            record: gid.clone(),
                            sprite: g.files.get("grass sprite").cloned().unwrap_or_default(),
                            min_size: (fl("min width") * 9.0, fl("min height") * 9.0),
                            max_size: (fl("max width") * 17.0, fl("max height") * 17.0),
                            density: fl("density") * 0.005,
                            altitude: (fl("min altitude"), fl("max altitude")),
                            max_slope: fl("max slope"),
                            cross_quads: b("cross quads"),
                            wind_factor: fl("wind factor"),
                            noise_scale: fl("noise scale"),
                            zero_cutoff: fl("zero cutoff"),
                            blackout: b("blackout"),
                            blackout_noise_scale: fl("blackout noise scale"),
                            blackout_zero_cutoff: fl("blackout zero cutoff"),
                            cap: fl("cap"),
                            brightness_boost: fl("brightness boost"),
                        });
                        f.grass.len() - 1
                    });
                    grass.push(GrassUse {
                        grass: gi,
                        channel: if v[0] != 0 { 1 } else { 2 },
                        visibility_range: *l.ints.get("visibility range").unwrap_or(&0),
                    });
                }
                if l.refs("meshes").is_empty() {
                    continue;
                }
                let li = match layer_index.get(id) {
                    Some(&i) => i,
                    None => {
                        let meshes = l
                            .refs("meshes")
                            .iter()
                            .filter_map(|(mid, v)| data.get(mid).map(|m| (m, v[0] as f32)))
                            .map(|(m, n)| (mesh_of(data, m, &mut f, &mut mesh_index, 0), n))
                            .collect();
                        let i = f.layers.len();
                        f.layers.push(FoliageLayer {
                            record: l.string_id.clone(),
                            seed_base: parse_int(&l.string_id),
                            meshes,
                            visibility_range: *l.ints.get("visibility range").unwrap_or(&0),
                            wind: l.bools.get("wind").copied().unwrap_or(false),
                        });
                        layer_index.insert(id.clone(), i);
                        i
                    }
                };
                list.push(li);
            }
            f.biome_layers.insert(biome.string_id.clone(), list);
            f.biome_grass.insert(biome.string_id.clone(), grass);
        }
        f
    }
}

/// Kenshi's MT19937 (the global generator at 0x142131e90).
pub struct Mt {
    state: [u32; 624],
    index: usize,
}

impl Mt {
    pub fn new(seed: u32) -> Mt {
        let mut state = [0u32; 624];
        state[0] = seed;
        for i in 1..624 {
            state[i] = (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(0x6c07_8965).wrapping_add(i as u32);
        }
        Mt { state, index: 624 }
    }

    fn reload(&mut self) {
        for i in 0..624 {
            let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
            let mut v = self.state[(i + 397) % 624] ^ (y >> 1);
            if y & 1 != 0 {
                v ^= 0x9908_b0df;
            }
            self.state[i] = v;
        }
        self.index = 0;
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            self.reload();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// @fn F041815
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        let u = self.next_u32() as f64;
        (u * (1.0 / 4_294_967_296.0) * (b - a) as f64 + a as f64) as f32
    }

    /// @fn F041775
    ///
    /// An integer in [0, n].
    pub fn int(&mut self, n: u32) -> u32 {
        let mut m = n >> 1 | n;
        m |= m >> 2;
        m |= m >> 4;
        m |= m >> 8;
        m |= m >> 16;
        loop {
            let v = self.next_u32() & m;
            if v <= n {
                return v;
            }
        }
    }
}

/// What the scatter reads of the world (world positions).
pub trait Ground {
    fn height(&mut self, x: f32, z: f32) -> f32;
    /// 1 - the terrain normal's y (F050506).
    fn slope(&mut self, x: f32, z: f32) -> f32;
    /// The biome index there (F051563), for zones of several biomes.
    fn biome_index(&mut self, x: f32, z: f32) -> i32;
    /// The overlay map's pixel there as Ogre has it in memory (B, G, R, A), or None off it.
    fn overlay(&mut self, x: f32, z: f32) -> Option<[u8; 4]>;
    /// The overlay's pixel (column, row) and the pixel size.
    fn overlay_cell(&mut self, x: f32, z: f32) -> Option<(i32, i32, f32)>;
    fn overlay_at(&mut self, col: i32, row: i32) -> Option<[u8; 4]>;
    /// An inner pixel's centre (world x, z): None on the overlay's rim or off it.
    fn overlay_centre(&mut self, col: i32, row: i32) -> Option<(f32, f32)>;
    fn set_overlay_green(&mut self, col: i32, row: i32, v: u8);
    fn set_overlay_channel(&mut self, col: i32, row: i32, channel: usize, v: u8);
    /// A pixel's centre (world x, z), any pixel of the tile.
    fn pixel_world(&mut self, col: i32, row: i32) -> Option<(f32, f32)>;
    fn water_level(&self) -> f32;
}

/// The zone's rectangle: x0, z0, x1, z1 (world).
pub type Rect = [f32; 4];

/// @fn F041803
///
/// One layer's meshes scattered over a zone. `biome` is the biome index the places must be in
/// (None: the zone has one biome).
pub fn scatter_layer(
    f: &Foliage,
    layer: usize,
    rect: Rect,
    biome: Option<i32>,
    towns: &[Town],
    ground: &mut dyn Ground,
    out: &mut Vec<Placed>,
) {
    let l = &f.layers[layer];
    // the seed: the layer's id less a hash of the zone's corner
    let mut u = (rect[1] as i32).wrapping_mul(0x39).wrapping_add(rect[0] as i32) as u32;
    u ^= u << 13;
    let v = (u.wrapping_mul(u).wrapping_mul(0xec4d).wrapping_add(0x0131_071f).wrapping_mul(u).wrapping_add(0xd208_dd0d))
        & 0x7fff_ffff;
    let off = (v as f64 * 9.313_225_746_154_785e-10 * 16_777_215.0 - 16_777_215.0) as i32;
    let mut mt = Mt::new(l.seed_base.wrapping_sub(off) as u32);
    for &(mi, count) in &l.meshes {
        let m = &f.meshes[mi];
        let n = count as i32;
        let mut remaining: i32 = 0;
        let mut radius = 0.0f32;
        let mut centre = (0.0f32, 0.0f32);
        let mut i = 0;
        while i < n {
            remaining -= 1;
            let mut skip = false;
            let pos = if !m.clustered || remaining < 1 {
                if m.clustered {
                    let lo = m.cluster_num.0 as i32;
                    let span = (m.cluster_num.1 as i32 - lo) as u32;
                    remaining = mt.int(span) as i32 + lo;
                    radius = mt.range(m.cluster_radius.0, m.cluster_radius.1);
                }
                // a random place in the zone (on grass, if limited to it: 20 more tries)
                let mut tries = 0;
                loop {
                    let x = mt.range(rect[0], rect[2]);
                    let z = mt.range(rect[1], rect[3]);
                    centre = (x, z);
                    if !m.limit_to_grass {
                        break;
                    }
                    let px = ground.overlay(x, z).unwrap_or([0; 4]);
                    if px[1] as u32 + px[2] as u32 > 0x95 {
                        break;
                    }
                    if tries > 20 {
                        reject(4);
                        remaining = 0;
                        skip = true;
                        break;
                    }
                    tries += 1;
                }
                centre
            } else {
                let a = mt.range(-1.0, 1.0);
                let b = mt.range(-1.0, 1.0);
                let (dx, dz) = normalise2(b, a);
                let d = mt.range(5.0, radius);
                (centre.0 + dx * d, centre.1 + dz * d)
            };
            if !skip {
                if biome.is_none_or(|bi| ground.biome_index(pos.0, pos.1) == bi) {
                    if !place(f, mi, layer, [pos.0, 0.0, pos.1], rect, towns, &mut mt, ground, out) {
                        remaining = 0;
                    }
                } else {
                    reject(5);
                    if remaining > 0 {
                        i += remaining;
                    }
                    remaining = 0;
                }
            }
            i += 1;
        }
    }
}

/// (testing) why objects were not placed: slope, altitude, town, road, grass-limit, biome, placed.
pub static REJECTS: [std::sync::atomic::AtomicU32; 7] = [const { std::sync::atomic::AtomicU32::new(0) }; 7];
fn reject(k: usize) {
    REJECTS[k].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn normalise2(x: f32, y: f32) -> (f32, f32) {
    let l = (x * x + y * y).sqrt();
    if l > 1e-8 { (x / l, y / l) } else { (x, y) }
}

/// @fn F041802
///
/// One object at (x, z): false when the ground's slope or height is out of the mesh's range
/// (ending its cluster).
#[allow(clippy::too_many_arguments)]
fn place(
    f: &Foliage,
    mi: usize,
    layer: usize,
    at: [f32; 3],
    rect: Rect,
    towns: &[Town],
    mt: &mut Mt,
    ground: &mut dyn Ground,
    out: &mut Vec<Placed>,
) -> bool {
    let m = &f.meshes[mi];
    let mut p = at;
    let slope = ground.slope(p[0], p[2]);
    if slope > m.max_slope || slope < m.min_slope {
        reject(0);
        if std::env::var("KENSHI_SLOPE_DEBUG").is_ok() {
            eprintln!("slope {slope:.3} min {} max {} {}", m.min_slope, m.max_slope, m.mesh);
        }
        return false;
    }
    // outside the page (the zone)
    if p[0] < rect[0] || p[0] > rect[2] || p[2] < rect[1] || p[2] > rect[3] {
        return true;
    }
    let mut blocked = false;
    if m.avoid_towns
        && let Some(t) = nearest(towns, p, 10, None)
    {
        let t = &towns[t];
        let (dx, dz) = (t.position[0] - p[0], t.position[2] - p[2]);
        // the town's "no-foliage range" (+0x384), not its radius
        if !m.building_type && dz * dz + dx * dx < t.no_foliage * t.no_foliage {
            blocked = true;
            reject(2);
        }
    }
    p[1] = ground.height(p[0], p[2]);
    if p[1] > m.max_altitude || p[1] < m.min_altitude {
        reject(1);
        return false;
    }
    if m.floating {
        p[1] = p[1].max(ground.water_level());
    }
    // the scale: from twice the min and max height, within the loader's 0.01 .. 10
    let lo = (m.height.0 * 2.0).clamp(0.01, 10.0);
    let hi = (m.height.1 * 2.0).clamp(0.01, 10.0);
    let scale = mt.range(lo, hi);
    let mut off = m.vertical_offset.0;
    if off != m.vertical_offset.1 {
        off = mt.range(m.vertical_offset.0, m.vertical_offset.1);
    }
    p[1] += off * scale;
    // roads (the overlay's alpha) round the square of the road avoidance: its rim and middle
    if m.road_avoidance > 0.0 {
        let r = m.road_avoidance * scale;
        if let (Some((c0, r0, _)), Some((c1, r1, _))) =
            (ground.overlay_cell(p[0] - r, p[2] - r), ground.overlay_cell(p[0] + r, p[2] + r))
        {
            let road = |g: &mut dyn Ground, c: i32, r: i32| g.overlay_at(c, r).is_some_and(|px| px[3] != 0);
            for c in c0..=c1 {
                if road(ground, c, r0) {
                    blocked = true;
                    break;
                }
            }
            if !blocked && (c1 != c0 || r1 != r0) {
                if road(ground, (c0 + c1) / 2, (r0 + r1) / 2) {
                    blocked = true;
                } else if r0 < r1 {
                    for c in c0..=c1 {
                        if road(ground, c, r1) {
                            blocked = true;
                            break;
                        }
                    }
                    if !blocked {
                        for r in r0 + 1..r1 {
                            if road(ground, c0, r) || road(ground, c1, r) {
                                blocked = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    // the yaw, in 255 steps
    let r = mt.range(0.0, 360.0);
    let yaw = (((r / 360.0) * 255.0) as i64 as u32 & 0xff) as f32 * (360.0 / 255.0);
    if !blocked && !m.building_type {
        out.push(Placed { mesh: mi, layer, position: p, yaw, scale });
        reject(6);
    } else if blocked {
        reject(3);
    }
    // child clusters round it
    for &ci in &m.children {
        let c = &f.meshes[ci];
        let lo = c.cluster_num.0 as i32;
        let n = mt.int((c.cluster_num.1 as i32 - lo) as u32) as i32 + lo;
        let radius = mt.range(c.cluster_radius.0, c.cluster_radius.1);
        let base = c.child_cluster_radius + m.child_cluster_radius;
        for _ in 0..n {
            let a = mt.range(-1.0, 1.0);
            let b = mt.range(-1.0, 1.0);
            let (dx, dz) = normalise2(b, a);
            let d = mt.range(0.0, radius) + base;
            place(f, ci, layer, [p[0] + dx * d, 0.0, p[2] + dz * d], rect, towns, mt, ground, out);
        }
    }
    // a grass spot painted into the overlay's green: the pixels inside its square (not the
    // overlay's rim), the value multiplied down pixel by pixel by 1 - 0.3 d^2 / r^2 (as Kenshi
    // does, compounding)
    if !blocked && m.grass_spot > 0.0 {
        let r = m.grass_spot * scale;
        if let (Some((c0, r0, cell)), Some((c1, r1, _))) =
            (ground.overlay_cell(p[0] - r, p[2] - r), ground.overlay_cell(p[0] + r, p[2] + r))
        {
            let several = c0 != c1 || r0 != r1;
            let mut v = ((r * 2.0) / cell).clamp(0.3, 1.0);
            for c in c0..=c1 {
                for row in r0..=r1 {
                    let Some(centre) = ground.overlay_centre(c, row) else { continue };
                    if several {
                        let d2 = (centre.0 - p[0]).powi(2) + (centre.1 - p[2]).powi(2);
                        v *= 1.0 - (d2 / (r * r)) * 0.3;
                    }
                    ground.set_overlay_green(c, row, (v * 96.0) as i64 as u8);
                }
            }
        }
    }
    true
}

/// The noise lattice (Hugo Elias' Noise2: an integer hash to -1 .. 1).
fn lattice(x: i32, y: i32) -> f64 {
    let n = x.wrapping_add(y.wrapping_mul(0x39));
    let n = (n << 13) ^ n;
    let v = n
        .wrapping_mul(n.wrapping_mul(n).wrapping_mul(0xec4d).wrapping_add(0x0131_071f))
        .wrapping_add(0xd208_dd0du32 as i32)
        & 0x7fff_ffff;
    1.0 - v as f64 * 9.313_225_746_154_785e-10
}

/// @fn F041778
///
/// The lattice smoothed: the corners / 16, the sides / 8, the middle / 4.
fn smoothed(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x as i32, y as i32);
    let (xm, xp, ym, yp) = ((x - 1.0) as i32, (x + 1.0) as i32, (y - 1.0) as i32, (y + 1.0) as i32);
    let corners = (lattice(xm, ym) + lattice(xp, ym) + lattice(xm, yp) + lattice(xp, yp)) * 0.0625;
    let sides = (lattice(xm, yi) + lattice(xp, yi) + lattice(xi, ym) + lattice(xi, yp)) * 0.125;
    (corners as f32) + (sides as f32) + ((lattice(xi, yi) * 0.25) as f32)
}

/// @fn F041780
///
/// The smoothed lattice at a point, cosine-interpolated.
fn interpolated(x: f32, y: f32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let pi = std::f32::consts::PI as f64;
    let tx = (1.0 - (((x - fx) as f64) * pi).cos()) * 0.5;
    let ty = (1.0 - (((y - fy) as f64) * pi).cos()) * 0.5;
    let (a, b) = (smoothed(fx, fy), smoothed(fx + 1.0, fy));
    let (c, d) = (smoothed(fx, fy + 1.0), smoothed(fx + 1.0, fy + 1.0));
    let top = (a as f64 * (1.0 - tx) + b as f64 * tx) as f32;
    let bottom = (c as f64 * (1.0 - tx) + d as f64 * tx) as f32;
    (top as f64 * (1.0 - ty) + bottom as f64 * ty) as f32
}

/// @fn F041781
///
/// Five octaves of the noise at (x, z) over `scale`, mapped to 0 .. 1, cut at `cutoff` (and
/// stretched back to 0 .. 1), times the brightness boost, at most the cap.
fn grass_noise(x: f64, z: f64, scale: f64, cutoff: f32, boost: f32, cap: f32) -> f32 {
    let mut sum = 0.0f64;
    for i in 0..5 {
        let freq = 2f32.powi(i);
        let amp = 0.5f64.powi(i);
        let v = interpolated((freq as f64 * x / scale) as f32, (freq as f64 * z / scale) as f32);
        sum += v as f64 * amp;
    }
    let n = sum * 0.5 + 0.5;
    let mut v = if n <= cutoff as f64 { 0.0 } else { ((n as f32 - cutoff) / (1.0 - cutoff)) as f64 };
    v *= boost as f64;
    v.max(0.0).min(cap as f64) as f32
}

/// @fn F041791
///
/// A grass's density painted into the overlay's red channel over the zone's 129 x 129 pixels
/// from (`col0`, `row0`): its noise (times its blackout noise) where the blue channel is at most
/// 70, faded over 300 units outside its altitudes; a pixel is only raised, and never on a road.
pub fn paint_grass(g: &Grass, col0: i32, row0: i32, biome: Option<i32>, ground: &mut dyn Ground) {
    let Some((_, _, cell)) = ground.overlay_cell(0.0, 0.0) else { return };
    let base = cell as f64 * 2.0;
    let (ns, bs) = (g.noise_scale as f64 * base, g.blackout_noise_scale as f64 * base);
    for col in col0..col0 + 129 {
        for row in row0..row0 + 129 {
            let Some((x, z)) = ground.pixel_world(col, row) else { continue };
            if biome.is_some_and(|b| ground.biome_index(x, z) != b) {
                continue;
            }
            let h = ground.height(x, z);
            let fade = if h < g.altitude.0 {
                (h - (g.altitude.0 - 300.0)) / 300.0
            } else if h > g.altitude.1 {
                ((g.altitude.1 + 300.0) - h) / 300.0
            } else {
                1.0
            }
            .clamp(0.0, 1.0);
            let Some(px) = ground.overlay_at(col, row) else { continue };
            let full = if px[0] as f32 > 70.0 { 0.0 } else { 255.0 };
            let mut n = grass_noise(x as f64, z as f64, ns, g.zero_cutoff, g.brightness_boost, g.cap);
            if g.blackout {
                n *= grass_noise(x as f64 + 50007.0, z as f64 + 50007.0, bs, g.blackout_zero_cutoff, g.brightness_boost, g.cap);
            }
            let v = (n * full * fade) as i64 as u8;
            if px[3] > 10 {
                continue;
            }
            if px[2] < v {
                ground.set_overlay_channel(col, row, 2, v);
            }
        }
    }
}

/// One blade: position (x, z), its size between the minimum and maximum (0 .. 1), its yaw.
#[derive(Debug, Clone, Copy)]
pub struct Blade {
    pub x: f32,
    pub z: f32,
    pub size: f32,
    pub yaw: f32,
}

/// @fn F052233
///
/// A grass's blades over a page (x0, z0, x1, z1): density x area candidates at random places,
/// kept where the ground's slope is within the grass's and a random number is under the density
/// map's value (the overlay's channel / 255), and within the altitudes when set.
pub fn grass_blades(g: &Grass, use_: &GrassUse, density_setting: f32, page: Rect, rng: &mut Mt, ground: &mut dyn Ground) -> Vec<Blade> {
    let count = (g.density * density_setting * (page[2] - page[0]) * (page[3] - page[1])) as u32;
    let (lo, hi) = (
        if g.altitude.0 == 0.0 { f32::NEG_INFINITY } else { g.altitude.0 },
        if g.altitude.1 == 0.0 { f32::INFINITY } else { g.altitude.1 },
    );
    let mut out = Vec::new();
    let unit = |rng: &mut Mt| (rng.next_u32() as f64 * (1.0 / 4_294_967_296.0)) as f32;
    for _ in 0..count {
        let x = page[0] + (page[2] - page[0]) * unit(rng);
        let z = page[1] + (page[3] - page[1]) * unit(rng);
        if ground.slope(x, z) > g.max_slope {
            continue;
        }
        let r = unit(rng);
        let d = ground.overlay(x, z).map_or(0.0, |px| px[use_.channel] as f32 / 255.0);
        if d <= r {
            unit(rng);
            unit(rng);
            continue;
        }
        let h = ground.height(x, z);
        if h < lo || hi < h {
            unit(rng);
            unit(rng);
            continue;
        }
        let size = unit(rng);
        let yaw = std::f32::consts::TAU * unit(rng);
        out.push(Blade { x, z, size, yaw });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt19937_reference() {
        // the reference generator's first outputs for seed 5489
        let mut mt = Mt::new(5489);
        assert_eq!(mt.next_u32(), 3_499_211_612);
        assert_eq!(mt.next_u32(), 581_869_302);
    }

    #[test]
    fn parse_int_like_ogre() {
        assert_eq!(parse_int("3606-Newwworld.mod"), 3606);
        assert_eq!(parse_int("abc"), 0);
    }
}
