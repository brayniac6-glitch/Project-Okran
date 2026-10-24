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
//! Kenshi hands the searches to Havok AI, an engine library replaced here rather than ported:
//! what Kenshi passes it and does round it is ported (the request steps, goal placement, the
//! line of sight it accepts, the cluster test, the edges it keeps and how its mover follows
//! them), and the engine's part follows Havok's documented behaviour: the query mediator's
//! closest point and vertical (bidirectional) ray, the clearance of a point (its distance to the
//! nearest boundary edge), an A* over faces that will not cross a stretch the agent's diameter
//! does not fit, costed by the distance between the points it crosses edges at (edge midpoints),
//! and a line of sight that keeps the agent's radius off the boundary. The path's points
//! (`FoundPath::points`, a funnel through the crossed stretches kept a radius off their ends)
//! are Havok's string pulling; Kenshi's mover does not follow them (F024361 works its own
//! heading from the edges).

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
/// How far off a face's edge (per metre out from the origin) a point still lies over the face:
/// four float epsilons, the rounding of a point placed on the edge.
const ON_EDGE: f32 = 4.0 * f32::EPSILON;

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
    /// The face the asker stood on (+0x38, setGoal F024356 from AICharacter +0x14).
    pub face: Option<FaceRef>,
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
        self.submit_from(owner, start, None, goal, radius, water_cost, urgent, pass);
    }

    /// A request from a mover standing on face `face` (setGoal F024356: +0x38).
    #[allow(clippy::too_many_arguments)]
    pub fn submit_from(&mut self, owner: Owner, start: [f32; 3], face: Option<FaceRef>, goal: [f32; 3], radius: f32, water_cost: f32, urgent: bool, pass: Pass) {
        self.incoming.push(PathRequest {
            face,
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
            face: None,
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
        let result = nav.find_path_from(r.face, r.start, r.goal, r.radius, r.water_cost, r.pass);
        let owner = r.owner.clone();
        self.last = Some(r);
        Some((owner, result))
    }
}

/// A stretch of edge a path crosses between two faces.
pub type Portal = ([f32; 3], [f32; 3]);

/// Distance between segments p-q and a-b on the ground plane (y ignored).
fn segment_distance(p: [f32; 3], q: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let flat = |v: [f32; 3]| [v[0], 0.0, v[2]];
    let (p, q, a, b) = (flat(p), flat(q), flat(a), flat(b));
    // crossing: 0
    let (s1, s2) = (side(p, q, a), side(p, q, b));
    let (s3, s4) = (side(a, b, p), side(a, b, q));
    if s1 * s2 < 0.0 && s3 * s4 < 0.0 {
        return 0.0;
    }
    let d = |x: [f32; 3], u: [f32; 3], v: [f32; 3]| distance(x, lerp(u, v, segment_t(x, u, v)));
    d(p, a, b).min(d(q, a, b)).min(d(a, p, q)).min(d(b, p, q))
}

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
/// The path search places the goal again (F032208 calls F032185 with radius + 0.1, 2 m).
pub const REQUEST_MARGIN: f32 = 0.1;
pub const REQUEST_SEARCH: f32 = 2.0;
/// A* iteration cap (F032208).
pub const MAX_ITERATIONS: usize = 100_000;

/// The loaded navmesh tiles.
#[derive(Debug, Clone, Default)]
pub struct NavWorld {
    pub sections: HashMap<i64, Section>,
    /// The tiles loaded.
    pub tiles: std::collections::HashSet<(i32, i32)>,
    /// The faces by GRID-metre cell (x, z) they overlap, with their bounds: what the query
    /// mediator's searches look through (Havok keeps a tree of them; the answers are the same).
    pub(crate) grid: HashMap<(i32, i32), GridCell, std::hash::BuildHasherDefault<CellHash>>,
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
    /// A number new each time the faces or their grid change (0: none loaded), so a caller may
    /// keep what `closest_point` found while it stays the same (`generation`).
    pub(crate) generation: u64,
    /// The clearances of crossed edges worked out so far (Havok's clearance cache, seeded per
    /// face as F032202 asks for them).
    pub(crate) clearances: ClearanceCache,
}

/// The faces' generations, unique across all navmeshes.
static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A face in the grid: it and its bounds (low, high).
pub(crate) type GridFace = (FaceRef, [f32; 3], [f32; 3]);

/// A grid cell: its faces, and bounds holding all those it has had (none nearer a point than
/// they are: `closest_point` passes a cell no face of which it would look at).
#[derive(Debug, Clone)]
pub(crate) struct GridCell {
    pub(crate) faces: Vec<GridFace>,
    pub(crate) lo: [f32; 3],
    pub(crate) hi: [f32; 3],
}

impl Default for GridCell {
    fn default() -> GridCell {
        GridCell { faces: Vec::new(), lo: [f32::MAX; 3], hi: [f32::MIN; 3] }
    }
}

impl<'a> IntoIterator for &'a GridCell {
    type Item = &'a GridFace;
    type IntoIter = std::slice::Iter<'a, GridFace>;
    fn into_iter(self) -> Self::IntoIter {
        self.faces.iter()
    }
}

/// The grid's hasher (FxHash's mix): its keys are small cell numbers, looked up thousands of
/// times a search; the map is only ever read by key.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CellHash(u64);

impl std::hash::Hasher for CellHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
    fn write_i32(&mut self, v: i32) {
        self.0 = (self.0.rotate_left(5) ^ v as u32 as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

/// The face grid's cell side (metres).
/// (testing) Wall sweeps taken, and those that ended stuck (F024365's logged case).
pub static SWEEPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static STUCK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// (testing) Path requests answered: found, and not (no start, no goal, unreachable).
pub static PATHS: [std::sync::atomic::AtomicUsize; 2] = [std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicUsize::new(0)];

/// What the clearance stand-in knows of a crossed stretch of edge (F032202's three values): the
/// clearance at each end, and where along it (0 at `a`, 1 at `b`) the most clearance lies with
/// how much there is, all capped at the radius they were worked out for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clearance {
    pub at_a: f32,
    pub at_b: f32,
    pub interp: f32,
    pub most: f32,
}

/// The clearances worked out so far, by (section, edge, face across, radius bits, the stretch's
/// ends' bits: one edge can meet a face along more than one stretch); emptied whenever the faces
/// change.
#[derive(Debug, Default)]
pub(crate) struct ClearanceCache(std::sync::Mutex<HashMap<(i64, u32, FaceRef, u32, [u32; 4]), Clearance>>);

impl Clone for ClearanceCache {
    fn clone(&self) -> Self {
        ClearanceCache(std::sync::Mutex::new(self.0.lock().map(|m| m.clone()).unwrap_or_default()))
    }
}

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
    /// The faces' generation: the same only while no face or grid cell has changed (what
    /// `closest_point` reads), so its answers may be kept for as long.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The faces or the grid are about to change: a new generation.
    pub(crate) fn touch(&mut self) {
        self.generation = NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(m) = self.clearances.0.get_mut() {
            m.clear();
        }
    }

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
        self.touch();
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
        self.touch();
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
                    let cell = self.grid.entry((gx, gz)).or_default();
                    cell.faces.push((f, lo, hi));
                    for k in 0..3 {
                        cell.lo[k] = cell.lo[k].min(lo[k]);
                        cell.hi[k] = cell.hi[k].max(hi[k]);
                    }
                }
            }
        }
    }

    /// @fn F032165
    ///
    /// Whether face key `f` names a face of the loaded mesh: its section loaded and the face's
    /// index within it.
    pub fn face_valid(&self, f: FaceRef) -> bool {
        self.sections.get(&f.section).is_some_and(|s| (f.face as usize) < s.faces.len())
    }

    /// Whether face `f` was taken out by the cutter: Havok's cutter leaves such a face empty in
    /// the mesh instance, where `navcut` shuts it in place (FACE_BLOCKED over a face that was
    /// open as loaded).
    pub fn face_cut(&self, f: FaceRef) -> bool {
        self.sections.get(&f.section).and_then(|s| s.faces.get(f.face as usize)).is_some_and(|face| face.kind == FACE_BLOCKED)
            && self.pristine.get(&f.section).and_then(|o| o.faces.get(f.face as usize)).is_some_and(|g| g.kind != FACE_BLOCKED)
    }

    /// @fn F032163
    ///
    /// NavMeshManager::closestFace: the face a ray from `p` up and down `range` (m) meets, the
    /// hit nearest `p` (the query mediator's castBidirectionalRay, vtable +0x40), with its height
    /// there.
    pub fn vertical_face(&self, p: [f32; 3], range: f32) -> Option<(FaceRef, f32)> {
        let mut best: Option<(f32, FaceRef, f32)> = None;
        for &(f, lo, hi) in self.grid.get(&(grid_cell(p[0]), grid_cell(p[2]))).into_iter().flatten() {
            if p[0] < lo[0] - 1e-4 || p[0] > hi[0] + 1e-4 || p[2] < lo[2] - 1e-4 || p[2] > hi[2] + 1e-4 || lo[1] > p[1] + range || hi[1] < p[1] - range {
                continue;
            }
            if !self.over(f, p) {
                continue;
            }
            let y = self.height_on(f, p);
            let d = (y - p[1]).abs();
            if d <= range && best.is_none_or(|b| d < b.0) {
                best = Some((d, f, y));
            }
        }
        best.map(|(_, f, y)| (f, y))
    }

    /// @fn F032166
    ///
    /// The face a mover at `p` stands on, as AICharacter keeps it (+0x14): the coherent
    /// bidirectional ray of `vertical_face` over `range`, the face it stood on (`hint`) taken
    /// first when the ray meets it; with its height there (the mover is set onto it).
    pub fn track_face(&self, p: [f32; 3], hint: Option<FaceRef>, range: f32) -> Option<(FaceRef, f32)> {
        if let Some(h) = hint
            && self.face_valid(h)
            && !self.face_cut(h)
            && self.over(h, p)
        {
            let y = self.height_on(h, p);
            if (y - p[1]).abs() <= range {
                return Some((h, y));
            }
        }
        self.vertical_face(p, range)
    }

    /// The query mediator's getClosestPoint (vtable +0x20, as F032290 asks it): the closest
    /// point on any face within `radius` of `p`.
    /// The query mediator's getClosestPoint over the faces overlapping the box of half extent
    /// `half` round `p` that `keep` lets through (a hit filter): the face holding the closest
    /// point of them, and the point.
    pub fn closest_in_box(&self, p: [f32; 3], half: f32, keep: &dyn Fn(FaceRef) -> bool) -> Option<(FaceRef, [f32; 3])> {
        let mut best: Option<(f32, FaceRef, [f32; 3])> = None;
        for gx in grid_cell(p[0] - half)..=grid_cell(p[0] + half) {
            for gz in grid_cell(p[2] - half)..=grid_cell(p[2] + half) {
                let Some(cell) = self.grid.get(&(gx, gz)) else { continue };
                for &(f, lo, hi) in &cell.faces {
                    if (0..3).any(|k| lo[k] > p[k] + half || hi[k] < p[k] - half) || !keep(f) {
                        continue;
                    }
                    let t = &self.sections[&f.section];
                    let mut c = t.corners(f.face);
                    let (Some(c0), Some(mut prev)) = (c.next(), c.next()) else { continue };
                    for cur in c {
                        let q = closest_on_triangle(p, c0, prev, cur);
                        let d = distance(p, q);
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, f, q));
                        }
                        prev = cur;
                    }
                }
            }
        }
        best.map(|(_, f, q)| (f, q))
    }

    pub fn closest_point(&self, p: [f32; 3], radius: f32) -> Option<(FaceRef, [f32; 3])> {
        let mut best: Option<(f32, FaceRef, [f32; 3])> = None;
        for gx in grid_cell(p[0] - radius)..=grid_cell(p[0] + radius) {
            for gz in grid_cell(p[2] - radius)..=grid_cell(p[2] + radius) {
                let Some(cell) = self.grid.get(&(gx, gz)) else { continue };
                // the cell's bounds no nearer than the best (or the radius): so too each of its
                // faces' (they lie within them), every one of which would be skipped below
                let near = std::array::from_fn::<f32, 3, _>(|k| p[k].clamp(cell.lo[k], cell.hi[k]));
                let bound = distance(p, near);
                if bound > radius || best.is_some_and(|b| bound >= b.0) {
                    continue;
                }
                for &(f, lo, hi) in &cell.faces {
                    // the face's bounds no nearer than the best (or the radius): skipped
                    let near = std::array::from_fn::<f32, 3, _>(|k| p[k].clamp(lo[k], hi[k]));
                    let bound = distance(p, near);
                    if bound > radius || best.is_some_and(|b| bound >= b.0) {
                        continue;
                    }
                    let t = &self.sections[&f.section];
                    // the face's fan of triangles (corner 0, k, k + 1)
                    let mut c = t.corners(f.face);
                    let (Some(c0), Some(mut prev)) = (c.next(), c.next()) else { continue };
                    for cur in c {
                        let q = closest_on_triangle(p, c0, prev, cur);
                        let d = distance(p, q);
                        if d <= radius && best.is_none_or(|b| d < b.0) {
                            best = Some((d, f, q));
                        }
                        prev = cur;
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

    pub(crate) fn edge_ends(&self, f: FaceRef, edge: u32) -> Portal {
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

    /// @fn F032202
    ///
    /// The clearances Havok's cache gives for the stretch `a` - `b` of edge `edge` of face `f`
    /// (crossed into face `next`), for a mover of `radius`: Havok's clearance of a point is its
    /// distance to the nearest boundary edge (an edge nothing passes through, reached across the
    /// mesh, within 1 m of its height), so a boundary vertex has none; each end's, and that of
    /// the point of the stretch with the most (where along it, 0 at `a`), on the ground plane and
    /// capped at the radius (Havok's ceiling: no caller asks beyond it). The point with the most
    /// is found to within a twentieth of a metre (Havok stores the values in bytes).
    pub fn clearance(&self, f: FaceRef, edge: u32, next: FaceRef, a: [f32; 3], b: [f32; 3], radius: f32) -> Clearance {
        let key = (f.section, edge, next, radius.to_bits(), [a[0].to_bits(), a[2].to_bits(), b[0].to_bits(), b[2].to_bits()]);
        if let Some(c) = self.clearances.0.lock().ok().and_then(|m| m.get(&key).copied()) {
            return c;
        }
        let mut walls = Vec::new();
        self.walls_along(f, a, b, radius, &mut walls);
        let flat = |v: [f32; 3]| [v[0], 0.0, v[2]];
        // walls more than 1 m above or below (another floor, a ramp's far end) do not count
        let at = |q: [f32; 3]| {
            let qf = flat(q);
            walls
                .iter()
                .filter_map(|&(wa, wb, _)| {
                    let (fa, fb) = (flat(wa), flat(wb));
                    let t = segment_t(qf, fa, fb);
                    ((wa[1] + (wb[1] - wa[1]) * t - q[1]).abs() <= 1.0).then(|| distance(qf, lerp(fa, fb, t)))
                })
                .fold(radius, f32::min)
        };
        let len = distance(flat(a), flat(b));
        let n = ((len / 0.05).ceil() as usize).clamp(1, 512);
        let mut best = (0.0f32, -1.0f32);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let c = at(lerp(a, b, t));
            if c > best.1 {
                best = (t, c);
                if c >= radius {
                    break;
                }
            }
        }
        // the best sample refined within a sample either side
        if best.1 < radius && n > 1 {
            let (mut lo, mut hi) = ((best.0 - 1.0 / n as f32).max(0.0), (best.0 + 1.0 / n as f32).min(1.0));
            for _ in 0..16 {
                let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                if at(lerp(a, b, m1)) < at(lerp(a, b, m2)) {
                    lo = m1;
                } else {
                    hi = m2;
                }
            }
            let t = (lo + hi) * 0.5;
            let c = at(lerp(a, b, t));
            if c > best.1 {
                best = (t, c);
            }
        }
        let c = Clearance { at_a: at(a), at_b: at(b), interp: best.0, most: best.1.max(0.0) };
        if let Ok(mut m) = self.clearances.0.lock() {
            m.insert(key, c);
        }
        c
    }

    pub(crate) fn face_edges(&self, f: FaceRef) -> std::ops::Range<u32> {
        let face = self.face(f);
        face.start_edge..face.start_edge + face.num_edges
    }

    /// Whether `p` lies over face `f` (x, z), faces wound either way.
    pub fn is_over(&self, f: FaceRef, p: [f32; 3]) -> bool {
        self.over(f, p)
    }

    /// Whether `p` lies over face `f` (x, z), faces wound either way: inside its outline (the
    /// meshes' faces are not all quite convex: some bend in by a few cm at a corner) or on an
    /// edge. One off an edge by no more than the rounding of a float at these coordinates (a few
    /// ulps, 2.5 mm some 5 km out) counts as on it, so the point getClosestPoint gives on a
    /// face's boundary (F032167, F024360) is found again by the vertical ray Kenshi casts from it.
    fn over(&self, f: FaceRef, p: [f32; 3]) -> bool {
        let c: Vec<[f32; 3]> = self.sections[&f.section].corners(f.face).collect();
        let tol = ON_EDGE * p[0].abs().max(p[2].abs()).max(1.0);
        let mut inside = false;
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            let ab = [b[0] - a[0], b[2] - a[2]];
            let ap = [p[0] - a[0], p[2] - a[2]];
            let ll = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if ll > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1]) / ll).clamp(0.0, 1.0) } else { 0.0 };
            if (ap[0] - ab[0] * t).hypot(ap[1] - ab[1] * t) <= tol {
                return true;
            }
            // the outline crossed by a ray from p toward +x
            if (a[2] > p[2]) != (b[2] > p[2]) && p[0] < a[0] + (p[2] - a[2]) / (b[2] - a[2]) * ab[0] {
                inside = !inside;
            }
        }
        inside
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
        self.line_end(from, start, goal, radius, enabled).is_some()
    }

    /// The face a clear straight line (as `line_clear` walks it) ends on.
    pub fn line_end(&self, from: FaceRef, start: [f32; 3], goal: [f32; 3], radius: f32, enabled: fn(i32) -> bool) -> Option<FaceRef> {
        let mut f = from;
        let mut came: Option<u32> = None;
        for _ in 0..4096 {
            if self.over(f, goal) {
                // Havok's line of sight with the agent's diameter (2 x radius): the line keeps the
                // radius off every wall
                let mut walls = Vec::new();
                self.walls_along(from, start, goal, radius, &mut walls);
                return walls.iter().all(|&(a, b, _)| segment_distance(start, goal, a, b) >= radius).then_some(f);
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
            let Some(e) = next else { return None };
            // where the segment crosses the edge, and the face on the far side there
            let (a, b) = self.edge_ends(f, e);
            let (sa, sb) = (side(start, goal, a), side(start, goal, b));
            let hit = lerp(a, b, if sa == sb { 0.5 } else { sa / (sa - sb) });
            let Some((g, (pa, pb))) = self.across(f, e).into_iter().min_by(|x, y| {
                let d = |p: &Portal| distance(hit, lerp(p.0, p.1, segment_t(hit, p.0, p.1)));
                d(&x.1).total_cmp(&d(&y.1))
            }) else {
                return None;
            };
            if distance(pa, pb) < radius * 2.0 || !enabled(self.face(g).kind) {
                return None;
            }
            came = self.face_edges(g).find(|&ge| {
                let (c, d) = self.edge_ends(g, ge);
                distance(hit, lerp(c, d, segment_t(hit, c, d))) < 1e-3
            });
            f = g;
        }
        None
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

    /// @fn F032187
    ///
    /// The walls near `p` (m), as NavMeshManager gathers them for a mover (F032181's boundary
    /// edges): the edges nothing passes through of the faces reached across passable edges from
    /// `face` within `reach` of it on the ground plane, each with its normal into the walkable
    /// side. Whether one of them has a face across it (an edge with its opposite: a shut face's,
    /// which the mesh keeps; Kenshi's +0x164, the mover then gathers them again each time).
    pub fn walls_near(&self, face: FaceRef, p: [f32; 3], reach: f32, out: &mut Vec<([f32; 3], [f32; 3], [f32; 3])>) -> bool {
        self.walls_along(face, p, p, reach, out)
    }

    /// The walls (as `walls_near` gathers them: ends at their heights, normal on the ground
    /// plane) within `reach` of the segment `p` - `q` on the ground plane; whether one has a
    /// face across it.
    pub fn walls_along(&self, face: FaceRef, p: [f32; 3], q: [f32; 3], reach: f32, out: &mut Vec<([f32; 3], [f32; 3], [f32; 3])>) -> bool {
        out.clear();
        let mut shared = false;
        let flat = |v: [f32; 3]| [v[0], 0.0, v[2]];
        let (pf, qf) = (flat(p), flat(q));
        let point = pf == qf;
        let mut seen: HashSet<FaceRef> = HashSet::new();
        let mut open = vec![face];
        seen.insert(face);
        while let Some(f) = open.pop() {
            if seen.len() > 512 {
                break;
            }
            let c: Vec<[f32; 3]> = self.sections[&f.section].corners(f.face).collect();
            let mid = scale(c.iter().fold([0.0; 3], |m, q| add(m, flat(*q))), 1.0 / c.len().max(1) as f32);
            for e in self.face_edges(f) {
                let (a3, b3) = self.edge_ends(f, e);
                let (a, b) = (flat(a3), flat(b3));
                let near = if point {
                    distance(pf, lerp(a, b, segment_t(pf, a, b)))
                } else {
                    segment_distance(pf, qf, a, b)
                };
                if near > reach {
                    continue;
                }
                if self.wall(f, e) {
                    let ab = sub(b, a);
                    let mut n = [ab[2], 0.0, -ab[0]];
                    let l = dot(n, n).sqrt();
                    if l <= 1e-9 {
                        continue;
                    }
                    n = scale(n, 1.0 / l);
                    if dot(n, sub(mid, a)) < 0.0 {
                        n = scale(n, -1.0);
                    }
                    out.push((a3, b3, n));
                    shared |= !self.across(f, e).is_empty();
                } else {
                    for (g, _) in self.across(f, e) {
                        if edge_enabled(self.face(g).kind) && seen.insert(g) {
                            open.push(g);
                        }
                    }
                }
            }
        }
        shared
    }

    /// @fn F024351
    ///
    /// AICharacter::sweepWalls: a circle of `radius` at `p` moved by `d` (m) against `walls`.
    /// Walls the move goes against only (normal . direction <= -1e-5). The circle's leading point
    /// (p less the radius along the wall's normal) runs along the move's line on the ground plane
    /// to where it meets the wall's line, at t: below -0.5 r / |d| (the circle that deep into the
    /// wall already) no touch, else at least 0 (an overlapping circle stops where it is). The
    /// meeting must lie within the radius (over its length) of the wall's ends, before the move's
    /// end (t < 1) and within 1 m of the wall's height; off the wall's ends the circle is met by
    /// the end point instead (a graze under 0.02 m deep no touch, a start already inside 0). The
    /// first touch: its t (1 when none), and the normal from the touch point to `p` on the
    /// ground plane with that distance.
    pub fn sweep_wall(p: [f32; 3], d: [f32; 3], radius: f32, walls: &[([f32; 3], [f32; 3], [f32; 3])]) -> (f32, Option<([f32; 3], f32)>) {
        let df = [d[0], 0.0, d[2]];
        let len = dot(df, df).sqrt();
        let mut best_t = 1.0f32;
        let mut touch: Option<[f32; 3]> = None;
        if len <= 0.0 {
            return (1.0, None);
        }
        let dir = scale(df, 1.0 / len);
        let back = scale(dir, -1.0);
        let r2 = radius * radius;
        let deepest = -(radius as f64 * 0.5 / dot(d, d).sqrt() as f64) as f32;
        for &(a, b, n) in walls {
            let e3 = sub(b, a);
            let e = [e3[0], 0.0, e3[2]];
            let el = dot(e, e).sqrt();
            if el <= 0.0 {
                continue;
            }
            let w = 1.0 / el;
            // Kenshi takes the normal from the wall's winding, (e.z, 0, -e.x) / |e|, which its
            // faces wind into the walkable side: walls_near turns it that way
            if dot(n, dir) > -1e-5 {
                continue;
            }
            let lead = [p[0] - radius * n[0], p[1], p[2] - radius * n[2]];
            let la = sub(lead, a);
            let (ee, ed, de, dd, d2) = (dot(e, e), dot(e, df), la[0] * e[0] + la[2] * e[2], la[0] * df[0] + la[2] * df[2], dot(df, df));
            let denom = d2 * ee - ed * ed;
            let mut t = if denom != 0.0 { (ed * de - dd * ee) / denom } else { 0.0 };
            if t < deepest {
                continue;
            }
            if t < 0.0 {
                t = 0.0;
            }
            let u = (t * ed + de) / ee;
            let height = (t * d[1] + p[1]) - (e3[1] * u + a[1]);
            if !(height.abs() <= 1.0 && -radius * w < u && u < radius * w + 1.0 && t < 1.0) {
                continue;
            }
            let (t, at) = if !(0.0..=1.0).contains(&u) {
                let c = if u >= 0.0 { b } else { a };
                let m = [c[0] - p[0], 0.0, c[2] - p[2]];
                let k = dot(m, back);
                let disc = k * k - (dot(m, m) - r2);
                let root = if disc > 0.0 { disc.sqrt() } else { 0.0 };
                let t = if root >= 0.02 {
                    let s = -k - root;
                    if s >= 0.0 { s / len } else { 0.0 }
                } else {
                    best_t
                };
                (t, c)
            } else {
                (t, add(a, scale(e3, u)))
            };
            if t < best_t {
                best_t = t;
                touch = Some(at);
            }
        }
        let Some(at) = touch else { return (1.0, None) };
        let v = [p[0] - at[0], 0.0, p[2] - at[2]];
        let l = dot(v, v).sqrt();
        let n = if l > 0.0 { scale(v, 1.0 / l) } else { [0.0; 3] };
        (best_t, Some((n, l)))
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
    ) -> Option<(Vec<Portal>, Vec<FaceRef>, Vec<(FaceRef, u32)>)> {
        self.search_with(from, start, to, goal, radius, water_cost, pass, true, edge_enabled)
    }

    /// The search with the edge filter `enabled` (F032309's EdgeFilter for paths; GateFilter
    /// F030396 for the gate codes' searches).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn search_with(
        &self,
        from: FaceRef,
        start: [f32; 3],
        to: FaceRef,
        goal: [f32; 3],
        radius: f32,
        water_cost: f32,
        pass: Pass,
        clear: bool,
        enabled: fn(i32) -> bool,
    ) -> Option<(Vec<Portal>, Vec<FaceRef>, Vec<(FaceRef, u32)>)> {
        // a face the cutter took out is empty: nothing leads out of it
        if self.face_cut(from) {
            return None;
        }
        let gated = !pass.ignore && self.barriers.iter().any(|b| b.shut);
        let mut open = BinaryHeap::new();
        let mut best: HashMap<FaceRef, f32> = HashMap::from([(from, 0.0)]);
        let mut point: HashMap<FaceRef, [f32; 3]> = HashMap::from([(from, start)]);
        let mut parent: HashMap<FaceRef, (FaceRef, Portal, u32)> = HashMap::new();
        open.push(Open {
            f: distance(start, goal),
            g: 0.0,
            face: from,
        });
        let mut iterations = 0;
        while let Some(Open { g, face, .. }) = open.pop() {
            if face == to {
                let (mut portals, mut faces, mut crossed) = (Vec::new(), Vec::new(), Vec::new());
                let mut f = to;
                while let Some(&(p, portal, e)) = parent.get(&f) {
                    portals.push(portal);
                    faces.push(f);
                    crossed.push((p, e));
                    f = p;
                }
                portals.reverse();
                faces.reverse();
                crossed.reverse();
                return Some((portals, faces, crossed));
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
                    if !enabled(kind) {
                        continue;
                    }
                    // Havok's clearance test for the agent's diameter (2 x radius): the crossed
                    // stretch must have a point the radius off every wall
                    if clear && self.clearance(face, e, next, a, b, radius).most < radius {
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
                    parent.insert(next, (face, (a, b), e));
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
        self.find_path_from(None, start, goal, radius, water_cost, pass)
    }

    /// A path from a mover on face `face` (the request's +0x38; checked still there, F032165).
    pub fn find_path_from(&self, face: Option<FaceRef>, start: [f32; 3], goal: [f32; 3], radius: f32, water_cost: f32, pass: Pass) -> PathResult {
        let r = self.find_path_inner(face, start, goal, radius, water_cost, pass);
        PATHS[!matches!(r, PathResult::Found(_)) as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        r
    }

    fn find_path_inner(&self, face: Option<FaceRef>, start: [f32; 3], goal: [f32; 3], radius: f32, water_cost: f32, pass: Pass) -> PathResult {
        let Some(from) = face.filter(|&f| self.face_valid(f)).or_else(|| self.vertical_face(start, START_SEARCH).map(|(f, _)| f)) else {
            return PathResult::NoStart;
        };
        let wanted = goal;
        // F032207: a clear line to the goal asked for; its end (on the mesh under it) within 1 m
        // of the goal, or the face straight above or below the goal (within 500 m, nearest)
        // the face it ends on, the goal then set onto it
        if let Some(end) = self.line_end(from, start, wanted, radius, water_edge_enabled)
            && !self.barred(pass, start, wanted)
        {
            let y = self.height_on(end, wanted);
            let g = if (y - wanted[1]).abs() < 1.0 {
                Some(wanted)
            } else {
                self.vertical_face(wanted, START_SEARCH).filter(|&(f, _)| f == end).map(|(_, y)| [wanted[0], y, wanted[2]])
            };
            if let Some(g) = g {
                return PathResult::Found(FoundPath {
                    points: vec![start, g],
                    follow: GatePath { start: Some(from), ..GatePath::straight(g) },
                    face: from,
                });
            }
        }
        // the goal kept off the boundary (F032185, radius + 0.05, within 30 m), then placed
        // again by the search itself (F032208: radius + 0.1, within 2 m)
        let Some((to, placed)) = self.place_goal(wanted, radius + GOAL_MARGIN, GOAL_SEARCH) else {
            return PathResult::NoGoal;
        };
        if !self.reachable(from, to) {
            return PathResult::Unreachable;
        }
        let Some((to, goal)) = self.place_goal(placed, radius + REQUEST_MARGIN, REQUEST_SEARCH) else {
            return PathResult::Unreachable;
        };
        match self.search(from, start, to, goal, radius, water_cost, pass) {
            Some((portals, faces, crossed)) => {
                // the path's edges with their clearances (F032204 / F032202)
                let edges = crossed.iter().zip(&faces).map(|(&(f, e), &g)| self.path_edge(f, e, g, radius)).collect();
                let follow = GatePath { faces, edges, start: Some(from), ..GatePath::new(start, goal, &portals, radius) };
                PathResult::Found(FoundPath { points: funnel(start, goal, &portals, radius), follow, face: from })
            }
            None => {
                PathResult::Unreachable
            }
        }
    }

    /// The edges across edge `edge` of face `f`: each with the face it bounds there (the
    /// section's own pairing, or its streaming links into loaded sections).
    fn twins(&self, f: FaceRef, edge: u32) -> Vec<(FaceRef, u32)> {
        let t = &self.sections[&f.section];
        let e = t.edges[edge as usize];
        if e.opposite_face >= 0 {
            return vec![(FaceRef { section: f.section, face: e.opposite_face as u32 }, e.opposite_edge.max(0) as u32)];
        }
        t.links
            .get(&edge)
            .into_iter()
            .flatten()
            .filter(|&&(other, face, oe)| self.sections.get(&other).is_some_and(|o| (face as usize) < o.faces.len() && (oe as usize) < o.edges.len()))
            .map(|&(other, face, oe)| (FaceRef { section: other, face }, oe))
            .collect()
    }

    /// Edge `edge` + `dir` of face `f`, wrapped round its edges (Havok's next edge round a face).
    fn step_round(&self, f: FaceRef, edge: u32, dir: i32) -> u32 {
        let face = self.face(f);
        let (s, n) = (face.start_edge as i64, face.num_edges as i64);
        let mut i = edge as i64 + dir as i64;
        if i < s {
            i += n;
        } else if i >= s + n {
            i -= n;
        }
        i as u32
    }

    /// The stretch of edge `edge` of face `f` shared with face `g`.
    pub fn stretch(&self, f: FaceRef, edge: u32, g: FaceRef) -> Option<Portal> {
        self.across(f, edge).into_iter().find(|(n, _)| *n == g).map(|(_, p)| p)
    }

    /// @fn F032172
    ///
    /// Whether path edge `e2` follows `e1` round a vertex on the `dir` side: in the face `e1`
    /// leads into, the edge next to `e2`'s (by `dir`) is `e1`'s twin.
    pub fn next_round(&self, e1: &PathEdge, e2: &PathEdge, dir: i32) -> bool {
        let f = e1.face;
        if !self.sections.get(&f.section).is_some_and(|s| (f.face as usize) < s.faces.len()) {
            return false;
        }
        let face = self.face(f);
        if e2.edge < face.start_edge || e2.edge >= face.start_edge + face.num_edges {
            return false;
        }
        let next = self.step_round(f, e2.edge, dir);
        self.twins(f, next).iter().any(|&(g, ge)| g.section == e1.from.section && ge == e1.edge)
    }

    /// @fn F032204
    ///
    /// The path edge for edge `edge` of face `from` into face `face`: its ends (F030504: the
    /// whole edge), and its clearances for a mover of `radius` (F032202; worked out a little
    /// past the radius + 0.5 that F024361 judges them by).
    pub fn path_edge(&self, from: FaceRef, edge: u32, face: FaceRef, radius: f32) -> PathEdge {
        let (a, b) = self.edge_ends(from, edge);
        let c = self.clearance(from, edge, face, a, b, radius + 0.6);
        PathEdge { a, b, face, from, edge, clear_a: c.at_a, clear_b: c.at_b, interp: c.interp }
    }

    /// @fn F032205
    ///
    /// The edges round a vertex the other way: from the edge next to edge `edge` of face `from`
    /// (by `dir`), across each and on to the next one round in the face beyond, until one leads
    /// into face `target`; as path edges for a mover of `radius` (F032204). None where the way
    /// round meets the mesh's edge.
    pub fn round_vertex(&self, from: FaceRef, edge: u32, dir: i32, target: FaceRef, radius: f32) -> Option<Vec<PathEdge>> {
        let mut f = from;
        let mut e = self.step_round(f, edge, dir);
        // the vertex turned round: the end the next edge shares
        let (ea, eb) = self.edge_ends(f, e);
        let v = if dir > 0 { ea } else { eb };
        let mut keys: Vec<(FaceRef, u32, FaceRef)> = Vec::new();
        // a fan has but a few faces; Kenshi walks until it meets the target (it would turn for
        // ever round a vertex whose fan never reaches it: here it gives up after 64)
        for _ in 0..64 {
            let twins = self.twins(f, e);
            let (g, ge) = *twins.iter().min_by(|x, y| {
                let d = |t: &(FaceRef, u32)| {
                    let (p, q) = self.edge_ends(t.0, t.1);
                    distance(v, lerp(p, q, segment_t(v, p, q)))
                };
                d(x).total_cmp(&d(y))
            })?;
            keys.push((f, e, g));
            if g == target {
                return Some(keys.into_iter().map(|(f, e, g)| self.path_edge(f, e, g, radius)).collect());
            }
            e = self.step_round(g, ge, dir);
            f = g;
        }
        None
    }

    /// @fn F032186
    ///
    /// NavMeshManager::isStuck: whether a mover of `radius` at `pos` is stuck there. Kenshi
    /// places the point as a goal (F032185 with the mover's radius, its face within 1 m: none,
    /// not stuck), then runs Havok's findNearestFeature from it with the agent's diameter and an
    /// HkStuckCallback (F032380) that takes only edges with both ends within `range` of the
    /// mover, and calls it stuck when that finds nothing. Havok's search is read here as the
    /// flood of the faces the diameter reaches through those edges: stuck when none of them has
    /// such an edge leading out of the range.
    pub fn stuck(&self, pos: [f32; 3], radius: f32, range: f32) -> bool {
        let Some((f, _)) = self.place_goal(pos, radius, 1.0) else { return false };
        let r2 = range * range;
        let within = |v: [f32; 3]| dist2(v, pos) < r2;
        let mut seen: HashSet<FaceRef> = HashSet::from([f]);
        let mut open = vec![f];
        while let Some(g) = open.pop() {
            if seen.len() > 2048 {
                return false;
            }
            for e in self.face_edges(g) {
                let (ea, eb) = self.edge_ends(g, e);
                for (n, (a, b)) in self.across(g, e) {
                    if !edge_enabled(self.face(n).kind) || self.clearance(g, e, n, a, b, radius).most < radius {
                        continue;
                    }
                    if !(within(ea) && within(eb)) {
                        return false;
                    }
                    if seen.insert(n) {
                        open.push(n);
                    }
                }
            }
        }
        true
    }

    /// (testing) Whether the face search finds a way.
    pub fn search_found(&self, from: FaceRef, start: [f32; 3], to: FaceRef, goal: [f32; 3]) -> bool {
        self.search(from, start, to, goal, 0.4, 3.0, Pass::ANYONE).is_some()
    }

    /// @fn F032181
    ///
    /// The boundary edges within `radius` of `p` (on the ground plane) round face `f`: from `f`,
    /// every edge within the radius that leads into a face not blocked leads on to that face; one
    /// leading nowhere (or only into blocked faces) is collected, with the face it bounds.
    fn boundary_near(&self, f: FaceRef, p: [f32; 3], radius: f32) -> Vec<(FaceRef, u32)> {
        let mut open = vec![f];
        let mut seen: HashSet<FaceRef> = HashSet::from([f]);
        let mut out = Vec::new();
        let mut i = 0;
        while i < open.len() && open.len() <= 4096 {
            let g = open[i];
            i += 1;
            for e in self.face_edges(g) {
                let (a, b) = self.edge_ends(g, e);
                let q = lerp(a, b, segment_t(p, a, b));
                if (q[0] - p[0]) * (q[0] - p[0]) + (q[2] - p[2]) * (q[2] - p[2]) >= radius * radius {
                    continue;
                }
                let next: Vec<FaceRef> = self
                    .across(g, e)
                    .into_iter()
                    .map(|(n, _)| n)
                    .filter(|n| edge_enabled(self.face(*n).kind))
                    .collect();
                if next.is_empty() {
                    out.push((g, e));
                }
                for n in next {
                    if seen.insert(n) {
                        open.push(n);
                    }
                }
            }
        }
        out
    }

    /// The unit normal of edge (a, b) of face `g` into the face (on the ground plane).
    fn inward(&self, g: FaceRef, a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        let e = sub(b, a);
        let mut n = [e[2], 0.0, -e[0]];
        let l = dot(n, n).sqrt();
        if l <= 1e-9 {
            return [0.0; 3];
        }
        n = scale(n, 1.0 / l);
        let c: Vec<[f32; 3]> = self.sections[&g.section].corners(g.face).collect();
        let mid = scale(c.iter().fold([0.0; 3], |m, q| add(m, *q)), 1.0 / c.len().max(1) as f32);
        if dot(n, sub(mid, a)) < 0.0 { scale(n, -1.0) } else { n }
    }

    /// @fn F032178
    ///
    /// Push `p` out to `radius` from each of `edges` in turn (on the ground plane): a point within
    /// the radius on the face's side, or off an edge's ends, goes out along the line from the
    /// edge's nearest point; one beyond the edge's line goes over to the face's side, the radius
    /// in; one right on the edge, the radius in along the edge's normal. How many it pushed.
    fn push_off(&self, p: &mut [f32; 3], radius: f32, edges: &[(FaceRef, u32)]) -> usize {
        let mut pushed = 0;
        for &(g, e) in edges {
            let (a, b) = self.edge_ends(g, e);
            let t = segment_t(*p, a, b);
            let q = lerp(a, b, t);
            let d = [p[0] - q[0], 0.0, p[2] - q[2]];
            let dist = dot(d, d).sqrt();
            if dist >= radius {
                continue;
            }
            let n = self.inward(g, a, b);
            let step = if dist == 0.0 {
                scale(n, radius)
            } else if dot(d, n) >= 0.0 || t <= 0.0 || t >= 1.0 {
                scale(d, (radius - dist) / dist)
            } else {
                scale(d, (-radius - dist) / dist)
            };
            *p = add(*p, step);
            pushed += 1;
        }
        pushed
    }

    /// @fn F032185
    ///
    /// NavMeshManager::placeGoal: a goal `goal` kept a `radius` off the navmesh boundary, with
    /// the face it ends on. First the closest point within `search` (the query mediator's
    /// getClosestPoint), else the face straight above or below within 500 m (F032163, the goal
    /// as given); none: no face. The boundary edges within the radius round it (F032181) push it
    /// out once (F032178); its face then the one straight above or below within 1 m (F032163;
    /// Kenshi passes `(1 or 500) != 0`, so 1 either way), else the closest point within the
    /// radius (it moves there); if it still has an edge within the radius less 0.01, the
    /// corners of the boundary edges within the radius + 2 m, nearest `goal` first, are tried
    /// in turn: each the closest point within `search`, pushed out three times from the
    /// boundary within the radius + 1 m, and taken (with its face) when the third push moves it
    /// no more and a ray 1 m up and down from it meets a face. None taken: Havok's search for a
    /// free spot (F064741, `free_spot`) moves it, and its face is the one straight above or
    /// below within 500 m.
    fn place_goal(&self, goal: [f32; 3], radius: f32, search: f32) -> Option<(FaceRef, [f32; 3])> {
        let (f, mut out) = match self.closest_point(goal, search) {
            Some(fq) => fq,
            None => (self.vertical_face(goal, START_SEARCH)?.0, goal),
        };
        let edges = self.boundary_near(f, out, radius);
        if edges.is_empty() {
            return Some((f, out));
        }
        self.push_off(&mut out, radius, &edges);
        let face = match self.vertical_face(out, 1.0) {
            Some((g, _)) => g,
            None => {
                let (g, q) = self.closest_point(out, radius)?;
                out = q;
                g
            }
        };
        if self.boundary_near(face, out, radius - 0.01).is_empty() {
            return Some((face, out));
        }
        let mut corners: Vec<[f32; 3]> = self
            .boundary_near(face, out, radius + 2.0)
            .into_iter()
            .flat_map(|(g, e)| {
                let (a, b) = self.edge_ends(g, e);
                [a, b]
            })
            .collect();
        let d2 = |q: &[f32; 3]| dot(sub(*q, goal), sub(*q, goal));
        corners.sort_by(|x, y| d2(x).total_cmp(&d2(y)));
        for c in corners {
            let Some((g, mut q)) = self.closest_point(c, search) else { continue };
            let near = self.boundary_near(g, q, radius + 1.0);
            self.push_off(&mut q, radius, &near);
            self.push_off(&mut q, radius, &near);
            if self.push_off(&mut q, radius, &near) == 0 && self.vertical_face(q, 1.0).is_some() {
                return Some((g, q));
            }
        }
        if let Some(q) = self.free_spot(out, radius) {
            if std::env::var("KENSHI_PATH_DEBUG").is_ok() {
                eprintln!("free spot for {:?}: {:?}", out.map(|v| (v * 10.0).round()), q.map(|v| (v * 10.0).round()));
            }
            out = q;
        }
        Some((self.vertical_face(out, START_SEARCH).map_or(face, |(g, _)| g), out))
    }

    /// @fn F064741
    ///
    /// Havok's search for a free spot near `p` for an agent of `radius` (as placeGoal asks it:
    /// boxes of half extent radius / 2, doubled each time, three tries, up to 2 m; the first
    /// found is taken): F066889 in each box.
    fn free_spot(&self, p: [f32; 3], radius: f32) -> Option<[f32; 3]> {
        let mut e = radius * 0.5;
        for _ in 0..3 {
            if let Some(q) = self.free_spot_in(p, e.min(2.0), radius) {
                return Some(q);
            }
            e += e;
        }
        None
    }

    /// @fn F066889
    ///
    /// One box of the free-spot search: the faces overlapping the box of half extent `e` round
    /// `p` (the DoorHitFilter: shut faces, data 5, left out), grouped into regions joined
    /// through their shared edges; each region's outline (the edges leading out of the region,
    /// on the ground plane) shrunk by `radius`, and the point of what is left nearest `p` put
    /// back on the region's faces; of the regions, the point nearest `p`. Havok shrinks the
    /// outline as polygons (F0dc8c50); here the shrunk region is exactly the points of its faces
    /// at least `radius` from its outline, the nearest found among `p` itself, the points
    /// `radius` off each outline edge and those `radius` from each corner, toward `p`.
    fn free_spot_in(&self, p: [f32; 3], e: f32, radius: f32) -> Option<[f32; 3]> {
        let mut set: Vec<FaceRef> = Vec::new();
        for gx in grid_cell(p[0] - e)..=grid_cell(p[0] + e) {
            for gz in grid_cell(p[2] - e)..=grid_cell(p[2] + e) {
                let Some(cell) = self.grid.get(&(gx, gz)) else { continue };
                for &(f, lo, hi) in &cell.faces {
                    if (0..3).any(|k| lo[k] > p[k] + e || hi[k] < p[k] - e) || self.face(f).kind == FACE_BLOCKED || set.contains(&f) {
                        continue;
                    }
                    set.push(f);
                }
            }
        }
        set.sort_by_key(|f| (f.section, f.face));
        let mut region = vec![usize::MAX; set.len()];
        let mut best: Option<(f32, [f32; 3])> = None;
        for start in 0..set.len() {
            if region[start] != usize::MAX {
                continue;
            }
            region[start] = start;
            let mut members = vec![set[start]];
            let mut i = 0;
            while i < members.len() {
                let f = members[i];
                i += 1;
                for e in self.face_edges(f) {
                    for (g, _) in self.across(f, e) {
                        if let Ok(k) = set.binary_search_by_key(&(g.section, g.face), |x| (x.section, x.face))
                            && region[k] == usize::MAX
                        {
                            region[k] = start;
                            members.push(g);
                        }
                    }
                }
            }
            // the region's outline: its faces' edges leading nowhere in it
            let mut outline: Vec<([f32; 3], [f32; 3])> = Vec::new();
            for &f in &members {
                for e in self.face_edges(f) {
                    if !self.across(f, e).iter().any(|(g, _)| members.contains(g)) {
                        outline.push(self.edge_ends(f, e));
                    }
                }
            }
            let flat = |v: [f32; 3]| [v[0], 0.0, v[2]];
            let pf = flat(p);
            let clear = |q: [f32; 3]| {
                outline.iter().all(|&(a, b)| {
                    let (a, b) = (flat(a), flat(b));
                    distance(q, lerp(a, b, segment_t(q, a, b))) >= radius - 1e-4
                })
            };
            let mut cands = vec![pf];
            for &(a, b) in &outline {
                let (a, b) = (flat(a), flat(b));
                let ab = sub(b, a);
                let l = dot(ab, ab).sqrt();
                if l > 1e-6 {
                    for n in [[ab[2] / l, 0.0, -ab[0] / l], [-ab[2] / l, 0.0, ab[0] / l]] {
                        let (oa, ob) = (add(a, scale(n, radius)), add(b, scale(n, radius)));
                        cands.push(lerp(oa, ob, segment_t(pf, oa, ob)));
                    }
                }
                for c in [a, b] {
                    let d = sub(pf, c);
                    let dl = dot(d, d).sqrt();
                    if dl > 1e-6 {
                        cands.push(add(c, scale(d, radius / dl)));
                    }
                }
            }
            for q in cands {
                if !clear(q) {
                    continue;
                }
                let Some(&f) = members.iter().find(|&&f| self.over(f, q)) else { continue };
                let d = distance(q, pf);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, [q[0], self.height_on(f, q), q[2]]));
                }
            }
        }
        best.map(|(_, q)| q)
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

/// A found path as the mover follows it (AICharacter's edge path, 0x40 bytes an edge): the edges
/// still ahead and the goal. Each tick the heading is worked out from where the character stands
/// (F024361), so a character pushed off the line steers back through the same edges.
#[derive(Debug, Clone, PartialEq)]
pub struct GatePath {
    /// Each crossed stretch as a gate (left, right) narrowed by the radius: what `funnel` pulls
    /// the path's points through (the fighters' corners and the debug output).
    pub gates: Vec<Portal>,
    /// +0x40
    pub goal: [f32; 3],
    /// The first edge not yet passed (+0x18).
    pub next: usize,
    /// The face each edge leads into (+0x20 of each edge).
    pub faces: Vec<FaceRef>,
    /// The edges (+0x0, +0x8 count).
    pub edges: Vec<PathEdge>,
    /// The mover's radius (+0x70) the path was found for.
    pub radius: f32,
    /// The face the request started from (+0x10).
    pub start: Option<FaceRef>,
}

/// A path edge as AICharacter keeps it (0x40 bytes, F032204): its ends as the face it leaves
/// winds them (+0x0, +0x10), the face it leads into (+0x20), its key (+0x24: the face it leaves
/// and its edge there), each end's clearance (+0x28, +0x2c) and where along it the most
/// clearance lies (+0x30).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathEdge {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub face: FaceRef,
    pub from: FaceRef,
    pub edge: u32,
    pub clear_a: f32,
    pub clear_b: f32,
    pub interp: f32,
}

/// What F024361 gives the step: the heading (unit, or zero), the way to the point the heading
/// is judged by (the corner or goal; for braking), and the edge it heads round (the edge count
/// when it heads for the goal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heading {
    pub dir: [f32; 3],
    pub to: [f32; 3],
    pub index: usize,
}

impl PathEdge {
    /// @fn F024383
    ///
    /// The first corner (+0x0) as a mover of `radius` at `pos` heads for it: when its clearance
    /// is under the radius and the most clearance lies past it (+0x30 > 0), moved by what it
    /// lacks along up x (corner - pos); else the corner itself.
    pub fn first(&self, pos: [f32; 3], radius: f32) -> [f32; 3] {
        if !(self.clear_a < radius && self.interp > 0.0) {
            return self.a;
        }
        let v = sub(self.a, pos);
        let n = [v[2], 0.0, -v[0]];
        let l = dot(n, n);
        if l <= 0.0 {
            return self.a;
        }
        add(self.a, scale(n, (radius - self.clear_a) / l.sqrt()))
    }

    /// @fn F024384
    ///
    /// The second corner (+0x10): when its clearance is under the radius and the most clearance
    /// lies before it (+0x30 < 1), moved by what it lacks along (corner - pos) x up.
    pub fn second(&self, pos: [f32; 3], radius: f32) -> [f32; 3] {
        if !(self.clear_b < radius && self.interp < 1.0) {
            return self.b;
        }
        let v = sub(self.b, pos);
        let n = [-v[2], 0.0, v[0]];
        let l = dot(n, n);
        if l <= 0.0 {
            return self.b;
        }
        add(self.b, scale(n, (radius - self.clear_b) / l.sqrt()))
    }
}

/// Two points the same to within 0.001 m each way (Kenshi's corner test).
fn same_point(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= 0.001)
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    dot(sub(a, b), sub(a, b))
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v);
    if l <= 0.0 { [0.0; 3] } else { scale(v, 1.0 / l.sqrt()) }
}

impl GatePath {
    pub fn new(start: [f32; 3], goal: [f32; 3], portals: &[Portal], radius: f32) -> Self {
        GatePath {
            gates: gates(start, portals, radius),
            goal,
            next: 0,
            faces: vec![],
            edges: vec![],
            radius,
            start: None,
        }
    }

    /// A straight path (the request's line check passed): no edges.
    pub fn straight(goal: [f32; 3]) -> Self {
        GatePath {
            gates: vec![],
            goal,
            next: 0,
            faces: vec![],
            edges: vec![],
            radius: 0.0,
            start: None,
        }
    }

    /// No edges left: heading straight for the goal.
    pub fn on_last_segment(&self) -> bool {
        self.next >= self.edges.len()
    }

    /// @fn F024361
    ///
    /// AICharacter::pathHeading: where a mover of `radius` at `pos` moving at `velocity` heads.
    /// No edges left: for the goal. Else a funnel from `pos` through the edges from the current
    /// one, their corners as F024383 / F024384 give them for the radius + 0.5: the first edge's
    /// two corners open it (when the second lies on the first's inward side), each following
    /// edge's first corner then second either tightens its side or, past the other side, ends
    /// it at whichever of that side's corner and the new one lies nearer the mover (by their
    /// raw corners); through all edges, the goal ends it unless it lies past a side (that
    /// side's corner). A first edge whose corners cross over: the end with more clearance
    /// (the nearer when both have none), judged by the farther raw corner. Heading round a
    /// corner, with `advance`, the run of following edges sharing that corner is passed over
    /// while its clearance is within the radius + 0.5 and the most clearance lies past it, and
    /// an open corner (more clearance, or none past it) has its fan of edges rebuilt round
    /// the vertex's other side (F024359). The way to the corner replaces the way to the point
    /// that ended the funnel when the corner lies farther along the velocity, or that point
    /// lies on the corner's side of it.
    pub fn heading(&mut self, pos: [f32; 3], velocity: [f32; 3], radius: f32, advance: bool, nav: Option<&NavWorld>) -> Heading {
        let lim = (radius as f64 + 0.5) as f32;
        let count = self.edges.len();
        let cur = self.next;
        if count <= cur {
            let d = sub(self.goal, pos);
            let dir = if (0..3).all(|k| d[k].abs() <= 0.001) { [0.0; 3] } else { unit(d) };
            return Heading { dir, to: d, index: count };
        }
        // up x v and v x up
        let left_of = |v: [f32; 3]| [v[2], 0.0, -v[0]];
        let right_of = |v: [f32; 3]| [-v[2], 0.0, v[0]];
        let e = self.edges[cur];
        let a = sub(e.first(pos, lim), pos);
        let mut d_a = dist2(pos, e.a);
        let mut n_a = left_of(a);
        let b = sub(e.second(pos, lim), pos);
        let mut d_b = dist2(pos, e.b);
        let mut n_b = right_of(b);
        let (mut left, mut right) = (cur, cur);
        // 1: round a first (left) corner, 2: a second (right) one
        let mut kind = 0u8;
        let mut to = [0.0; 3];
        if dot(n_a, b) > 0.0 {
            let mut j = cur + 1;
            let mut ended = false;
            while j < count {
                let ej = self.edges[j];
                let l = sub(ej.first(pos, lim), pos);
                let d_l = dist2(pos, ej.a);
                if dot(n_b, l) <= 0.0 {
                    if d_b <= d_l {
                        kind = 2;
                    } else {
                        kind = 1;
                        left = j;
                    }
                    to = l;
                    ended = true;
                    break;
                }
                if dot(n_a, l) > 0.0 {
                    left = j;
                    n_a = left_of(l);
                    d_a = d_l;
                }
                let r = sub(ej.second(pos, lim), pos);
                let d_r = dist2(pos, ej.b);
                if dot(n_a, r) <= 0.0 {
                    if d_a > d_r {
                        kind = 2;
                        right = j;
                    } else {
                        kind = 1;
                    }
                    to = r;
                    ended = true;
                    break;
                }
                if dot(n_b, r) > 0.0 {
                    n_b = right_of(r);
                    d_b = d_r;
                    right = j;
                }
                j += 1;
            }
            if !ended {
                let g = sub(self.goal, pos);
                if dot(n_b, g) <= 0.0 {
                    kind = 2;
                } else if dot(n_a, g) <= 0.0 {
                    kind = 1;
                }
                if kind == 0 {
                    return Heading { dir: unit(g), to: g, index: count };
                }
                to = g;
            }
        } else {
            kind = if e.clear_b > e.clear_a { 1 } else { 2 };
            if e.clear_a == 0.0 && e.clear_b == 0.0 {
                kind = if d_b > d_a { 1 } else { 2 };
            }
            to = sub(if d_a > d_b { e.a } else { e.b }, pos);
        }
        let (corner, index) = if kind == 1 {
            let p = self.edges[left].first(pos, lim);
            let mut k = left;
            if advance {
                loop {
                    let ek = self.edges[k];
                    if ek.clear_a > lim || ek.interp == 0.0 {
                        if let Some(nav) = nav {
                            self.rebuild_fan(nav, -1, k);
                        }
                        break;
                    }
                    if k + 1 >= self.edges.len() || !same_point(ek.a, self.edges[k + 1].a) {
                        break;
                    }
                    k += 1;
                }
            }
            (p, k)
        } else {
            let p = self.edges[right].second(pos, lim);
            let mut k = right;
            if advance {
                loop {
                    let ek = self.edges[k];
                    if ek.clear_b > lim || ek.interp == 1.0 {
                        if let Some(nav) = nav {
                            self.rebuild_fan(nav, 1, k);
                        }
                        break;
                    }
                    if k + 1 >= self.edges.len() || !same_point(ek.b, self.edges[k + 1].b) {
                        break;
                    }
                    k += 1;
                }
            }
            (p, k)
        };
        let dir = sub(corner, pos);
        if dot(dir, velocity) > dot(to, velocity) || ((dot(right_of(velocity), to) < 0.0) == (kind == 1)) {
            to = dir;
        }
        Heading { dir: unit(dir), to, index }
    }

    /// @fn F024347
    ///
    /// The edges passed: from the current one on, each the mover no longer stands strictly
    /// inside the face it leaves of (on the face's side of the edge's line) is passed, and the
    /// mover is on the face it leads into. The face it came to, if any.
    pub fn advance(&mut self, pos: [f32; 3]) -> Option<FaceRef> {
        let mut onto = None;
        while let Some(e) = self.edges.get(self.next) {
            let (a, b) = (e.a, e.b);
            if (b[2] - a[2]) * (pos[0] - a[0]) - (b[0] - a[0]) * (pos[2] - a[2]) > 0.0 {
                break;
            }
            onto = Some(e.face);
            self.next += 1;
        }
        onto
    }

    /// @fn F024359
    ///
    /// The fan of edges round the corner of edge `at` (its first end for `dir` -1, its second
    /// for 1): the run of path edges either side of it, from the current one, each next to the
    /// last round the same vertex (F032172), is swapped for the edges round the vertex's other
    /// side from the run's first edge to the face its last leads into (F032205); near the water
    /// line (the corner within 0.4 m of 10 m high) not when the new way crosses water and the
    /// old does not (F032164).
    pub fn rebuild_fan(&mut self, nav: &NavWorld, dir: i32, at: usize) {
        let corner = if dir < 1 { self.edges[at].a } else { self.edges[at].b };
        let mut last = at;
        while last + 1 < self.edges.len() && nav.next_round(&self.edges[last], &self.edges[last + 1], dir) {
            last += 1;
        }
        let mut first = at;
        while first > self.next && nav.next_round(&self.edges[first - 1], &self.edges[first], dir) {
            first -= 1;
        }
        let start = self.edges[first];
        let Some(new) = nav.round_vertex(start.from, start.edge, dir, self.edges[last].face, self.radius) else {
            if std::env::var("KENSHI_PATH_DEBUG").is_ok() {
                eprintln!("no way round the corner at {:?}", corner.map(|v| (v * 10.0).round()));
            }
            return;
        };
        if ((corner[1] * 10.0 - 100.0) as f64).abs() < 4.0 {
            let water = |edges: &[PathEdge]| edges.iter().any(|e| nav.face(e.face).kind == FACE_WATER);
            let old = water(&self.edges[first..last.min(self.edges.len())]);
            if water(&new) && !old {
                return;
            }
        }
        let portals: Vec<Portal> = new.iter().map(|e| nav.stretch(e.from, e.edge, e.face).unwrap_or((e.a, e.b))).collect();
        let prev = if first == 0 { None } else { self.gates.get(first - 1).map(|g| lerp(g.0, g.1, 0.5)) };
        self.gates.splice(first..=last, gates(prev.unwrap_or(corner), &portals, self.radius));
        self.faces.splice(first..=last, new.iter().map(|e| e.face));
        self.edges.splice(first..=last, new);
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
        // one edge at x = 5 from z = 6 to z = 2 (as the face west of it winds it), the goal
        // past it at (10, 0, 0): the funnel from the origin turns round the edge's second end
        let face = FaceRef { section: 0, face: 1 };
        let edge = |clear: f32| PathEdge { a: [5.0, 0.0, 6.0], b: [5.0, 0.0, 2.0], face, from: FaceRef { section: 0, face: 0 }, edge: 0, clear_a: clear, clear_b: clear, interp: 0.5 };
        let mut p = GatePath { edges: vec![edge(10.0)], ..GatePath::new([0.0; 3], [10.0, 0.0, 0.0], &[([5.0, 0.0, 2.0], [5.0, 0.0, 6.0])], 0.4) };
        let h = p.heading([0.0; 3], [0.0; 3], 0.4, false, None);
        let l = (25.0f32 + 4.0).sqrt();
        assert!((h.dir[0] - 5.0 / l).abs() < 1e-5 && (h.dir[2] - 2.0 / l).abs() < 1e-5, "{h:?}");
        assert_eq!(h.index, 0);
        // a wall corner (no clearance): moved out by the radius + 0.5 square to the way to it
        p.edges[0] = edge(0.0);
        let h = p.heading([0.0; 3], [0.0; 3], 0.4, false, None);
        let c = [5.0 - 0.9 * 2.0 / l, 0.0, 2.0 + 0.9 * 5.0 / l];
        let cl = (c[0] * c[0] + c[2] * c[2]).sqrt();
        assert!((h.dir[0] - c[0] / cl).abs() < 1e-5 && (h.dir[2] - c[2] / cl).abs() < 1e-5, "{h:?}");
        assert!(!p.on_last_segment());
        // past the edge (F024347): onto its face, straight for the goal
        assert_eq!(p.advance([6.0, 0.0, 3.0]), Some(face));
        assert!(p.on_last_segment());
        let h = p.heading([6.0, 0.0, 3.0], [0.0; 3], 0.4, false, None);
        assert_eq!(h.index, 1);
        let g = [4.0f32, 0.0, -3.0];
        assert!((h.dir[0] - g[0] / 5.0).abs() < 1e-5 && (h.dir[2] - g[2] / 5.0).abs() < 1e-5);
    }

    #[test]
    fn sweep_wall_stops_overlapping_circles() {
        // a wall along x = 1 (z -5..5) wound so its normal is -x (walkable side)
        let walls = [([1.0, 0.0, 5.0], [1.0, 0.0, -5.0], [-1.0, 0.0, 0.0])];
        // 2 m toward it from x = 0, radius 0.4: touches at x = 0.6 (t 0.3), normal back to the start
        let (t, hit) = NavWorld::sweep_wall([0.0; 3], [2.0, 0.0, 0.0], 0.4, &walls);
        assert!((t - 0.3).abs() < 1e-4, "{t}");
        let (n, w) = hit.unwrap();
        assert!((n[0] + 1.0).abs() < 1e-4 && (w - 1.0).abs() < 1e-4);
        // already within the radius (0.25 off it): stopped where it is
        assert_eq!(NavWorld::sweep_wall([0.75, 0.0, 0.0], [0.5, 0.0, 0.0], 0.4, &walls).0, 0.0);
        // deeper in than half the radius head on: passes
        assert_eq!(NavWorld::sweep_wall([0.85, 0.0, 0.0], [0.5, 0.0, 0.0], 0.4, &walls).0, 1.0);
        // along it or away: no touch; a wall more than 1 m above: none
        assert_eq!(NavWorld::sweep_wall([0.0; 3], [0.0, 0.0, 2.0], 0.4, &walls).0, 1.0);
        assert_eq!(NavWorld::sweep_wall([0.0; 3], [-2.0, 0.0, 0.0], 0.4, &walls).0, 1.0);
        assert_eq!(NavWorld::sweep_wall([0.0, 1.5, 0.0], [2.0, 0.0, 0.0], 0.4, &walls).0, 1.0);
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
