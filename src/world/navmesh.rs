//! Kenshi's navmesh (port_map `ai`/pathing): the shipped Havok AI tiles, Kenshi's own face-type
//! cost modifier and edge filters, and the path request steps of its navmesh manager.
//!
//! Spec: sheet rows F032195 (tile file name), F032235 (manager update: one path request per tick),
//! F032163/F032185 (start and goal face searches), F032192 (cluster reachability), F032207 (straight
//! line check), F032208 (A* request), F032307 WaterCostModifier::getModifiedCost, F032309
//! EdgeFilter::isEnabled, F032316 WaterEdgeFilter::isEnabled.
//!
//! Tiles: `data/newland/land/navtiles/tileX.Y.hkt` (Havok tagfile, `kenshi_io::hkt`) hold the
//! tile's ground navmesh and one navmesh per building in it, in Havok metres, Y up. The ground mesh
//! is relative to the tile corner: tile X.Y covers zone cell X.Y (`ZONE_SIZE` units, X along world
//! x, Y along world z), which puts every vertex on the terrain to within a unit (checked with the
//! `navfit` example). A building mesh is relative to the building position its "Info" string
//! gives, unrotated (its edges then meet the ground mesh edges they are linked to). Havok works
//! in metres = units x 0.1, so this module does too: positions are world units x 0.1.
//!
//! Face data (one int per face) is Kenshi's face type: 3 water, 4 costly, 5 blocked (-1 and 1 are
//! plain ground to the filters). Each mesh is a section with a streaming uid (ground: Y * 256 + X;
//! buildings: the hex number leading their Info string); streaming sets link edges to other
//! sections (the shipped seams are slightly stale: ~1.7% of links have no link back and ~140 point
//! past the far section's faces; those lead nowhere). Each section also carries a cluster graph (`hkaiDirectedGraphExplicitCost`) whose
//! nodes are its face clusters.
//!
//! What runs inside Havok (A* over faces, clearance, string pulling, closest-point queries) is
//! library code, not Kenshi's: it is replaced here by an A* over face edges whose width admits
//! the agent's diameter, edge-midpoint costs, and a funnel over the crossed edges kept one radius
//! off their ends.

use kenshi_io::hkt::{self, Tagfile};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::Path;

use crate::{WORLD_SIZE, ZONE_SIZE};

/// Tiles per side.
pub const TILES: i32 = 64;
/// Havok metres per world unit.
pub const METRES: f32 = 0.1;

/// Navmesh tile file relative to the install root (F032195: base path + "tile" + X + "." + Y + ".hkt").
/// @fn F032195
pub fn tile_file(x: i32, y: i32) -> String {
    format!("data/newland/land/navtiles/tile{x}.{y}.hkt")
}

/// Streaming uid of a tile's ground mesh, as streaming sets name it: Y * 256 + X.
pub fn tile_uid(x: i32, y: i32) -> i64 {
    y as i64 * 256 + x as i64
}

/// Tile containing a position in metres.
pub fn tile_of(p: [f32; 3]) -> (i32, i32) {
    let half = WORLD_SIZE * METRES / 2.0;
    let size = ZONE_SIZE * METRES;
    (
        ((p[0] + half) / size).floor() as i32,
        ((p[2] + half) / size).floor() as i32,
    )
}

/// Kenshi's face types (face data).
pub const FACE_WATER: i32 = 3;
pub const FACE_COSTLY: i32 = 4;
pub const FACE_BLOCKED: i32 = 5;
/// Cost added to a step onto a costly face (F032307, constant 0x1416c8f80).
pub const COSTLY_EXTRA: f32 = 5.0;

/// WaterCostModifier::getModifiedCost (F032307): water multiplies the step cost by the agent's
/// water cost, costly faces add 5 m, others keep it.
/// @fn F032307
pub fn modified_cost(kind: i32, cost: f32, water_cost: f32) -> f32 {
    match kind {
        FACE_WATER => cost * water_cost,
        FACE_COSTLY => COSTLY_EXTRA + cost,
        _ => cost,
    }
}

/// EdgeFilter::isEnabled (F032309): only blocked faces are shut. Used by path searches.
/// @fn F032309
pub fn edge_enabled(kind: i32) -> bool {
    kind != FACE_BLOCKED
}

/// WaterEdgeFilter::isEnabled (F032316): blocked faces and water are shut. Used by the straight
/// line check unless the caller passes the manager's filter.
/// @fn F032316
pub fn water_edge_enabled(kind: i32) -> bool {
    kind != FACE_BLOCKED && kind != FACE_WATER
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub start_edge: u32,
    pub num_edges: u32,
    pub cluster: i32,
    pub kind: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub a: u32,
    pub b: u32,
    /// Opposite face and edge in this section, -1 for none.
    pub opposite_face: i32,
    pub opposite_edge: i32,
}

/// A face in the loaded world: section uid and face index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceRef {
    pub section: i64,
    pub face: u32,
}

/// Cluster graph of one section (`hkaiDirectedGraphExplicitCost`): node n covers the faces whose
/// cluster is n.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clusters {
    pub positions: Vec<[f32; 3]>,
    /// Out-edges per node: (cost, target node).
    pub edges: Vec<Vec<(f32, u32)>>,
    /// Links to nodes of other sections: (node, section uid, opposite node, cost).
    pub links: Vec<(u32, i64, u32, f32)>,
}

/// One navmesh of a tile file: the tile's ground, or a building's (placed at the position its
/// "Info" string gives).
#[derive(Debug, Clone, Default, PartialEq)]
/// @type NavTileSection
pub struct Section {
    pub uid: i64,
    pub tile: (i32, i32),
    /// Building sections: "uid hex;x;y;z" (position in world units).
    pub info: Option<String>,
    /// Vertices in metres, world placed.
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    /// Edges continued in other sections: edge index -> (section uid, opposite face, opposite
    /// edge). Section seams need not match edge for edge, so one edge can meet several.
    pub links: HashMap<u32, Vec<(i64, u32, u32)>>,
    pub clusters: Clusters,
}

impl Section {
    /// A building section's building position (world units), from its "Info" string.
    pub fn building_position(&self) -> Option<[f32; 3]> {
        let mut f = self.info.as_deref()?.split(';').skip(1);
        let mut n = || f.next()?.trim().parse::<f32>().ok();
        Some([n()?, n()?, n()?])
    }
}

/// Every section of tile X.Y; empty where the world has no tile file.
pub fn load_tile(install: &Path, x: i32, y: i32) -> Result<Vec<Section>, hkt::Error> {
    let path = install.join(tile_file(x, y));
    if !path.exists() {
        return Ok(vec![]);
    }
    sections(x, y, &Tagfile::open(&path)?)
}

fn missing(what: &str) -> hkt::Error {
    hkt::Error::Unsupported(format!("navtile without {what}"))
}

/// The sections of a tile file: its root container lists, per navmesh, "NavMesh", "Graph",
/// "Mediator", an "Info" string for buildings, and "Hash".
pub fn sections(x: i32, y: i32, file: &Tagfile) -> Result<Vec<Section>, hkt::Error> {
    let root = file.object(1).ok_or_else(|| missing("root"))?;
    let mut groups: Vec<[Option<u32>; 3]> = vec![];
    for v in root
        .array("namedVariants")
        .iter()
        .filter_map(|v| v.object())
    {
        let r = v.get("variant").and_then(|r| r.object_ref());
        let slot = match v.get("name").and_then(|n| n.str()) {
            Some("NavMesh") => {
                groups.push([None; 3]);
                0
            }
            Some("Graph") => 1,
            Some("Info") => 2,
            _ => continue,
        };
        if let Some(g) = groups.last_mut() {
            g[slot] = r;
        }
    }
    groups
        .into_iter()
        .map(|[mesh, graph, info]| {
            let mesh = mesh
                .and_then(|m| file.object(m))
                .ok_or_else(|| missing("hkaiNavMesh"))?;
            let graph = graph.and_then(|g| file.object(g));
            let info = info
                .and_then(|i| file.object(i))
                .and_then(|i| i.get("string"))
                .and_then(|s| s.str())
                .map(str::to_owned);
            section(x, y, mesh, graph, info)
        })
        .collect()
}

fn section(
    x: i32,
    y: i32,
    mesh: &hkt::Object,
    graph: Option<&hkt::Object>,
    info: Option<String>,
) -> Result<Section, hkt::Error> {
    let half = WORLD_SIZE * METRES / 2.0;
    let origin = match &info {
        Some(i) => {
            let f: Vec<f32> = i
                .split(';')
                .skip(1)
                .filter_map(|v| v.parse().ok())
                .collect();
            if f.len() != 3 {
                return Err(missing("building position"));
            }
            [f[0] * METRES, f[1] * METRES, f[2] * METRES]
        }
        None => [
            x as f32 * ZONE_SIZE * METRES - half,
            0.0,
            y as f32 * ZONE_SIZE * METRES - half,
        ],
    };
    let place = |v: &[f32]| [origin[0] + v[0], origin[1] + v[1], origin[2] + v[2]];
    let vertices = mesh
        .array("vertices")
        .iter()
        .map(|v| v.vector().map(place).ok_or_else(|| missing("vertex")))
        .collect::<Result<Vec<_>, _>>()?;
    let stride = mesh.int("faceDataStriding").max(0) as usize;
    let face_data = mesh.array("faceData");
    let faces = mesh
        .array("faces")
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let f = f.object().ok_or_else(|| missing("face"))?;
            let kind = if stride > 0 {
                face_data
                    .get(i * stride)
                    .and_then(|v| v.int())
                    .unwrap_or(-1) as i32
            } else {
                -1
            };
            Ok(Face {
                start_edge: f.int("startEdgeIndex") as u32,
                num_edges: f.int("numEdges") as u32,
                cluster: f.int("clusterIndex") as i32,
                kind,
            })
        })
        .collect::<Result<Vec<_>, hkt::Error>>()?;
    let edges = mesh
        .array("edges")
        .iter()
        .map(|e| {
            let e = e.object().ok_or_else(|| missing("edge"))?;
            Ok(Edge {
                a: e.int("a") as u32,
                b: e.int("b") as u32,
                opposite_face: e.int("oppositeFace") as i32,
                opposite_edge: e.int("oppositeEdge") as i32,
            })
        })
        .collect::<Result<Vec<_>, hkt::Error>>()?;
    let sets: Vec<&hkt::Object> = mesh
        .array("streamingSets")
        .iter()
        .filter_map(|s| s.object())
        .collect();
    let uid = match (sets.first(), &info) {
        (Some(s), _) => s.int("thisUid"),
        (None, Some(i)) => i64::from_str_radix(i.split(';').next().unwrap_or(""), 16)
            .map_err(|_| missing("uid"))?,
        (None, None) => tile_uid(x, y),
    };
    let mut links: HashMap<u32, Vec<(i64, u32, u32)>> = HashMap::new();
    for set in &sets {
        let other = set.int("oppositeUid");
        for c in set
            .array("meshConnections")
            .iter()
            .filter_map(|c| c.object())
        {
            links.entry(c.int("edgeIndex") as u32).or_default().push((
                other,
                c.int("oppositeFaceIndex") as u32,
                c.int("oppositeEdgeIndex") as u32,
            ));
        }
    }
    let mut clusters = Clusters::default();
    if let Some(g) = graph {
        clusters.positions = g
            .array("positions")
            .iter()
            .filter_map(|v| v.vector().map(place))
            .collect();
        let all: Vec<(f32, u32)> = g
            .array("edges")
            .iter()
            .filter_map(|e| e.object())
            .map(|e| (e.real("cost"), e.int("target") as u32))
            .collect();
        clusters.edges = g
            .array("nodes")
            .iter()
            .filter_map(|n| n.object())
            .map(|n| {
                let s = n.int("startEdgeIndex") as usize;
                all.get(s..s + n.int("numEdges") as usize)
                    .unwrap_or(&[])
                    .to_vec()
            })
            .collect();
        for set in g.array("streamingSets").iter().filter_map(|s| s.object()) {
            let other = set.int("oppositeUid");
            for c in set
                .array("graphConnections")
                .iter()
                .filter_map(|c| c.object())
            {
                clusters.links.push((
                    c.int("nodeIndex") as u32,
                    other,
                    c.int("oppositeNodeIndex") as u32,
                    c.real("edgeCost"),
                ));
            }
        }
    }
    Ok(Section {
        uid,
        tile: (x, y),
        info,
        vertices,
        faces,
        edges,
        links,
        clusters,
    })
}

impl Section {
    /// Corners of a face in order.
    pub fn corners(&self, face: u32) -> impl Iterator<Item = [f32; 3]> + '_ {
        let f = self.faces[face as usize];
        (f.start_edge..f.start_edge + f.num_edges)
            .map(|e| self.vertices[self.edges[e as usize].a as usize])
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    dot(sub(a, b), sub(a, b)).sqrt()
}
fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(a, scale(sub(b, a), t))
}

/// Closest point to `p` on triangle abc (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, scale(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, scale(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let denom = 1.0 / (va + vb + vc);
    add(a, add(scale(ab, vb * denom), scale(ac, vc * denom)))
}

/// Closest point on segment ab to p, as the parameter along it.
fn segment_t(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let ab = sub(b, a);
    let l = dot(ab, ab);
    if l <= 0.0 {
        0.0
    } else {
        (dot(sub(p, a), ab) / l).clamp(0.0, 1.0)
    }
}

/// 2D (x, z) cross product of b - a and c - a.
fn side(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[0] - a[0])
}

/// Priority of a path request (F032210): urgent requests (goTo mode 2) jump the normal ones.
pub const PRIORITY_URGENT: i32 = 20;
pub const PRIORITY_NORMAL: i32 = 10;

/// A queued path request (the 0xa0 request object): who asked, from where to where, with the
/// asker's radius and water cost.
#[derive(Debug, Clone, PartialEq)]
pub struct PathRequest<Owner> {
    pub owner: Owner,
    pub start: [f32; 3],
    pub goal: [f32; 3],
    pub radius: f32,
    pub water_cost: f32,
    /// +0x2c: 20 urgent, 10 normal, -1 cancel (F032211).
    pub priority: i32,
    /// Which shut gates the asker may pass.
    pub pass: Pass,
}

/// A shut door across the navmesh (DoorStuff marks its door along the navmesh while it is under
/// 0.6 open, F029335): a line (x, z metres) between heights `lo` and `hi`, its owner's faction
/// (a number the game gives it), and whether it is shut and locked.
/// (assumption) Kenshi blocks the door's navmesh for all and lets its own people open it; here a
/// shut gate is passed by its owner's people, and by the player's while it is not locked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Barrier {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub lo: f32,
    pub hi: f32,
    pub owner: u32,
    pub shut: bool,
    pub locked: bool,
}

/// Whom a path is for: their faction (as the barriers number them), whether the player's, or
/// anyone (barriers ignored).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pass {
    pub faction: u32,
    pub player: bool,
    pub ignore: bool,
}

impl Pass {
    pub const ANYONE: Pass = Pass { faction: u32::MAX, player: false, ignore: true };
}

impl Default for Pass {
    fn default() -> Self {
        Pass::ANYONE
    }
}

impl Barrier {
    /// Whether this barrier stops `pass` going from `p` to `q` (metres).
    pub fn stops(&self, pass: Pass, p: [f32; 3], q: [f32; 3]) -> bool {
        if !self.shut || pass.ignore || pass.faction == self.owner || (pass.player && !self.locked) {
            return false;
        }
        let y = p[1].min(q[1]);
        if y > self.hi + 1.0 || p[1].max(q[1]) < self.lo - 1.0 {
            return false;
        }
        let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
        let (p2, q2) = ([p[0], p[2]], [q[0], q[2]]);
        let (d1, d2) = (cross(self.a, self.b, p2), cross(self.a, self.b, q2));
        let (d3, d4) = (cross(p2, q2, self.a), cross(p2, q2, self.b));
        (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
    }
}

/// The navmesh manager's request queue (F032210 submit, F032211 cancel, F032235 update): requests
/// arrive on an incoming list, are merged into a list kept highest priority first (a request from
/// an owner already queued replaces that one; one below priority 1 that replaces nothing is
/// dropped), and one request is answered per update.
#[derive(Debug, Clone)]
pub struct PathManager<Owner> {
    incoming: Vec<PathRequest<Owner>>,
    pub queue: Vec<PathRequest<Owner>>,
    /// The request answered last.
    pub last: Option<PathRequest<Owner>>,
}

impl<Owner> Default for PathManager<Owner> {
    fn default() -> Self {
        PathManager {
            incoming: vec![],
            queue: vec![],
            last: None,
        }
    }
}

impl<Owner: PartialEq + Clone> PathManager<Owner> {
    /// @fn F032210
    pub fn submit(
        &mut self,
        owner: Owner,
        start: [f32; 3],
        goal: [f32; 3],
        radius: f32,
        water_cost: f32,
        urgent: bool,
    ) {
        self.submit_as(owner, start, goal, radius, water_cost, urgent, Pass::ANYONE);
    }

    /// A request for someone the shut gates may stop (`pass`).
    #[allow(clippy::too_many_arguments)]
    pub fn submit_as(&mut self, owner: Owner, start: [f32; 3], goal: [f32; 3], radius: f32, water_cost: f32, urgent: bool, pass: Pass) {
        self.incoming.push(PathRequest {
            owner,
            start,
            goal,
            radius,
            water_cost,
            priority: if urgent {
                PRIORITY_URGENT
            } else {
                PRIORITY_NORMAL
            },
            pass,
        });
    }

    /// @fn F032211
    pub fn cancel(&mut self, owner: Owner) {
        self.incoming.push(PathRequest {
            owner,
            start: [0.0; 3],
            goal: [0.0; 3],
            radius: 0.0,
            water_cost: 1.0,
            priority: -1,
            pass: Pass::ANYONE,
        });
    }

    /// @fn F032235
    ///
    /// Merge the incoming requests, then answer the first queued one. A cancel that replaced a
    /// queued request removes it (its answer would go to no one).
    pub fn update(&mut self, nav: &NavWorld) -> Option<(Owner, PathResult)> {
        for r in std::mem::take(&mut self.incoming) {
            if let Some(i) = self.queue.iter().position(|q| q.owner == r.owner) {
                if r.priority < 1 {
                    self.queue.remove(i);
                } else {
                    self.queue[i] = r;
                }
            } else if r.priority >= 1 {
                let at = self
                    .queue
                    .iter()
                    .position(|q| q.priority < r.priority)
                    .unwrap_or(self.queue.len());
                self.queue.insert(at, r);
            }
        }
        if self.queue.is_empty() {
            return None;
        }
        let r = self.queue.remove(0);
        let result = nav.find_path_as(r.start, r.goal, r.radius, r.water_cost, r.pass);
        let owner = r.owner.clone();
        self.last = Some(r);
        Some((owner, result))
    }
}

/// A stretch of edge a path crosses between two faces.
pub type Portal = ([f32; 3], [f32; 3]);

/// A found path: the string-pulled points and the gates to follow.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundPath {
    pub points: Vec<[f32; 3]>,
    pub follow: GatePath,
    /// The start's face.
    pub face: FaceRef,
}

/// What a path request came to (the request status F032235 writes at +0x90).
#[derive(Debug, Clone, PartialEq)]
pub enum PathResult {
    /// Points from start to goal, in metres.
    Found(FoundPath),
    /// No navmesh face within reach of the start (status 1).
    NoStart,
    /// No face for the goal (status 2).
    NoGoal,
    /// The goal's cluster can't be reached from the start's, or the search failed (status 3/4).
    Unreachable,
}

/// Search radius for the start face (F032235 passes 500 m to F032163).
pub const START_SEARCH: f32 = 500.0;
/// Search radius for the goal face before falling back to `START_SEARCH` (F032185, 30 m).
pub const GOAL_SEARCH: f32 = 30.0;
/// Margin added to the agent radius for the goal (F032235: radius + 0.05).
pub const GOAL_MARGIN: f32 = 0.05;
/// A* iteration cap (F032208).
pub const MAX_ITERATIONS: usize = 100_000;

/// The loaded navmesh tiles.
#[derive(Debug, Clone, Default)]
pub struct NavWorld {
    pub sections: HashMap<i64, Section>,
    /// The tiles loaded.
    pub tiles: std::collections::HashSet<(i32, i32)>,
    /// (stand-in for Havok's face tree) the faces by GRID-metre cell (x, z) they overlap, with
    /// their bounds: what closest_point searches.
    pub(crate) grid: HashMap<(i32, i32), Vec<GridFace>>,
    /// The navmesh cutters (`navcut`): what stands on the ground, and the ground sections as
    /// loaded (before cutting).
    pub(crate) cutters: Vec<crate::navcut::Cutter>,
    pub(crate) cutter_grid: HashMap<(i32, i32), Vec<usize>>,
    pub(crate) pristine: HashMap<i64, Section>,
    pub(crate) cut_keys: HashSet<(i64, i64)>,
    /// Each key's cutters (to take them away again: a building dismantled or destroyed).
    pub(crate) key_cutters: HashMap<(i64, i64), Vec<usize>>,
    /// The shut doors paths may not cross (gates; `Barrier`).
    pub barriers: Vec<Barrier>,
}

/// A face in the grid: it and its bounds (low, high).
pub(crate) type GridFace = (FaceRef, [f32; 3], [f32; 3]);

/// The face grid's cell side (metres).
pub(crate) const GRID: f32 = 16.0;

pub(crate) fn grid_cell(v: f32) -> i32 {
    (v / GRID).floor() as i32
}

#[derive(Clone, Copy, PartialEq)]
struct Open {
    f: f32,
    g: f32,
    face: FaceRef,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then_with(|| o.face.cmp(&self.face))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl NavWorld {
    /// Load the tiles within `reach` tiles of tile (`cx`, `cy`).
    pub fn load_around(
        install: &Path,
        cx: i32,
        cy: i32,
        reach: i32,
    ) -> Result<NavWorld, hkt::Error> {
        let mut w = NavWorld::default();
        for x in (cx - reach).max(0)..=(cx + reach).min(TILES - 1) {
            for y in (cy - reach).max(0)..=(cy + reach).min(TILES - 1) {
                w.ensure(install, x, y)?;
            }
        }
        Ok(w)
    }

    /// Load tile (`x`, `y`) unless it is loaded or off the map; whether it was loaded now.
    pub fn ensure(&mut self, install: &Path, x: i32, y: i32) -> Result<bool, hkt::Error> {
        if !(0..TILES).contains(&x) || !(0..TILES).contains(&y) || !self.tiles.insert((x, y)) {
            return Ok(false);
        }
        let mut ground = None;
        let mut own = Vec::new();
        for s in load_tile(install, x, y)? {
            self.index(&s);
            if s.info.is_none() {
                ground = Some(s.uid);
            } else {
                own.push(s.uid);
            }
            self.sections.insert(s.uid, s);
        }
        if let Some(uid) = ground {
            self.cut_section(uid);
        }
        // the buildings' own navmeshes, round the furniture already known to stand on them
        for uid in own {
            self.cut_section(uid);
        }
        Ok(true)
    }

    /// Whether face `f` is of a building's own navmesh (its floors, stairs and roofs), or of the
    /// pieces cut from one (`navcut`).
    pub fn on_building(&self, f: FaceRef) -> bool {
        self.sections.get(&f.section).is_some_and(|s| s.info.is_some())
            || self.sections.get(&f.section.wrapping_sub(crate::navcut::CUT_UID)).is_some_and(|s| s.info.is_some())
    }

    pub fn face(&self, f: FaceRef) -> &Face {
        &self.sections[&f.section].faces[f.face as usize]
    }

    /// A section's faces put in the grid.
    pub(crate) fn index(&mut self, t: &Section) {
        for fi in 0..t.faces.len() as u32 {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for c in t.corners(fi) {
                for k in 0..3 {
                    lo[k] = lo[k].min(c[k]);
                    hi[k] = hi[k].max(c[k]);
                }
            }
            if lo[0] > hi[0] {
                continue;
            }
            let f = FaceRef { section: t.uid, face: fi };
            for gx in grid_cell(lo[0])..=grid_cell(hi[0]) {
                for gz in grid_cell(lo[2])..=grid_cell(hi[2]) {
                    self.grid.entry((gx, gz)).or_default().push((f, lo, hi));
                }
            }
        }
    }

    /// Closest point on any face within `radius` of `p`.
    /// @fn F032163
    pub fn closest_point(&self, p: [f32; 3], radius: f32) -> Option<(FaceRef, [f32; 3])> {
        let mut best: Option<(f32, FaceRef, [f32; 3])> = None;
        for gx in grid_cell(p[0] - radius)..=grid_cell(p[0] + radius) {
            for gz in grid_cell(p[2] - radius)..=grid_cell(p[2] + radius) {
                for &(f, lo, hi) in self.grid.get(&(gx, gz)).into_iter().flatten() {
                    // the face's bounds no nearer than the best (or the radius): skipped
                    let near = std::array::from_fn::<f32, 3, _>(|k| p[k].clamp(lo[k], hi[k]));
                    let bound = distance(p, near);
                    if bound > radius || best.is_some_and(|b| bound >= b.0) {
                        continue;
                    }
                    let t = &self.sections[&f.section];
                    let c: Vec<[f32; 3]> = t.corners(f.face).collect();
                    for k in 1..c.len().saturating_sub(1) {
                        let q = closest_on_triangle(p, c[0], c[k], c[k + 1]);
                        let d = distance(p, q);
                        if d <= radius && best.is_none_or(|b| d < b.0) {
                            best = Some((d, f, q));
                        }
                    }
                }
            }
        }
        best.map(|(_, f, q)| (f, q))
    }

    /// Faces across an edge of `f`, with the shared stretch of the edge (the portal): the
    /// section's own pairing, or its streaming links into loaded sections.
    pub fn across(&self, f: FaceRef, edge: u32) -> Vec<(FaceRef, Portal)> {
        let t = &self.sections[&f.section];
        let e = t.edges[edge as usize];
        let (a, b) = self.edge_ends(f, edge);
        if e.opposite_face >= 0 {
            let g = FaceRef {
                section: f.section,
                face: e.opposite_face as u32,
            };
            return vec![(g, (a, b))];
        }
        let mut out = Vec::new();
        for &(other, face, oe) in t.links.get(&edge).into_iter().flatten() {
            // links into unloaded sections, or stale ones past the far section's end, lead nowhere
            let Some(o) = self.sections.get(&other) else {
                continue;
            };
            if face as usize >= o.faces.len() || oe as usize >= o.edges.len() {
                continue;
            }
            let g = FaceRef {
                section: other,
                face,
            };
            // the stretch of this edge the opposite edge covers
            let (c, d) = self.edge_ends(g, oe);
            let ab = sub(b, a);
            let l = dot(ab, ab);
            if l <= 0.0 {
                continue;
            }
            let (tc, td) = (dot(sub(c, a), ab) / l, dot(sub(d, a), ab) / l);
            let (lo, hi) = (tc.min(td).max(0.0), tc.max(td).min(1.0));
            if hi > lo {
                out.push((g, (lerp(a, b, lo), lerp(a, b, hi))));
            }
        }
        out
    }

    fn edge_ends(&self, f: FaceRef, edge: u32) -> Portal {
        let t = &self.sections[&f.section];
        let e = t.edges[edge as usize];
        (t.vertices[e.a as usize], t.vertices[e.b as usize])
    }

    /// An edge nothing passes through (no link into a face the edge filter lets in).
    fn wall(&self, f: FaceRef, edge: u32) -> bool {
        self.across(f, edge)
            .iter()
            .all(|(n, _)| !edge_enabled(self.face(*n).kind))
    }

    /// Whether an edge of `f` is a gap between walls: the face's edges on either side of it are
    /// both walls. (Replacement rule: Havok passes an edge by the clearance round it, not its
    /// length, so a short edge in open ground stays passable.)
    pub(crate) fn gap(&self, f: FaceRef, edge: u32) -> bool {
        let face = self.face(f);
        let (s, n) = (face.start_edge, face.num_edges);
        let i = edge - s;
        self.wall(f, s + (i + n - 1) % n) && self.wall(f, s + (i + 1) % n)
    }

    fn face_edges(&self, f: FaceRef) -> std::ops::Range<u32> {
        let face = self.face(f);
        face.start_edge..face.start_edge + face.num_edges
    }

    /// Whether `p` lies over face `f` (x, z), faces wound either way.
    pub fn is_over(&self, f: FaceRef, p: [f32; 3]) -> bool {
        self.over(f, p)
    }

    /// Whether `p` lies over face `f` (x, z), faces wound either way.
    fn over(&self, f: FaceRef, p: [f32; 3]) -> bool {
        let c: Vec<[f32; 3]> = self.sections[&f.section].corners(f.face).collect();
        let s: Vec<f32> = (0..c.len())
            .map(|i| side(c[i], c[(i + 1) % c.len()], p))
            .collect();
        s.iter().all(|&v| v >= -1e-4) || s.iter().all(|&v| v <= 1e-4)
    }

    /// Straight-line check (F032207): walk the faces the segment start -> goal crosses (x, z); every
    /// face entered must pass `enabled`, every crossed edge must fit the agent's diameter, and the
    /// walk must end over the goal.
    /// @fn F032207
    pub fn line_clear(
        &self,
        from: FaceRef,
        start: [f32; 3],
        goal: [f32; 3],
        radius: f32,
        enabled: fn(i32) -> bool,
    ) -> bool {
        let mut f = from;
        let mut came: Option<u32> = None;
        for _ in 0..4096 {
            if self.over(f, goal) {
                return true;
            }
            let mut next = None;
            for e in self.face_edges(f) {
                if Some(e) == came {
                    continue;
                }
                let (a, b) = self.edge_ends(f, e);
                // the segment leaves through this edge if a and b straddle it and start/goal straddle ab
                let (sa, sb) = (side(start, goal, a), side(start, goal, b));
                if sa * sb > 0.0 || side(a, b, start) * side(a, b, goal) > 0.0 {
                    continue;
                }
                next = Some(e);
                break;
            }
            let Some(e) = next else { return false };
            // where the segment crosses the edge, and the face on the far side there
            let (a, b) = self.edge_ends(f, e);
            let (sa, sb) = (side(start, goal, a), side(start, goal, b));
            let hit = lerp(a, b, if sa == sb { 0.5 } else { sa / (sa - sb) });
            let Some((g, (pa, pb))) = self.across(f, e).into_iter().min_by(|x, y| {
                let d = |p: &Portal| distance(hit, lerp(p.0, p.1, segment_t(hit, p.0, p.1)));
                d(&x.1).total_cmp(&d(&y.1))
            }) else {
                return false;
            };
            if distance(pa, pb) < radius * 2.0 || !enabled(self.face(g).kind) {
                return false;
            }
            came = self.face_edges(g).find(|&ge| {
                let (c, d) = self.edge_ends(g, ge);
                distance(hit, lerp(c, d, segment_t(hit, c, d))) < 1e-3
            });
            f = g;
        }
        false
    }

    /// The face under `to`, found by walking the faces from `face` (under `from`) along the segment
    /// (x, z), as a character tracks the face it stands on; `None` if the walk leaves the mesh.
    pub fn walk(&self, face: FaceRef, from: [f32; 3], to: [f32; 3]) -> Option<FaceRef> {
        let mut f = face;
        let mut came: Option<u32> = None;
        for _ in 0..256 {
            if self.over(f, to) {
                return Some(f);
            }
            let e = self.face_edges(f).find(|&e| {
                if Some(e) == came {
                    return false;
                }
                let (a, b) = self.edge_ends(f, e);
                side(from, to, a) * side(from, to, b) <= 0.0
                    && side(a, b, from) * side(a, b, to) <= 0.0
            })?;
            let (a, b) = self.edge_ends(f, e);
            let (sa, sb) = (side(from, to, a), side(from, to, b));
            let hit = lerp(a, b, if sa == sb { 0.5 } else { sa / (sa - sb) });
            let (g, _) = self.across(f, e).into_iter().min_by(|x, y| {
                let d = |p: &Portal| distance(hit, lerp(p.0, p.1, segment_t(hit, p.0, p.1)));
                d(&x.1).total_cmp(&d(&y.1))
            })?;
            came = self.face_edges(g).find(|&ge| {
                let (c, d) = self.edge_ends(g, ge);
                distance(hit, lerp(c, d, segment_t(hit, c, d))) < 1e-3
            });
            f = g;
        }
        None
    }

    /// (stand-in for the Havok character's collision with the walls) A step from `from` (over
    /// face `face`) toward `to`, as far as the mesh lets it: where the walk meets an edge nothing
    /// crosses, what is left of the step runs along that edge instead (once; a corner stops it).
    /// The face and point it ends on; None when it can't move at all.
    pub fn slide(&self, face: FaceRef, from: [f32; 3], to: [f32; 3]) -> Option<(FaceRef, [f32; 3])> {
        // a body standing just off its face (on the terrain) starts from the face's nearest point
        let from = if self.over(face, from) {
            from
        } else {
            let c: Vec<[f32; 3]> = self.sections[&face.section].corners(face.face).collect();
            let mid = c.iter().fold([0.0f32; 3], |m, q| add(m, scale(*q, 1.0 / c.len() as f32)));
            let (_, q) = self.closest_point(from, 5.0)?;
            lerp(q, mid, 0.01)
        };
        if let Some(g) = self.walk(face, from, to) {
            return Some((g, to));
        }
        let mut f = face;
        let mut came: Option<u32> = None;
        for _ in 0..256 {
            if self.over(f, to) {
                return Some((f, to));
            }
            let e = self.face_edges(f).find(|&e| {
                if Some(e) == came {
                    return false;
                }
                let (a, b) = self.edge_ends(f, e);
                side(from, to, a) * side(from, to, b) <= 0.0 && side(a, b, from) * side(a, b, to) <= 0.0
            })?;
            let (a, b) = self.edge_ends(f, e);
            let (sa, sb) = (side(from, to, a), side(from, to, b));
            let hit = lerp(a, b, if sa == sb { 0.5 } else { sa / (sa - sb) });
            let next = self.across(f, e).into_iter().min_by(|x, y| {
                let d = |p: &Portal| distance(hit, lerp(p.0, p.1, segment_t(hit, p.0, p.1)));
                d(&x.1).total_cmp(&d(&y.1))
            });
            match next {
                Some((g, _)) => {
                    came = self.face_edges(g).find(|&ge| {
                        let (c, d) = self.edge_ends(g, ge);
                        distance(hit, lerp(c, d, segment_t(hit, c, d))) < 1e-3
                    });
                    f = g;
                }
                None => {
                    // the wall: from just inside it, the rest of the step along it
                    let ab = [b[0] - a[0], 0.0, b[2] - a[2]];
                    let l = dot(ab, ab).sqrt();
                    if l <= 0.0 {
                        return None;
                    }
                    let dir = scale(ab, 1.0 / l);
                    let c: Vec<[f32; 3]> = self.sections[&f.section].corners(f.face).collect();
                    let mid = c.iter().fold([0.0f32; 3], |m, q| add(m, scale(*q, 1.0 / c.len() as f32)));
                    let start = lerp(hit, mid, 0.02);
                    let rest = [to[0] - hit[0], 0.0, to[2] - hit[2]];
                    let along = dot(rest, dir);
                    let slid = add(start, scale(dir, along));
                    return self.walk(f, start, slid).map(|g| (g, slid)).or(Some((f, start)));
                }
            }
        }
        None
    }

    /// Height of face `f` under `p` (x, z): interpolated on the triangle of the face's fan that
    /// holds `p`, else the closest point of the face.
    pub fn height_on(&self, f: FaceRef, p: [f32; 3]) -> f32 {
        let c: Vec<[f32; 3]> = self.sections[&f.section].corners(f.face).collect();
        for k in 1..c.len().saturating_sub(1) {
            let (a, b, d) = (c[0], c[k], c[k + 1]);
            let area = side(a, b, d);
            if area.abs() < 1e-9 {
                continue;
            }
            let (u, v) = (side(b, d, p) / area, side(d, a, p) / area);
            let w = 1.0 - u - v;
            if u >= -1e-4 && v >= -1e-4 && w >= -1e-4 {
                return u * a[1] + v * b[1] + w * d[1];
            }
        }
        (1..c.len().saturating_sub(1))
            .map(|k| closest_on_triangle([p[0], p[1], p[2]], c[0], c[k], c[k + 1]))
            .min_by(|x, y| {
                let d = |q: &[f32; 3]| (q[0] - p[0]).powi(2) + (q[2] - p[2]).powi(2);
                d(x).total_cmp(&d(y))
            })
            .map_or(p[1], |q| q[1])
    }

    /// Cluster reachability (F032192): is the goal face's cluster node reachable from the start's
    /// over the cluster graphs and their links?
    /// @fn F032192
    pub fn reachable(&self, from: FaceRef, to: FaceRef) -> bool {
        let start = (from.section, self.face(from).cluster as u32);
        let goal = (to.section, self.face(to).cluster as u32);
        let mut seen = HashSet::from([start]);
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            if n == goal {
                return true;
            }
            let Some(t) = self.sections.get(&n.0) else {
                continue;
            };
            let local = t
                .clusters
                .edges
                .get(n.1 as usize)
                .into_iter()
                .flatten()
                .map(|&(_, m)| (n.0, m));
            let links = t
                .clusters
                .links
                .iter()
                .filter(|l| l.0 == n.1)
                .map(|l| (l.1, l.2));
            for m in local.chain(links).collect::<Vec<_>>() {
                if self.sections.contains_key(&m.0) && seen.insert(m) {
                    stack.push(m);
                }
            }
        }
        false
    }

    /// A* over faces (F032208's search): steps cost the distance between crossing points (edge
    /// midpoints), modified by the entered face's type; gaps between walls narrower than the
    /// agent's diameter, and edges into faces the edge filter shuts, are skipped. Returns the crossed edges as portals.
    /// @fn F032208
    fn search(
        &self,
        from: FaceRef,
        start: [f32; 3],
        to: FaceRef,
        goal: [f32; 3],
        radius: f32,
        water_cost: f32,
        pass: Pass,
    ) -> Option<(Vec<Portal>, Vec<FaceRef>)> {
        let gated = !pass.ignore && self.barriers.iter().any(|b| b.shut);
        let mut open = BinaryHeap::new();
        let mut best: HashMap<FaceRef, f32> = HashMap::from([(from, 0.0)]);
        let mut point: HashMap<FaceRef, [f32; 3]> = HashMap::from([(from, start)]);
        let mut parent: HashMap<FaceRef, (FaceRef, Portal)> = HashMap::new();
        open.push(Open {
            f: distance(start, goal),
            g: 0.0,
            face: from,
        });
        let mut iterations = 0;
        while let Some(Open { g, face, .. }) = open.pop() {
            if face == to {
                let (mut portals, mut faces) = (Vec::new(), Vec::new());
                let mut f = to;
                while let Some(&(p, portal)) = parent.get(&f) {
                    portals.push(portal);
                    faces.push(f);
                    f = p;
                }
                portals.reverse();
                faces.reverse();
                return Some((portals, faces));
            }
            if g > best[&face] {
                continue;
            }
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                return None;
            }
            let here = point[&face];
            for e in self.face_edges(face) {
                for (next, (a, b)) in self.across(face, e) {
                    let kind = self.face(next).kind;
                    if !edge_enabled(kind) || (distance(a, b) < radius * 2.0 && self.gap(face, e)) {
                        continue;
                    }
                    let cross = if next == to { goal } else { lerp(a, b, 0.5) };
                    // a shut gate across the step
                    if gated && self.barred(pass, here, cross) {
                        continue;
                    }
                    let ng = g + modified_cost(kind, distance(here, cross), water_cost);
                    if best.get(&next).is_some_and(|&old| ng >= old) {
                        continue;
                    }
                    best.insert(next, ng);
                    point.insert(next, cross);
                    parent.insert(next, (face, (a, b)));
                    open.push(Open {
                        f: ng + distance(cross, goal),
                        g: ng,
                        face: next,
                    });
                }
            }
        }
        None
    }

    /// The path request steps of F032235: start face within 500 m, straight line if clear (water
    /// shut), goal face (30 m, else 500 m) kept a radius off the mesh boundary, cluster
    /// reachability, then the search.
    /// @fn F032235
    pub fn find_path(&self, start: [f32; 3], goal: [f32; 3], radius: f32, water_cost: f32) -> PathResult {
        self.find_path_as(start, goal, radius, water_cost, Pass::ANYONE)
    }

    /// Whether a shut gate stops `pass` going straight from `p` to `q` (metres).
    pub fn barred(&self, pass: Pass, p: [f32; 3], q: [f32; 3]) -> bool {
        !pass.ignore && self.barriers.iter().any(|b| b.stops(pass, p, q))
    }

    /// A path for `pass` (shut gates it may not cross left out).
    pub fn find_path_as(&self, start: [f32; 3], goal: [f32; 3], radius: f32, water_cost: f32, pass: Pass) -> PathResult {
        let Some((from, start)) = self.closest_point(start, START_SEARCH) else {
            return PathResult::NoStart;
        };
        let Some((to, goal)) = self
            .closest_point(goal, GOAL_SEARCH)
            .or_else(|| self.closest_point(goal, START_SEARCH))
        else {
            return PathResult::NoGoal;
        };
        if self.line_clear(from, start, goal, radius, water_edge_enabled) && !self.barred(pass, start, goal) {
            return PathResult::Found(FoundPath {
                points: vec![start, goal],
                follow: GatePath::straight(goal),
                face: from,
            });
        }
        let goal = self.off_boundary(to, goal, radius + GOAL_MARGIN);
        if !self.reachable(from, to) {
            return PathResult::Unreachable;
        }
        match self.search(from, start, to, goal, radius, water_cost, pass) {
            Some((portals, faces)) => PathResult::Found(FoundPath {
                points: funnel(start, goal, &portals, radius),
                follow: GatePath {
                    faces,
                    ..GatePath::new(start, goal, &portals, radius)
                },
                face: from,
            }),
            None => PathResult::Unreachable,
        }
    }

    /// (testing) Whether the face search finds a way.
    pub fn search_found(&self, from: FaceRef, start: [f32; 3], to: FaceRef, goal: [f32; 3]) -> bool {
        self.search(from, start, to, goal, 0.4, 3.0, Pass::ANYONE).is_some()
    }

    /// Push a goal out of `clearance` of the boundary edges of its face and the faces round it
    /// (F032185 collects the boundary edges within the radius and moves the goal clear of them).
    /// @fn F032185
    fn off_boundary(&self, f: FaceRef, p: [f32; 3], clearance: f32) -> [f32; 3] {
        let mut faces = vec![f];
        for e in self.face_edges(f) {
            faces.extend(self.across(f, e).into_iter().map(|(g, _)| g));
        }
        let mut q = p;
        for g in faces {
            for e in self.face_edges(g) {
                if self
                    .across(g, e)
                    .iter()
                    .any(|(n, _)| edge_enabled(self.face(*n).kind))
                {
                    continue;
                }
                let (a, b) = self.edge_ends(g, e);
                let c = lerp(a, b, segment_t(q, a, b));
                let d = [q[0] - c[0], 0.0, q[2] - c[2]];
                let l = dot(d, d).sqrt();
                if l < clearance {
                    // push away from the edge, inward if q sits on it
                    let n = if l > 1e-5 {
                        scale(d, 1.0 / l)
                    } else {
                        let along = sub(b, a);
                        let n = cross([0.0, 1.0, 0.0], along);
                        let centre = self.sections[&g.section]
                            .corners(g.face)
                            .fold([0.0; 3], add);
                        let toward = sub(scale(centre, 1.0 / self.face(g).num_edges as f32), a);
                        let n = if dot(n, toward) < 0.0 {
                            scale(n, -1.0)
                        } else {
                            n
                        };
                        let nl = dot(n, n).sqrt().max(1e-6);
                        scale(n, 1.0 / nl)
                    };
                    q = add(c, scale(n, clearance));
                    q[1] = p[1];
                }
            }
        }
        self.closest_point(q, clearance * 2.0).map_or(p, |(_, r)| r)
    }
}

/// Orient the crossed edges as gates (left, right) walking forward from `start` (x, z), each
/// narrowed by the agent radius at both ends.
pub fn gates(start: [f32; 3], portals: &[Portal], radius: f32) -> Vec<Portal> {
    let mut out = Vec::with_capacity(portals.len());
    let mut prev = start;
    for &(a, b) in portals {
        let len = distance(a, b);
        let t = if len > 0.0 {
            (radius / len).min(0.5)
        } else {
            0.5
        };
        let (a, b) = (lerp(a, b, t), lerp(a, b, 1.0 - t));
        let mid = lerp(a, b, 0.5);
        out.push(if side(prev, mid, a) > 0.0 {
            (a, b)
        } else {
            (b, a)
        });
        prev = mid;
    }
    out
}

/// String-pull from `start` through `gates` to `goal` (simple stupid funnel); stops once
/// `corners` corners are found (the goal counts as one).
pub fn pull(start: [f32; 3], goal: [f32; 3], gates: &[Portal], corners: usize) -> Vec<[f32; 3]> {
    let mut ports = gates.to_vec();
    ports.push((goal, goal));
    let mut path = vec![start];
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut li, mut ri) = (0usize, 0usize);
    let mut i = 0;
    while i < ports.len() && path.len() <= corners {
        let (pl, pr) = ports[i];
        // tighten the right side
        if side(apex, right, pr) >= 0.0 {
            if apex == right || side(apex, left, pr) < 0.0 {
                right = pr;
                ri = i;
            } else {
                path.push(left);
                apex = left;
                (right, ri) = (apex, li);
                i = li + 1;
                continue;
            }
        }
        // tighten the left side
        if side(apex, left, pl) <= 0.0 {
            if apex == left || side(apex, right, pl) > 0.0 {
                left = pl;
                li = i;
            } else {
                path.push(right);
                apex = right;
                (left, li) = (apex, ri);
                i = ri + 1;
                continue;
            }
        }
        i += 1;
    }
    if path.len() <= corners && path.last() != Some(&goal) {
        path.push(goal);
    }
    path
}

/// The whole string-pulled path through the crossed edges, kept a radius off their ends.
pub fn funnel(start: [f32; 3], goal: [f32; 3], portals: &[Portal], radius: f32) -> Vec<[f32; 3]> {
    pull(start, goal, &gates(start, portals, radius), usize::MAX)
}

/// A found path as the mover follows it (AICharacter's edge path): the gates still ahead and the
/// goal. Each tick the heading is pulled through the gates from where the character stands
/// (F024361), so a character pushed off the line steers back through the same gates.
/// How near a gate counts as through it (m).
pub const GATE_REACH: f32 = 0.3;

#[derive(Debug, Clone, PartialEq)]
pub struct GatePath {
    pub gates: Vec<Portal>,
    pub goal: [f32; 3],
    /// First gate not yet crossed (AICharacter +0x18).
    pub next: usize,
    /// The face each gate leads into (the search's corridor), so a follower crossing a gate is
    /// on that face even where floors overlap (stairs over a ground floor).
    pub faces: Vec<FaceRef>,
}

impl GatePath {
    pub fn new(start: [f32; 3], goal: [f32; 3], portals: &[Portal], radius: f32) -> Self {
        GatePath {
            gates: gates(start, portals, radius),
            goal,
            next: 0,
            faces: vec![],
        }
    }

    /// A straight path (the request's line check passed): no gates.
    pub fn straight(goal: [f32; 3]) -> Self {
        GatePath {
            gates: vec![],
            goal,
            next: 0,
            faces: vec![],
        }
    }

    /// No gates left: heading straight for the goal (the mover then brakes onto it).
    pub fn on_last_segment(&self) -> bool {
        self.next >= self.gates.len()
    }

    /// @fn F024361
    ///
    /// Where to head from `pos`: gates the character has crossed are dropped, then the first
    /// corner of the path pulled from `pos` through the rest (the goal once none are left).
    pub fn target(&mut self, pos: [f32; 3]) -> [f32; 3] {
        while let Some(&(l, r)) = self.gates.get(self.next) {
            // crossed, or standing on it: a path pulled tight round a corner puts that corner on
            // the gate line, where the side test alone never passes (replacement rule)
            let c = lerp(l, r, segment_t(pos, l, r));
            let near = (c[0] - pos[0]).hypot(c[2] - pos[2]) < GATE_REACH;
            if side(l, r, pos) > 0.0 || near {
                self.next += 1;
            } else {
                break;
            }
        }
        pull(pos, self.goal, &self.gates[self.next..], 1)
            .get(1)
            .copied()
            .unwrap_or(self.goal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_type_costs_and_filters() {
        assert_eq!(modified_cost(FACE_WATER, 10.0, 3.0), 30.0);
        assert_eq!(modified_cost(FACE_COSTLY, 10.0, 3.0), 15.0);
        assert_eq!(modified_cost(-1, 10.0, 3.0), 10.0);
        assert!(edge_enabled(FACE_WATER) && !edge_enabled(FACE_BLOCKED));
        assert!(
            !water_edge_enabled(FACE_WATER)
                && !water_edge_enabled(FACE_BLOCKED)
                && water_edge_enabled(1)
        );
    }

    #[test]
    fn tiles_cover_zone_cells() {
        assert_eq!(
            tile_file(30, 31),
            "data/newland/land/navtiles/tile30.31.hkt"
        );
        assert_eq!(tile_uid(34, 46), 11810);
        assert_eq!(tile_of([0.1, 0.0, -0.1]), (32, 31));
    }

    #[test]
    fn funnel_goes_straight_when_portals_allow() {
        let portals = [([5.0, 0.0, -5.0], [5.0, 0.0, 5.0])];
        let p = funnel([0.0; 3], [10.0, 0.0, 0.0], &portals, 0.5);
        assert_eq!(p, vec![[0.0; 3], [10.0, 0.0, 0.0]]);
    }

    #[test]
    fn gate_path_heads_for_the_corner_then_the_goal() {
        let portals = [([5.0, 0.0, 2.0], [5.0, 0.0, 6.0])];
        let mut p = GatePath::new([0.0; 3], [10.0, 0.0, 0.0], &portals, 0.5);
        let t = p.target([0.0; 3]);
        assert!(
            (t[0] - 5.0).abs() < 1e-5 && (t[2] - 2.5).abs() < 1e-5,
            "{t:?}"
        );
        assert!(!p.on_last_segment());
        // past the gate: straight for the goal
        assert_eq!(p.target([6.0, 0.0, 3.0]), [10.0, 0.0, 0.0]);
        assert!(p.on_last_segment());
    }

    #[test]
    fn requests_queue_by_priority_and_owner() {
        let mut m: PathManager<u32> = PathManager::default();
        let nav = NavWorld::default();
        m.submit(1, [0.0; 3], [1.0; 3], 0.4, 1.0, false);
        m.submit(2, [0.0; 3], [1.0; 3], 0.4, 1.0, false);
        m.submit(3, [0.0; 3], [1.0; 3], 0.4, 1.0, true);
        // owner 1 asks again: replaces its queued request in place
        m.submit(1, [5.0; 3], [1.0; 3], 0.4, 1.0, false);
        m.cancel(2);
        m.cancel(9);
        let order: Vec<u32> = std::iter::from_fn(|| m.update(&nav).map(|(o, _)| o)).collect();
        assert_eq!(order, vec![3, 1]);
    }

    #[test]
    fn funnel_bends_round_a_corner() {
        // a portal entirely to the left of the straight line forces a turn at its near end
        let portals = [([5.0, 0.0, 2.0], [5.0, 0.0, 6.0])];
        let p = funnel([0.0; 3], [10.0, 0.0, 0.0], &portals, 0.5);
        assert_eq!(p.len(), 3);
        assert!((p[1][2] - 2.5).abs() < 1e-5, "{p:?}");
    }
}

impl NavWorld {
    /// The nearest walkable face a ray meets (metres: origin, unit direction, length) at a
    /// point `accept` takes: its point, and the face. Blocked faces are passed through.
    pub fn raycast(&self, origin: [f32; 3], dir: [f32; 3], max: f32, accept: &dyn Fn([f32; 3]) -> bool) -> Option<([f32; 3], FaceRef)> {
        let mut best: Option<(f32, FaceRef)> = None;
        let mut seen: HashSet<FaceRef> = HashSet::new();
        let step = GRID * 0.5;
        let mut t = 0.0;
        while t <= max {
            let p = add(origin, scale(dir, t));
            for gx in grid_cell(p[0] - step)..=grid_cell(p[0] + step) {
                for gz in grid_cell(p[2] - step)..=grid_cell(p[2] + step) {
                    for &(f, _, _) in self.grid.get(&(gx, gz)).into_iter().flatten() {
                        if !seen.insert(f) || self.face(f).kind == FACE_BLOCKED {
                            continue;
                        }
                        let c: Vec<[f32; 3]> = self.sections[&f.section].corners(f.face).collect();
                        for k in 1..c.len().saturating_sub(1) {
                            if let Some(h) = ray_triangle(origin, dir, c[0], c[k], c[k + 1])
                                && h <= max
                                && best.is_none_or(|b| h < b.0)
                                && accept(add(origin, scale(dir, h)))
                            {
                                best = Some((h, f));
                            }
                        }
                    }
                }
            }
            // the nearest hit found before this stretch ends the walk
            if best.is_some_and(|b| b.0 < t - step) {
                break;
            }
            t += step;
        }
        best.map(|(h, f)| (add(origin, scale(dir, h)), f))
    }
}

/// Where a ray meets a triangle (either side), as a distance along it (Möller–Trumbore).
fn ray_triangle(o: [f32; 3], d: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Option<f32> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, a);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t > 0.0).then_some(t)
}
