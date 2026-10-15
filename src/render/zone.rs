//! One world zone as a Bevy scene (port_map `zones`, `terrain`): the terrain around it, coloured
//! from the painted tiles, and every building its zone file places, assembled from BUILDING_PART
//! meshes. Positions are relative to `ZoneScene::origin` (the buildings' centre on the ground).

use std::sync::Arc;
use crate::object_material::{KenshiObjectMaterial, ObjectLook};
use crate::terrain_material::{KenshiTerrainMaterial, TerrainLook};
use crate::{terrain_pieces_where, to_bevy};
use bevy::camera::primitives::MeshAabb;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use kenshi_data::{GameData, GameDataContainer};
use kenshi_io::{ogre_mesh, resources::Resources};
use kenshi_world::{Patch, Terrain, WORLD_SIZE, ZONE_SIZE, colour_tile, placements};
use std::collections::HashMap;
use std::path::Path;

/// Zone folders under data/newland/leveldata, later wins (assumption: port_map `zones`).
pub use kenshi_world::ZONE_FOLDERS;

/// A building piece's world bounds (scene units), for hiding roofs over the view.
#[derive(Component, Debug, Clone, Copy)]
pub struct BuildingPiece {
    pub lo: Vec3,
    pub hi: Vec3,
}

/// The zone an entity was spawned for (unloaded with it).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ZoneTag(pub (i32, i32));

/// A building part's building (zone, placement index) and its "building floor" (BUILDING_PART,
/// F037849: kept on the part, at most 3); furniture takes the floor its height stands on.
#[derive(Component, Clone, Copy, Debug)]
pub struct BuildingFloor {
    pub zone: (i32, i32),
    pub building: usize,
    pub floor: i32,
    /// One of the building's "interior" parts.
    pub interior: bool,
}

/// A placed building with an interior: its "interior mask" volume (world units, F037793), the
/// world-to-building transform (world units) and where each floor's parts start (floor, lowest
/// local height).
#[derive(Component, Clone, Debug)]
pub struct BuildingView {
    pub zone: (i32, i32),
    pub building: usize,
    /// Its BUILDING record (string id).
    pub record: String,
    pub interior: kenshi_world::interior::Interior,
    pub to_local: Mat4,
    pub levels: Vec<(i32, f32)>,
}

impl BuildingView {
    /// The floor a building-frame height stands on: the highest floor whose parts start at or
    /// below it (5 units of slack). (assumption: Kenshi's floor comes from the physics floor
    /// group under the character, F037849's collision groups 9 - 12)
    pub fn floor_of(&self, local_y: f32) -> i32 {
        floor_of(&self.levels, local_y)
    }
}

fn floor_of(levels: &[(i32, f32)], local_y: f32) -> i32 {
    levels
        .iter()
        .filter(|(_, y)| *y <= local_y + 5.0)
        .map(|(f, _)| *f)
        .max()
        .unwrap_or(0)
}

/// An uploaded mesh, its local bounds and whether it has vertex colours.
type UploadedMesh = (Handle<Mesh>, Option<(Vec3, Vec3)>, bool);

pub struct Piece {
    pub mesh: String,
    pub look: ObjectLook,
    pub transform: Transform,
    /// Foliage: drawn while its page is within this range (no roof hiding, no keep-out disc).
    pub foliage_range: Option<FoliageRange>,
    /// The building it belongs to (placement index) and its floor (None: by its height,
    /// `local_y` in the building's frame).
    pub building: Option<usize>,
    pub floor: Option<i32>,
    pub local_y: f32,
    /// One of the building's "interior" parts (they mark where its floors start).
    pub interior: bool,
    /// A turret's swivelling part.
    pub swivel: Option<Swivel>,
    /// One of the building's doors (its place in the "doors" list), moved as it opens.
    pub door: Option<usize>,
}

pub struct ZoneScene {
    /// World position of the scene origin (on the ground).
    pub origin: Vec3,
    /// The building parts' collision ("xml collision"), each where its part is placed.
    pub collision: Vec<(Transform, Arc<kenshi_physics::world::Solids>)>,
    /// Which of them are doors (left out of the navmesh cutting), and the building (placement
    /// index) each is of.
    pub collision_door: Vec<bool>,
    pub collision_building: Vec<Option<usize>>,
    pub patch: Patch,
    /// Terrain pieces: (colour tile, blend cell, mesh).
    pub terrain: Vec<crate::TerrainPiece>,
    pub pieces: Vec<Piece>,
    pub meshes: HashMap<String, Vec<Mesh>>,
    /// The EFFECT records placed on its buildings and their parts (F037795): effect, scene
    /// position and orientation.
    pub effects: Vec<(String, Vec3, Quat)>,
    /// The LIGHT records placed on its buildings, their parts and their furniture
    /// (`placed_lights`): light, scene position and orientation.
    pub lights: Vec<(String, Vec3, Quat)>,
    /// Grass pages: (mesh, sprite, range, centre in scene coordinates).
    pub grass: Vec<(Mesh, String, FoliageRange, Vec3)>,
    /// Keep-out discs (centre, radius) of placed parts, in scene coordinates.
    pub occupied: Vec<(Vec3, f32)>,
    /// The zone and its buildings with interiors.
    pub zone: (i32, i32),
    pub buildings: Vec<BuildingView>,
}

/// The meshes of a building with the material each is drawn with: its `parts` (BUILDING_PART
/// `phs or mesh`, `.mesh` only), recursively. A part uses its own `material`, else its parent's,
/// else the building's.
pub fn part_meshes<'a>(
    data: &'a GameDataContainer,
    b: &'a GameData,
) -> Vec<(&'a str, Option<&'a GameData>)> {
    part_meshes_of(data, b, false)
}

/// @fn F037849
///
/// The mesh a BUILDING_PART draws: its "phs or mesh"; in a destroyed building its "destroyed
/// mesh" when it has one, else on the ground floor ("building floor" 0) its own, and above it
/// nothing (the part made without its entity: the ruin's roof and upper walls); stairs ("is
/// stairs") with no destroyed mesh are not made at all. (assumption: such a part's collision,
/// which the game keeps, is left out with it)
pub fn part_mesh(part: &GameData, destroyed: bool) -> Option<&String> {
    let mesh = |k: &str| part.files.get(k).filter(|s| s.to_lowercase().ends_with(".mesh"));
    if !destroyed {
        return mesh("phs or mesh");
    }
    if let Some(m) = mesh("destroyed mesh") {
        return Some(m);
    }
    let floor = part.ints.get("building floor").copied().unwrap_or(0);
    let stairs = part.bools.get("is stairs").copied().unwrap_or(false);
    if floor != 0 || stairs {
        return None;
    }
    mesh("phs or mesh")
}

/// `part_meshes` of a building in ruins or not ([`part_mesh`]: F037846, F037849).
pub fn part_meshes_of<'a>(
    data: &'a GameDataContainer,
    b: &'a GameData,
    destroyed: bool,
) -> Vec<(&'a str, Option<&'a GameData>)> {
    fn material<'a>(data: &'a GameDataContainer, r: &'a GameData) -> Option<&'a GameData> {
        r.refs("material")
            .first()
            .and_then(|(sid, _)| data.get(sid))
    }
    fn walk<'a>(
        data: &'a GameDataContainer,
        node: &'a GameData,
        inherited: Option<&'a GameData>,
        depth: u32,
        destroyed: bool,
        out: &mut Vec<(&'a str, Option<&'a GameData>)>,
    ) {
        if depth > 8 {
            return; // guard against reference cycles
        }
        for part in node
            .refs("parts")
            .iter()
            .filter_map(|(sid, _)| data.get(sid))
        {
            let mat = material(data, part).or(inherited);
            if let Some(m) = part_mesh(part, destroyed) {
                out.push((m.as_str(), mat));
            }
            walk(data, part, mat, depth + 1, destroyed, out);
        }
    }
    let mut out = Vec::new();
    walk(data, b, material(data, b), 0, destroyed, &mut out);
    out
}

/// A building part's mesh where it sits: its material, its place in the building (each part's
/// "offset X/Y/Z" from its parent part, Ogre child nodes), and the chain of nodes from the
/// building down to it with each node's swivel axis (a part of "rotation function" 5 turns:
/// "rotation axis" 1 about y, a turret's yaw; 0 about x, its pitch).
#[derive(Debug, Clone)]
pub struct PlacedPart<'a> {
    pub mesh: &'a str,
    /// Its BUILDING_PART record.
    pub record: &'a GameData,
    pub material: Option<&'a GameData>,
    pub local: Transform,
    pub chain: Vec<(Vec3, Option<i32>)>,
    /// The nodes of its chain that spin or turn by themselves (`SpinDef`).
    pub spins: Vec<SpinDef>,
}

/// @fn F037803
///
/// A building part that moves by itself ("rotation function" 1 to 4; 5 is a turret's swivel),
/// as F037803 reads it: its node in the chain, its "rotation axis" (0 x, 1 y, 2 z), "rotation
/// speed min / max" (degrees a second, kept in radians) and "wind speed rotation min / max".
/// Function 1 spins at its top speed, made 0.95 to 1.05 of itself (a random factor, here by
/// the part's place), as do 2 and 4; for 3 the top speed is at least the bottom one and the top
/// wind at least the bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct SpinDef {
    pub node: usize,
    pub function: i32,
    pub axis: Vec3,
    pub min: f32,
    pub max: f32,
    pub wind_min: f32,
    pub wind_max: f32,
}

impl SpinDef {
    fn of(part: &GameData, node: usize) -> Option<SpinDef> {
        let function = part.ints.get("rotation function").copied().unwrap_or(0);
        if !(1..=4).contains(&function) {
            return None;
        }
        let f = |k: &str| part.floats.get(k).copied().or_else(|| part.ints.get(k).map(|&v| v as f32)).unwrap_or(0.0);
        let axis = match part.ints.get("rotation axis").copied().unwrap_or(1) {
            0 => Vec3::X,
            1 => Vec3::Y,
            2 => Vec3::Z,
            _ => Vec3::ZERO,
        };
        let (mut min, mut max) = (f("rotation speed min").to_radians(), f("rotation speed max").to_radians());
        let (wind_min, mut wind_max) = (f("wind speed rotation min"), f("wind speed rotation max"));
        if function == 3 {
            max = max.max(min);
            wind_max = wind_max.max(wind_min);
        }
        min = min.min(max.max(min));
        Some(SpinDef { node, function, axis, min, max, wind_min, wind_max })
    }

    /// F037682 / F029... (the wind listener, 0x140549c60): the part's spin speed (radians a
    /// second) for wind speed `wind` and machine work rate `rate` (0 .. 1): function 1 its top
    /// speed (times `jitter`, 0.95 .. 1.05); 2 its speed range times the work rate; 3 none under
    /// its bottom wind, its top speed from its top wind, in between in proportion; 4 (a vane)
    /// its turning speed.
    pub fn speed(&self, wind: f32, rate: f32, jitter: f32) -> f32 {
        match self.function {
            1 => self.max * jitter,
            2 => (self.max - self.min) * jitter * rate,
            3 => {
                if wind < self.wind_min {
                    0.0
                } else if wind >= self.wind_max {
                    self.max
                } else {
                    self.min + (self.max - self.min) * (wind - self.wind_min) / (self.wind_max - self.wind_min).max(1e-3)
                }
            }
            _ => self.max * jitter,
        }
    }
}

impl PlacedPart<'_> {
    /// Whether a node above it (or it) turns: a turret's swivel, or a part that spins.
    pub fn swivels(&self) -> bool {
        self.chain.iter().any(|(_, a)| a.is_some()) || !self.spins.is_empty()
    }

    /// Its place with the swivelling nodes turned: `yaw` about y, `pitch` about x (radians).
    pub fn posed(&self, yaw: f32, pitch: f32) -> Transform {
        posed_chain(&self.chain, yaw, pitch)
    }
}

/// A part's node chain's place with its swivelling nodes turned (`PlacedPart::posed`).
pub fn posed_chain(chain: &[(Vec3, Option<i32>)], yaw: f32, pitch: f32) -> Transform {
    let mut t = Transform::IDENTITY;
    for (offset, axis) in chain {
        t = t * Transform::from_translation(*offset);
        match axis {
            Some(1) => t = t * Transform::from_rotation(Quat::from_rotation_y(yaw)),
            Some(0) => t = t * Transform::from_rotation(Quat::from_rotation_x(pitch)),
            _ => {}
        }
    }
    t
}

/// `part_meshes_of` with each part placed ([`PlacedPart`]).
pub fn part_meshes_placed<'a>(data: &'a GameDataContainer, b: &'a GameData, destroyed: bool) -> Vec<PlacedPart<'a>> {
    fn walk<'a>(
        data: &'a GameDataContainer,
        node: &'a GameData,
        inherited: Option<&'a GameData>,
        chain: &[(Vec3, Option<i32>)],
        spins: &[SpinDef],
        depth: u32,
        destroyed: bool,
        out: &mut Vec<PlacedPart<'a>>,
    ) {
        if depth > 8 {
            return;
        }
        for part in node.refs("parts").iter().filter_map(|(sid, _)| data.get(sid)) {
            let mat = part.refs("material").first().and_then(|(sid, _)| data.get(sid)).or(inherited);
            let f = |k: &str| part.floats.get(k).copied().unwrap_or(0.0);
            let swivel = (part.ints.get("rotation function") == Some(&5)).then(|| part.ints.get("rotation axis").copied().unwrap_or(1));
            let mut here = chain.to_vec();
            here.push((Vec3::new(f("offset X"), f("offset Y"), f("offset Z")), swivel));
            let mut spinning = spins.to_vec();
            spinning.extend(SpinDef::of(part, here.len() - 1));
            if let Some(m) = part_mesh(part, destroyed) {
                let local = here.iter().fold(Transform::IDENTITY, |t, (o, _)| t * Transform::from_translation(*o));
                out.push(PlacedPart { mesh: m.as_str(), record: part, material: mat, local, chain: here.clone(), spins: spinning.clone() });
            }
            walk(data, part, mat, &here, &spinning, depth + 1, destroyed, out);
        }
    }
    let mut out = Vec::new();
    let mat = b.refs("material").first().and_then(|(sid, _)| data.get(sid));
    walk(data, b, mat, &[], &[], 0, destroyed, &mut out);
    out
}

/// A door's mesh: its zone, building (placement), door (its place in the building's "doors")
/// and its place shut; moved by the game as the door opens.
#[derive(Component, Clone, Copy, Debug)]
pub struct DoorPiece {
    pub zone: (i32, i32),
    pub building: usize,
    pub index: usize,
    pub rest: Transform,
}

/// A building part that moves: a turret's head (swivelling nodes, posed by [`TurretAims`]) or
/// a part that spins by itself (`spins`: fans, machines, wind generators); its building's place
/// (world units) and its node chain, posed each frame (`pose_swivels`).
#[derive(Component, Clone, Debug)]
pub struct Swivel {
    pub building: Transform,
    /// The building's world position (units), the key into [`TurretAims`] and [`PartMotion`].
    pub at: Vec3,
    pub chain: Vec<(Vec3, Option<i32>)>,
    pub spins: Vec<SpinDef>,
    /// Each spinning node's angle (radians; F037803 starts it at random: by the place here).
    pub angles: Vec<f32>,
}

impl Swivel {
    pub fn new(building: Transform, at: Vec3, chain: Vec<(Vec3, Option<i32>)>, spins: Vec<SpinDef>) -> Swivel {
        let angles = spins.iter().enumerate().map(|(k, _)| jitter_of(at, k) * std::f32::consts::TAU).collect();
        Swivel { building, at, chain, spins, angles }
    }
}

/// A stable 0 .. 1 for a part at `at` (its spin's random start and speed factor).
fn jitter_of(at: Vec3, k: usize) -> f32 {
    let h = (at.x * 12.9898 + at.z * 78.233 + k as f32 * 37.719).sin() * 43758.547;
    h - h.floor()
}

/// What moves the spinning parts: the wind where the camera is (the weather's speed and the
/// way it blows on the ground plane), each machine's work rate by its building's place
/// ([`TurretAims::key`]), and the game's frame time.
#[derive(Resource, Default)]
pub struct PartMotion {
    pub wind_speed: f32,
    pub wind_dir: Vec2,
    pub rates: HashMap<(i32, i32, i32), f32>,
    pub dt: f32,
}

/// Turret heads' aim by their building's world position (rounded units): yaw and pitch
/// (radians) relative to the building.
#[derive(Resource, Default)]
pub struct TurretAims(pub HashMap<(i32, i32, i32), (f32, f32)>);

impl TurretAims {
    /// The key: the ground-plane position in whole units (the height left out: a placement's
    /// height may come from its state record).
    pub fn key(at: Vec3) -> (i32, i32, i32) {
        (at.x.round() as i32, 0, at.z.round() as i32)
    }
}

/// Swivelling parts posed by their turret's aim.
pub fn pose_swivels(aims: Res<TurretAims>, motion: Option<Res<PartMotion>>, mut q: Query<(&mut Swivel, &mut Transform)>, mut once: Local<u32>) {
    *once += 1;
    if *once == 900 && std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
        let mut keys: Vec<_> = q.iter().map(|(s, _)| TurretAims::key(s.at)).collect();
        keys.sort();
        keys.dedup();
        let hit = keys.iter().filter(|k| aims.0.contains_key(k)).count();
        eprintln!("swivels at {keys:?}; {hit} of {} aimed ({} aims)", keys.len(), aims.0.len());
    }
    let still = PartMotion::default();
    let m = motion.as_deref().unwrap_or(&still);
    // (testing) KENSHI_SPIN_DEBUG: the wind and the first spinning parts every 60 frames
    if *once % 60 == 0 && std::env::var("KENSHI_SPIN_DEBUG").is_ok() {
        let spinning: Vec<String> = q.iter().filter(|(s, _)| !s.spins.is_empty()).take(4).map(|(s, _)| format!("{:?} f{:?} {:?}", s.at.round(), s.spins.iter().map(|d| d.function).collect::<Vec<_>>(), s.angles)).collect();
        eprintln!("spin: wind {:.1} {:?}, {} rates; {spinning:?}", m.wind_speed, m.wind_dir, m.rates.len());
    }
    for (mut s, mut t) in &mut q {
        let (yaw, pitch) = aims.0.get(&TurretAims::key(s.at)).copied().unwrap_or((0.0, 0.0));
        if s.spins.is_empty() {
            *t = s.building * posed_chain(&s.chain, yaw, pitch);
            continue;
        }
        // the spinning nodes (F037682): each turned by its speed, a vane toward the wind
        let rate = m.rates.get(&TurretAims::key(s.at)).copied().unwrap_or(0.0);
        let at = s.at;
        let building_rot = s.building.rotation;
        for k in 0..s.spins.len() {
            let d = s.spins[k].clone();
            let jitter = 0.95 + 0.1 * jitter_of(at, k + 7);
            if d.function == 4 {
                // the head turns its local x across the wind ((-z, 0, x) of the way it blows in
                // its frame) at its turning speed; no wind, it stays
                if m.wind_speed > 0.0 && m.wind_dir != Vec2::ZERO {
                    let w = building_rot.inverse() * Vec3::new(m.wind_dir.x, 0.0, m.wind_dir.y);
                    let target = (-w.x).atan2(-w.z);
                    let mut diff = (target - s.angles[k]).rem_euclid(std::f32::consts::TAU);
                    if diff > std::f32::consts::PI {
                        diff -= std::f32::consts::TAU;
                    }
                    let step = d.speed(m.wind_speed, rate, jitter) * m.dt;
                    s.angles[k] += diff.clamp(-step, step);
                }
            } else {
                s.angles[k] = (s.angles[k] + d.speed(m.wind_speed, rate, jitter) * m.dt).rem_euclid(std::f32::consts::TAU);
            }
        }
        let mut pose = Transform::IDENTITY;
        for (n, (offset, axis)) in s.chain.iter().enumerate() {
            pose = pose * Transform::from_translation(*offset);
            match axis {
                Some(1) => pose = pose * Transform::from_rotation(Quat::from_rotation_y(yaw)),
                Some(0) => pose = pose * Transform::from_rotation(Quat::from_rotation_x(pitch)),
                _ => {}
            }
            for (k, d) in s.spins.iter().enumerate() {
                if d.node == n && d.axis != Vec3::ZERO {
                    pose = pose * Transform::from_rotation(Quat::from_axis_angle(d.axis, s.angles[k]));
                }
            }
        }
        *t = s.building * pose;
    }
}

/// A record's own mesh (a BUILDING_PART's "phs or mesh") and its parts' meshes.
pub fn record_meshes<'a>(data: &'a GameDataContainer, r: &'a GameData) -> Vec<(&'a str, Option<&'a GameData>)> {
    record_meshes_of(data, r, false)
}

/// `record_meshes` in a building in ruins or not ([`part_mesh`]).
pub fn record_meshes_of<'a>(data: &'a GameDataContainer, r: &'a GameData, destroyed: bool) -> Vec<(&'a str, Option<&'a GameData>)> {
    let mat = r.refs("material").first().and_then(|(sid, _)| data.get(sid));
    let mut out: Vec<(&str, Option<&GameData>)> = part_mesh(r, destroyed).map(|m| (m.as_str(), mat)).into_iter().collect();
    for (m, pm) in part_meshes_of(data, r, destroyed) {
        out.push((m, pm.or(mat)));
    }
    out
}

/// The "building floor" of each mesh a building's parts, interior parts and doors draw (a part
/// without one takes its parent's).
pub fn mesh_floors<'a>(data: &'a GameDataContainer, b: &'a GameData) -> HashMap<&'a str, i32> {
    fn walk<'a>(data: &'a GameDataContainer, r: &'a GameData, inherited: i32, depth: u32, out: &mut HashMap<&'a str, i32>) {
        if depth > 8 {
            return;
        }
        let f = r.ints.get("building floor").copied().unwrap_or(inherited).min(3);
        if let Some(m) = r.files.get("phs or mesh").filter(|s| s.to_lowercase().ends_with(".mesh")) {
            out.entry(m.as_str()).or_insert(f);
        }
        for (sid, _) in r.refs("parts").iter().chain(r.refs("interior")).chain(r.refs("doors")) {
            if let Some(c) = data.get(sid) {
                walk(data, c, f, depth + 1, out);
            }
        }
    }
    let mut out = HashMap::new();
    walk(data, b, 0, 0, &mut out);
    out
}

/// What a building draws besides its shell, in its own frame: its "interior" parts (floors,
/// inner walls, stairs), its "doors", the furniture its record places as instances, and the
/// furniture of its interior and exterior layouts (interiors.level, chosen by the placement's
/// state record, `kenshi_world::nodes::Layouts`). Nodes (invisible) are left out.
/// A mesh of a building's interior parts, doors and furniture: mesh, material, place in the
/// building, whether an interior part, and for a turret's swivelling part its instance's place
/// and node chain.
pub type InteriorMesh<'a> = (&'a str, Option<&'a GameData>, Transform, bool, Option<(Transform, Vec<(Vec3, Option<i32>)>, Vec<SpinDef>)>, Option<usize>);

pub fn interior_meshes<'a>(
    data: &'a GameDataContainer,
    b: &'a GameData,
    layouts: &'a kenshi_world::nodes::Layouts,
    p: &kenshi_world::Placement,
) -> Vec<InteriorMesh<'a>> {
    let mut out = Vec::new();
    // (a ruin's interior parts by F037849's rule, `part_mesh`; it has no doors, F037851)
    for (sid, _) in b.refs("interior") {
        if let Some(r) = data.get(sid) {
            for (m, mat) in record_meshes_of(data, r, p.destroyed) {
                out.push((m, mat, Transform::IDENTITY, true, None, None));
            }
        }
    }
    for (n, (sid, _)) in b.refs("doors").iter().enumerate().filter(|_| !p.destroyed) {
        if let Some(r) = data.get(sid) {
            for (m, mat) in record_meshes(data, r) {
                out.push((m, mat, Transform::IDENTITY, false, None, Some(n)));
            }
        }
    }
    let place = |i: &'a kenshi_io::fcs::Instance, out: &mut Vec<InteriorMesh<'a>>| {
        let Some(t) = data.get(&i.target) else { return };
        if t.bools.get("is node").copied().unwrap_or(false) {
            return;
        }
        let [w, x, y, z] = i.rotation;
        let local = Transform::from_translation(Vec3::from(i.position))
            .with_rotation(Quat::from_xyzw(x, y, z, w).normalize());
        for part in part_meshes_placed(data, t, false) {
            let chain = part.swivels().then(|| (part.chain.clone(), part.spins.clone()));
            out.push((part.mesh, part.material, local * part.local, false, chain.map(|(c, sp)| (local, c, sp)), None));
        }
        for (sid, _) in t.refs("interior") {
            if let Some(r) = data.get(sid) {
                for (m, mat) in record_meshes(data, r) {
                    out.push((m, mat, local, false, None, None));
                }
            }
        }
    };
    for i in &b.instances {
        place(i, &mut out);
    }
    for name in p.interior.iter().chain(&p.exterior) {
        for i in layouts.for_building(data, &p.building, name) {
            place(i, &mut out);
        }
    }
    out
}

/// The furnishing layouts (read once).
pub fn layouts(root: &Path) -> &'static kenshi_world::nodes::Layouts {
    static L: std::sync::OnceLock<kenshi_world::nodes::Layouts> = std::sync::OnceLock::new();
    L.get_or_init(|| kenshi_world::nodes::Layouts::load(root))
}

/// @fn F037795
///
/// The EFFECT instances placed on a building and on its parts (recursively): effect record,
/// local position and orientation. (The building's scale is taken as 1.)
pub fn placed_effects(data: &GameDataContainer, b: &GameData) -> Vec<(String, Vec3, Quat)> {
    fn walk(data: &GameDataContainer, node: &GameData, depth: u32, out: &mut Vec<(String, Vec3, Quat)>) {
        if depth > 8 {
            return;
        }
        for i in &node.instances {
            if data.get(&i.target).and_then(|t| t.item_type()) == Some(kenshi_data::ItemType::Effect) {
                let [w, x, y, z] = i.rotation;
                out.push((i.target.clone(), Vec3::from(i.position), Quat::from_xyzw(x, y, z, w).normalize()));
            }
        }
        for part in node.refs("parts").iter().filter_map(|(sid, _)| data.get(sid)) {
            walk(data, part, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(data, b, 0, &mut out);
    out
}

/// The LIGHT instances a building places (F037794 / F045912 make an Ogre light of each, on the
/// scene node of what holds it): those on it and on its parts (each part at its offsets), and
/// those of the furniture it places (its own instances and its interior and exterior layouts'):
/// light record, position and orientation in the building's frame.
pub fn placed_lights(
    data: &GameDataContainer,
    b: &GameData,
    layouts: Option<(&kenshi_world::nodes::Layouts, &kenshi_world::Placement)>,
) -> Vec<(String, Vec3, Quat)> {
    lights_of(data, b, layouts, true)
}

/// `placed_lights`, with or without the furniture's.
fn lights_of(
    data: &GameDataContainer,
    b: &GameData,
    layouts: Option<(&kenshi_world::nodes::Layouts, &kenshi_world::Placement)>,
    furniture: bool,
) -> Vec<(String, Vec3, Quat)> {
    fn walk(data: &GameDataContainer, node: &GameData, at: Transform, depth: u32, furniture: bool, out: &mut Vec<(String, Vec3, Quat)>) {
        if depth > 8 {
            return;
        }
        for i in &node.instances {
            let Some(t) = data.get(&i.target) else { continue };
            let [w, x, y, z] = i.rotation;
            let local = at * Transform::from_translation(Vec3::from(i.position)).with_rotation(Quat::from_xyzw(x, y, z, w).normalize());
            match t.item_type() {
                Some(kenshi_data::ItemType::Light) => out.push((i.target.clone(), local.translation, local.rotation)),
                // furniture it places, with its own lights
                Some(kenshi_data::ItemType::Building) if depth == 0 && furniture => walk(data, t, local, depth + 1, furniture, out),
                _ => {}
            }
        }
        for part in node.refs("parts").iter().filter_map(|(sid, _)| data.get(sid)) {
            let f = |k: &str| part.floats.get(k).copied().unwrap_or(0.0);
            walk(data, part, at * Transform::from_translation(Vec3::new(f("offset X"), f("offset Y"), f("offset Z"))), depth + 1, furniture, out);
        }
    }
    let mut out = Vec::new();
    walk(data, b, Transform::IDENTITY, 0, furniture, &mut out);
    if let Some((layouts, p)) = layouts {
        for name in p.interior.iter().chain(&p.exterior) {
            for i in layouts.for_building(data, &p.building, name) {
                let Some(t) = data.get(&i.target) else { continue };
                let [w, x, y, z] = i.rotation;
                let local = Transform::from_translation(Vec3::from(i.position)).with_rotation(Quat::from_xyzw(x, y, z, w).normalize());
                walk(data, t, local, 1, furniture, &mut out);
            }
        }
    }
    out
}

/// The LIGHT instances of a building itself and its parts (not of the furniture it places), as
/// `placed_lights` gives them: what its setLightsOn switches (F029364).
pub fn own_lights(data: &GameDataContainer, b: &GameData) -> Vec<(String, Vec3, Quat)> {
    lights_of(data, b, None, false)
}

/// Load zone (X, Y) with `margin` units of terrain around it.
/// The map's features by zone (read once).
pub fn map_features(root: &Path) -> &'static HashMap<(i32, i32), Vec<kenshi_world::map_features::MapFeature>> {
    static FEATURES: std::sync::OnceLock<HashMap<(i32, i32), Vec<kenshi_world::map_features::MapFeature>>> = std::sync::OnceLock::new();
    FEATURES.get_or_init(|| kenshi_world::map_features::load(root))
}

pub fn load_zone(
    root: &Path,
    data: &GameDataContainer,
    res: &Resources,
    terrain: &mut Terrain,
    zone: (i32, i32),
    margin: f32,
) -> Result<ZoneScene, String> {
    // a zone without a zone file: its terrain only
    let placed = kenshi_world::read_zone(root, zone.0, zone.1).map(|z| placements(&z)).unwrap_or_default();
    load_zone_at(root, data, res, terrain, zone, margin, None, placed)
}

/// Load zone (X, Y) placed relative to `origin` (else its own: its buildings' centre on the
/// ground); a zone without a zone file has its terrain only.
pub fn load_zone_shared(
    root: &Path,
    data: &GameDataContainer,
    res: &Resources,
    terrain: &mut Terrain,
    zone: (i32, i32),
    margin: f32,
    origin: Vec3,
) -> Result<ZoneScene, String> {
    let placed = kenshi_world::read_zone(root, zone.0, zone.1).map(|z| placements(&z)).unwrap_or_default();
    load_zone_at(root, data, res, terrain, zone, margin, Some(origin), placed)
}

#[allow(clippy::too_many_arguments)]
fn load_zone_at(
    root: &Path,
    data: &GameDataContainer,
    res: &Resources,
    terrain: &mut Terrain,
    zone: (i32, i32),
    margin: f32,
    shared: Option<Vec3>,
    placed: Vec<kenshi_world::Placement>,
) -> Result<ZoneScene, String> {
    // centre on the buildings, or the zone centre when empty
    let (cx, cz) = if placed.is_empty() {
        let c = |i: i32| i as f32 * ZONE_SIZE - WORLD_SIZE / 2.0 + ZONE_SIZE / 2.0;
        (c(zone.0), c(zone.1))
    } else {
        let k = placed.len() as f32;
        (
            placed.iter().map(|p| p.position[0]).sum::<f32>() / k,
            placed.iter().map(|p| p.position[2]).sum::<f32>() / k,
        )
    };
    let origin = match shared {
        Some(o) => o,
        None => Vec3::new(
            cx,
            terrain.height_at(cx, cz).map_err(|e| e.to_string())?,
            cz,
        ),
    };
    // the terrain round the zone's own square when placed on a shared origin
    let (cx, cz) = if shared.is_some() {
        let c = |i: i32| i as f32 * ZONE_SIZE - WORLD_SIZE / 2.0 + ZONE_SIZE / 2.0;
        (c(zone.0), c(zone.1))
    } else {
        (cx, cz)
    };

    let half = ZONE_SIZE / 2.0 + margin;
    let (c0, r0) = terrain.sample_of(cx - half, cz - half);
    let n = ((2.0 * half) / terrain.spacing()) as u32 + 1;
    let patch = terrain
        .patch(c0.max(0.0) as u32, r0.max(0.0) as u32, n, n, 1)
        .map_err(|e| e.to_string())?;

    // the interiors (world units), before the placements are consumed
    let interiors = kenshi_world::interior::interiors(data, res, &placed);
    let mut to_local: HashMap<usize, Mat4> = HashMap::new();
    let mut records: HashMap<usize, String> = HashMap::new();
    let mut pieces = Vec::new();
    let mut effects = Vec::new();
    let mut lights = Vec::new();
    let placed_names: Vec<String> = placed.iter().map(|p| data.get(&p.building).map_or(p.building.clone(), |r| r.name.clone())).collect();
    for (bi, p) in placed.into_iter().enumerate() {
        let Some(b) = data.get(&p.building) else {
            continue;
        };
        records.insert(bi, p.building.clone());
        // (testing) KENSHI_HIDE_BUILDINGS=a|b: buildings whose name holds any of those not drawn
        if let Ok(h) = std::env::var("KENSHI_HIDE_BUILDINGS")
            && h.split('|').any(|w| !w.is_empty() && b.name.contains(w))
        {
            continue;
        }
        let [x, y, z] = p.position;
        let [w, qx, qy, qz] = p.rotation;
        let transform =
            Transform::from_translation(Vec3::new(x, p.world_y.unwrap_or(y), z) - origin)
                .with_rotation(Quat::from_xyzw(qx, qy, qz, w).normalize());
        to_local.insert(bi, (Transform::from_translation(origin) * transform).to_matrix().inverse());
        let floors = mesh_floors(data, b);
        if std::env::var("KENSHI_BUILDING_DEBUG").is_ok() {
            let parts: Vec<String> = b
                .refs("parts")
                .iter()
                .filter_map(|(sid, _)| data.get(sid))
                .map(|r| format!("{} f{:?} roof {:?}", r.name, r.ints.get("building floor"), r.bools.get("is unwalkable roof")))
                .collect();
            eprintln!("building {bi}: {} ({}) parts {parts:?}", b.name, b.string_id);
        }
        // F029401 -> UseableStuff::setOn (F029284): one switched off has its lights and effects
        // off (slots 140 / 141; the effects otherwise run while it is complete and in view,
        // F037702 / F037859); its furniture's lights are their own
        let off = p.power_on == Some(false);
        if !off {
            for (effect, at, rot) in placed_effects(data, b) {
                effects.push((effect, transform.transform_point(at), transform.rotation * rot));
            }
        }
        let mut own = if off { own_lights(data, b) } else { Vec::new() };
        for (light, at, rot) in placed_lights(data, b, Some((layouts(root), &p))) {
            if let Some(k) = own.iter().position(|o| o.0 == light && o.1 == at) {
                own.swap_remove(k);
                continue;
            }
            lights.push((light, transform.transform_point(at), transform.rotation * rot));
        }
        // each part where its offsets put it; a turret's swivelling parts also keep their chain
        for part in part_meshes_placed(data, b, p.destroyed) {
            if let Some(mesh) = res.find(part.mesh) {
                let swivel = part.swivels().then(|| Swivel::new(transform, Vec3::new(x, p.world_y.unwrap_or(y), z), part.chain.clone(), part.spins.clone()));
                pieces.push(Piece {
                    mesh,
                    look: ObjectLook::from_record(part.material, |f| res.find(f)).for_building(),
                    transform: transform * part.local,
                    foliage_range: None,
                    building: Some(bi),
                    floor: Some(floors.get(part.mesh).copied().unwrap_or(0)),
                    local_y: 0.0,
                    interior: false,
                    swivel,
                    door: None,
                });
            }
        }
        for (m, mat, local, interior, swivel, door) in interior_meshes(data, b, layouts(root), &p) {
            if let Some(mesh) = res.find(m) {
                let swivel = swivel.map(|(inst, chain, spins)| {
                    let base = transform * inst;
                    Swivel::new(base, base.translation + origin, chain, spins)
                });
                // interior parts and doors have their floor; furniture goes by its height
                let floor = (local == Transform::IDENTITY).then(|| floors.get(m).copied().unwrap_or(0));
                pieces.push(Piece {
                    mesh,
                    look: ObjectLook::from_record(mat, |f| res.find(f)).for_building(),
                    transform: transform * local,
                    foliage_range: None,
                    building: Some(bi),
                    floor,
                    local_y: local.translation.y,
                    interior,
                    swivel,
                    door,
                });
            }
        }
    }
    // the zone's map features (kenshi_world::map_features; F032048 builds each: its mesh, the
    // object material getObjectMaterial gives its "texture mode", F046403, so mode 2 is drawn
    // with the terrain's material; its "collision"); a "hidden" one is not drawn
    // (assumption: "local coordinates" and the "distance" it is drawn to are not read)
    let mut feature_collision: HashMap<String, String> = HashMap::new();
    if std::env::var("KENSHI_NO_MAP_FEATURES").is_err() {
        for f in map_features(root).get(&zone).into_iter().flatten() {
            let Some(r) = data.get(&f.record) else { continue };
            if r.bools.get("hidden").copied().unwrap_or(false) {
                continue;
            }
            let Some(mesh) = r.files.get("mesh").filter(|m| !m.is_empty()).and_then(|m| res.find(m)) else { continue };
            if let Some(c) = r.files.get("collision").filter(|c| !c.is_empty()).and_then(|c| res.find(c)) {
                feature_collision.insert(mesh.clone(), c);
            }
            let mut look = ObjectLook::from_record(Some(r), |f| res.find(f));
            look.kind = r.ints.get("texture mode").copied().unwrap_or(0);
            let [w, qx, qy, qz] = f.rotation;
            let transform = Transform::from_translation(Vec3::from(f.position) - origin)
                .with_rotation(Quat::from_xyzw(qx, qy, qz, w).normalize())
                .with_scale(Vec3::from(f.scale));
            pieces.push(Piece {
                mesh,
                look,
                transform,
                foliage_range: None,
                building: None,
                floor: None,
                local_y: 0.0,
                interior: false,
                swivel: None,
                door: None,
            });
        }
    }
    // a mesh reached twice for the same place (a part both among a building's parts and its
    // interior's, a layout named as both its interior and exterior one) is drawn once: two
    // copies of a surface in one place fight for the depth and flicker
    {
        let mut seen = std::collections::HashSet::new();
        pieces.retain(|p| {
            let t = p.transform.translation;
            let q = p.transform.rotation;
            seen.insert((
                p.mesh.clone(),
                [(t.x * 10.0).round() as i32, (t.y * 10.0).round() as i32, (t.z * 10.0).round() as i32],
                [(q.x * 1000.0).round() as i32, (q.y * 1000.0).round() as i32, (q.z * 1000.0).round() as i32, (q.w * 1000.0).round() as i32],
            ))
        });
    }
    // (testing) KENSHI_DUP_DEBUG: pieces drawn twice in the same place (they flicker)
    if std::env::var("KENSHI_DUP_DEBUG").is_ok() {
        let mut seen: HashMap<(String, [i32; 3], [i32; 4]), usize> = HashMap::new();
        let mut owners: HashMap<(String, [i32; 3], [i32; 4]), Vec<(Option<usize>, bool)>> = HashMap::new();
        for p in &pieces {
            let t = p.transform.translation;
            let q = p.transform.rotation;
            let k = (p.mesh.clone(), [(t.x * 10.0) as i32, (t.y * 10.0) as i32, (t.z * 10.0) as i32], [(q.x * 1000.0) as i32, (q.y * 1000.0) as i32, (q.z * 1000.0) as i32, (q.w * 1000.0) as i32]);
            owners.entry(k.clone()).or_default().push((p.building, p.interior));
            *seen.entry(k).or_default() += 1;
        }
        for (k, o) in owners.iter().filter(|(_, o)| o.len() > 1).take(8) {
            let names: Vec<String> = o.iter().map(|(b, i)| format!("{}#{:?} {}", b.and_then(|b| placed_names.get(b)).cloned().unwrap_or_default(), b, i)).collect();
            eprintln!("  dup {} from {names:?}", k.0);
        }
        let mut dups: Vec<_> = seen.iter().filter(|(_, n)| **n > 1).map(|(k, n)| (k.0.clone(), *n)).collect();
        dups.sort();
        eprintln!("zone {:?}: {} pieces, {} drawn more than once: {:?}", zone, pieces.len(), dups.len(), dups.iter().take(30).collect::<Vec<_>>());
    }
    // each part mesh's collision: its BUILDING_PART's "xml collision"
    let mut collision_of: HashMap<String, String> = data
        .of_type(kenshi_data::ItemType::BuildingPart)
        .filter_map(|r| {
            let m = r.files.get("phs or mesh").and_then(|m| res.find(m))?;
            let x = r.files.get("xml collision").filter(|x| !x.is_empty()).and_then(|x| res.find(x))?;
            Some((m, x))
        })
        .collect();
    collision_of.extend(feature_collision);
    // the doors' meshes (their collision is placed shut; people open doors, so they do not cut
    // the navmesh)
    let door_meshes: std::collections::HashSet<String> = data
        .of_type(kenshi_data::ItemType::Building)
        .flat_map(|b| b.refs("doors").iter().filter_map(|(sid, _)| data.get(sid)).collect::<Vec<_>>())
        .flat_map(|d| record_meshes(data, d).into_iter().filter_map(|(m, _)| res.find(m)))
        .collect();
    let (collision, marks): (Vec<_>, Vec<(bool, Option<usize>)>) = pieces
        .iter()
        .filter(|p| p.foliage_range.is_none())
        .filter_map(|p| Some(((p.transform, solids(root, collision_of.get(&p.mesh)?)?), (door_meshes.contains(&p.mesh), p.building))))
        .unzip();
    let (collision_door, collision_building): (Vec<bool>, Vec<Option<usize>>) = marks.into_iter().unzip();
    if std::env::var("KENSHI_COLLISION_DEBUG").is_ok() {
        let missing: std::collections::BTreeSet<&str> =
            pieces.iter().filter(|p| !collision_of.contains_key(&p.mesh)).map(|p| p.mesh.as_str()).collect();
        eprintln!("zone {zone:?}: {} pieces, {} with collision; without: {:?}", pieces.len(), collision.len(), missing);
    }
    let mut meshes: HashMap<String, Vec<Mesh>> = HashMap::new();
    for piece in &pieces {
        if !meshes.contains_key(&piece.mesh) {
            let m = ogre_mesh::read(&root.join(&piece.mesh))
                .map(|m| to_bevy(&m).into_iter().map(|(_, m)| m).collect())
                .unwrap_or_default();
            meshes.insert(piece.mesh.clone(), m);
        }
    }
    // each building's floors: where its interior parts of each floor start (building frame; the
    // shell spans every floor from the ground, so it can't tell)
    let mut levels: HashMap<usize, Vec<(i32, f32)>> = HashMap::new();
    // and the top of its interior: furniture at or above it stands on the roof
    let mut tops: HashMap<usize, (f32, i32)> = HashMap::new();
    for piece in pieces.iter().filter(|p| p.interior) {
        let (Some(b), Some(f)) = (piece.building, piece.floor) else { continue };
        let boxes: Vec<_> = meshes.get(&piece.mesh).into_iter().flatten().filter_map(|m| m.compute_aabb()).collect();
        let lo = boxes.iter().map(|a| a.min().y).fold(f32::MAX, f32::min);
        let hi = boxes.iter().map(|a| a.max().y).fold(f32::MIN, f32::max);
        if lo == f32::MAX {
            continue;
        }
        let t = tops.entry(b).or_insert((f32::MIN, 0));
        *t = (t.0.max(hi), t.1.max(f));
        let l = levels.entry(b).or_default();
        match l.iter_mut().find(|(lf, _)| *lf == f) {
            Some(e) => e.1 = e.1.min(lo),
            None => l.push((f, lo)),
        }
    }
    for piece in pieces.iter_mut() {
        if let (Some(b), None) = (piece.building, piece.floor) {
            piece.floor = Some(match tops.get(&b) {
                // (assumption) on the roof: the floor above the interior's highest
                Some(&(top, f)) if piece.local_y >= top - 5.0 => f + 1,
                _ => floor_of(levels.get(&b).map_or(&[][..], |v| v), piece.local_y),
            });
        }
    }
    let buildings = interiors
        .into_iter()
        .map(|i| BuildingView {
            zone,
            building: i.placement,
            record: records.get(&i.placement).cloned().unwrap_or_default(),
            to_local: to_local.get(&i.placement).copied().unwrap_or(Mat4::IDENTITY),
            levels: levels.get(&i.placement).cloned().unwrap_or_default(),
            interior: i,
        })
        .collect();
    let radius_of = |path: &str| {
        meshes.get(path).map_or(0.0, |ms| {
            ms.iter()
                .filter_map(|m| match m.attribute(Mesh::ATTRIBUTE_POSITION) {
                    Some(VertexAttributeValues::Float32x3(p)) => Some(p),
                    _ => None,
                })
                .flatten()
                .map(|v| v[0].hypot(v[2]))
                .fold(0.0f32, f32::max)
        })
    };
    let occupied = pieces
        .iter()
        .filter(|p| p.foliage_range.is_none())
        .map(|p| (p.transform.translation, radius_of(&p.mesh)))
        .collect();
    Ok(ZoneScene {
        origin,
        collision,
        collision_door,
        collision_building,
        terrain: terrain_pieces_where(&patch, Some(origin), 512, 300.0, |_, _| true),
        patch,
        pieces,
        meshes,
        occupied,
        effects,
        lights,
        grass: Vec::new(),
        zone,
        buildings,
    })
}

/// What a BUILDING (a piece of furniture) stands on the ground with, in its own frame: each
/// placed part's collision ("xml collision") as point sets (`Solids::footprints`), else (a part
/// without one) the corners of its mesh's bounds.
pub fn building_footprints(data: &GameDataContainer, res: &Resources, root: &Path, b: &GameData) -> Vec<Vec<Vec3>> {
    let mut out = Vec::new();
    for part in part_meshes_placed(data, b, false) {
        let coll = part.record.files.get("xml collision").filter(|x| !x.is_empty()).and_then(|x| res.find(x)).and_then(|x| solids(root, &x));
        match coll {
            Some(sol) => {
                for (pts, _) in sol.footprints() {
                    out.push(pts.iter().map(|p| part.local.transform_point(*p)).collect());
                }
            }
            None => {
                let Some(path) = res.path(part.mesh) else { continue };
                let Ok(mesh) = ogre_mesh::read(&path) else { continue };
                let Some((mn, mx, _)) = mesh.bounds else { continue };
                out.push(
                    (0..8)
                        .map(|c| {
                            part.local.transform_point(Vec3::new(
                                if c & 1 == 0 { mn[0] } else { mx[0] },
                                if c & 2 == 0 { mn[1] } else { mx[1] },
                                if c & 4 == 0 { mn[2] } else { mx[2] },
                            ))
                        })
                        .collect(),
                );
            }
        }
    }
    out
}

/// A collision file's solids, read once.
fn solids(root: &Path, path: &str) -> Option<Arc<kenshi_physics::world::Solids>> {
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<String, Option<Arc<kenshi_physics::world::Solids>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(s) = cache.lock().ok()?.get(path) {
        return s.clone();
    }
    let s = kenshi_io::nxu::read(&root.join(path))
        .ok()
        .map(|shapes| Arc::new(kenshi_physics::world::Solids::new(&shapes)));
    cache.lock().ok()?.insert(path.to_string(), s.clone());
    s
}

/// A foliage object's PagedGeometry page and detail level: drawn while the camera's ground-plane
/// distance to the page's square is under `range` (F052104: per axis max(|camera - centre| -
/// half the page, 0); one detail level from 0 to its far range). F041804: a foliage layer's pages
/// are half a zone (2304 units) on the zone's bounds, a grass's a quarter (1152).
#[derive(Component, Clone, Copy, Debug)]
pub struct FoliageRange {
    pub range: f32,
    /// The page's centre (scene x, z) and half its side.
    pub centre: Vec2,
    pub half: f32,
}

impl FoliageRange {
    /// The page of side `side` on the zone at `zone_min` (scene x, z) holding `at`.
    pub fn page(range: f32, zone_min: Vec2, side: f32, at: Vec2) -> FoliageRange {
        let cell = ((at - zone_min) / side).floor().clamp(Vec2::ZERO, Vec2::splat(ZONE_SIZE / side - 1.0));
        FoliageRange {
            range,
            centre: zone_min + (cell + 0.5) * side,
            half: side * 0.5,
        }
    }

    /// Is the page in range of a camera at scene (x, z)?
    pub fn visible(&self, cam: Vec2) -> bool {
        let d = ((cam - self.centre).abs() - self.half).max(Vec2::ZERO);
        d.length_squared() < self.range * self.range
    }
}

/// The point the foliage measures its range from (scene units): the camera's focus, the node
/// Kenshi hands every PagedGeometry as its camera (F041804: camera manager +0x30 -> +0x58, the
/// node F045490 reads). Without it, the camera.
#[derive(Resource, Clone, Copy, Default)]
pub struct FoliageCentre(pub Option<Vec3>);

/// Shows the foliage whose page is in range of the focus, hides the rest.
pub fn foliage_ranges(
    cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::reflection::ReflectionCamera>, Without<crate::interior_mask::MaskCamera>)>,
    centre: Option<Res<FoliageCentre>>,
    mut objects: Query<(&GlobalTransform, &FoliageRange, &mut Visibility)>,
) {
    let Some(cam) = cams.iter().next() else { return };
    let c = centre.and_then(|c| c.0).unwrap_or(cam.translation()).xz();
    if std::env::var("KENSHI_FOLIAGE_DEBUG").is_ok() {
        let shown = objects.iter().filter(|(_, r, _)| r.visible(c)).count();
        eprintln!("foliage cam {c:.0} {shown} of {} shown", objects.iter().count());
    }
    for (_, r, mut v) in &mut objects {
        let want = if r.visible(c) { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

/// What the zones' foliage reads (made once, shared with the loader threads).
pub struct FoliageWorld {
    pub foliage: kenshi_world::foliage::Foliage,
    pub towns: Vec<kenshi_world::town::Town>,
    pub blend: kenshi_world::terrain_biome::BlendInfo,
    pub map: Option<kenshi_world::terrain_biome::BlendMap>,
    /// BIOMES index -> record.
    pub biomes: HashMap<i32, String>,
    /// Per FOLIAGE_MESH: its mesh file (found) and look.
    pub looks: Vec<(Option<String>, ObjectLook)>,
    /// Per FOLIAGE_MESH: its "leaves mesh" (found) and their look (F041798: the "leaves
    /// texture", "leaves normal" and "leaves alpha threshold" as an object material of type 4,
    /// cut out and double-sided).
    pub leaves: Vec<Option<(String, ObjectLook)>>,
    pub water_level: f32,
    /// settings.cfg "foliage range", "grass range", "grass density".
    pub foliage_range: f32,
    pub grass_range: f32,
    pub grass_density: f32,
    /// Per GRASS: its sprite (found).
    pub sprites: Vec<Option<String>>,
}

impl FoliageWorld {
    pub fn new(
        install: &Path,
        data: &GameDataContainer,
        res: &Resources,
        water_level: f32,
        settings: (f32, f32, f32),
    ) -> Option<FoliageWorld> {
        let foliage = kenshi_world::foliage::Foliage::load(data);
        let sprites: Vec<Option<String>> = foliage.grass.iter().map(|g| res.find(&g.sprite)).collect();
        let looks = foliage
            .meshes
            .iter()
            .map(|m| {
                let r = data.get(&m.record);
                (res.find(&m.mesh), ObjectLook::from_record(r, |f| res.find(f)))
            })
            .collect();
        let leaves = foliage
            .meshes
            .iter()
            .map(|m| {
                let r = data.get(&m.record)?;
                let file = |k: &str| r.files.get(k).filter(|s| !s.is_empty()).and_then(|f| res.find(f));
                let mesh = file("leaves mesh")?;
                let threshold = r
                    .ints
                    .get("leaves alpha threshold")
                    .map(|&v| v as f32)
                    .or_else(|| r.floats.get("leaves alpha threshold").copied())
                    .unwrap_or(128.0)
                    / 255.0;
                Some((
                    mesh,
                    ObjectLook {
                        texture: file("leaves texture"),
                        normal: file("leaves normal"),
                        metal: None,
                        texture2: None,
                        normal2: None,
                        metal2: None,
                        tiling: [1.0, 1.0],
                        gloss_mult: 1.0,
                        threshold,
                        kind: 4,
                        grass: false,
                        foliage: true,
                    },
                ))
            })
            .collect();
        Some(FoliageWorld {
            foliage,
            towns: kenshi_world::town::towns(install, data),
            blend: kenshi_world::terrain_biome::BlendInfo::load(install)?,
            map: kenshi_world::terrain_biome::BlendMap::load(install),
            biomes: kenshi_world::terrain_biome::terrain_biomes(data)
                .into_iter()
                .map(|b| (b.index, b.record))
                .collect(),
            sprites,
            looks,
            leaves,
            water_level,
            foliage_range: settings.0,
            grass_range: settings.1,
            grass_density: settings.2,
        })
    }

    /// The BIOMES index at world (x, z) (F051563: the blend cell's slot weighing most).
    fn biome_index(&self, x: f32, z: f32) -> i32 {
        let (cx, cy) = self.blend.cell_at(x, z);
        let slot = self.map.as_ref().map_or(0, |m| m.dominant_slot(x, z));
        self.blend.slots(cx, cy)[slot.min(4)]
    }
}

/// The ground a zone's foliage reads: its terrain patch and its overlay tile.
struct ZoneGround<'a> {
    patch: &'a Patch,
    world: &'a FoliageWorld,
    /// The overlay tile (BGRA as Ogre has it), its side in pixels and its first corner.
    overlay: Vec<[u8; 4]>,
    side: i32,
    x0: f32,
    z0: f32,
    /// The zone's corner (world x, z): the origin of its slope grid.
    zone_x0: f32,
    zone_z0: f32,
}

/// The zone map's slope grid spacing (F045396).
const SLOPE_CELL: f32 = 36.0;

impl ZoneGround<'_> {
    fn cell(&self, x: f32, z: f32) -> (i32, i32, f32) {
        let size = WORLD_SIZE / 8.0;
        let cell = size / self.side as f32;
        let k = |v: f32, o: f32| {
            let i = ((v - o) / cell) as i32;
            if v < o {
                0
            } else if o + size < v {
                self.side - 1
            } else {
                i
            }
        };
        (k(x, self.x0), k(z, self.z0), cell)
    }
}

impl kenshi_world::foliage::Ground for ZoneGround<'_> {
    fn height(&mut self, x: f32, z: f32) -> f32 {
        ground_on(self.patch, Vec3::ZERO, x, z)
    }
    /// @fn F045396
    ///
    /// The zone map's slope (F050506 -> F051542 -> F045396): on the zone's height grid every 36
    /// units from its corner, the grid point at or before (x, z); the larger of its height
    /// difference to the point before it in x and to the next one in z (height units per 36,
    /// the unit of the records' "min / max slope"). (assumption) The grid holds the terrain's
    /// heights at its points; the grid's last row / column compare with themselves.
    fn slope(&mut self, x: f32, z: f32) -> f32 {
        let n = (ZONE_SIZE / SLOPE_CELL) as i32;
        let col = ((x - self.zone_x0) / SLOPE_CELL).clamp(0.0, n as f32) as i32;
        let row = ((z - self.zone_z0) / SLOPE_CELL).clamp(0.0, n as f32) as i32;
        let at = |c: i32, r: i32| {
            ground_on(self.patch, Vec3::ZERO, self.zone_x0 + c as f32 * SLOPE_CELL, self.zone_z0 + r as f32 * SLOPE_CELL)
        };
        let h = at(col, row);
        let left = if col > 0 { at(col - 1, row) } else { h };
        let next = if row < n { at(col, row + 1) } else { h };
        (h - left).abs().max((h - next).abs())
    }
    fn biome_index(&mut self, x: f32, z: f32) -> i32 {
        self.world.biome_index(x, z)
    }
    fn overlay(&mut self, x: f32, z: f32) -> Option<[u8; 4]> {
        let (c, r, _) = self.cell(x, z);
        self.overlay_at(c, r)
    }
    fn overlay_cell(&mut self, x: f32, z: f32) -> Option<(i32, i32, f32)> {
        (!self.overlay.is_empty()).then(|| self.cell(x, z))
    }
    fn overlay_at(&mut self, col: i32, row: i32) -> Option<[u8; 4]> {
        if col < 0 || row < 0 || col >= self.side || row >= self.side {
            return None;
        }
        self.overlay.get((row * self.side + col) as usize).copied()
    }
    fn overlay_centre(&mut self, col: i32, row: i32) -> Option<(f32, f32)> {
        if col <= 0 || row <= 0 || col >= self.side - 1 || row >= self.side - 1 {
            return None;
        }
        let cell = WORLD_SIZE / 8.0 / self.side as f32;
        Some(((col as f32 + 0.5) * cell + self.x0, (row as f32 + 0.5) * cell + self.z0))
    }
    fn set_overlay_green(&mut self, col: i32, row: i32, v: u8) {
        self.set_overlay_channel(col, row, 1, v);
    }
    fn set_overlay_channel(&mut self, col: i32, row: i32, channel: usize, v: u8) {
        if col >= 0 && row >= 0 && col < self.side && row < self.side {
            self.overlay[(row * self.side + col) as usize][channel] = v;
        }
    }
    fn pixel_world(&mut self, col: i32, row: i32) -> Option<(f32, f32)> {
        if col < 0 || row < 0 || col >= self.side || row >= self.side {
            return None;
        }
        let cell = WORLD_SIZE / 8.0 / self.side as f32;
        Some(((col as f32 + 0.5) * cell + self.x0, (row as f32 + 0.5) * cell + self.z0))
    }
    fn water_level(&self) -> f32 {
        self.world.water_level
    }
}

impl ZoneScene {
    /// @fn F041804
    ///
    /// The zone's foliage meshes (kenshi_world::foliage): each biome of its blend cell, each of the
    /// biome's foliage layers scattered over the zone, the objects added as pieces drawn out to the
    /// layer's range (50, 100, 800 or 4000 by its "visibility range", x 10 x "foliage range").
    /// (assumption) The zone's biomes are its blend cell's slots, in slot order; the overlay tile
    /// is read afresh for each zone (Kenshi's grass spots written by neighbouring zones are not
    /// seen).
    pub fn add_foliage(&mut self, root: &Path, world: &FoliageWorld, zone: (i32, i32)) {
        let x0 = zone.0 as f32 * ZONE_SIZE - WORLD_SIZE / 2.0;
        let z0 = zone.1 as f32 * ZONE_SIZE - WORLD_SIZE / 2.0;
        // the grass and foliage inside buildings' interiors are cut where they are drawn
        // (CLIP_INTERIOR, kenshi_render::interior_mask)
        let rect = [x0, z0, x0 + ZONE_SIZE, z0 + ZONE_SIZE];
        let tile = (zone.0.div_euclid(8), zone.1.div_euclid(8));
        let (side, overlay) = image::open(root.join(format!(
            "data/newland/land/overlaymaps/new_overlay.{}.{}.png",
            tile.0, tile.1
        )))
        .map(|i| {
            let i = i.to_rgba8();
            (i.width() as i32, i.pixels().map(|p| [p[2], p[1], p[0], p[3]]).collect())
        })
        .unwrap_or((1, Vec::new()));
        let mut ground = ZoneGround {
            patch: &self.patch,
            world,
            overlay,
            side,
            x0: tile.0 as f32 * WORLD_SIZE / 8.0 - WORLD_SIZE / 2.0,
            z0: tile.1 as f32 * WORLD_SIZE / 8.0 - WORLD_SIZE / 2.0,
            zone_x0: x0,
            zone_z0: z0,
        };
        let (cx, cy) = world.blend.cell_at(x0 + ZONE_SIZE / 2.0, z0 + ZONE_SIZE / 2.0);
        let slots: Vec<i32> = world.blend.slots(cx, cy).into_iter().filter(|&i| i != 0).collect();
        // the grass density painted first (F041791: the layers' grass with reference value 0)
        let (col0, row0, _) = ground.cell(x0, z0);
        for &index in &slots {
            let Some(record) = world.biomes.get(&index) else { continue };
            let filter = (slots.len() > 1).then_some(index);
            for u in world.foliage.biome_grass.get(record).into_iter().flatten() {
                if u.channel == 2 {
                    kenshi_world::foliage::paint_grass(&world.foliage.grass[u.grass], col0, row0, filter, &mut ground);
                }
            }
        }
        let mut placed = Vec::new();
        for &index in &slots {
            let Some(record) = world.biomes.get(&index) else { continue };
            let Some(layers) = world.foliage.biome_layers.get(record) else { continue };
            let filter = (slots.len() > 1).then_some(index);
            for &l in layers {
                kenshi_world::foliage::scatter_layer(&world.foliage, l, rect, filter, &world.towns, &mut ground, &mut placed);
            }
        }
        if std::env::var("KENSHI_FOLIAGE_DEBUG").is_ok() {
            let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
            for p in &placed {
                *by.entry(world.foliage.meshes[p.mesh].record.as_str()).or_default() += 1;
            }
            let found = placed.iter().filter(|p| world.looks[p.mesh].0.is_some()).count();
            let first = world.foliage.meshes.first().map(|m| (m.mesh.clone(), world.looks[0].0.clone()));
            eprintln!("zone {zone:?} biomes {slots:?} foliage {} found {found} e.g. {first:?}", placed.len());
            let r: Vec<u32> = kenshi_world::foliage::REJECTS.iter().map(|a| a.load(std::sync::atomic::Ordering::Relaxed)).collect();
            eprintln!("  rejects slope/alt/town/blocked/grasslimit/biome/placed {r:?}");
            let mut byl: std::collections::BTreeMap<(i32, &str), usize> = Default::default();
            for p in &placed {
                *byl.entry((world.foliage.layers[p.layer].visibility_range, world.foliage.meshes[p.mesh].mesh.as_str())).or_default() += 1;
            }
            for ((v, m), n) in &byl {
                eprintln!("  vis {v} {n:5} {m}");
            }
            for &index in &slots {
                if let Some(rec) = world.biomes.get(&index) {
                    let layers = world.foliage.biome_layers.get(rec).map_or(0, |l| l.len());
                    eprintln!("  biome {index:06x} {rec} layers {layers}");
                }
            }
        }
        const RANGES: [f32; 4] = [50.0, 100.0, 800.0, 4000.0];
        // the grass (F041797, F052214..F052216): blades over the zone, a quad each
        let mut rng = kenshi_world::foliage::Mt::new((zone.0 * 64 + zone.1) as u32);
        for &index in &slots {
            let Some(record) = world.biomes.get(&index) else { continue };
            // (testing) KENSHI_NO_GRASS: none
            if std::env::var("KENSHI_NO_GRASS").is_ok() {
                continue;
            }
            let several = slots.len() > 1;
            for u in world.foliage.biome_grass.get(record).into_iter().flatten() {
                let g = &world.foliage.grass[u.grass];
                let Some(sprite) = world.sprites[u.grass].clone() else { continue };
                let range = RANGES[u.visibility_range.clamp(0, 2) as usize] * 10.0 * world.grass_range;
                if std::env::var("KENSHI_FOLIAGE_DEBUG").is_ok() {
                    eprintln!("zone {zone:?} grass {} sprite {sprite} density {}", g.record, g.density);
                }
                // F041804: the grass's pages, a quarter of the zone
                let step = ZONE_SIZE / 4.0;
                for pz in 0..4 {
                    for px in 0..4 {
                        let page = [x0 + px as f32 * step, z0 + pz as f32 * step, x0 + (px + 1) as f32 * step, z0 + (pz + 1) as f32 * step];
                        let mut blades = kenshi_world::foliage::grass_blades(g, u, world.grass_density, page, &mut rng, &mut ground);
                        if several {
                            blades.retain(|b| world.biome_index(b.x, b.z) == index);
                        }
                        if blades.is_empty() {
                            continue;
                        }
                        let centre = Vec3::new((page[0] + page[2]) * 0.5, 0.0, (page[1] + page[3]) * 0.5);
                        let centre = centre.with_y(ground_on(&self.patch, Vec3::ZERO, centre.x, centre.z));
                        let mesh = grass_mesh(g, &blades, &self.patch, centre);
                        let page = FoliageRange {
                            range,
                            centre: Vec2::new(centre.x - self.origin.x, centre.z - self.origin.z),
                            half: step * 0.5,
                        };
                        self.grass.push((mesh, sprite.clone(), page, centre - self.origin));
                    }
                }
            }
        }
        for p in placed {
            // the leaves, on the same transform as the trunk
            if let Some((lmesh, llook)) = world.leaves[p.mesh].clone() {
                let layer = &world.foliage.layers[p.layer];
                let range = RANGES[layer.visibility_range.clamp(0, 3) as usize] * 10.0 * world.foliage_range;
                let transform = Transform::from_translation(Vec3::from(p.position) - self.origin)
                    .with_rotation(Quat::from_rotation_y(p.yaw.to_radians()))
                    .with_scale(Vec3::splat(p.scale));
                if !self.meshes.contains_key(&lmesh) {
                    let m = ogre_mesh::read(&root.join(&lmesh))
                        .map(|m| to_bevy(&m).into_iter().map(|(_, m)| m).collect())
                        .unwrap_or_default();
                    self.meshes.insert(lmesh.clone(), m);
                }
                let zone_min = Vec2::new(x0 - self.origin.x, z0 - self.origin.z);
                let page = FoliageRange::page(range, zone_min, ZONE_SIZE / 2.0, transform.translation.xz());
                self.pieces.push(Piece {
                    mesh: lmesh,
                    look: llook,
                    transform,
                    foliage_range: Some(page),
                    building: None,
                    floor: None,
                    local_y: 0.0,
                    interior: false,
                    swivel: None,
                    door: None,
                });
            }
            let (Some(mesh), mut look) = world.looks[p.mesh].clone() else { continue };
            look.foliage = true;
            let layer = &world.foliage.layers[p.layer];
            let range = RANGES[layer.visibility_range.clamp(0, 3) as usize] * 10.0 * world.foliage_range;
            let transform = Transform::from_translation(Vec3::from(p.position) - self.origin)
                .with_rotation(Quat::from_rotation_y(p.yaw.to_radians()))
                .with_scale(Vec3::splat(p.scale));
            if !self.meshes.contains_key(&mesh) {
                let m = ogre_mesh::read(&root.join(&mesh))
                    .map(|m| to_bevy(&m).into_iter().map(|(_, m)| m).collect())
                    .unwrap_or_default();
                self.meshes.insert(mesh.clone(), m);
            }
            // F041804: the layer's pages, half the zone
            let zone_min = Vec2::new(x0 - self.origin.x, z0 - self.origin.z);
            let page = FoliageRange::page(range, zone_min, ZONE_SIZE / 2.0, transform.translation.xz());
            self.pieces.push(Piece {
                mesh,
                look,
                transform,
                foliage_range: Some(page),
                building: None,
                floor: None,
                local_y: 0.0,
                interior: false,
                swivel: None,
                door: None,
            });
        }
    }

    /// Is (x, z) (scene coordinates) at least `margin` outside every placed part's disc?
    pub fn is_clear(&self, x: f32, z: f32, margin: f32) -> bool {
        self.occupied
            .iter()
            .all(|(o, r)| (o.x - x).hypot(o.z - z) > r + margin)
    }

    /// Terrain height at scene (x, z), relative to the origin; bilinear on the loaded patch.
    pub fn ground(&self, x: f32, z: f32) -> f32 {
        ground_on(&self.patch, self.origin, x, z)
    }

    /// Spawn the terrain with Kenshi's terrain material (each piece the biomes of its blend
    /// cell) and the buildings with Kenshi's object material.
    pub fn spawn_kenshi(
        &mut self,
        commands: &mut Commands,
        assets: &AssetServer,
        meshes: &mut Assets<Mesh>,
        object_materials: &mut Assets<KenshiObjectMaterial>,
        terrain_materials: &mut Assets<KenshiTerrainMaterial>,
        look: &mut TerrainLook,
    ) {
        let origin = self.origin;
        let tag = ZoneTag(self.zone);
        for (tile, cell, ground) in std::mem::take(&mut self.terrain) {
            let m = look.material(assets, tile, cell, origin);
            commands.spawn((
                tag,
                Mesh3d(meshes.add(ground)),
                MeshMaterial3d(terrain_materials.add(m)),
                crate::reflection::reflected(),
            ));
        }
        // the object shader's normal maps want tangents
        for ms in self.meshes.values_mut() {
            for m in ms.iter_mut() {
                if m.attribute(Mesh::ATTRIBUTE_TANGENT).is_none() {
                    let _ = m.generate_tangents();
                }
            }
        }
        let mut mats: HashMap<String, Handle<KenshiObjectMaterial>> = HashMap::new();
        for (mesh, sprite, range, centre) in std::mem::take(&mut self.grass) {
            // the sprite stands in as the normal map too: the cutout (here and in the prepass)
            // reads its alpha
            let gl = ObjectLook {
                texture: Some(sprite.clone()),
                normal: Some(sprite),
                metal: None,
                texture2: None,
                normal2: None,
                metal2: None,
                tiling: [1.0, 1.0],
                gloss_mult: 0.0,
                // Grass_FP's threshold (foliage.material)
                threshold: 0.6,
                kind: 4,
                grass: true,
                foliage: true,
            };
            let m = mats
                .entry(format!("{gl:?}"))
                .or_insert_with(|| object_materials.add(look.object(assets, &gl, origin, false, [1.0; 3])))
                .clone();
            commands.spawn((
                tag,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(m),
                Transform::from_translation(centre),
                range,
                bevy::light::NotShadowCaster,
            ));
        }
        self.spawn_features(commands, meshes, terrain_materials, look, assets);
        self.spawn_parts(commands, meshes, |o, colours, at| {
            let ground = look.ground_colour(at.x + origin.x, at.z + origin.z);
            mats.entry(format!("{o:?}{colours}{ground:?}"))
                .or_insert_with(|| {
                    object_materials.add(look.object(assets, o, origin, colours, ground))
                })
                .clone()
        });
    }

    /// Spawn the buildings with Bevy's standard material (consumes their meshes).
    pub fn spawn_buildings(
        &mut self,
        commands: &mut Commands,
        assets: &AssetServer,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        let mut mats: HashMap<(Option<String>, Option<String>), Handle<StandardMaterial>> =
            HashMap::new();
        self.spawn_parts(commands, meshes, |o, _, _| {
            let (texture, normal) = (o.texture.clone(), o.normal.clone());
            mats.entry((texture.clone(), normal.clone()))
                .or_insert_with(|| {
                    materials.add(StandardMaterial {
                        base_color: if texture.is_some() {
                            Color::WHITE
                        } else {
                            Color::srgb(0.6, 0.57, 0.52)
                        },
                        base_color_texture: texture.map(|p| assets.load(p)),
                        normal_map_texture: normal.map(|p| assets.load(p)),
                        perceptual_roughness: 0.85,
                        ..default()
                    })
                })
                .clone()
        });
    }

    /// Material type 2 (F046403: "Feature_Terrain_DX11", mapfeature.material): the piece is
    /// drawn with the terrain's own material where it stands (terrainfp4.hlsl mapfeature_fs,
    /// Feature_VP: the overlay coordinates from the world position), so rocks take the ground's
    /// textures; their MATERIAL_SPEC texture fields are not read. Here: the terrain material of
    /// the colour tile and blend cell at the piece's centre, the mesh flagged for it (UV_1.y: 1 +
    /// the colour tile, see kenshi_terrain.wgsl), no geomorph.
    fn spawn_features(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        terrain_materials: &mut Assets<KenshiTerrainMaterial>,
        look: &mut TerrainLook,
        assets: &AssetServer,
    ) {
        let origin = self.origin;
        let (features, rest): (Vec<Piece>, Vec<Piece>) =
            std::mem::take(&mut self.pieces).into_iter().partition(|p| p.look.kind == 2);
        self.pieces = rest;
        let samples = 16384.0;
        let tile_px = kenshi_world::COLOUR_TILE_PX as f32;
        let ws = kenshi_world::WORLD_SIZE;
        let mut mats = HashMap::new();
        let mut copies: HashMap<(String, u32), Vec<Handle<Mesh>>> = HashMap::new();
        for piece in features {
            let w = piece.transform.translation + origin;
            let px = |v: f32| ((v + ws / 2.0) / ws * samples).clamp(0.0, samples - 1.0);
            let tile = (((px(w.x) / tile_px) as u32).min(7), ((px(w.z) / tile_px) as u32).min(7));
            let cell = ((px(w.x) / 512.0) as u32, (px(w.z) / 512.0) as u32);
            let mat = mats
                .entry((tile, cell))
                .or_insert_with(|| terrain_materials.add(look.material(assets, tile, cell, origin)))
                .clone();
            let code = tile.0 + 8 * tile.1;
            let hs = copies.entry((piece.mesh.clone(), code)).or_insert_with(|| {
                self.meshes
                    .get(&piece.mesh)
                    .into_iter()
                    .flatten()
                    .map(|m| {
                        let mut m = m.clone();
                        let n = m.count_vertices();
                        if m.attribute(Mesh::ATTRIBUTE_UV_0).is_none() {
                            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; n]);
                        }
                        m.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32, 1.0 + code as f32]; n]);
                        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; n]);
                        m.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
                        meshes.add(m)
                    })
                    .collect()
            });
            for h in hs.iter() {
                let mut e = commands.spawn((ZoneTag(self.zone), Mesh3d(h.clone()), MeshMaterial3d(mat.clone()), piece.transform));
                if let Some(page) = piece.foliage_range {
                    e.insert(page);
                }
            }
        }
    }

    /// Spawn the building parts, each with the material `material` gives its look.
    fn spawn_parts<M: Material>(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        mut material: impl FnMut(&ObjectLook, bool, Vec3) -> Handle<M>,
    ) {
        for v in std::mem::take(&mut self.buildings) {
            commands.spawn((v, ZoneTag(self.zone)));
        }
        let mut handles: HashMap<String, Vec<UploadedMesh>> = HashMap::new();
        for (path, ms) in std::mem::take(&mut self.meshes) {
            let hs = ms
                .into_iter()
                .map(|m| {
                    let b = m
                        .compute_aabb()
                        .map(|a| (Vec3::from(a.min()), Vec3::from(a.max())));
                    let colours = m.attribute(Mesh::ATTRIBUTE_COLOR).is_some();
                    (meshes.add(m), b, colours)
                })
                .collect();
            handles.insert(path, hs);
        }
        for piece in std::mem::take(&mut self.pieces) {
            for (h, b, colours) in handles.get(&piece.mesh).into_iter().flatten() {
                let mat = material(&piece.look, *colours, piece.transform.translation);
                let mut e = commands.spawn((
                    ZoneTag(self.zone),
                    Mesh3d(h.clone()),
                    MeshMaterial3d(mat.clone()),
                    piece.transform,
                ));
                if let Some(page) = piece.foliage_range {
                    e.insert(page);
                    continue;
                }
                if let Some((lo, hi)) = *b {
                    // the box's corners placed, then bounded again
                    let (mut wlo, mut whi) = (Vec3::MAX, Vec3::MIN);
                    for k in 0..8 {
                        let c = Vec3::new(
                            if k & 1 == 0 { lo.x } else { hi.x },
                            if k & 2 == 0 { lo.y } else { hi.y },
                            if k & 4 == 0 { lo.z } else { hi.z },
                        );
                        let w = piece.transform.transform_point(c);
                        wlo = wlo.min(w);
                        whi = whi.max(w);
                    }
                    e.insert(BuildingPiece { lo: wlo, hi: whi });
                }
                if let (Some(building), Some(floor)) = (piece.building, piece.floor) {
                    e.insert(BuildingFloor { zone: self.zone, building, floor, interior: piece.interior });
                }
                if let Some(s) = &piece.swivel {
                    e.insert(s.clone());
                }
                if let (Some(building), Some(index)) = (piece.building, piece.door) {
                    e.insert(DoorPiece { zone: self.zone, building, index, rest: piece.transform });
                }
            }
        }
    }

    /// Spawn the terrain and buildings (consumes the meshes).
    pub fn spawn(
        &mut self,
        commands: &mut Commands,
        assets: &AssetServer,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        for ((tx, ty), _, ground) in std::mem::take(&mut self.terrain) {
            commands.spawn((
                Mesh3d(meshes.add(ground)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color_texture: Some(assets.load(colour_tile(tx, ty))),
                    perceptual_roughness: 0.95,
                    ..default()
                })),
            ));
        }
        self.spawn_buildings(commands, assets, meshes, materials);
    }
}

/// Terrain height of a patch at scene (x, z) for a scene `origin`, relative to the origin.
pub fn ground_on(p: &Patch, origin: Vec3, x: f32, z: f32) -> f32 {
    let fx = ((x + origin.x - p.x0) / p.spacing).clamp(0.0, (p.w - 2) as f32);
    let fz = ((z + origin.z - p.z0) / p.spacing).clamp(0.0, (p.h - 2) as f32);
    let (i, j) = (fx as u32, fz as u32);
    let (tx, tz) = (fx.fract(), fz.fract());
    let top = p.at(i, j) * (1.0 - tx) + p.at(i + 1, j) * tx;
    let bottom = p.at(i, j + 1) * (1.0 - tx) + p.at(i + 1, j + 1) * tx;
    top * (1.0 - tz) + bottom * tz - origin.y
}

/// A grass's quads (F052216, GRASSTECH_QUAD): per blade a quad of its size (between the grass's
/// minimum and maximum) turned by its yaw, its foot on the ground at both ends; drawn from both
/// sides (two faces). Positions relative to `origin`.
pub fn grass_mesh(g: &kenshi_world::foliage::Grass, blades: &[kenshi_world::foliage::Blade], patch: &Patch, origin: Vec3) -> Mesh {
    let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for b in blades {
        let half = (g.min_size.0 + (g.max_size.0 - g.min_size.0) * b.size) * 0.5;
        let height = g.min_size.1 + (g.max_size.1 - g.min_size.1) * b.size;
        let (dx, dz) = (b.yaw.cos() * half, b.yaw.sin() * half);
        let (x1, z1, x2, z2) = (b.x - dx, b.z - dz, b.x + dx, b.z + dz);
        let h1 = ground_on(patch, Vec3::ZERO, x1, z1);
        let h2 = ground_on(patch, Vec3::ZERO, x2, z2);
        let k = pos.len() as u32;
        for (p, t) in [
            (Vec3::new(x1, h1 + height, z1), [0.0, 0.0]),
            (Vec3::new(x2, h2 + height, z2), [1.0, 0.0]),
            (Vec3::new(x1, h1, z1), [0.0, 1.0]),
            (Vec3::new(x2, h2, z2), [1.0, 1.0]),
        ] {
            pos.push((p - origin).to_array());
            nrm.push([0.0, 1.0, 0.0]);
            uv.push(t);
        }
        idx.extend([k, k + 2, k + 1, k + 1, k + 2, k + 3]);
        idx.extend([k, k + 1, k + 2, k + 1, k + 3, k + 2]);
    }
    let mut m = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(bevy::mesh::Indices::U32(idx));
    m
}
