//! The banks' sound hierarchy (bank version 112, Wwise 2016) and what an event plays.
//!
//! Read from the banks (found by their bytes, the format not documented):
//! - every node (sound 2, random / sequence container 5, switch container 6, actor-mixer 7,
//!   layer container 9) starts: FX (override u8, count u8, [bypass u8, count x 7]), a byte,
//!   output bus u32, parent u32, a flag byte, then its properties (count u8, ids, 4 byte values;
//!   0 volume dB, 2 pitch cents, 3 low pass, 0x3a loop count (0 forever), 0x3b initial delay s);
//!   a sound has its source first (plugin u32, stream type u8, source id u32, file id u32, file
//!   offset u32, size u32, flags u8: 22 bytes);
//! - a container's children are the nodes naming it parent: their list (count u32, ids) found in
//!   it by them; a random / sequence container has 24 bytes before it (loop count u16, ...,
//!   avoid repeat u16, transition mode u8, random mode u8, mode u8 (0 random, 1 sequence), flags
//!   u8 (bit 3 continuous)) and its playlist after it (count u16, (id u32, weight i32)); a switch
//!   container has 10 before it (group type u8 (0 switch, 1 state), group u32, default u32, u8)
//!   and its switches after it (count u32, (switch u32, count u32, ids));
//! - an event: count u32, action ids; an action: type u16 (0x0403 play, 0x0102 / 0x0103 stop,
//!   0x1204 set state), target u32, ...; a set state: its group and state after its properties.

use crate::bnk::{Bank, Object};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Props {
    pub volume: f32,
    pub pitch: f32,
    /// Times played (0 forever), when set.
    pub loops: Option<u32>,
    pub delay: f32,
}

#[derive(Clone, Debug)]
pub enum Kind {
    /// A sound: its source (a WEM in some bank), or silence.
    Sound { source: u32, silent: bool },
    /// Random (or sequence) container: its playlist (child, weight), whether it plays them one
    /// after another (continuous), how often it loops (0 forever).
    /// Between children of a continuous one: the transition (1 cross-fade, 2 delay, 3 sample
    /// accurate, 4 trigger rate), its time (s) and random offset (s, min .. max).
    RanSeq {
        random: bool,
        continuous: bool,
        loops: u16,
        avoid_repeat: u16,
        playlist: Vec<(u32, i32)>,
        transition: (u8, f32, f32, f32),
    },
    /// Switch container: its group (a state group when `state`), default, switch -> children.
    Switch { state: bool, group: u32, default: u32, map: Vec<(u32, Vec<u32>)> },
    /// Layer container: all its children at once.
    Layer,
    /// Actor-mixer: not played itself.
    Mixer,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: u32,
    pub props: Props,
    pub children: Vec<u32>,
    pub kind: Kind,
}

#[derive(Clone, Debug)]
pub enum Action {
    Play(u32),
    Stop(u32),
    SetState { group: u32, state: u32 },
    Other(u16),
}

/// The hierarchies of the banks loaded.
#[derive(Default)]
pub struct Library {
    pub nodes: HashMap<u32, Node>,
    pub events: HashMap<u32, Vec<Action>>,
}

fn u16le(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32le(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The node header at `o`: (parent, properties).
fn header(b: &[u8], o: usize) -> Option<(u32, Props)> {
    let nfx = *b.get(o + 1)? as usize;
    let mut p = o + 2 + if nfx > 0 { 1 + 7 * nfx } else { 0 };
    p += 1 + 4;
    let parent = u32le(b, p)?;
    p += 4 + 1;
    let n = *b.get(p)? as usize;
    let ids = b.get(p + 1..p + 1 + n)?;
    let vals = p + 1 + n;
    let mut props = Props::default();
    for (k, &id) in ids.iter().enumerate() {
        let raw = u32le(b, vals + 4 * k)?;
        let f = f32::from_bits(raw);
        match id {
            0x00 => props.volume = f,
            0x02 => props.pitch = f,
            0x3a => props.loops = Some(raw),
            0x3b => props.delay = f,
            _ => {}
        }
    }
    Some((parent, props))
}

impl Library {
    /// Add a bank's hierarchy.
    pub fn add(&mut self, bank: &Bank) {
        let objects: HashMap<u32, &Object> = bank.objects.iter().map(|o| (o.id, o)).collect();
        let known = |id: u32| objects.contains_key(&id) || self.nodes.contains_key(&id);
        let mut parsed: Vec<(u32, u8, u32, Props, usize)> = Vec::new();
        for o in &bank.objects {
            let b = bank.body(o);
            match o.kind {
                2 => {
                    // the header after the source (a source plugin's has its parameters first)
                    let heads: &[usize] = if u32le(b, 0) == Some(0x0065_0002) { &[26, 27, 30, 34, 22] } else { &[22, 26, 27, 30, 34] };
                    for &h in heads {
                        if let Some((parent, props)) = header(b, h)
                            && (parent == 0 || known(parent))
                        {
                            parsed.push((o.id, 2, parent, props, h));
                            break;
                        }
                    }
                }
                5 | 6 | 7 | 9 => {
                    if let Some((parent, props)) = header(b, 0) {
                        parsed.push((o.id, o.kind, parent, props, 0));
                    }
                }
                _ => {}
            }
        }
        let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
        for (id, _, parent, _, _) in &parsed {
            kids.entry(*parent).or_default().push(*id);
        }
        for (id, kind, parent, props, _) in parsed {
            let b = bank.body(objects[&id]);
            let children = kids.get(&id).cloned().unwrap_or_default();
            // the child list in the container's bytes
            let list_at = || -> Option<usize> {
                let n = children.len();
                if n == 0 {
                    return None;
                }
                (0..b.len().saturating_sub(4 + 4 * n)).find(|&i| u32le(b, i) == Some(n as u32) && (0..n).all(|j| u32le(b, i + 4 + 4 * j).is_some_and(|c| children.contains(&c))))
            };
            let kind = match kind {
                2 => Kind::Sound { source: u32le(b, 5).unwrap_or(0), silent: u32le(b, 0) == Some(0x0065_0002) },
                5 => match list_at() {
                    Some(i) if i >= 24 => {
                        let s = i - 24;
                        let loops = u16le(b, s).unwrap_or(1);
                        let avoid_repeat = u16le(b, s + 18).unwrap_or(0);
                        let f = |k: usize| u32le(b, s + k).map_or(0.0, f32::from_bits);
                        let transition = (b[s + 20], f(6), f(10), f(14));
                        let mode = b[s + 22];
                        let flags = b[s + 23];
                        let mut playlist = Vec::new();
                        let mut p = i + 4 + 4 * children.len();
                        let count = u16le(b, p).unwrap_or(0) as usize;
                        p += 2;
                        for _ in 0..count {
                            if let (Some(c), Some(w)) = (u32le(b, p), u32le(b, p + 4)) {
                                playlist.push((c, w as i32));
                            }
                            p += 8;
                        }
                        if playlist.is_empty() {
                            playlist = children.iter().map(|&c| (c, 50000)).collect();
                        }
                        Kind::RanSeq { random: mode == 0, continuous: flags & 0x08 != 0, loops, avoid_repeat, playlist, transition }
                    }
                    _ => Kind::RanSeq {
                        random: true,
                        continuous: false,
                        loops: 1,
                        avoid_repeat: 0,
                        playlist: children.iter().map(|&c| (c, 50000)).collect(),
                        transition: (0, 0.0, 0.0, 0.0),
                    },
                },
                6 => match list_at() {
                    Some(i) if i >= 10 => {
                        let s = i - 10;
                        let state = b[s] == 1;
                        let group = u32le(b, s + 1).unwrap_or(0);
                        let default = u32le(b, s + 5).unwrap_or(0);
                        let mut p = i + 4 + 4 * children.len();
                        let groups = u32le(b, p).unwrap_or(0) as usize;
                        p += 4;
                        let mut map = Vec::new();
                        for _ in 0..groups.min(4096) {
                            let (Some(sw), Some(n)) = (u32le(b, p), u32le(b, p + 4)) else { break };
                            p += 8;
                            let items: Vec<u32> = (0..n as usize).filter_map(|k| u32le(b, p + 4 * k)).collect();
                            p += 4 * n as usize;
                            map.push((sw, items));
                        }
                        Kind::Switch { state, group, default, map }
                    }
                    _ => Kind::Switch { state: false, group: 0, default: 0, map: Vec::new() },
                },
                9 => Kind::Layer,
                _ => Kind::Mixer,
            };
            self.nodes.insert(id, Node { parent, props, children, kind });
        }
        // events and their actions
        for o in bank.objects.iter().filter(|o| o.kind == 4) {
            let b = bank.body(o);
            let n = u32le(b, 0).unwrap_or(0) as usize;
            let mut acts = Vec::new();
            for k in 0..n.min(256) {
                let Some(aid) = u32le(b, 4 + 4 * k) else { break };
                let Some(a) = objects.get(&aid) else { continue };
                let ab = bank.body(a);
                let (Some(t), Some(target)) = (u16le(ab, 0), u32le(ab, 2)) else { continue };
                acts.push(match t {
                    0x0403 => Action::Play(target),
                    0x0102 | 0x0103 | 0x0104 | 0x0105 => Action::Stop(target),
                    0x1204 => {
                        // after its properties: group, state
                        let mut p = 7;
                        let n = *ab.get(p).unwrap_or(&0) as usize;
                        p += 1 + 5 * n;
                        let m = *ab.get(p).unwrap_or(&0) as usize;
                        p += 1 + 9 * m;
                        Action::SetState { group: u32le(ab, p).unwrap_or(0), state: u32le(ab, p + 4).unwrap_or(0) }
                    }
                    t => Action::Other(t),
                });
            }
            self.events.insert(o.id, acts);
        }
    }

    /// The volume (dB) and pitch (cents) a node plays at: its own and its parents' summed.
    pub fn mix(&self, mut id: u32) -> (f32, f32) {
        let (mut v, mut p) = (0.0, 0.0);
        for _ in 0..32 {
            let Some(n) = self.nodes.get(&id) else { break };
            v += n.props.volume;
            p += n.props.pitch;
            id = n.parent;
        }
        (v, p)
    }
}

/// What a node plays, picked now: the sounds (source, node) to start together, and for a
/// continuous container the rest to follow one after another.
#[derive(Clone, Debug, Default)]
pub struct Pick {
    pub sounds: Vec<(u32, u32)>,
}

/// Switches and states set by the game (group -> value).
pub type Switches = HashMap<u32, u32>;

impl Library {
    /// The sounds node `id` plays now (a random container one of its children by weight, a
    /// switch container the children of the switch set, else its default; a layer all).
    pub fn pick(&self, id: u32, sw: &Switches, rand: &mut dyn FnMut() -> f32, out: &mut Pick, depth: u32) {
        if depth > 16 {
            return;
        }
        let Some(n) = self.nodes.get(&id) else { return };
        match &n.kind {
            Kind::Sound { source, silent } => {
                if !silent {
                    out.sounds.push((*source, id));
                }
            }
            Kind::RanSeq { random, playlist, .. } => {
                if playlist.is_empty() {
                    return;
                }
                let c = if *random {
                    let total: f32 = playlist.iter().map(|(_, w)| (*w).max(0) as f32).sum();
                    let mut x = rand() * total.max(1.0);
                    playlist.iter().find(|(_, w)| {
                        x -= (*w).max(0) as f32;
                        x <= 0.0
                    }).or(playlist.last()).map(|p| p.0)
                } else {
                    playlist.first().map(|p| p.0)
                };
                if let Some(c) = c {
                    self.pick(c, sw, rand, out, depth + 1);
                }
            }
            Kind::Switch { group, default, map, .. } => {
                let want = sw.get(group).copied().unwrap_or(*default);
                let items = map.iter().find(|(s, _)| *s == want).or_else(|| map.iter().find(|(s, _)| s == default)).map(|(_, i)| i.clone());
                for c in items.unwrap_or_default() {
                    self.pick(c, sw, rand, out, depth + 1);
                }
            }
            Kind::Layer => {
                for &c in &n.children {
                    self.pick(c, sw, rand, out, depth + 1);
                }
            }
            Kind::Mixer => {}
        }
    }

    /// Whether node `id` loops (its own loop property or a container ancestor's set to 0).
    pub fn looped(&self, mut id: u32) -> bool {
        for _ in 0..32 {
            let Some(n) = self.nodes.get(&id) else { break };
            if n.props.loops == Some(0) {
                return true;
            }
            if let Kind::RanSeq { loops: 0, continuous: true, .. } = n.kind {
                return true;
            }
            id = n.parent;
        }
        false
    }
}
