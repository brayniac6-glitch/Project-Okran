//! Birds (Kenshi's BirdManager): flocks of each WILDLIFE_BIRDS type flying over the biomes that
//! list it.
//!
//! - F028452 loads a type: "mesh"/"texture", "speed", "height", "ground height", "contour
//!   strength", "spawn height min/max", "spawn count", "distance", "flocking", "circling",
//!   "scale min/max"; each BIOME_GROUP's "birds" reference gives the type's number per zone.
//! - F028446, how many there should be: the zones within "distance" of the camera (a square)
//!   whose biome lists the type, their numbers summed.
//! - F028455 spawn, while the birds alive plus "spawn count" are no more than that: a random
//!   point in a random such zone, on the ground there, at least 1000 units from the camera and
//!   between the spawn heights; "spawn count" birds 5 units apart, facing the camera (F028454:
//!   wandering, for 30-180 s).
//! - F028456 update, each bird: on along its heading at "speed"; then by its state
//!   1 turning back (it flew out of its biomes),
//!   2 wandering (6: after an attractor) and looking for a leader within 100,
//!   3 following its leader (100 behind it),
//!   4 circling a point, 5 circling an attractor;
//!   it climbs at once under 30 units above the ground, else heads for "height" above it
//!   (varied +-15% bird by bird; over its circle's centre, at most 500 under the centre), the
//!   gap over 2000; it turns toward where it wants to go at a radian a second. Wandering or
//!   circling a point it takes the best attractor not full (F028445). Its timer up: wandering
//!   or following it circles where it is (a "circling" type; 30-180 s, radius 400-600) else
//!   looks for a leader within 1000 (10-40 s); turning back, circling a point or done with an
//!   attractor it wanders (5-30 s); at an attractor it leaves it (10-40 s). Farther than
//!   "distance" from the camera and out of view it is gone.
//! - F028457 / F028453: "bird attractor" references (radius m, most birds) on buildings and
//!   animals draw a type's birds within ten times the radius (units), the nearest by radius
//!   squared over distance squared.
//!
//! (assumption) "contour strength" turns a wandering bird down the slope when the ground is
//! above "ground height", up it below (the vector F028456 scales is not decoded). Following, it
//! heads for the point 100 behind its leader along the leader's heading. Map features'
//! attractors are those of the zones asked for. The birds are drawn with HWBirds
//! (deferred/birds.material): Object_FP (objects.hlsl with no defines: glossMult 1, no dust or
//! colouring, back faces culled) with the type's "texture", flat.dds and black.dds; its
//! instancing vertex shader only places the static mesh (no wing beat).
use super::Game;
use bevy::camera::primitives::Frustum;
use bevy::prelude::*;
use kenshi_io::ogre_mesh;
use kenshi_world::{WORLD_SIZE, ZONE_SIZE};
use std::collections::HashMap;

/// F028455: the least distance from the camera a flock appears at (units).
const SPAWN_CLEAR: f32 = 1000.0;
/// F028455: the gap between birds of a new flock.
const FLOCK_GAP: f32 = 5.0;
/// F028456: under this height over the ground a bird climbs straight away.
const CLIMB_UNDER: f32 = 30.0;
/// F028456: the height gap the climb is taken over.
const CLIMB_OVER: f32 = 2000.0;
/// F028456: a leader is looked for this near while wandering, this near when a timer runs out.
const LEADER_NEAR: f32 = 100.0;
const LEADER_FAR: f32 = 1000.0;
/// F028452: how far behind its leader a bird keeps.
const FOLLOW_GAP: f32 = 100.0;
/// F028456: a circle's floor under its centre.
const CIRCLE_FLOOR: f32 = 500.0;

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

/// What draws birds: a placed building or a character.
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
    /// F028453: (radius x 10)^2.
    weight: f32,
    max: i32,
    count: i32,
}

struct Kind {
    id: String,
    meshes: Vec<Handle<Mesh>>,
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
    /// The camera zone the count was taken at, the zones listing the type in reach, and the
    /// number wanted.
    counted: Option<(i32, i32)>,
    zones: Vec<(i32, i32)>,
    wanted: f32,
}

struct Bird {
    ty: usize,
    at: Vec3,
    heading: Vec3,
    state: State,
    timer: f32,
    radius: f32,
    centre: Vec3,
    leader: Option<usize>,
    attractor: Option<Source>,
    /// Its place in its type's list when made: its height varies by it.
    slot: u32,
    entity: Entity,
}

#[derive(Resource, Default)]
pub struct Birds {
    kinds: Option<Vec<Kind>>,
    birds: Vec<Bird>,
    attractors: HashMap<Source, Attractor>,
    attractors_for: usize,
    terrain: Option<kenshi_world::Terrain>,
    seed: u64,
    test_flock: bool,
    held: Vec<(usize, Vec3)>,
}

impl Birds {
    fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.rand()
    }

    fn ground(&mut self, x: f32, z: f32) -> f32 {
        self.terrain.as_mut().and_then(|t| t.height_at(x, z).ok()).unwrap_or(0.0)
    }

    fn release(&mut self, b: usize) {
        if let Some(a) = self.birds[b].attractor.take().and_then(|s| self.attractors.get_mut(&s)) {
            a.count -= 1;
        }
        if self.birds[b].state == State::Attractor {
            self.birds[b].state = State::Wander;
        }
    }

    /// F028443: the nearest bird of `b`'s type within `near` that `b` is behind, not circling,
    /// whose leaders don't lead back to `b`.
    fn leader(&self, b: usize, near: f32, kinds: &[Kind]) -> Option<usize> {
        let me = &self.birds[b];
        if !kinds[me.ty].flocking {
            return None;
        }
        let mut best = None;
        let mut d2 = near * near;
        for (i, o) in self.birds.iter().enumerate() {
            if i == b || o.ty != me.ty || matches!(o.state, State::Gone | State::Circle | State::Attractor) {
                continue;
            }
            let d = o.at.distance_squared(me.at);
            if d < d2 && o.heading.dot(o.at - me.at) > 0.0 {
                best = Some(i);
                d2 = d;
            }
        }
        let mut at = best;
        let mut steps = 0;
        while let Some(i) = at {
            if i == b || steps > self.birds.len() {
                return None;
            }
            at = self.birds[i].leader;
            steps += 1;
        }
        best
    }

    /// F028445: the attractor of `b`'s type not full that draws it most.
    fn attractor(&self, b: usize) -> Option<Source> {
        let me = &self.birds[b];
        let mut best = None;
        let mut score = 1.0;
        for (s, a) in &self.attractors {
            if a.ty != me.ty || a.count >= a.max {
                continue;
            }
            let v = a.weight / a.at.distance_squared(me.at).max(1.0);
            if v > score {
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
    let mut ids = Vec::new();
    for r in data.of_type(kenshi_data::ItemType::WildlifeBirds) {
        let file = |k: &str| r.files.get(k).filter(|s| !s.is_empty()).cloned();
        let Some(mesh) = file("mesh").and_then(|p| game.res.path(&p)).and_then(|p| ogre_mesh::read(&p).ok()) else { continue };
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
        ids.push(r.string_id.clone());
        out.push(Kind {
            id: r.string_id.clone(),
            // (the mesh's vertex colours are not the HWBirds shader's: they would tint and cut it)
            meshes: kenshi_render::to_bevy(&mesh)
                .into_iter()
                .map(|(_, mut m)| {
                    m.remove_attribute(Mesh::ATTRIBUTE_COLOR);
                    meshes.add(m)
                })
                .collect(),
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
            scale: (f("scale min").max(0.01), f("scale max").max(0.01)),
            per_zone: HashMap::new(),
            counted: None,
            zones: Vec::new(),
            wanted: 0.0,
        });
    }
    for g in data.of_type(kenshi_data::ItemType::BiomeGroup) {
        for (s, v) in g.refs("birds") {
            if let Some(k) = ids.iter().position(|i| i == s) {
                out[k].per_zone.insert(g.string_id.clone(), v[0] as f32);
            }
        }
    }
    out
}

/// F028457 / F028453: the attractors of placed buildings (once) and of the characters (when
/// their number changes), keeping the counts of those already drawing birds.
fn attractors(birds: &mut Birds, game: &Game, ids: &[String]) {
    let data = &game.sim.data;
    let refs = |rec: &str| -> Vec<(usize, f32, i32)> {
        let Some(r) = data.get(rec) else { return Vec::new() };
        r.refs("bird attractor")
            .iter()
            .filter_map(|(s, v)| Some((ids.iter().position(|i| i == s)?, (v[0] as f32 * 10.0).powi(2), v[1])))
            .collect()
    };
    // (made again when the number of people or of zones asked for changes)
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
    for (i, p) in game.sim.placed.iter().enumerate() {
        for (ty, weight, max) in refs(&p.building) {
            fresh.insert(Source::Placed(i, ty), Attractor { ty, at: Vec3::from(game.sim.placed_at[i]), weight, max, count: 0 });
        }
    }
    // the map features of the zones asked for (kenshi_world::map_features)
    let features = kenshi_render::zone::map_features(&game.root);
    for &z in &game.asked {
        for (k, f) in features.get(&z).into_iter().flatten().enumerate() {
            for (ty, weight, max) in refs(&f.record) {
                fresh.insert(Source::Feature(z, k, ty), Attractor { ty, at: Vec3::from(f.position), weight, max, count: 0 });
            }
        }
    }
    for p in game.sim.folk.iter().filter(|p| !p.dead) {
        let Some(c) = &p.character else { continue };
        for (ty, weight, max) in refs(c) {
            fresh.insert(Source::Person(p.id, ty), Attractor { ty, at: Vec3::from(p.mover.position_units()), weight, max, count: 0 });
        }
    }
    for (s, a) in fresh.iter_mut() {
        if let Some(old) = birds.attractors.get(s) {
            a.count = old.count;
        }
    }
    birds.attractors = fresh;
    // a bird whose attractor went goes back to wandering
    for b in 0..birds.birds.len() {
        if birds.birds[b].attractor.is_some_and(|s| !birds.attractors.contains_key(&s)) {
            birds.birds[b].attractor = None;
            if birds.birds[b].state == State::Attractor {
                birds.birds[b].state = State::WanderAfter;
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
    cams: Query<(&GlobalTransform, &Frustum), super::MainCam>,
    mut shown: Query<&mut Transform>,
    looks: Query<(&GlobalTransform, &InheritedVisibility, Option<&Children>)>,
    seen: Query<(&ViewVisibility, &GlobalTransform)>,
) {
    if std::env::var("KENSHI_NO_BIRDS").is_ok() {
        return;
    }
    let Ok((cam, frustum)) = cams.single() else { return };
    let Some(map) = weather.map.as_ref() else { return };
    if birds.kinds.is_none() {
        birds.seed = 0x9e37_79b9_7f4a_7c15;
        birds.terrain = kenshi_world::Terrain::open(&game.root).ok();
        birds.kinds = Some(kinds(&game, &assets, &mut meshes, &mut materials, &mut look));
    }
    let mut kinds = birds.kinds.take().unwrap();
    let ids: Vec<String> = kinds.iter().map(|k| k.id.clone()).collect();
    attractors(&mut birds, &game, &ids);
    let origin = game.zone.origin;
    let eye = cam.translation() + origin;
    let dt = time.delta_secs().min(0.1) * if game.paused { 0.0 } else { game.speed };
    let biome_of = |x: i32, z: i32| kenshi_world::biome::biome_of_zone(map, &weather.biomes, x, z).map(|i| weather.biomes[i].record.clone());

    // F028446 / F028455: count and spawn
    let here = kenshi_world::zone_of(eye.x, eye.z);
    for (t, k) in kinds.iter_mut().enumerate() {
        if k.counted != Some(here) {
            k.counted = Some(here);
            k.zones.clear();
            k.wanted = 0.0;
            for x in cell(eye.x - k.distance)..=cell(eye.x + k.distance) {
                for z in cell(eye.z - k.distance)..=cell(eye.z + k.distance) {
                    let Some(v) = biome_of(x, z).and_then(|b| k.per_zone.get(&b).copied()) else { continue };
                    k.zones.push((x, z));
                    k.wanted += v.max(0.0);
                }
            }
        }
        let alive = birds.birds.iter().filter(|b| b.ty == t && b.state != State::Gone).count() as f32;
        if k.zones.is_empty() || dt <= 0.0 || alive + k.spawn_count as f32 > k.wanted {
            continue;
        }
        let (zx, zz) = k.zones[(birds.rand() * k.zones.len() as f32) as usize % k.zones.len()];
        let lo = Vec2::new(zx as f32 * ZONE_SIZE - WORLD_SIZE / 2.0, zz as f32 * ZONE_SIZE - WORLD_SIZE / 2.0);
        let x = birds.range(lo.x, lo.x + ZONE_SIZE);
        let z = birds.range(lo.y, lo.y + ZONE_SIZE);
        let y = birds.ground(x, z);
        let at = Vec3::new(x, y, z);
        if (at.xz() - eye.xz()).length_squared() < SPAWN_CLEAR * SPAWN_CLEAR || y <= k.spawn_min || y >= k.spawn_max {
            continue;
        }
        spawn_flock(&mut commands, &mut birds, k, t, at, eye, origin);
    }
    // (testing) KENSHI_BIRD_HERE=<type number>: a flock of that type 250 units ahead of the
    // camera, crossing the view, once
    if let Some(t) = std::env::var("KENSHI_BIRD_HERE").ok().and_then(|v| v.parse::<usize>().ok()).filter(|t| *t < kinds.len()) {
        let when = std::env::var("KENSHI_BIRD_AT").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(5.0);
        if !birds.test_flock && time.elapsed_secs() > when {
            let fwd = cam.forward().as_vec3();
            let at = eye + fwd * 250.0;
            let side = cam.right().as_vec3();
            let before = birds.birds.iter().map(|b| b.entity).collect::<Vec<_>>();
            spawn_flock(&mut commands, &mut birds, &kinds[t], t, at, at + side * 2000.0, origin);
            birds.held = birds.birds.iter().enumerate().filter(|(_, b)| !before.contains(&b.entity)).map(|(i, b)| (i, b.at)).collect();
            eprintln!("birds: at {:.1} s a flock of {} at {at:.0} (eye {eye:.0}, origin {origin:.0}, {} meshes)", time.elapsed_secs(), kinds[t].id, kinds[t].meshes.len());
            for h in &kinds[t].meshes {
                if let Some(m) = meshes.get(h) {
                    eprintln!("  mesh {} vertices, aabb {:?}, attrs {:?}", m.count_vertices(), bevy::camera::primitives::MeshAabb::compute_aabb(m), m.attributes().map(|(a, _)| a.name).collect::<Vec<_>>());
                }
            }
            birds.test_flock = true;
        }
    }
    // (testing) KENSHI_BIRD_HOLD: the test flock stays where it was made (only turning)
    let held: Vec<(usize, Vec3)> = if std::env::var("KENSHI_BIRD_HOLD").is_ok() { birds.held.clone() } else { Vec::new() };
    let clock = std::time::Instant::now();
    fly(&mut commands, &mut birds, &kinds, dt, eye, origin, frustum, &biome_of);
    if std::env::var("KENSHI_BIRD_DEBUG").is_ok() && time.elapsed_secs().fract() < time.delta_secs() {
        eprintln!("birds: flight {:.2} ms for {} (frame {:.1} ms)", clock.elapsed().as_secs_f64() * 1000.0, birds.birds.iter().filter(|b| b.state != State::Gone).count(), time.delta_secs() * 1000.0);
    }
    for (b, at) in held {
        birds.birds[b].at = at;
    }
    // draw: heading along the mesh's z, level wings
    for me in birds.birds.iter().filter(|b| b.state != State::Gone) {
        let Ok(mut t) = shown.get_mut(me.entity) else { continue };
        let fwd = me.heading;
        let right = Vec3::Y.cross(fwd).normalize_or(Vec3::X);
        let up = fwd.cross(right);
        t.translation = me.at - origin;
        t.rotation = Quat::from_mat3(&Mat3::from_cols(right, up, fwd));
    }
    debug(&birds, &kinds, &time, eye, here);
    if std::env::var("KENSHI_BIRD_DEBUG").is_ok() && time.elapsed_secs().fract() < time.delta_secs() {
        if let Some((b, _)) = birds.held.first() {
            let e = birds.birds[*b].entity;
            if let Ok((g, v, ch)) = looks.get(e) {
                eprintln!("held bird at {:.0} scene {:.0} visible {:?} cam {:.0} fwd {:.2}", birds.birds[*b].at, g.translation(), v.get(), cam.translation(), cam.forward());
                for c in ch.into_iter().flatten() {
                    if let Ok((vv, cg)) = seen.get(*c) {
                        eprintln!("  child in view {} at {:.0}", vv.get(), cg.translation());
                    }
                }
            }
        }
    }
    birds.kinds = Some(kinds);
}

/// F028455 / F028454: "spawn count" birds of type `t` from `at`, 5 units apart, facing `toward`.
#[allow(clippy::too_many_arguments)]
fn spawn_flock(commands: &mut Commands, birds: &mut Birds, k: &Kind, t: usize, mut at: Vec3, toward: Vec3, origin: Vec3) {
    let first = birds.birds.iter().filter(|b| b.ty == t).count() as u32;
    {
        for i in 0..k.spawn_count {
            let heading = (toward - at).normalize_or(Vec3::Z);
            let scale = birds.range(k.scale.0, k.scale.1);
            let root = commands.spawn((Transform::from_translation(at - origin).with_scale(Vec3::splat(scale)), Visibility::default())).id();
            for h in &k.meshes {
                let e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(k.material.clone()), Transform::default())).id();
                commands.entity(root).add_child(e);
            }
            let timer = birds.range(30.0, 180.0);
            let slot = first + i as u32;
            // reuse a gone bird's place
            let bird = Bird { ty: t, at, heading, state: State::Wander, timer, radius: 0.0, centre: at, leader: None, attractor: None, slot, entity: root };
            match birds.birds.iter().position(|b| b.state == State::Gone) {
                Some(g) => birds.birds[g] = bird,
                None => birds.birds.push(bird),
            }
            let a = birds.range(-1.0, 1.0);
            let c = birds.range(-1.0, 1.0);
            at += Vec3::new(a, 0.0, c).normalize_or(Vec3::X) * FLOCK_GAP;
        }
    }
}

/// F028456: one frame of flight.
#[allow(clippy::too_many_arguments)]
fn fly(commands: &mut Commands, birds: &mut Birds, kinds: &[Kind], dt: f32, eye: Vec3, origin: Vec3, frustum: &Frustum, biome_of: &dyn Fn(i32, i32) -> Option<String>) {
    {
        for b in 0..birds.birds.len() {
            if dt <= 0.0 || birds.birds[b].state == State::Gone {
                continue;
            }
            let k = &kinds[birds.birds[b].ty];
            let (speed, height) = (k.speed, k.height);
            {
                let me = &mut birds.birds[b];
                me.at += me.heading * speed * dt;
            }
            let at = birds.birds[b].at;
            let z = kenshi_world::zone_of(at.x, at.z);
            let in_biome = biome_of(z.0, z.1).is_some_and(|g| k.per_zone.contains_key(&g));
            if !in_biome {
                birds.release(b);
                let me = &mut birds.birds[b];
                me.state = State::TurnBack;
                me.timer = 1e6;
                me.centre = me.at - me.heading * speed * dt;
                me.leader = None;
            } else if birds.birds[b].state == State::TurnBack {
                birds.birds[b].timer = 0.0;
            }
            let mut ground = birds.ground(at.x, at.z);
            let mut desired = birds.birds[b].heading;
            match birds.birds[b].state {
                State::TurnBack => {
                    let me = &birds.birds[b];
                    desired = Vec3::new(me.centre.x - at.x, 0.0, me.centre.z - at.z);
                }
                State::Wander | State::WanderAfter => {
                    if k.contour > 0.0 {
                        let up = Vec3::new(birds.ground(at.x + 50.0, at.z) - ground, 0.0, birds.ground(at.x, at.z + 50.0) - ground);
                        let sign = if k.ground_height <= ground { -1.0 } else { 1.0 };
                        desired += up.normalize_or_zero() * sign * k.contour;
                    }
                    if let Some(l) = birds.leader(b, LEADER_NEAR, &kinds) {
                        birds.birds[b].leader = Some(l);
                        birds.birds[b].state = State::Follow;
                    }
                }
                State::Follow => {
                    if let Some(l) = birds.leader(b, LEADER_NEAR, &kinds) {
                        birds.birds[b].leader = Some(l);
                    }
                    match birds.birds[b].leader.filter(|l| birds.birds[*l].state != State::Gone) {
                        Some(l) => {
                            let lead = &birds.birds[l];
                            let to = lead.at - lead.heading * FOLLOW_GAP - at;
                            desired = (to / FOLLOW_GAP).clamp_length_max(1.0) + lead.heading;
                        }
                        None => {
                            birds.birds[b].leader = None;
                            birds.birds[b].state = State::Wander;
                        }
                    }
                }
                State::Circle | State::Attractor => {
                    if birds.birds[b].state == State::Attractor {
                        if let Some(a) = birds.birds[b].attractor.and_then(|s| birds.attractors.get(&s)) {
                            birds.birds[b].centre = a.at;
                        }
                    } else if ground > birds.birds[b].centre.y {
                        birds.birds[b].centre.y = ground;
                    }
                    let me = &birds.birds[b];
                    let to = Vec3::new(me.centre.x - at.x, 0.0, me.centre.z - at.z);
                    let d = to.length();
                    desired = if d > me.radius * 1.4 {
                        to
                    } else {
                        // round the centre, pulled toward the circle
                        let round = Vec3::new(-to.z, 0.0, to.x).normalize_or_zero();
                        round + to.normalize_or_zero() * ((d - me.radius) / me.radius.max(1.0))
                    };
                    ground = ground.max(me.centre.y - CIRCLE_FLOOR);
                }
                State::Gone => {}
            }
            // the height
            let me = &mut birds.birds[b];
            if me.at.y < ground {
                me.at.y = ground;
            }
            let over = me.at.y - ground;
            let vary = ((me.slot as f64 * 4.32167).fract() - 0.5) as f32;
            desired.y = if over < CLIMB_UNDER {
                5.0
            } else if me.state == State::Follow {
                desired.y
            } else {
                ((vary * 0.3 + 1.0) * height - over) / CLIMB_OVER
            };
            // turn toward it, a radian a second
            let want = desired.normalize_or(me.heading);
            let dot = me.heading.dot(want);
            if dot > 0.99 {
                me.heading = want;
            } else {
                let angle = if dot > -1.0 { dot.acos() } else { std::f32::consts::PI };
                let t = (dt / angle).min(1.0);
                me.heading = (me.heading + (want - me.heading) * t).normalize_or(me.heading);
            }
            // attractors
            if matches!(me.state, State::Wander | State::Circle) {
                if let Some(s) = birds.attractor(b) {
                    if birds.birds[b].attractor != Some(s) {
                        birds.release(b);
                        if let Some(a) = birds.attractors.get_mut(&s) {
                            a.count += 1;
                        }
                        birds.birds[b].attractor = Some(s);
                    }
                    let timer = birds.range(30.0, 90.0);
                    let radius = birds.range(400.0, 600.0);
                    let me = &mut birds.birds[b];
                    me.state = State::Attractor;
                    me.timer = timer;
                    me.radius = radius;
                    me.leader = None;
                }
            }
            // the timer
            birds.birds[b].timer -= dt;
            if birds.birds[b].timer < 0.0 {
                match birds.birds[b].state {
                    State::TurnBack | State::Circle | State::WanderAfter => {
                        birds.birds[b].state = State::Wander;
                        birds.birds[b].timer = birds.range(5.0, 30.0);
                        if let Some(l) = birds.leader(b, LEADER_FAR, &kinds) {
                            birds.birds[b].leader = Some(l);
                            birds.birds[b].state = State::Follow;
                        }
                    }
                    State::Wander | State::Follow if !kinds[birds.birds[b].ty].circling => {
                        birds.birds[b].state = State::Wander;
                        birds.birds[b].timer = birds.range(10.0, 40.0);
                        if let Some(l) = birds.leader(b, LEADER_FAR, &kinds) {
                            birds.birds[b].leader = Some(l);
                            birds.birds[b].state = State::Follow;
                        }
                    }
                    State::Wander | State::Follow => {
                        let timer = birds.range(30.0, 180.0);
                        let radius = birds.range(400.0, 600.0);
                        let me = &mut birds.birds[b];
                        me.state = State::Circle;
                        me.centre = Vec3::new(me.at.x, ground, me.at.z);
                        me.timer = timer;
                        me.radius = radius;
                        me.leader = None;
                    }
                    State::Attractor => {
                        birds.release(b);
                        birds.birds[b].state = State::WanderAfter;
                        birds.birds[b].timer = birds.range(10.0, 40.0);
                    }
                    State::Gone => {}
                }
            }
            // gone: far and out of view
            let me = &birds.birds[b];
            if me.at.distance(eye) > k.distance && !frustum.intersects_sphere(&bevy::camera::primitives::Sphere { center: (me.at - origin).into(), radius: 10.0 }, true) {
                birds.release(b);
                birds.birds[b].state = State::Gone;
                commands.entity(birds.birds[b].entity).despawn();
            }
        }
    }
}

fn debug(birds: &Birds, kinds: &[Kind], time: &Time, eye: Vec3, here: (i32, i32)) {
    if std::env::var("KENSHI_BIRD_DEBUG").is_ok() && (time.elapsed_secs() as u32) % 5 == 0 && time.elapsed_secs().fract() < time.delta_secs() {
        for (t, k) in kinds.iter().enumerate() {
            let alive: Vec<&Bird> = birds.birds.iter().filter(|b| b.ty == t && b.state != State::Gone).collect();
            let near = alive.iter().filter(|b| b.at.distance(eye) < 10000.0).count();
            let mut states = [0; 7];
            for b in &alive {
                states[b.state as usize] += 1;
            }
            eprintln!("birds {}: wanted {} alive {} near {near} states {states:?} zones {} (camera zone {here:?}, e.g. {:?})", k.id, k.wanted, alive.len(), k.zones.len(), k.zones.first());
        }
        eprintln!("bird attractors {}", birds.attractors.len());
    }
}
