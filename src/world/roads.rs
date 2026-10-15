//! Kenshi's road network (RoadManager, port_map `world`): the roads the level data lays between
//! junctions, the search for a way along them, and the walk along a found way. Movers crossing
//! the map take the roads: AbstractMovementBase (a squad away from the player, UnloadedPlatoon,
//! and the base of CharMovement) for a goal over 10000 units away (1000 for a character), when
//! the walk to the road nearest it and from the road nearest the goal, plus the way along the
//! roads times its road factor (1 - its faction's "road preference"), is no longer than the
//! straight way.
//!
//! Spec:
//! - Loading (RoadManager F034416, a road from its record F034415, F034402, junctions by id
//!   F034669): the level data's "4-roadlist" record lists the ROAD records to load (its instances
//!   by id, in id order); without one, every ROAD record. A ROAD record: "start" and "end"
//!   junction ids (the same id is the same junction), "length", "width", "weight", "hidden",
//!   and its instances "0", "1", ... as the road's markers: a position and the fraction of the
//!   road it lies at (the instance rotation's x), taken until the first at 1. A record with fewer
//!   than 2 instances is not a road, nor one with fewer than 2 markers or no length (F034405).
//!   Junction position: the end of its first road that is at it.
//! - The nearest road (F034386): the point of any road's segment nearest across (x, z) within the
//!   radius, scored by its distance less how far it lies along `bias`.
//! - The search (F034412, A* F034677 with F034675 / F034673): from the point on the first road to
//!   the point on the other, over junctions; the two points are temporary junctions joined to
//!   their road's ends by temporary roads a fraction of its length long (F034398), and the two
//!   roads themselves are left out; on the same road a third temporary road joins the points.
//!   A road costs its length ("weight" is not read); the estimate is the straight distance to the
//!   goal point.
//! - The way (RoadPath, F034394): the junctions between, the marker walked to next on the road
//!   it is on (+0x2c), which way along it (+0x40), the last marker on the goal's road (+0x28).
//!   setStart F034399, the walk F034400 (along a road a marker's distance is its share of the
//!   road's "length", not the straight line), next marker F034396, next road F034391 (the
//!   shortest road between the two junctions), on the road or not F034390, skip a marker F034401.
//! - AbstractMovementBase (ctor F040687, goTo F040645, the road decision F040644, update F040640):
//!   off the roads it goes straight at the goal, unchecked for ground or water.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::path::Path;

/// A road's marker (Road::Marker): a position and the fraction of the road it lies at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub position: [f32; 3],
    pub t: f32,
}

/// One road (Road, 0x68 bytes): its markers, the junctions it joins and its record's numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Road {
    /// The ROAD record's string id (+0x38).
    pub name: String,
    /// Junction indices (+0x18 start, +0x20 end).
    pub start: usize,
    pub end: usize,
    pub markers: Vec<Marker>,
    /// "length" (+0x28): what the search counts the road as, and the walk's distance scale.
    pub length: f32,
    /// "weight" (+0x2c), not read by the search.
    pub weight: f32,
    /// "width" (+0x30): within it of a marker the walk has reached it (F034399).
    pub width: f32,
    /// "hidden" (+0x34).
    pub hidden: bool,
}

/// A junction (a list of the roads at it); `id` is the records' junction id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Junction {
    pub id: i32,
    pub roads: Vec<usize>,
}

/// A point on a road (F034386's answer, 0x20 bytes): the road, the segment (markers `segment`
/// and `segment + 1`), the point, its distance across and its fraction of the road.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadPoint {
    pub road: usize,
    pub segment: usize,
    pub point: [f32; 3],
    pub distance: f32,
    pub t: f32,
}

/// The road network (RoadManager): roads in load order and their junctions.
#[derive(Debug, Clone, Default)]
pub struct RoadNetwork {
    pub roads: Vec<Road>,
    pub junctions: Vec<Junction>,
}

/// RoadPath's state (+0x18).
pub const ROAD_NONE: i32 = 0;
/// Walking to the next marker from off the road.
pub const ROAD_TO_ROAD: i32 = 1;
/// On the road.
pub const ROAD_ON_ROAD: i32 = 2;
/// At the way's last marker.
pub const ROAD_DONE: i32 = 3;
/// No way was found ("Road path failed").
pub const ROAD_FAILED: i32 = 4;

/// Radius of the nearest-road searches of AbstractMovementBase's road decision (F040644).
pub const ROAD_SEARCH: f32 = 20_000.0;
/// AbstractMovementBase::goTo (F040645) tries the roads for a goal further than this (squared,
/// 10000 units); CharMovement::goTo (F040653) from 1000 (squared 1e6).
pub const ROAD_GOAL_FAR: f32 = 1.0e8;
pub const ROAD_GOAL_FAR_CHARACTER: f32 = 1.0e6;

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
fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    dot(sub(a, b), sub(a, b)).sqrt()
}
fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    dot(sub(a, b), sub(a, b))
}
/// Ogre's Vector3::normalise: unchanged when shorter than 1e-8.
fn normalise(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 1e-8 { scale(a, 1.0 / l) } else { a }
}

/// The fraction along a->b of the point nearest p, clamped to the segment (a zero-length
/// segment gives 0, as the original's NaN does).
fn along(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
    let ab = sub(b, a);
    let u = dot(sub(p, a), ab) / dot(ab, ab);
    if u >= 0.0 {
        if 1.0 < u { 1.0 } else { u }
    } else {
        0.0
    }
}

/// F034377: the squared distance across (x, z) from p to the segment a-b.
/// @fn F034377
pub fn segment_dist2(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
    let q = add(a, scale(sub(b, a), along(a, b, p)));
    (p[2] - q[2]) * (p[2] - q[2]) + (p[0] - q[0]) * (p[0] - q[0])
}

impl RoadNetwork {
    /// @fn F034416
    ///
    /// The network from the merged level data (`crate::read_level`): the roads "4-roadlist" lists
    /// (by instance id, in id order: the record's instance map), else every ROAD record in file
    /// order. A listed road with no record is left out.
    pub fn from_level(level: &kenshi_io::fcs::File) -> RoadNetwork {
        const ROAD: i32 = 53;
        let mut net = RoadNetwork::default();
        let mut ids: HashMap<i32, usize> = HashMap::new();
        let by_id: HashMap<&str, &kenshi_io::fcs::Record> =
            level.records.iter().filter(|r| r.kind == ROAD).map(|r| (r.string_id.as_str(), r)).collect();
        match level.records.iter().find(|r| r.string_id == "4-roadlist") {
            Some(list) => {
                let mut inst: Vec<&kenshi_io::fcs::Instance> = list.instances.iter().collect();
                inst.sort_by(|a, b| a.id.cmp(&b.id));
                for i in inst {
                    if let Some(r) = by_id.get(i.target.as_str()) {
                        net.add_road(&i.id, r, &mut ids);
                    }
                }
            }
            None => {
                for r in level.records.iter().filter(|r| r.kind == ROAD) {
                    net.add_road(&r.string_id, r, &mut ids);
                }
            }
        }
        net
    }

    /// The network of an install: its level files merged in folder order (`crate::read_level`).
    pub fn load(install: &Path) -> RoadNetwork {
        crate::read_level(install).map(|l| Self::from_level(&l)).unwrap_or_default()
    }

    /// @fn F034669
    ///
    /// The junction of an id, made the first time it is asked for.
    fn junction(&mut self, id: i32, ids: &mut HashMap<i32, usize>) -> usize {
        *ids.entry(id).or_insert_with(|| {
            self.junctions.push(Junction { id, roads: Vec::new() });
            self.junctions.len() - 1
        })
    }

    /// @fn F034415
    ///
    /// A road from its record: the junctions of "start" and "end", the numbers, the markers
    /// "0", "1", ... up to the first at fraction 1. Dropped (F034405) with fewer than 2 markers or
    /// no length; a record with fewer than 2 instances makes none (its junctions are not made).
    fn add_road(&mut self, name: &str, r: &kenshi_io::fcs::Record, ids: &mut HashMap<i32, usize>) {
        if r.instances.len() < 2 {
            return;
        }
        let int = |k: &str| r.ints.iter().find(|(n, _)| n == k).map_or(0, |(_, v)| *v);
        let float = |k: &str| r.floats.iter().find(|(n, _)| n == k).map_or(0.0, |(_, v)| *v);
        let start = self.junction(int("start"), ids);
        let end = self.junction(int("end"), ids);
        let mut markers = Vec::new();
        for i in 0.. {
            let key = i.to_string();
            let Some(m) = r.instances.iter().find(|x| x.id == key) else { break };
            let t = m.rotation[1];
            markers.push(Marker { position: m.position, t });
            if t >= 1.0 {
                break;
            }
        }
        let road = Road {
            name: name.to_string(),
            start,
            end,
            markers,
            length: float("length"),
            weight: float("weight"),
            width: float("width"),
            hidden: r.bools.iter().find(|(n, _)| n == "hidden").is_some_and(|(_, v)| *v),
        };
        if road.markers.len() < 2 || road.length <= 0.0 {
            return;
        }
        let k = self.roads.len();
        self.roads.push(road);
        // F034402: on the start junction's list, then the end's (twice on a loop)
        self.junctions[start].roads.push(k);
        self.junctions[end].roads.push(k);
    }

    /// A junction's position: the end of its first road that lies at it (F034677, F034398).
    pub fn junction_position(&self, j: usize) -> [f32; 3] {
        let r = &self.roads[self.junctions[j].roads[0]];
        if r.start == j { r.markers[0].position } else { r.markers[r.markers.len() - 1].position }
    }

    /// @fn F034386
    ///
    /// The point of any road nearest `p` across (x, z) within `radius`, scored by its distance
    /// less its offset from `p` along `bias` (the lowest score wins, ties to the first found).
    pub fn nearest(&self, p: [f32; 3], radius: f32, bias: [f32; 3]) -> Option<RoadPoint> {
        let mut best = f32::MAX;
        let mut out = None;
        for (k, road) in self.roads.iter().enumerate() {
            if road.markers.len() == 1 {
                continue;
            }
            for i in 0..road.markers.len() - 1 {
                let (a, b) = (road.markers[i], road.markers[i + 1]);
                let u = along(a.position, b.position, p);
                let q = add(a.position, scale(sub(b.position, a.position), u));
                let d2 = (p[2] - q[2]) * (p[2] - q[2]) + (p[0] - q[0]) * (p[0] - q[0]);
                if d2 < radius * radius {
                    let d = d2.sqrt();
                    let s = d - dot(bias, sub(q, p));
                    if s < best {
                        best = s;
                        out = Some(RoadPoint { road: k, segment: i, point: q, distance: d, t: (b.t - a.t) * u + a.t });
                    }
                }
            }
        }
        out
    }

    /// @fn F034412
    ///
    /// The way from point `a` to point `b` along the roads. On the same segment of the same road
    /// there is none (state 0); no way found: state 4.
    pub fn find(&self, a: &RoadPoint, b: &RoadPoint) -> RoadPath {
        let mut path = RoadPath::default();
        if a.road == b.road && a.segment == b.segment {
            return path;
        }
        let ra = &self.roads[a.road];
        let rb = &self.roads[b.road];
        // the temporary roads (F034398): (its start junction, the temporary junction, length,
        // the point); the temporary junctions' road lists keep them in this order
        let mut temp: Vec<Temp> = Vec::new();
        let extra_count;
        if a.road == b.road {
            if b.t <= a.t {
                temp.push(Temp { from: Node::J(ra.end), to: Node::Start, length: (1.0 - a.t) * ra.length });
                temp.push(Temp { from: Node::J(ra.start), to: Node::Goal, length: b.t * ra.length });
            } else {
                temp.push(Temp { from: Node::J(ra.start), to: Node::Start, length: a.t * ra.length });
                temp.push(Temp { from: Node::J(ra.end), to: Node::Goal, length: (1.0 - b.t) * ra.length });
            }
            temp.push(Temp { from: Node::Start, to: Node::Goal, length: (a.t - b.t).abs() * ra.length });
            extra_count = 2;
        } else {
            temp.push(Temp { from: Node::J(ra.start), to: Node::Start, length: a.t * ra.length });
            temp.push(Temp { from: Node::J(ra.end), to: Node::Start, length: (1.0 - a.t) * ra.length });
            temp.push(Temp { from: Node::J(rb.start), to: Node::Goal, length: b.t * rb.length });
            temp.push(Temp { from: Node::J(rb.end), to: Node::Goal, length: (1.0 - b.t) * rb.length });
            extra_count = 4;
        }
        // the temporary junctions' lists: the roads made onto them, the direct one on both
        let mut start_list: Vec<usize> = (0..temp.len()).filter(|&i| temp[i].to == Node::Start).collect();
        let goal_list: Vec<usize> = (0..temp.len()).filter(|&i| temp[i].to == Node::Goal).collect();
        if a.road == b.road {
            start_list.push(2);
        }
        // the extra edges, by the real junction they start from (at most 4 entries)
        let mut extra: Vec<(usize, Vec<usize>)> = Vec::new();
        for (i, t) in temp.iter().enumerate().take(extra_count) {
            let Node::J(j) = t.from else { continue };
            match extra.iter_mut().find(|e| e.0 == j) {
                Some(e) => e.1.push(i),
                None => extra.push((j, vec![i])),
            }
        }
        let graph = Search { net: self, temp: &temp, start_list, goal_list, extra, skip: (a.road, b.road), a: a.point, b: b.point };
        match graph.solve() {
            Some((nodes, cost)) => {
                // nodes run goal..start: the real junctions between, in walking order
                for n in nodes.iter().rev().skip(1).take(nodes.len().saturating_sub(2)) {
                    if let Node::J(j) = n {
                        path.junctions.push(*j);
                    }
                }
                path.marker = a.segment as i32;
                path.end_marker = b.segment as i32;
                if path.junctions.is_empty() {
                    if path.marker < path.end_marker {
                        path.marker += 1;
                    } else if path.end_marker < path.marker {
                        path.end_marker += 1;
                    }
                } else {
                    if ra.end == path.junctions[0] {
                        path.marker += 1;
                    }
                    if rb.end == path.junctions[path.junctions.len() - 1] {
                        path.end_marker += 1;
                    }
                }
                path.to_next = -1.0;
                path.cost = cost;
                path.end_road = b.road;
                path.road = a.road;
                path.dir = if a.segment as i32 == path.marker { -1 } else { 1 };
                path.state = ROAD_TO_ROAD;
            }
            None => path.state = ROAD_FAILED,
        }
        path
    }
}

/// A node of the search: a junction or one of the two temporary junctions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Node {
    J(usize),
    Start,
    Goal,
}

/// A temporary road of the search.
#[derive(Debug, Clone, Copy)]
struct Temp {
    from: Node,
    to: Node,
    length: f32,
}

struct Search<'a> {
    net: &'a RoadNetwork,
    temp: &'a [Temp],
    start_list: Vec<usize>,
    goal_list: Vec<usize>,
    extra: Vec<(usize, Vec<usize>)>,
    skip: (usize, usize),
    a: [f32; 3],
    b: [f32; 3],
}

#[derive(PartialEq)]
struct Open(f32, u64, Node);
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

impl Search<'_> {
    fn position(&self, n: Node) -> [f32; 3] {
        match n {
            Node::J(j) => self.net.junction_position(j),
            Node::Start => self.a,
            Node::Goal => self.b,
        }
    }

    /// @fn F034675
    ///
    /// A node's neighbours and the road lengths to them: its roads (not the two the points are
    /// on), then the temporary roads from it.
    fn adjacent(&self, n: Node, out: &mut Vec<(Node, f32)>) {
        out.clear();
        let other = |from: Node, to: Node| if from == n { to } else { from };
        match n {
            Node::J(j) => {
                for &r in &self.net.junctions[j].roads {
                    if r == self.skip.0 || r == self.skip.1 {
                        continue;
                    }
                    let road = &self.net.roads[r];
                    let next = if road.start == j { road.end } else { road.start };
                    out.push((Node::J(next), road.length));
                }
                for (key, list) in &self.extra {
                    if *key == j {
                        for &t in list {
                            out.push((other(self.temp[t].from, self.temp[t].to), self.temp[t].length));
                        }
                    }
                }
            }
            Node::Start | Node::Goal => {
                let list = if n == Node::Start { &self.start_list } else { &self.goal_list };
                for &t in list {
                    out.push((other(self.temp[t].from, self.temp[t].to), self.temp[t].length));
                }
            }
        }
    }

    /// @fn F034677
    ///
    /// A* from the start point to the goal point (F034673 adds a neighbour or lowers its cost;
    /// a closed node stays closed). The nodes from the goal back to the start, and the cost.
    fn solve(&self) -> Option<(Vec<Node>, f32)> {
        struct State {
            g: f32,
            h: f32,
            parent: Option<Node>,
            closed: bool,
        }
        let goal = self.position(Node::Goal);
        let mut nodes: HashMap<Node, State> = HashMap::new();
        let mut open = BinaryHeap::new();
        let mut seq = 0u64;
        let h0 = dist(self.position(Node::Start), goal);
        nodes.insert(Node::Start, State { g: 0.0, h: h0, parent: None, closed: false });
        open.push(Open(h0, seq, Node::Start));
        let mut adj = Vec::new();
        while let Some(Open(f, _, n)) = open.pop() {
            let s = nodes.get_mut(&n).unwrap();
            if s.closed || f != s.g + s.h {
                continue;
            }
            s.closed = true;
            if n == Node::Goal {
                let cost = s.g;
                let mut out = vec![n];
                let mut at = n;
                while let Some(p) = nodes[&at].parent {
                    out.push(p);
                    at = p;
                }
                return Some((out, cost));
            }
            let g = s.g;
            self.adjacent(n, &mut adj);
            for &(m, cost) in &adj {
                let ng = g + cost;
                match nodes.get_mut(&m) {
                    Some(t) if t.closed => {}
                    Some(t) => {
                        if ng < t.g {
                            t.g = ng;
                            t.parent = Some(n);
                            seq += 1;
                            open.push(Open(t.g + t.h, seq, m));
                        }
                    }
                    None => {
                        let h = dist(self.position(m), goal);
                        nodes.insert(m, State { g: ng, h, parent: Some(n), closed: false });
                        seq += 1;
                        open.push(Open(ng + h, seq, m));
                    }
                }
            }
        }
        None
    }
}

/// A way along the roads and the walk along it (RoadPath, 0x68 bytes, F034394).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadPath {
    /// The junctions still ahead (+0x08/+0x10).
    pub junctions: Vec<usize>,
    /// ROAD_* (+0x18).
    pub state: i32,
    /// The goal point's road and the last marker to walk to on it (+0x20, +0x28).
    pub end_road: usize,
    pub end_marker: i32,
    /// The marker walked to next (+0x2c) on the road walked (+0x38), which way along it (+0x40).
    pub marker: i32,
    pub road: usize,
    pub dir: i32,
    /// The distance left to the next marker, -1 when not worked out yet (+0x30).
    pub to_next: f32,
    /// The search's cost (+0x44).
    pub cost: f32,
    /// Where the walk is (+0x48).
    pub position: [f32; 3],
}

impl RoadPath {
    /// @fn F034381
    pub fn is_active(&self) -> bool {
        !matches!(self.state, ROAD_NONE | ROAD_DONE | ROAD_FAILED)
    }

    /// @fn F034389
    pub fn reset(&mut self) {
        self.junctions.clear();
        self.state = ROAD_NONE;
    }

    fn clamped(&self, net: &RoadNetwork) -> usize {
        let n = net.roads[self.road].markers.len() as i32;
        self.marker.min(n - 1).max(0) as usize
    }

    /// @fn F034395
    ///
    /// The marker walked to next (its index clamped to the road).
    pub fn target(&self, net: &RoadNetwork) -> [f32; 3] {
        net.roads[self.road].markers[self.clamped(net)].position
    }

    /// @fn F034396
    ///
    /// On to the next marker: its distance (its share of the road's length), or 0 at the way's
    /// end. At a road's end with junctions ahead, the next road (F034391).
    pub fn next_marker(&mut self, net: &RoadNetwork) -> f32 {
        let road = &net.roads[self.road];
        let n = road.markers.len() as i32;
        if self.dir < 0 {
            if self.marker == 0 && !self.junctions.is_empty() {
                return self.next_road(net);
            }
            if self.junctions.is_empty() && self.marker == self.end_marker {
                return 0.0;
            }
            // (the original walks off the road's start here; guarded)
            if self.marker <= 0 {
                return 0.0;
            }
            self.marker -= 1;
            let m = self.marker as usize;
            (road.markers[m + 1].t - road.markers[m].t) * road.length
        } else if self.dir > 0 {
            if !self.junctions.is_empty() {
                if self.marker == n - 1 {
                    return self.next_road(net);
                }
            } else if self.marker == self.end_marker {
                return 0.0;
            }
            if self.marker >= n - 1 {
                return 0.0;
            }
            self.marker += 1;
            let m = self.marker as usize;
            (road.markers[m].t - road.markers[m - 1].t) * road.length
        } else {
            0.0
        }
    }

    /// @fn F034391
    ///
    /// Past the junction ahead: onto the shortest road from it to the junction after (the goal's
    /// road when it was the last), from its far end; the first marker's distance, or 0 when that
    /// junction is the way's end.
    pub fn next_road(&mut self, net: &RoadNetwork) -> f32 {
        let j = self.junctions.remove(0);
        let road = if self.junctions.is_empty() {
            Some(self.end_road)
        } else {
            let next = self.junctions[0];
            let mut best: Option<usize> = None;
            for &r in &net.junctions[j].roads {
                let x = &net.roads[r];
                let joins = (x.start == j && x.end == next) || (x.start == next && x.end == j);
                if joins && best.is_none_or(|b| x.length < net.roads[b].length) {
                    best = Some(r);
                }
            }
            best
        };
        // (the original reads a null road here; no road joins them in the shipped data)
        let Some(r) = road else {
            self.junctions.clear();
            return 0.0;
        };
        self.road = r;
        let x = &net.roads[r];
        self.dir = if x.start == j { 1 } else { -1 };
        let n = x.markers.len() as i32;
        if self.dir < 1 {
            self.marker = n - 2;
            if r != self.end_road || self.end_marker <= self.marker {
                let m = self.marker as usize;
                return (x.markers[m + 1].t - x.markers[m].t) * x.length;
            }
        } else {
            self.marker = 1;
            if r != self.end_road || self.end_marker != 0 {
                return (x.markers[1].t - x.markers[0].t) * x.length;
            }
        }
        0.0
    }

    /// @fn F034390
    ///
    /// On the road (within its width of the segment it walks) or walking to it.
    pub fn update_state(&mut self, net: &RoadNetwork) -> i32 {
        if self.is_active() {
            let road = &net.roads[self.road];
            let n = road.markers.len() as i32;
            let other = (self.marker + if 0 < self.dir { -1 } else { 1 }).clamp(0, n - 1) as usize;
            let here = self.clamped(net);
            let d = segment_dist2(road.markers[here].position, road.markers[other].position, self.position);
            self.state = if d < road.width * road.width { ROAD_ON_ROAD } else { ROAD_TO_ROAD };
        }
        self.state
    }

    /// @fn F034399
    ///
    /// The walk restarts from `p`: within the road's width (across) of the marker walked to,
    /// that marker counts as reached (true); the state is worked out again.
    pub fn set_start(&mut self, net: &RoadNetwork, p: [f32; 3]) -> bool {
        self.position = p;
        self.to_next = -1.0;
        if self.is_active() {
            let road = &net.roads[self.road];
            let m = road.markers[self.clamped(net)].position;
            let (dx, dz) = (p[0] - m[0], p[2] - m[2]);
            if dx * dx + dz * dz < road.width * road.width {
                if self.next_marker(net) != 0.0 {
                    self.update_state(net);
                } else {
                    self.junctions.clear();
                    self.state = ROAD_DONE;
                }
                return true;
            }
            self.update_state(net);
        }
        false
    }

    /// @fn F034401
    ///
    /// Skip the marker walked to ("Skip road node"): on to the next, or the end.
    pub fn skip(&mut self, net: &RoadNetwork) {
        if self.next_marker(net) == 0.0 {
            self.junctions.clear();
            self.state = ROAD_DONE;
        } else {
            self.update_state(net);
        }
    }

    /// @fn F034400
    ///
    /// Walk `step` units along the way; the new position. Off the road it heads straight for
    /// the next marker; on it, between markers by their share of the road's length.
    pub fn advance(&mut self, net: &RoadNetwork, step: f32) -> [f32; 3] {
        if self.to_next < 0.0 {
            self.to_next = dist(self.position, self.target(net));
        }
        let mut d = self.to_next;
        while d <= step {
            let seg = self.next_marker(net);
            if seg == 0.0 {
                break;
            }
            d = seg + self.to_next;
            self.state = ROAD_ON_ROAD;
            self.to_next = d;
        }
        self.to_next -= step;
        if self.to_next <= 0.0 {
            self.junctions.clear();
            self.state = ROAD_DONE;
        } else if self.state == ROAD_TO_ROAD {
            let m = self.target(net);
            let s = self.to_next / (self.to_next + step);
            self.position = add(m, scale(sub(self.position, m), s));
        } else if self.state == ROAD_ON_ROAD {
            let road = &net.roads[self.road];
            let n = road.markers.len() as i32;
            let prev = if self.dir < 1 { self.marker + 1 } else { self.marker - 1 }.clamp(0, n - 1) as usize;
            let c = road.markers[self.clamped(net)];
            let p = road.markers[prev];
            let seg = (c.t - p.t).abs() * road.length;
            self.position = add(c.position, scale(sub(p.position, c.position), self.to_next / seg));
        }
        self.position
    }

    /// The markers the walk goes through from here, in order (the sequence `advance` heads for;
    /// F034392 draws the same way).
    pub fn waypoints(&self, net: &RoadNetwork) -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        if !self.is_active() {
            return out;
        }
        let mut p = self.clone();
        out.push(p.target(net));
        while p.next_marker(net) != 0.0 && out.len() < 1_000_000 {
            out.push(p.target(net));
        }
        out
    }
}

/// AbstractMovementBase (0x108 bytes): a mover that is a point, as a squad away from the player
/// moves (UnloadedPlatoon +0x58, F045436).
#[derive(Debug, Clone, PartialEq)]
pub struct AbstractMovement {
    /// Units (+0xc4), the goal (+0xdc), the heading (+0xa8).
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub direction: [f32; 3],
    /// This tick's speed (+0xb8): the lower of its own (+0xb4, 70) and the cap (+0xbc, 45 until
    /// set; the unloaded platoon sets it each tick, F045434).
    pub speed: f32,
    pub own_speed: f32,
    pub speed_cap: f32,
    /// The road factor (+0x100): the roads count this times their length; 1 or more, never taken.
    /// UnloadedPlatoon and CharMovement::init: max(0, 1 - the faction's "road preference").
    pub road_factor: f32,
    /// The way along the roads (+0xf8).
    pub road: Option<RoadPath>,
    /// Moving faster than 1 (+0x24); stopped (+8, set by stop, cleared by goTo).
    pub moving: bool,
    pub stopped: bool,
}

impl AbstractMovement {
    /// @fn F040687
    ///
    /// A mover at `position` (F045436: placed there and stopped) with a faction's road preference.
    pub fn new(position: [f32; 3], road_preference: f32) -> AbstractMovement {
        let mut m = AbstractMovement {
            position,
            target: [0.0; 3],
            direction: [1.0, 0.0, 0.0],
            speed: 0.0,
            own_speed: 70.0,
            speed_cap: 45.0,
            road_factor: road_factor(road_preference),
            road: None,
            moving: false,
            stopped: true,
        };
        m.stop();
        m
    }

    /// @fn F040620
    ///
    /// Stop: the goal is where it stands, no speed, the road way dropped.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.target = self.position;
        self.speed = 0.0;
        self.direction = [0.0; 3];
        if let Some(r) = &mut self.road {
            r.reset();
        }
    }

    /// @fn F040645
    ///
    /// A new goal: one over 10000 units away first tries the roads (F040644).
    pub fn go_to(&mut self, net: Option<&RoadNetwork>, target: [f32; 3]) {
        self.stopped = false;
        if self.target != target {
            if dist2(self.position, target) > ROAD_GOAL_FAR
                && let Some(net) = net
            {
                self.road_route(net, target);
            }
            self.target = target;
        }
    }

    /// @fn F040644
    ///
    /// Take the roads to `target` if they pay: the nearest roads (within 20000) to here and to the
    /// goal, each biased toward the other end by the road factor clamped to 0.2..0.9; not the
    /// same segment; the walks to and from them no longer than the straight way, and with the
    /// way along the roads times the road factor added still not. Whether it took them.
    pub fn road_route(&mut self, net: &RoadNetwork, target: [f32; 3]) -> bool {
        if self.road_factor >= 1.0 {
            return false;
        }
        let f = if 0.9 <= self.road_factor { 0.9 } else { self.road_factor };
        let f = if f <= 0.2 { 0.2 } else { f };
        let bias = scale(normalise(sub(target, self.position)), f);
        let (Some(a), Some(b)) = (
            net.nearest(self.position, ROAD_SEARCH, bias),
            net.nearest(target, ROAD_SEARCH, scale(bias, -1.0)),
        ) else {
            return false;
        };
        if a.road == b.road && a.segment == b.segment {
            return false;
        }
        let walk = dist(self.position, a.point) + dist(target, b.point);
        let straight = dist2(self.position, target);
        if walk * walk > straight {
            return false;
        }
        let mut path = net.find(&a, &b);
        path.set_start(net, self.position);
        if path.is_active() {
            let total = walk + path.cost * self.road_factor;
            if total * total <= straight {
                // (a mover with an owner, CharMovement, also goes to the first marker here; the
                // abstract one's handle is empty, F040703)
                self.road = Some(path);
                self.target = target;
                return true;
            }
            path.reset();
        }
        self.road = Some(path);
        false
    }

    /// @fn F040640
    ///
    /// One tick: the speed is the lower of its own and the cap; along the road way while it is
    /// active, else straight at the goal (no stop there: it steps past and back).
    /// Not ported: mode 3 (the speed of a leader it keeps up with).
    pub fn update(&mut self, net: Option<&RoadNetwork>, dt: f32) {
        if dt == 0.0 {
            return;
        }
        self.speed = if self.speed_cap <= self.own_speed { self.speed_cap } else { self.own_speed };
        match (&mut self.road, net) {
            (Some(r), Some(net)) if r.is_active() => {
                let p = r.advance(net, dt * self.speed);
                self.direction = sub(p, self.position);
                self.position = p;
            }
            _ => {
                self.direction = normalise(sub(self.target, self.position));
                self.position = add(self.position, scale(self.direction, self.speed * dt));
            }
        }
        self.moving = 1.0 < self.speed;
    }
}

/// CharMovement's use of the roads (+0xf8, +0x100, the goal +0xdc): a character on the navmesh
/// sent far walks from marker to marker of a road way, each one a navmesh goal.
#[derive(Debug, Clone, PartialEq)]
pub struct CharRoads {
    pub path: Option<RoadPath>,
    /// The goal past the roads.
    pub goal: [f32; 3],
    /// CharMovement::init (F040657): `road_factor` of the faction's "road preference".
    pub road_factor: f32,
}

impl Default for CharRoads {
    /// No faction's preference yet: a road factor of 1, the roads never taken (as "No Faction").
    fn default() -> Self {
        CharRoads { path: None, goal: [0.0; 3], road_factor: 1.0 }
    }
}

/// CharMovement::update (F040650) gives up the road way within this (squared, across) of the goal.
pub const ROAD_GOAL_NEAR: f32 = 300_000.0;

impl CharRoads {
    pub fn active(&self) -> bool {
        self.path.as_ref().is_some_and(|p| p.is_active())
    }

    /// @fn F040653
    ///
    /// The road part of CharMovement::goTo: a goal that is not the road way's next marker ends the
    /// way; one over 1000 units off takes the roads when they pay (F040644, the mover's road
    /// factor). The point to send the character to on the navmesh: the way's first marker, else
    /// the goal.
    pub fn go_to(&mut self, net: &RoadNetwork, position: [f32; 3], goal: [f32; 3]) -> [f32; 3] {
        if let Some(p) = &mut self.path
            && p.is_active()
            && p.target(net) != goal
        {
            p.reset();
        }
        if dist2(position, goal) > ROAD_GOAL_FAR_CHARACTER && !self.active() {
            let mut m = AbstractMovement {
                position,
                target: self.goal,
                road: self.path.take(),
                road_factor: self.road_factor,
                ..AbstractMovement::new(position, 0.0)
            };
            let took = m.road_route(net, goal);
            self.path = m.road;
            if took && let Some(p) = &self.path {
                self.goal = goal;
                return p.target(net);
            }
        }
        goal
    }

    /// @fn F040650
    ///
    /// The road part of a CharMovement tick while the way is on: the walk restarts from where
    /// the character stands (F034399; within the road's width of the marker it is reached). On
    /// reaching one, arriving where it was sent, being within ~548 of the goal, or the navmesh
    /// finding no path: past the way's end, arrived or near, the way ends and the goal is the
    /// navmesh goal; no path skips the marker ("Skip road node"); otherwise the next marker.
    /// The new navmesh goal, if it changes.
    pub fn tick(&mut self, net: &RoadNetwork, position: [f32; 3], arrived: bool, failed: bool) -> Option<[f32; 3]> {
        let p = self.path.as_mut().filter(|p| p.is_active())?;
        let (dx, dz) = (self.goal[0] - position[0], self.goal[2] - position[2]);
        let near = arrived || dz * dz + dx * dx < ROAD_GOAL_NEAR;
        let reached = p.set_start(net, position);
        if !(reached || near || failed) {
            return None;
        }
        if p.state == ROAD_DONE || near {
            p.reset();
            return Some(self.goal);
        }
        if failed {
            p.skip(net);
        }
        Some(p.target(net))
    }
}

/// The road factor of a faction's "road preference" (UnloadedPlatoon F045436, CharMovement::init
/// F040657): max(0, 1 - preference).
pub fn road_factor(road_preference: f32) -> f32 {
    let d = 1.0 - road_preference as f64;
    (if 0.0 <= d { d } else { 0.0 }) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Roads along x at z = 0: junctions 0 -(road 0)- 1 -(road 1)- 2, each 1000 long with a
    /// marker every 250, and a long way round from 0 to 2 (road 2) bending out to z = 3000.
    fn net() -> RoadNetwork {
        let line = |x0: f32, x1: f32| -> Vec<Marker> {
            (0..=4)
                .map(|i| Marker { position: [x0 + (x1 - x0) * i as f32 / 4.0, 0.0, 0.0], t: i as f32 / 4.0 })
                .collect()
        };
        let mut n = RoadNetwork::default();
        n.junctions = (0..3).map(|i| Junction { id: i, roads: vec![] }).collect();
        let mut add_road = |n: &mut RoadNetwork, s: usize, e: usize, markers: Vec<Marker>, length: f32| {
            let k = n.roads.len();
            n.roads.push(Road { name: format!("r{k}"), start: s, end: e, markers, length, weight: 1.0, width: 80.0, hidden: false });
            n.junctions[s].roads.push(k);
            n.junctions[e].roads.push(k);
        };
        add_road(&mut n, 0, 1, line(0.0, 1000.0), 1000.0);
        add_road(&mut n, 1, 2, line(1000.0, 2000.0), 1000.0);
        let round = vec![
            Marker { position: [0.0, 0.0, 0.0], t: 0.0 },
            Marker { position: [1000.0, 0.0, 3000.0], t: 0.5 },
            Marker { position: [2000.0, 0.0, 0.0], t: 1.0 },
        ];
        add_road(&mut n, 0, 2, round, 6400.0);
        n
    }

    #[test]
    fn nearest_point_on_a_segment() {
        let n = net();
        let p = n.nearest([600.0, 0.0, 50.0], 1000.0, [0.0; 3]).unwrap();
        assert_eq!((p.road, p.segment), (0, 2));
        assert_eq!(p.point, [600.0, 0.0, 0.0]);
        assert!((p.t - 0.6).abs() < 1e-6);
        assert!((p.distance - 50.0).abs() < 1e-4);
        assert!(n.nearest([600.0, 0.0, 5000.0], 1000.0, [0.0; 3]).is_none());
    }

    #[test]
    fn a_way_across_two_roads() {
        let n = net();
        let a = n.nearest([300.0, 0.0, 10.0], 1000.0, [0.0; 3]).unwrap();
        let b = n.nearest([1700.0, 0.0, 10.0], 1000.0, [0.0; 3]).unwrap();
        let path = n.find(&a, &b);
        assert_eq!(path.state, ROAD_TO_ROAD);
        assert_eq!(path.junctions, vec![1]);
        // 700 to junction 1, then 700 along road 1
        assert!((path.cost - 1400.0).abs() < 1e-3, "{}", path.cost);
        let w: Vec<f32> = path.waypoints(&n).iter().map(|p| p[0]).collect();
        assert_eq!(w, vec![500.0, 750.0, 1000.0, 1250.0, 1500.0]);
    }

    #[test]
    fn walking_the_way() {
        let n = net();
        let a = n.nearest([300.0, 0.0, 10.0], 1000.0, [0.0; 3]).unwrap();
        let b = n.nearest([1700.0, 0.0, 10.0], 1000.0, [0.0; 3]).unwrap();
        let mut path = n.find(&a, &b);
        path.set_start(&n, [300.0, 0.0, 10.0]);
        assert_eq!(path.state, ROAD_ON_ROAD);
        let mut at = path.position;
        for _ in 0..200 {
            if !path.is_active() {
                break;
            }
            at = path.advance(&n, 10.0);
        }
        // the step that ends the way leaves the position where the last one put it
        assert_eq!(path.state, ROAD_DONE);
        assert!((at[0] - 1500.0).abs() <= 10.0 && at[2].abs() < 1e-3, "{at:?}");
    }

    #[test]
    fn the_same_road_backwards() {
        let n = net();
        let a = n.nearest([900.0, 0.0, 0.0], 1000.0, [0.0; 3]).unwrap();
        let b = n.nearest([100.0, 0.0, 0.0], 1000.0, [0.0; 3]).unwrap();
        let path = n.find(&a, &b);
        assert!(path.junctions.is_empty());
        assert!((path.cost - 800.0).abs() < 1e-3);
        let w: Vec<f32> = path.waypoints(&n).iter().map(|p| p[0]).collect();
        assert_eq!(w, vec![750.0, 500.0, 250.0]);
    }

    #[test]
    fn the_mover_takes_the_road_only_when_it_pays() {
        let n = net();
        // roads at 0.2 of their length: worth it from just off one end of the network to the other
        let mut m = AbstractMovement::new([-100.0, 0.0, 200.0], 0.8);
        assert!((m.road_factor - 0.2).abs() < 1e-6);
        m.go_to(Some(&n), [2100.0, 0.0, 200.0]);
        // under 10000 away: no roads
        assert!(m.road.is_none());
        assert!(m.road_route(&n, [2100.0, 0.0, 200.0]));
        // a faction with no road preference never takes them
        let mut w = AbstractMovement::new([-100.0, 0.0, 200.0], 0.0);
        assert!(!w.road_route(&n, [2100.0, 0.0, 200.0]));
        // along the road: speed is the lower of the cap and its own 70
        m.speed_cap = 90.0;
        m.update(Some(&n), 1.0);
        assert_eq!(m.speed, 70.0);
    }
}
