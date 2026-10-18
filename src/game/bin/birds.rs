//! Birds (Kenshi's BirdManager, its own thread "Birds"): flocks of each WILDLIFE_BIRDS type
//! flying over the biomes that list it.
//!
//! - F028452 loads a type: "mesh"/"texture", "speed", "height", "ground height", "contour
//!   strength", "spawn height min/max", "spawn count", "distance", "flocking", "circling",
//!   "scale min/max" ("ground strength" is not read); a follow gap of 100; each BIOME_GROUP's
//!   "birds" reference gives the type's number per zone. A type with no mesh, or that no biome
//!   lists, is dropped. Its birds are instances (HWInstancingBasic, 100 a batch) of the mesh's
//!   first sub-mesh with the type's own copy of HWBirds ("HWBirds.<name>", DiffuseMap = its
//!   "texture"); each bird's node takes a scale between "scale min" and "scale max" when made
//!   (F028440) and its position and orientation every frame (F028448).
//! - F028446, how many there should be: the zones within "distance" of the camera (a square,
//!   clamped to the map) whose biome lists the type, their numbers summed (truncated as they go,
//!   none below 0).
//! - F028456, each frame per type: while the birds alive plus "spawn count" are no more than that,
//!   F028455 spawns: a random listed zone (a random 0 ..= n, the first zone taken for 0 and 1), a
//!   random point in it on the ground there (F050534), at least 1000 units from the camera and
//!   strictly between the spawn heights; "spawn count" birds facing the camera, each 5 units on in
//!   a random level direction from the last (F028454: wandering, for 30-180 s, in the first slot
//!   of the type's list a gone bird left, else a new one).
//! - F028456 then moves each bird of the type: on along its heading at "speed"; out of its biomes
//!   it turns back (1: 10^6 s, its centre where it just was, its attractor let go) every frame
//!   it is out. The ground under it and its normal are the terrain's approximate height
//!   (Terrain::getApproximateHeight: the nearest vertex of the drawn terrain patch; -99 and the
//!   last normal where there is none). By its state:
//!   1 turning back: toward its centre at its own height, 200 to the side of the line to it
//!     (so it wheels round); back in its biomes its timer ends;
//!   2 wandering / 6 after an attractor: on a slope (normal y under 0.999) along the contour
//!     (n.z, 0, -n.x), the way it heads, x "contour strength", plus the level normal down the
//!     slope where the ground is under "ground height" (else up it); a leader within 100;
//!   3 following: a leader within 100 if any, else the one it has; it heads for the point 100 to
//!     its side of its leader, abreast of it, and speeds up or slows (x(1 + 0.003 x its lead,
//!     within +-100), per second not per unit) for the slot 100 behind that;
//!   4 circling a point: the centre jumps to it where the ground is higher than the centre, and
//!     drifts 1 a second along the level normal (down the slope under "height", else up);
//!   4 and 5 (circling an attractor, whose position is the centre): within 1.4 radius of the
//!     centre it heads for the circle 100 ahead of it, farther out for the centre plus
//!     (heading.z, 0, heading.x) x radius (as Kenshi has it); the ground is taken no lower than
//!     500 under the centre.
//!   Its height: under 30 over the ground it climbs (y 5), following it keeps its own; else it
//!   heads for (1 + 0.3 v) x "height" over the ground (v = frac(4.32167 x its slot) - 0.5), or its
//!   leader's height over this ground + 5 v when it has a leader, the gap over 2000. It turns
//!   toward that at a radian a second (dt over the angle, unclamped), straight to it within
//!   0.99. Wandering or circling a point, the attractor that draws it most (F028445) takes it:
//!   5 for 30-90 s, radius 400-600. Its timer up: turning back, circling or after an attractor
//!   it wanders (10-40 s) or follows a leader within 1000; wandering or following it circles
//!   where it is (a "circling" type: 30-180 s, radius 400-600, no leader) else looks for a
//!   leader within 1000 (5-30 s; one that was following stays so); at an attractor it leaves it
//!   (6, 10-40 s). Farther than "distance" from the camera and over 10 outside any plane of
//!   the view it is gone. Day and night alike; no sound, fade or LOD.
//! - F028443, a leader: (a "flocking" type) the nearest bird within reach that is not gone,
//!   circling or at an attractor, with the bird behind it, whose leaders don't lead back.
//! - F028457 / F028449 / F028447: "bird attractor" references (radius m, most birds) of
//!   buildings (F037851), map features (F032040) and animals (F039228: asAnimal) draw a type's
//!   birds: the weight (radius x 10)^2, the nearest by weight over distance squared above 1;
//!   one object listing a type twice adds the most birds and keeps the larger weight. An
//!   attractor lasts while its object does (a dead animal's too) and follows its position.
//!
//! (assumption) The frame time is the game's (0 when paused, x the game speed). (stand-in) A
//! follower with no leader at all (its search at the timer failed) keeps its heading: Kenshi
//! reads through the null leader there. The normal before any is found is zero (Kenshi's is
//! uninitialised). Map features' attractors are those of the zones asked for. The birds are
//! drawn with HWBirds (deferred/birds.material): Object_FP (objects.hlsl with no defines: glossMult
//! 1, no dust or colouring, back faces culled) with the type's "texture", flat.dds and black.dds;
//! its instancing vertex shader only places the static mesh (no wing beat).
use super::Game;
use bevy::camera::primitives::Frustum;
use bevy::prelude::*;
use kenshi_io::ogre_mesh;
use kenshi_render::terrain_lod::LodTerrain;
use kenshi_world::{WORLD_SIZE, ZONE_SIZE};
use std::collections::HashMap;

/// F028455: the least distance from the camera a flock appears at (squared, level).
const SPAWN_CLEAR2: f32 = 1_000_000.0;
/// F028455: the gap between birds of a new flock.
const FLOCK_GAP: f32 = 5.0;
/// F028456: under this height over the ground a bird climbs straight away.
const CLIMB_UNDER: f32 = 30.0;
/// F028456: the height gap the climb is taken over.
const CLIMB_OVER: f32 = 2000.0;
/// F028456: the climb's y, and how much a leader's height is varied by.
const CLIMB_Y: f32 = 5.0;
/// F028456: a leader is looked for this near while wandering or following, this near when a
/// timer runs out.
const LEADER_NEAR: f32 = 100.0;
const LEADER_FAR: f32 = 1000.0;
/// F028452: how far beside and behind its leader a bird keeps.
const FOLLOW_GAP: f32 = 100.0;
/// F028456: a circle's floor under its centre.
const CIRCLE_FLOOR: f32 = 500.0;
/// F028456: turning back, how far beside the line to its centre a bird heads.
const TURN_ASIDE: f32 = 200.0;
/// F050509: the ground where the terrain has no answer.
const NO_GROUND: f32 = -99.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Gone,
    TurnBack,
    Wander,
    Follow,
    Circle,
    Attractor,
    WanderAfter,
}

/// What draws birds: a placed building, an animal or a map feature.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Source {
    /// (placement, bird type)
    Placed(usize, usize),
    /// (character, bird type)
    Person(usize, usize),
    /// (zone, map feature, bird type)
    Feature((i32, i32), usize, usize),
}

struct Attractor {
    ty: usize,
    at: Vec3,
    /// F028449: (radius x 10)^2.
    weight: f32,
    max: i32,
    count: i32,
}

struct Kind {
    id: String,
    mesh: Handle<Mesh>,
    material: Handle<kenshi_render::object_material::KenshiObjectMaterial>,
    speed: f32,
    height: f32,
    ground_height: f32,
    contour: f32,
    spawn_min: f32,
    spawn_max: f32,
    spawn_count: i32,
    distance: f32,
    flocking: bool,
    circling: bool,
    scale: (f32, f32),
    /// BIOME_GROUP -> its number per zone.
    per_zone: HashMap<String, f32>,
    /// The zone square the count was taken over, the zones in it listing the type, and the
    /// number wanted.
    counted: Option<((i32, i32), (i32, i32))>,
    zones: Vec<(i32, i32)>,
    wanted: i32,
}

struct Bird {
    at: Vec3,
    /// Its orientation's z axis.
    heading: Vec3,
    state: State,
    timer: f32,
    radius: f32,
    centre: Vec3,
    /// A bird of the same type's list (kept when that one is gone or its place taken).
    leader: Option<usize>,
    attractor: Option<Source>,
    entity: Option<Entity>,
}

#[derive(Resource, Default)]
pub struct Birds {
    kinds: Option<Vec<Kind>>,
    /// Each type's birds (gone ones keep their place until a new bird takes it).
    lists: Vec<Vec<Bird>>,
    attractors: HashMap<Source, Attractor>,
    attractors_for: usize,
    terrain: Option<kenshi_world::Terrain>,
    seed: u64,
    test_flock: bool,
    /// (type, place, where made) of the test flock.
    held: Vec<(usize, usize, Vec3)>,
}

impl Birds {
    fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 40) as f32 / (1u64 << 24) as f32
    }

    /// F050502: a random number between `lo` and `hi`.
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.rand()
    }

    /// F050548: a random whole number `lo ..= hi`.
    fn int(&mut self, lo: i32, hi: i32) -> i32 {
        (lo + (self.rand() * (hi - lo + 1) as f32) as i32).clamp(lo, hi)
    }

    /// The ground's height (F050534: the terrain's).
    fn ground(&mut self, x: f32, z: f32) -> f32 {
        self.terrain.as_mut().and_then(|t| t.height_at(x, z).ok()).unwrap_or(NO_GROUND)
    }

    /// Bird `b` of type `t` lets its attractor go (one at it turns to wandering).
    fn release(&mut self, t: usize, b: usize) {
        let me = &mut self.lists[t][b];
        if let Some(a) = me.attractor.take() {
            if me.state == State::Attractor {
                me.state = State::Wander;
            }
            if let Some(a) = self.attractors.get_mut(&a) {
                a.count -= 1;
            }
        }
    }

    /// F028443: the nearest bird of `b`'s list within `near` that `b` is behind, not gone,
    /// circling or at an attractor, whose leaders don't lead back to `b`.
    fn leader(&self, t: usize, b: usize, near: f32, flocking: bool) -> Option<usize> {
        if !flocking {
            return None;
        }
        let list = &self.lists[t];
        let me = &list[b];
        let mut best = None;
        let mut d2 = near * near;
        for (i, o) in list.iter().enumerate() {
            if i == b || matches!(o.state, State::Gone | State::Circle | State::Attractor) {
                continue;
            }
            let d = me.at.distance_squared(o.at);
            if o.heading.dot(o.at - me.at) > 0.0 && d < d2 {
                best = Some(i);
                d2 = d;
            }
        }
        // (the leaders never loop: the step count only guards it)
        let mut at = best;
        let mut steps = 0;
        while let Some(i) = at {
            if i == b || steps > list.len() {
                return None;
            }
            at = list[i].leader;
            steps += 1;
        }
        best
    }

    /// F028445: the attractor of type `t` not full that draws a bird at `at` most (weight over
    /// distance squared, above 1).
    fn attractor(&self, t: usize, at: Vec3) -> Option<Source> {
        let mut best = None;
        let mut score = 1.0;
        for (s, a) in &self.attractors {
            if a.ty != t || a.count >= a.max {
                continue;
            }
            let v = a.weight / a.at.distance_squared(at);
            if score < v {
                best = Some(*s);
                score = v;
            }
        }
        best
    }
}

fn kinds(game: &Game, assets: &AssetServer, meshes: &mut Assets<Mesh>, materials: &mut Assets<kenshi_render::object_material::KenshiObjectMaterial>, look: &mut kenshi_render::terrain_material::TerrainLook) -> Vec<Kind> {
    let data = &game.sim.data;
    let mut out = Vec::new();
    for r in data.of_type(kenshi_data::ItemType::WildlifeBirds) {
        let file = |k: &str| r.files.get(k).filter(|s| !s.is_empty()).cloned();
        let Some(mesh) = file("mesh").and_then(|p| game.res.path(&p)).and_then(|p| ogre_mesh::read(&p).ok()) else { continue };
        // the instances draw the mesh's first sub-mesh
        // (the mesh's vertex colours are not the HWBirds shader's: they would tint and cut it)
        let Some(mesh) = kenshi_render::to_bevy(&mesh).into_iter().next().map(|(_, mut m)| {
            m.remove_attribute(Mesh::ATTRIBUTE_COLOR);
            meshes.add(m)
        }) else {
            continue;
        };
        // HWBirds: Object_FP with the type's texture, flat.dds, black.dds
        let o = kenshi_render::object_material::ObjectLook {
            texture: file("texture").and_then(|t| game.res.find(&t)),
            normal: None,
            metal: None,
            texture2: None,
            normal2: None,
            metal2: None,
            tiling: [1.0, 1.0],
            gloss_mult: 1.0,
            threshold: 0.5,
            kind: 0,
            grass: false,
            foliage: false,
        };
        let mut m = look.object(assets, &o, game.zone.origin, false, [0.0; 3]);
        // no DUST (Object_FP is not a building part's)
        m.params.modes.y = 0.0;
        let material = materials.add(m);
        let f = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|v| *v as f32)).unwrap_or(0.0);
        let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
        out.push(Kind {
            id: r.string_id.clone(),
            mesh,
            material,
            speed: f("speed"),
            height: f("height"),
            ground_height: f("ground height"),
            contour: f("contour strength"),
            spawn_min: f("spawn height min"),
            spawn_max: f("spawn height max"),
            spawn_count: f("spawn count") as i32,
            distance: f("distance"),
            flocking: b("flocking"),
            circling: b("circling"),
            scale: (f("scale min"), f("scale max")),
            per_zone: HashMap::new(),
            counted: None,
            zones: Vec::new(),
            wanted: 0,
        });
    }
    for g in data.of_type(kenshi_data::ItemType::BiomeGroup) {
        for (s, v) in g.refs("birds") {
            if let Some(k) = out.iter_mut().find(|k| k.id == *s) {
                k.per_zone.insert(g.string_id.clone(), v[0] as f32);
            }
        }
    }
    // a type no biome lists is dropped
    out.retain(|k| !k.per_zone.is_empty());
    out
}

/// F028457 / F028449 / F028447: the attractors of placed buildings and map features (made again
/// when the zones asked for or the people change) and of animals, following their positions,
/// keeping the counts of those already drawing birds.
fn attractors(birds: &mut Birds, game: &Game, ids: &[String]) {
    let data = &game.sim.data;
    let refs = |rec: &str| -> Vec<(usize, f32, i32)> {
        let Some(r) = data.get(rec) else { return Vec::new() };
        r.refs("bird attractor")
            .iter()
            .filter_map(|(s, v)| Some((ids.iter().position(|i| i == s)?, (v[0] as f32).powi(2) * 100.0, v[1])))
            .collect()
    };
    let n = game.sim.folk.len() + game.asked.len() * 1_000_000;
    if birds.attractors_for == n + 1 {
        for (s, a) in birds.attractors.iter_mut() {
            if let Source::Person(id, _) = s {
                a.at = Vec3::from(game.sim.folk[*id].mover.position_units());
            }
        }
        return;
    }
    birds.attractors_for = n + 1;
    let mut fresh: HashMap<Source, Attractor> = HashMap::new();
    // F028447: the same object asking again for a type adds its birds, the larger weight kept
    let mut add = |s: Source, ty: usize, at: Vec3, weight: f32, max: i32| {
        let a = fresh.entry(s).or_insert(Attractor { ty, at, weight, max: 0, count: 0 });
        a.max += max;
        a.weight = a.weight.max(weight);
    };
    for (i, p) in game.sim.placed.iter().enumerate() {
        for (ty, weight, max) in refs(&p.building) {
            add(Source::Placed(i, ty), ty, Vec3::from(game.sim.placed_at[i]), weight, max);
        }
    }
    // the map features of the zones asked for (kenshi_world::map_features)
    let features = kenshi_render::zone::map_features(&game.root);
    for &z in &game.asked {
        for (k, f) in features.get(&z).into_iter().flatten().enumerate() {
            for (ty, weight, max) in refs(&f.record) {
                add(Source::Feature(z, k, ty), ty, Vec3::from(f.position), weight, max);
            }
        }
    }
    // F039228: animals (asAnimal), dead or alive
    for p in &game.sim.folk {
        let Some(c) = &p.character else { continue };
        if data.get(c).and_then(|r| r.item_type()) != Some(kenshi_data::ItemType::AnimalCharacter) {
            continue;
        }
        for (ty, weight, max) in refs(c) {
            add(Source::Person(p.id, ty), ty, Vec3::from(p.mover.position_units()), weight, max);
        }
    }
    for (s, a) in fresh.iter_mut() {
        if let Some(old) = birds.attractors.get(s) {
            a.count = old.count;
        }
    }
    birds.attractors = fresh;
    // a bird whose attractor went lets it go
    let Birds { lists, attractors, .. } = birds;
    for me in lists.iter_mut().flatten() {
        if me.attractor.is_some_and(|s| !attractors.contains_key(&s)) {
            me.attractor = None;
            if me.state == State::Attractor {
                me.state = State::Wander;
            }
        }
    }
}

/// The zone grid cell of a world point, clamped to the map.
fn cell(v: f32) -> i32 {
    (((v + WORLD_SIZE / 2.0) / ZONE_SIZE).floor() as i32).clamp(0, 63)
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut birds: ResMut<Birds>,
    game: Res<Game>,
    weather: Res<super::WeatherState>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    (mut materials, mut look): (ResMut<Assets<kenshi_render::object_material::KenshiObjectMaterial>>, ResMut<kenshi_render::terrain_material::TerrainLook>),
    cams: Query<(Entity, &GlobalTransform, &Frustum), super::MainCam>,
    mut shown: Query<&mut Transform>,
    lod: Option<Res<LodTerrain>>,
) {
    if std::env::var("KENSHI_NO_BIRDS").is_ok() {
        return;
    }
    let Ok((cam_entity, cam, frustum)) = cams.single() else { return };
    let Some(map) = weather.map.as_ref() else { return };
    if birds.kinds.is_none() {
        birds.seed = 0x9e37_79b9_7f4a_7c15;
        birds.terrain = kenshi_world::Terrain::open(&game.root).ok();
        let k = kinds(&game, &assets, &mut meshes, &mut materials, &mut look);
        birds.lists = k.iter().map(|_| Vec::new()).collect();
        birds.kinds = Some(k);
    }
    let mut kinds = birds.kinds.take().unwrap();
    let ids: Vec<String> = kinds.iter().map(|k| k.id.clone()).collect();
    attractors(&mut birds, &game, &ids);
    let origin = game.zone.origin;
    let eye = cam.translation() + origin;
    let dt = time.delta_secs().min(0.1) * if game.paused { 0.0 } else { game.speed };
    let biome_of = |x: i32, z: i32| kenshi_world::biome::biome_of_zone(map, &weather.biomes, x, z).map(|i| weather.biomes[i].record.clone());
    // (testing) KENSHI_BIRD_HERE=<type number>: a flock of that type 250 units ahead of the
    // camera, crossing the view, once
    if let Some(t) = std::env::var("KENSHI_BIRD_HERE").ok().and_then(|v| v.parse::<usize>().ok()).filter(|t| *t < kinds.len()) {
        let when = std::env::var("KENSHI_BIRD_AT").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(5.0);
        if !birds.test_flock && time.elapsed_secs() > when {
            let at = eye + cam.forward().as_vec3() * 250.0;
            let toward = at + cam.right().as_vec3() * 2000.0;
            birds.held = spawn_flock(&mut commands, &mut birds, &kinds[t], t, at, toward, origin);
            eprintln!("birds: at {:.1} s a flock of {} at {at:.0} (eye {eye:.0}, origin {origin:.0})", time.elapsed_secs(), kinds[t].id);
            birds.test_flock = true;
        }
    }
    // the terrain's normal is kept from bird to bird (where the terrain has no answer)
    let mut normal = Vec3::ZERO;
    let here = kenshi_world::zone_of(eye.x, eye.z);
    for (t, k) in kinds.iter_mut().enumerate() {
        // F028446: the count, over the zone square round the camera
        let square = ((cell(eye.x - k.distance), cell(eye.z - k.distance)), (cell(eye.x + k.distance), cell(eye.z + k.distance)));
        if k.counted != Some(square) {
            k.counted = Some(square);
            k.zones.clear();
            k.wanted = 0;
            for x in square.0.0..=square.1.0 {
                for z in square.0.1..=square.1.1 {
                    let Some(v) = biome_of(x, z).and_then(|b| k.per_zone.get(&b).copied()) else { continue };
                    k.zones.push((x, z));
                    k.wanted = (k.wanted as f32 + v.max(0.0)) as i32;
                }
            }
        }
        // F028455
        let alive = birds.lists[t].iter().filter(|b| b.state != State::Gone).count() as i32;
        if alive + k.spawn_count <= k.wanted && !k.zones.is_empty() {
            let pick = birds.int(0, k.zones.len() as i32);
            let (zx, zz) = k.zones[(pick - 1).max(0) as usize];
            let lo = Vec2::new(zx as f32 * ZONE_SIZE - WORLD_SIZE / 2.0, zz as f32 * ZONE_SIZE - WORLD_SIZE / 2.0);
            let x = birds.range(lo.x, lo.x + ZONE_SIZE);
            let z = birds.range(lo.y, lo.y + ZONE_SIZE);
            let y = birds.ground(x, z);
            let at = Vec3::new(x, y, z);
            if SPAWN_CLEAR2 <= (at.xz() - eye.xz()).length_squared() && k.spawn_min < y && y < k.spawn_max && 0 < k.spawn_count {
                spawn_flock(&mut commands, &mut birds, k, t, at, eye, origin);
            }
        }
        fly(&mut commands, &mut birds, k, t, dt, eye, origin, frustum, &biome_of, lod.as_deref(), &mut normal);
    }
    // (testing) KENSHI_BIRD_HOLD: the test flock stays where it was made (only turning)
    if std::env::var("KENSHI_BIRD_HOLD").is_ok() {
        for (t, b, at) in birds.held.clone() {
            birds.lists[t][b].at = at;
        }
    }
    // F028448: each bird's node at its position, its orientation from its heading (F028456:
    // FromAxes(Y x heading, heading x that, heading))
    for me in birds.lists.iter().flatten().filter(|b| b.state != State::Gone) {
        let Some(mut t) = me.entity.and_then(|e| shown.get_mut(e).ok()) else { continue };
        let fwd = me.heading;
        let right = Vec3::Y.cross(fwd).normalize_or_zero();
        let up = fwd.cross(right);
        t.translation = me.at - origin;
        t.rotation = Quat::from_mat3(&Mat3::from_cols(right, up, fwd));
    }
    // (testing) KENSHI_BIRD_VIEW=<distance>,<pitch>: the camera on the test flock's middle,
    // from that far at that pitch (radians), at the game's yaw
    if let Some(v) = std::env::var("KENSHI_BIRD_VIEW").ok().map(|v| v.split(',').filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 2)
        && !birds.held.is_empty()
    {
        let mid = birds.held.iter().map(|(t, b, _)| birds.lists[*t][*b].at).sum::<Vec3>() / birds.held.len() as f32 - origin;
        let (sy, cy, sp, cp) = (game.yaw.sin(), game.yaw.cos(), v[1].sin(), v[1].cos());
        let eye = mid + Vec3::new(sy * cp, sp, cy * cp) * v[0];
        if let Ok(mut t) = shown.get_mut(cam_entity) {
            *t = Transform::from_translation(eye).looking_at(mid, Vec3::Y);
        }
    }
    debug(&birds, &kinds, &time, eye, here);
    birds.kinds = Some(kinds);
}

/// F028455 / F028454 / F028440: "spawn count" birds of type `t` from `at`, each facing `toward`
/// (the camera), the next 5 units on in a random level direction; each in the first gone bird's
/// place, else a new one. Their places.
fn spawn_flock(commands: &mut Commands, birds: &mut Birds, k: &Kind, t: usize, mut at: Vec3, toward: Vec3, origin: Vec3) -> Vec<(usize, usize, Vec3)> {
    let mut made = Vec::new();
    for _ in 0..k.spawn_count {
        let heading = (toward - at).normalize_or(Vec3::Z);
        let timer = birds.range(30.0, 180.0);
        let scale = birds.range(k.scale.0, k.scale.1);
        let root = commands.spawn((Transform::from_translation(at - origin).with_scale(Vec3::splat(scale)), Visibility::default())).id();
        let e = commands.spawn((Mesh3d(k.mesh.clone()), MeshMaterial3d(k.material.clone()), Transform::default())).id();
        commands.entity(root).add_child(e);
        let bird = Bird { at, heading, state: State::Wander, timer, radius: 0.0, centre: Vec3::ZERO, leader: None, attractor: None, entity: Some(root) };
        let list = &mut birds.lists[t];
        let place = match list.iter().position(|b| b.state == State::Gone) {
            Some(g) => {
                list[g] = bird;
                g
            }
            None => {
                list.push(bird);
                list.len() - 1
            }
        };
        made.push((t, place, at));
        let a = birds.range(-1.0, 1.0);
        let c = birds.range(-1.0, 1.0);
        at += Vec3::new(a, 0.0, c).normalize_or_zero() * FLOCK_GAP;
    }
    made
}

/// F028456: one frame of flight for type `t`.
#[allow(clippy::too_many_arguments)]
fn fly(commands: &mut Commands, birds: &mut Birds, k: &Kind, t: usize, dt: f32, eye: Vec3, origin: Vec3, frustum: &Frustum, biome_of: &dyn Fn(i32, i32) -> Option<String>, lod: Option<&LodTerrain>, normal: &mut Vec3) {
    for b in 0..birds.lists[t].len() {
        if birds.lists[t][b].state == State::Gone {
            continue;
        }
        let step = dt * k.speed;
        let heading = birds.lists[t][b].heading;
        birds.lists[t][b].at += heading * step;
        let at = birds.lists[t][b].at;
        let z = kenshi_world::zone_of(at.x, at.z);
        let in_biome = biome_of(z.0, z.1).is_some_and(|g| k.per_zone.contains_key(&g));
        if !in_biome {
            let me = &mut birds.lists[t][b];
            me.state = State::TurnBack;
            me.timer = 1e6;
            me.centre = at - heading * step;
            birds.release(t, b);
        }
        // F050509: the terrain's approximate height and normal
        let mut ground = match lod.and_then(|l| l.approximate_height(at.x, at.z)) {
            Some((h, n)) => {
                *normal = n;
                h
            }
            None => NO_GROUND,
        };
        let n = *normal;
        let mut desired = heading;
        match birds.lists[t][b].state {
            State::TurnBack => {
                let me = &mut birds.lists[t][b];
                let mut to = Vec3::new(me.centre.x, at.y, me.centre.z);
                to += Vec3::new(me.centre.z - at.z, 0.0, at.x - me.centre.x).normalize_or_zero() * TURN_ASIDE;
                desired = to - at;
                if in_biome {
                    me.timer = 0.0;
                }
            }
            State::Wander | State::WanderAfter => {
                if (n.y as f64) < 0.999 {
                    let mut contour = Vec3::new(n.z, 0.0, -n.x);
                    if contour.dot(heading) < 0.0 {
                        contour = -contour;
                    }
                    let sign = if k.ground_height > ground { 1.0 } else { -1.0 };
                    desired = contour * k.contour + Vec3::new(n.x, 0.0, n.z).normalize_or_zero() * sign;
                }
                let l = birds.leader(t, b, LEADER_NEAR, k.flocking);
                let me = &mut birds.lists[t][b];
                me.leader = l;
                if l.is_some() {
                    me.state = State::Follow;
                }
            }
            State::Follow => {
                if let Some(l) = birds.leader(t, b, LEADER_NEAR, k.flocking) {
                    birds.lists[t][b].leader = Some(l);
                }
                if let Some(l) = birds.lists[t][b].leader {
                    // (the leader as it is, gone or its place taken)
                    let (lat, lh) = (birds.lists[t][l].at, birds.lists[t][l].heading);
                    let mut side = Vec3::new(lh.z, 0.0, -lh.x);
                    if side.dot(at - lat) < 0.0 {
                        side = -side;
                    }
                    let slot = lat + side * FOLLOW_GAP - lh * FOLLOW_GAP;
                    desired = slot + lh * FOLLOW_GAP - at;
                    let lead = heading.dot(slot - at).min(100.0).max(-100.0);
                    birds.lists[t][b].at += heading * ((lead as f64 * 0.003 + 1.0) as f32) * dt;
                }
            }
            State::Circle | State::Attractor => {
                let attractor = birds.lists[t][b].attractor.and_then(|s| birds.attractors.get(&s)).map(|a| a.at);
                let me = &mut birds.lists[t][b];
                if me.state == State::Circle {
                    if ground > me.centre.y {
                        me.centre = me.at;
                        me.centre.y = ground;
                    }
                    let sign = if k.height > me.centre.y { 1.0 } else { -1.0 };
                    me.centre += Vec3::new(n.x, 0.0, n.z).normalize_or_zero() * dt * sign;
                } else if let Some(a) = attractor {
                    me.centre = a;
                }
                let mut to = Vec3::new(me.centre.x, me.at.y, me.centre.z);
                let d2 = (to.x - me.at.x).powi(2) + (to.z - me.at.z).powi(2);
                let reach = ((me.radius as f64 * 1.4) as f32).powi(2);
                let way = if reach > d2 { (me.at + heading * 100.0 - to).normalize_or_zero() } else { Vec3::new(heading.z, 0.0, heading.x) };
                to += way * me.radius;
                desired = to - me.at;
                if me.centre.y - CIRCLE_FLOOR > ground {
                    ground = me.centre.y - CIRCLE_FLOOR;
                }
            }
            State::Gone => {}
        }
        desired = desired.normalize_or_zero();
        // the height
        let leader_y = birds.lists[t][b].leader.map(|l| birds.lists[t][l].at.y);
        let me = &mut birds.lists[t][b];
        let v = b as f64 * 4.32167;
        let vary = (v - v.floor() - 0.5) as f32;
        let over = me.at.y - ground;
        if over < 0.0 {
            me.at.y = ground;
        }
        if over < CLIMB_UNDER {
            desired.y = CLIMB_Y;
        } else if me.state != State::Follow {
            let aim = match leader_y {
                Some(y) => y - ground + vary * CLIMB_Y,
                None => ((vary as f64 * 0.3 + 1.0) * k.height as f64) as f32,
            };
            desired.y = (aim - over) / CLIMB_OVER;
        }
        desired = desired.normalize_or_zero();
        // the turn: a radian a second toward it, straight to it within 0.99
        let dot = heading.dot(desired);
        me.heading = if dot as f64 > 0.99 {
            desired
        } else {
            let angle = if -1.0 < dot { dot.acos() } else { std::f64::consts::PI as f32 };
            (heading + (desired - heading) * (dt / angle)).normalize_or_zero()
        };
        // F028445: wandering or circling a point, an attractor
        if matches!(me.state, State::Wander | State::Circle)
            && let Some(s) = birds.attractor(t, birds.lists[t][b].at)
        {
            if birds.lists[t][b].attractor != Some(s) {
                birds.release(t, b);
                birds.lists[t][b].attractor = Some(s);
                if let Some(a) = birds.attractors.get_mut(&s) {
                    a.count += 1;
                }
            }
            let timer = birds.range(30.0, 90.0);
            let radius = birds.range(400.0, 600.0);
            let me = &mut birds.lists[t][b];
            me.timer = timer;
            me.state = State::Attractor;
            me.radius = radius;
        }
        // the timer
        birds.lists[t][b].timer -= dt;
        if birds.lists[t][b].timer < 0.0 {
            match birds.lists[t][b].state {
                State::TurnBack | State::Circle | State::WanderAfter => {
                    birds.lists[t][b].state = State::Wander;
                    birds.lists[t][b].timer = birds.range(10.0, 40.0);
                    let l = birds.leader(t, b, LEADER_FAR, k.flocking);
                    birds.lists[t][b].leader = l;
                    if l.is_some() {
                        birds.lists[t][b].state = State::Follow;
                    }
                }
                State::Wander | State::Follow if k.circling => {
                    let timer = birds.range(30.0, 180.0);
                    let radius = birds.range(400.0, 600.0);
                    let me = &mut birds.lists[t][b];
                    me.state = State::Circle;
                    me.centre = me.at;
                    me.timer = timer;
                    me.centre.y = ground;
                    me.leader = None;
                    me.radius = radius;
                }
                State::Wander | State::Follow => {
                    // (one following that finds none stays following)
                    birds.lists[t][b].timer = birds.range(5.0, 30.0);
                    let l = birds.leader(t, b, LEADER_FAR, k.flocking);
                    birds.lists[t][b].leader = l;
                    if l.is_some() {
                        birds.lists[t][b].state = State::Follow;
                    }
                }
                State::Attractor => {
                    birds.lists[t][b].state = State::WanderAfter;
                    birds.lists[t][b].timer = birds.range(10.0, 40.0);
                    birds.release(t, b);
                }
                State::Gone => {}
            }
        }
        // gone: farther than "distance" and over 10 outside a plane of the view
        let at = birds.lists[t][b].at;
        if eye.distance(at) > k.distance && frustum.half_spaces.iter().any(|h| h.normal_d().dot((at - origin).extend(1.0)) < -10.0) {
            birds.lists[t][b].state = State::Gone;
            birds.release(t, b);
            if let Some(e) = birds.lists[t][b].entity.take() {
                commands.entity(e).despawn();
            }
        }
    }
}

fn debug(birds: &Birds, kinds: &[Kind], time: &Time, eye: Vec3, here: (i32, i32)) {
    if std::env::var("KENSHI_BIRD_DEBUG").is_err() || time.elapsed_secs().fract() >= time.delta_secs() {
        return;
    }
    if (time.elapsed_secs() as u32).is_multiple_of(5) {
        for (t, k) in kinds.iter().enumerate() {
            let alive: Vec<&Bird> = birds.lists[t].iter().filter(|b| b.state != State::Gone).collect();
            let near = alive.iter().filter(|b| b.at.distance(eye) < 10000.0).count();
            let mut states = [0; 7];
            for b in &alive {
                states[b.state as usize] += 1;
            }
            eprintln!("birds {}: wanted {} alive {} near {near} states {states:?} zones {} (camera zone {here:?}, e.g. {:?})", k.id, k.wanted, alive.len(), k.zones.len(), k.zones.first());
        }
        eprintln!("bird attractors {}", birds.attractors.len());
    }
    // the test flock
    if !birds.held.is_empty() {
        let s: Vec<String> = birds
            .held
            .iter()
            .map(|(t, b, _)| {
                let me = &birds.lists[*t][*b];
                format!("{b}:{:?}{} {:.0}", me.state, me.leader.map_or(String::new(), |l| format!("->{l}")), me.at)
            })
            .collect();
        eprintln!("test flock at {:.0} s: {}", time.elapsed_secs(), s.join(", "));
    }
}
