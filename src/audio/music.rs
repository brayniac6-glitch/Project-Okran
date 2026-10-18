//! Wwise's interactive music (bank version 112): music segments (10), tracks (11), switch
//! containers (12) and playlist containers (13), and how they are played one after another.
//!
//! Read from the banks (found by their bytes; the layout as Wwise 2016 writes it):
//! - every music node but a track: flags u8, then the node base: FX (override u8, count u8,
//!   [bypass u8, count x 7]), attachment u8, output bus u32, parent u32, priority bits u8,
//!   properties (count u8, ids, 4-byte values), ranged properties (count u8, ids, 8-byte values),
//!   positioning bits u8, aux bits u8 (bit 3: 4 aux busses u32), advanced settings (6 bytes),
//!   state groups (count u32 ...), RTPCs (count u16 ...); then its children (count u32, ids),
//!   its meter (grid period f64 ms, grid offset f64, tempo f32, beats a bar u8, beat value u8),
//!   a flag u8 and its stingers (count u32, 24 bytes each);
//! - a segment then: its duration (f64 ms) and markers (count u32, (id u32, position f64 ms,
//!   name length u32, name)): the first its entry cue, the last its exit cue;
//! - a track: flags u8, sources (count u32, 22 bytes each: plugin u32, stream type u8, source id
//!   u32, file id u32, file offset u32, size u32, flags u8), clips (count u32, (sub-track u32,
//!   source id u32, play at f64 ms, begin trim f64, end trim f64, source duration f64)), sub-track
//!   count u32, clip automation (count u32, (clip u32, type u32, points count u32, points 12
//!   bytes)), the node base, its type u8 (0 normal, 1 random step, 2 sequence step, 3 switch;
//!   a switch track then its group type u8, group u32, default u32, switches (count u32, ids)
//!   and fade parameters 28 bytes), look-ahead s32;
//! - a switch or playlist then its transition rules (count u32: sources (count u32, ids),
//!   destinations (count u32, ids), the source's rule (time s32 ms, fade curve u32, fade offset
//!   s32, sync type u32, cue filter u32, play post-exit u8), the destination's (time s32, curve
//!   u32, offset s32, cue filter u32, jump to u32, entry type u16, play pre-entry u8, match cue
//!   name u8), a transition segment flag u8 (then 30 bytes));
//! - a switch: continue playback u8, its decision tree (depth u32, group ids u32 x depth, group
//!   types u8 x depth, size u32, mode u8, then 12-byte nodes: key u32, a leaf's node u32 or a
//!   branch's first child u16 and child count u16, weight u16, probability u16);
//! - a playlist: its items in pre-order (count u32, (segment u32, item id u32, children u32,
//!   type u32 (0 continuous sequence, 1 step sequence, 2 continuous random, 3 step random, -1 a
//!   segment), loops s16 (0 forever), loop min s16, loop max s16, weight u32, avoid repeat u16,
//!   use weight u8, shuffle u8)).

use crate::bnk::{Bank, Object};
use crate::tree::Props;
use std::collections::HashMap;

/// A clip of a track: its sub-track, the source (a WEM) and where it plays in the segment (ms).
#[derive(Clone, Debug)]
pub struct Clip {
    pub sub_track: u32,
    pub source: u32,
    pub play_at: f64,
    pub begin_trim: f64,
    pub end_trim: f64,
    pub duration: f64,
}

/// One transition rule of a switch or playlist (times in ms).
#[derive(Clone, Debug, Default)]
pub struct Rule {
    pub src: Vec<u32>,
    pub dst: Vec<u32>,
    pub fade_out: i32,
    pub fade_out_offset: i32,
    /// 0 immediate, 1 next grid, 2 next bar, 3 next beat, 4 next cue, 5 next custom cue, 6 entry
    /// cue, 7 exit cue.
    pub sync: u32,
    pub play_post_exit: bool,
    pub fade_in: i32,
    pub fade_in_offset: i32,
    pub play_pre_entry: bool,
}

/// One node of a switch's decision tree.
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub key: u32,
    /// A leaf's music node (0 none).
    pub node: u32,
    pub first: u16,
    pub count: u16,
    pub weight: u16,
    pub probability: u16,
}

/// One item of a playlist (pre-order: an item's children follow it).
#[derive(Clone, Debug)]
pub struct Item {
    pub segment: u32,
    pub id: u32,
    pub children: u32,
    /// 0 continuous sequence, 1 step sequence, 2 continuous random, 3 step random, -1 a
    /// segment.
    pub kind: i32,
    pub loops: i16,
    pub weight: u32,
    pub avoid_repeat: u16,
    pub shuffle: bool,
}

#[derive(Clone, Debug)]
pub enum MusicKind {
    /// Its duration and its entry and exit cues (ms); its children are its tracks.
    Segment { duration: f64, entry: f64, exit: f64, grid: (f64, f32, u8) },
    /// Its clips; its type (0 normal, 1 random step, 2 sequence step, 3 switch) and for a switch
    /// track its group, default and the sub-track of each switch.
    Track { clips: Vec<Clip>, sub_tracks: u32, kind: u8, switch: Option<(u32, u32, Vec<u32>)> },
    /// Its decision tree (group ids by depth) and transition rules.
    Switch { groups: Vec<u32>, tree: Vec<TreeNode>, continue_playback: bool, rules: Vec<Rule> },
    /// Its playlist and transition rules.
    Playlist { items: Vec<Item>, rules: Vec<Rule> },
}

#[derive(Clone, Debug)]
pub struct MusicNode {
    pub parent: u32,
    pub props: Props,
    pub children: Vec<u32>,
    pub kind: MusicKind,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let v = self.b.get(self.p..self.p + 2)?;
        self.p += 2;
        Some(u16::from_le_bytes([v[0], v[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        let v = self.b.get(self.p..self.p + 4)?;
        self.p += 4;
        Some(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }
    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }
    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }
    fn f64(&mut self) -> Option<f64> {
        let v = self.b.get(self.p..self.p + 8)?;
        self.p += 8;
        Some(f64::from_le_bytes(v.try_into().ok()?))
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        (self.p + n <= self.b.len()).then(|| self.p += n)
    }
    fn ids(&mut self, n: usize) -> Option<Vec<u32>> {
        (0..n).map(|_| self.u32()).collect()
    }
}

/// The node base (see the module's notes): its parent and properties, the reader after it.
fn node_base(r: &mut Reader) -> Option<(u32, Props)> {
    r.u8()?; // FX override
    let nfx = r.u8()? as usize;
    if nfx > 0 {
        r.skip(1 + 7 * nfx)?;
    }
    r.u8()?; // attachment parameters
    r.u32()?; // output bus
    let parent = r.u32()?;
    r.u8()?; // priority bits
    let mut props = Props::default();
    let n = r.u8()? as usize;
    let ids: Vec<u8> = (0..n).map(|_| r.u8()).collect::<Option<_>>()?;
    for id in ids {
        let raw = r.u32()?;
        let f = f32::from_bits(raw);
        match id {
            0x00 => props.volume = f,
            0x02 => props.pitch = f,
            0x3a => props.loops = Some(raw),
            0x3b => props.delay = f,
            _ => {}
        }
    }
    let n = r.u8()? as usize;
    r.skip(n + 8 * n)?;
    r.u8()?; // positioning bits
    let aux = r.u8()?;
    if aux & 0x08 != 0 {
        r.skip(16)?;
    }
    r.skip(6)?; // advanced settings
    let states = r.u32()? as usize;
    for _ in 0..states {
        r.u32()?;
        r.u8()?;
        let n = r.u16()? as usize;
        r.skip(8 * n)?;
    }
    let rtpcs = r.u16()? as usize;
    for _ in 0..rtpcs {
        r.skip(4 + 1 + 1 + 4 + 4 + 1)?;
        let n = r.u16()? as usize;
        r.skip(12 * n)?;
    }
    Some((parent, props))
}

/// A music node's children, meter and stingers (after its node base).
fn music_children(r: &mut Reader) -> Option<(Vec<u32>, (f64, f32, u8))> {
    let n = r.u32()? as usize;
    let children = r.ids(n.min(4096))?;
    let grid = r.f64()?;
    r.f64()?;
    let tempo = r.f32()?;
    let beats = r.u8()?;
    r.u8()?;
    r.u8()?;
    let stingers = r.u32()? as usize;
    r.skip(24 * stingers)?;
    Some((children, (grid, tempo, beats)))
}

fn rules(r: &mut Reader) -> Option<Vec<Rule>> {
    let n = r.u32()? as usize;
    let mut out = Vec::new();
    for _ in 0..n.min(1024) {
        let ns = r.u32()? as usize;
        let src = r.ids(ns.min(4096))?;
        let nd = r.u32()? as usize;
        let dst = r.ids(nd.min(4096))?;
        let fade_out = r.i32()?;
        r.u32()?;
        let fade_out_offset = r.i32()?;
        let sync = r.u32()?;
        r.u32()?;
        let play_post_exit = r.u8()? != 0;
        let fade_in = r.i32()?;
        r.u32()?;
        let fade_in_offset = r.i32()?;
        r.u32()?;
        r.u32()?;
        r.u16()?;
        let play_pre_entry = r.u8()? != 0;
        r.u8()?;
        if r.u8()? != 0 {
            r.skip(30)?;
        }
        out.push(Rule { src, dst, fade_out, fade_out_offset, sync, play_post_exit, fade_in, fade_in_offset, play_pre_entry });
    }
    Some(out)
}

fn parse(kind: u8, b: &[u8]) -> Option<MusicNode> {
    let mut r = Reader { b, p: 0 };
    match kind {
        10 | 12 | 13 => {
            r.u8()?; // music flags
            let (parent, props) = node_base(&mut r)?;
            let (children, grid) = music_children(&mut r)?;
            let kind = match kind {
                10 => {
                    let duration = r.f64()?;
                    let n = r.u32()? as usize;
                    let mut cues = Vec::new();
                    for _ in 0..n.min(256) {
                        r.u32()?;
                        let at = r.f64()?;
                        let len = r.u32()? as usize;
                        r.skip(len)?;
                        cues.push(at);
                    }
                    let entry = cues.first().copied().unwrap_or(0.0);
                    let exit = if cues.len() >= 2 { *cues.last().unwrap() } else { duration };
                    MusicKind::Segment { duration, entry, exit, grid }
                }
                12 => {
                    let rules = rules(&mut r)?;
                    let continue_playback = r.u8()? != 0;
                    let depth = r.u32()? as usize;
                    let groups = r.ids(depth.min(16))?;
                    r.skip(depth)?;
                    let size = r.u32()? as usize;
                    r.u8()?;
                    let mut tree = Vec::new();
                    for _ in 0..size / 12 {
                        let key = r.u32()?;
                        let node_or_kids = r.u32()?;
                        let weight = r.u16()?;
                        let probability = r.u16()?;
                        tree.push(TreeNode {
                            key,
                            node: node_or_kids,
                            first: (node_or_kids & 0xffff) as u16,
                            count: (node_or_kids >> 16) as u16,
                            weight,
                            probability,
                        });
                    }
                    MusicKind::Switch { groups, tree, continue_playback, rules }
                }
                _ => {
                    let rules = rules(&mut r)?;
                    let n = r.u32()? as usize;
                    let mut items = Vec::new();
                    for _ in 0..n.min(4096) {
                        let segment = r.u32()?;
                        let id = r.u32()?;
                        let children = r.u32()?;
                        let kind = r.i32()?;
                        let loops = r.u16()? as i16;
                        r.u16()?;
                        r.u16()?;
                        let weight = r.u32()?;
                        let avoid_repeat = r.u16()?;
                        r.u8()?;
                        let shuffle = r.u8()? != 0;
                        items.push(Item { segment, id, children, kind, loops, weight, avoid_repeat, shuffle });
                    }
                    MusicKind::Playlist { items, rules }
                }
            };
            Some(MusicNode { parent, props, children, kind })
        }
        11 => {
            r.u8()?;
            let ns = r.u32()? as usize;
            r.skip(22 * ns)?;
            let nc = r.u32()? as usize;
            let mut clips = Vec::new();
            for _ in 0..nc.min(1024) {
                let sub_track = r.u32()?;
                let source = r.u32()?;
                let play_at = r.f64()?;
                let begin_trim = r.f64()?;
                let end_trim = r.f64()?;
                let duration = r.f64()?;
                clips.push(Clip { sub_track, source, play_at, begin_trim, end_trim, duration });
            }
            let sub_tracks = if nc > 0 { r.u32()? } else { 0 };
            let na = r.u32()? as usize;
            for _ in 0..na.min(1024) {
                r.skip(8)?;
                let n = r.u32()? as usize;
                r.skip(12 * n)?;
            }
            let (parent, props) = node_base(&mut r)?;
            let kind = r.u8()?;
            let switch = if kind == 3 {
                r.u8()?;
                let group = r.u32()?;
                let default = r.u32()?;
                let n = r.u32()? as usize;
                let ids = r.ids(n.min(256))?;
                Some((group, default, ids))
            } else {
                None
            };
            Some(MusicNode { parent, props, children: Vec::new(), kind: MusicKind::Track { clips, sub_tracks, kind, switch } })
        }
        _ => None,
    }
}

/// The music hierarchies of the banks loaded.
#[derive(Default)]
pub struct Music {
    pub nodes: HashMap<u32, MusicNode>,
    /// Ids of objects whose layout could not be read.
    pub unread: Vec<(u8, u32)>,
}

impl Music {
    pub fn add(&mut self, bank: &Bank) {
        for o in bank.objects.iter().filter(|o: &&Object| (10..=13).contains(&o.kind)) {
            match parse(o.kind, bank.body(o)) {
                Some(n) => {
                    self.nodes.insert(o.id, n);
                }
                None => self.unread.push((o.kind, o.id)),
            }
        }
    }

    /// The volume (dB) a music node plays at: its own and its music parents' summed.
    pub fn volume(&self, mut id: u32) -> f32 {
        let mut v = 0.0;
        for _ in 0..32 {
            let Some(n) = self.nodes.get(&id) else { break };
            v += n.props.volume;
            id = n.parent;
        }
        v
    }

    /// A switch's child for the switches set (its decision tree walked from the root by the value
    /// of each depth's group: the matching key, else the key 0 branch); 0 none.
    pub fn switch_child(&self, id: u32, values: &dyn Fn(u32) -> Option<u32>) -> u32 {
        let Some(MusicNode { kind: MusicKind::Switch { groups, tree, .. }, .. }) = self.nodes.get(&id) else { return 0 };
        let Some(mut at) = tree.first().map(|_| 0usize) else { return 0 };
        for g in groups {
            let n = &tree[at];
            let kids = (n.first as usize)..(n.first as usize + n.count as usize).min(tree.len());
            let want = values(*g);
            let pick = kids.clone().find(|&k| Some(tree[k].key) == want).or_else(|| kids.clone().find(|&k| tree[k].key == 0));
            match pick {
                Some(k) => at = k,
                None => return 0,
            }
        }
        tree[at].node
    }
}

/// Walks a playlist's items, as Wwise plays them: a continuous group plays its children (in
/// order or at random by weight, avoiding the last `avoid repeat` picks) one after another, a
/// step group one each time the group is played; each item `loops` times (0 forever).
#[derive(Clone, Debug, Default)]
pub struct PlaylistWalk {
    /// The stack: (item index, its children's indices, the next child, loops left (-1 forever)).
    stack: Vec<(usize, Vec<usize>, usize, i32)>,
    /// Step groups' next child (by item id) and random groups' recent picks.
    pub step: HashMap<u32, usize>,
    pub recent: HashMap<u32, Vec<usize>>,
}

fn children_of(items: &[Item], i: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut k = i + 1;
    for _ in 0..items[i].children {
        if k >= items.len() {
            break;
        }
        out.push(k);
        k = skip_subtree(items, k);
    }
    out
}

fn skip_subtree(items: &[Item], i: usize) -> usize {
    let mut k = i + 1;
    for _ in 0..items[i].children {
        if k >= items.len() {
            break;
        }
        k = skip_subtree(items, k);
    }
    k
}

impl PlaylistWalk {
    /// The next segment to play (None: the playlist is done).
    pub fn next(&mut self, items: &[Item], rand: &mut dyn FnMut() -> f32) -> Option<u32> {
        if items.is_empty() {
            return None;
        }
        if self.stack.is_empty() {
            let loops = if items[0].loops == 0 { -1 } else { items[0].loops as i32 };
            self.stack.push((0, Vec::new(), 0, loops));
        }
        for _ in 0..4096 {
            let (i, kids, next, loops) = self.stack.last()?.clone();
            let item = &items[i];
            if item.kind < 0 || item.children == 0 {
                // a segment: played `loops` times
                self.stack.pop();
                if loops > 1 || loops < 0 {
                    self.stack.push((i, kids, next, if loops < 0 { -1 } else { loops - 1 }));
                }
                if item.segment != 0 {
                    return Some(item.segment);
                }
                continue;
            }
            let all = children_of(items, i);
            let step = item.kind == 1 || item.kind == 3;
            let random = item.kind >= 2;
            if kids.is_empty() && next == 0 {
                // a new pass of the group: its order
                let order: Vec<usize> = if step {
                    vec![self.pick(items, i, &all, random, rand)]
                } else if random {
                    let mut order = Vec::new();
                    for _ in 0..all.len() {
                        order.push(self.pick(items, i, &all, true, rand));
                    }
                    order
                } else {
                    all.clone()
                };
                let top = self.stack.last_mut()?;
                top.1 = order;
                continue;
            }
            if next < kids.len() {
                let c = kids[next];
                self.stack.last_mut()?.2 = next + 1;
                let loops = if items[c].loops == 0 { -1 } else { items[c].loops as i32 };
                self.stack.push((c, Vec::new(), 0, loops));
                continue;
            }
            // the pass is done: again while loops are left
            self.stack.pop();
            if loops > 1 || loops < 0 {
                self.stack.push((i, Vec::new(), 0, if loops < 0 { -1 } else { loops - 1 }));
            }
            if self.stack.is_empty() {
                return None;
            }
        }
        None
    }

    /// A child of group `i`: the next in order (step sequence), else one at random by weight not
    /// among the last `avoid repeat` picks.
    fn pick(&mut self, items: &[Item], i: usize, all: &[usize], random: bool, rand: &mut dyn FnMut() -> f32) -> usize {
        let item = &items[i];
        if !random {
            let k = self.step.entry(item.id).or_insert(0);
            let c = all[*k % all.len()];
            *k += 1;
            return c;
        }
        let recent = self.recent.entry(item.id).or_default();
        let free: Vec<usize> = all.iter().copied().filter(|c| !recent.contains(c)).collect();
        let pool = if free.is_empty() { all.to_vec() } else { free };
        let total: f32 = pool.iter().map(|&c| items[c].weight.max(1) as f32).sum();
        let mut x = rand() * total;
        let mut c = *pool.last().unwrap_or(&all[0]);
        for &k in &pool {
            x -= items[k].weight.max(1) as f32;
            if x <= 0.0 {
                c = k;
                break;
            }
        }
        recent.push(c);
        let keep = (item.avoid_repeat as usize).min(all.len().saturating_sub(1));
        while recent.len() > keep {
            recent.remove(0);
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(segment: u32, id: u32, children: u32, kind: i32, loops: i16, avoid: u16) -> Item {
        Item { segment, id, children, kind, loops, weight: 50000, avoid_repeat: avoid, shuffle: false }
    }

    fn walk_all(items: &[Item], w: &mut PlaylistWalk) -> Vec<u32> {
        let mut seed = 7u32;
        let mut rand = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut out = Vec::new();
        while let Some(s) = w.next(items, &mut rand) {
            out.push(s);
            if out.len() > 50 {
                break;
            }
        }
        out
    }

    #[test]
    fn continuous_sequence_loops() {
        // a continuous sequence looped twice: its two segments in order, twice
        let items = [item(0, 1, 2, 0, 2, 0), item(10, 2, 0, -1, 1, 0), item(20, 3, 0, -1, 1, 0)];
        assert_eq!(walk_all(&items, &mut PlaylistWalk::default()), vec![10, 20, 10, 20]);
    }

    #[test]
    fn step_random_one_a_play_avoiding_repeats() {
        // a step random group (avoid 2 of 3): one segment a play, never one of the last two
        let items = [item(0, 1, 3, 3, 1, 2), item(10, 2, 0, -1, 1, 0), item(20, 3, 0, -1, 1, 0), item(30, 4, 0, -1, 1, 0)];
        let mut w = PlaylistWalk::default();
        let mut seen = Vec::new();
        for _ in 0..9 {
            let mut fresh = PlaylistWalk { step: w.step.clone(), recent: w.recent.clone(), ..Default::default() };
            let got = walk_all(&items, &mut fresh);
            assert_eq!(got.len(), 1);
            seen.push(got[0]);
            w.step = fresh.step;
            w.recent = fresh.recent;
        }
        for k in 2..seen.len() {
            assert!(seen[k] != seen[k - 1] && seen[k] != seen[k - 2], "{seen:?}");
        }
    }
}
