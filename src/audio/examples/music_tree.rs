//! A music bank's interactive music: each event's music targets and what they hold (switches by
//! their decision trees, playlists' items, segments' durations and tracks):
//! `cargo run -p kenshi-audio --example music_tree -- <bank.bnk> [walks]`
use kenshi_audio::bnk::Bank;
use kenshi_audio::music::{Music, MusicKind, PlaylistWalk};
use kenshi_audio::tree::{Action, Library};

fn show(m: &Music, id: u32, depth: usize) {
    let pad = "  ".repeat(depth);
    let Some(n) = m.nodes.get(&id) else {
        println!("{pad}{id}: (not a music node)");
        return;
    };
    match &n.kind {
        MusicKind::Segment { duration, entry, exit, .. } => {
            let tracks: Vec<String> = n
                .children
                .iter()
                .map(|t| match m.nodes.get(t).map(|t| &t.kind) {
                    Some(MusicKind::Track { clips, kind, .. }) => format!("track(type {kind}, {} clips: {:?})", clips.len(), clips.iter().map(|c| (c.source, c.play_at as i32, c.duration as i32)).collect::<Vec<_>>()),
                    _ => "?".into(),
                })
                .collect();
            println!("{pad}segment {id}: {:.1} s (entry {:.1}, exit {:.1}) vol {} {}", duration / 1000.0, entry / 1000.0, exit / 1000.0, n.props.volume, if tracks.is_empty() { "SILENT".to_string() } else { tracks.join(", ") });
        }
        MusicKind::Switch { groups, tree, rules, .. } => {
            println!("{pad}switch {id}: groups {groups:x?}, {} rules (sync {:?})", rules.len(), rules.iter().map(|r| (r.sync, r.fade_out, r.fade_in)).collect::<Vec<_>>());
            for t in tree.iter().skip(1) {
                println!("{pad}  key {:08x} -> {}", t.key, t.node);
                if t.node != 0 && depth < 6 {
                    show(m, t.node, depth + 2);
                }
            }
        }
        MusicKind::Playlist { items, rules } => {
            println!("{pad}playlist {id}: {} items, {} rules (sync {:?})", items.len(), rules.len(), rules.iter().map(|r| (r.sync, r.fade_out, r.fade_in)).collect::<Vec<_>>());
            for it in items.iter().take(60) {
                let what = match it.kind {
                    -1 => {
                        let seg = m.nodes.get(&it.segment);
                        match seg.map(|s| (&s.kind, s.children.len())) {
                            Some((MusicKind::Segment { duration, .. }, k)) => format!("segment {} ({:.0} s{})", it.segment, duration / 1000.0, if k == 0 { ", silent" } else { "" }),
                            _ => format!("segment {}", it.segment),
                        }
                    }
                    k => format!("group type {k}, {} children", it.children),
                };
                println!("{pad}  item {}: {what} loops {} weight {} avoid {}", it.id, it.loops, it.weight, it.avoid_repeat);
            }
        }
        MusicKind::Track { .. } => println!("{pad}track {id}"),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let b = Bank::parse(std::fs::read(&a[0]).unwrap()).unwrap();
    let mut m = Music::default();
    m.add(&b);
    let mut lib = Library::default();
    lib.add(&b);
    println!("{} music nodes, unread {:?}", m.nodes.len(), m.unread);
    for (e, acts) in &lib.events {
        println!("event {e}:");
        for act in acts {
            match act {
                Action::Play(t) => {
                    println!(" play {t}");
                    show(&m, *t, 2);
                }
                other => println!(" {other:?}"),
            }
        }
    }
    // (walks) the segments a playlist plays, walked a few times
    if let Some(n) = a.get(1).and_then(|v| v.parse::<usize>().ok()) {
        let mut seed = 12345u32;
        let mut rand = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for (id, node) in &m.nodes {
            if let MusicKind::Playlist { items, .. } = &node.kind {
                let mut w = PlaylistWalk::default();
                for pass in 0..n {
                    let mut segs = Vec::new();
                    let mut walk = w.clone();
                    while let Some(s) = walk.next(items, &mut rand) {
                        segs.push(s);
                        if segs.len() > 40 {
                            break;
                        }
                    }
                    w.step = walk.step.clone();
                    w.recent = walk.recent.clone();
                    println!("playlist {id} pass {pass}: {segs:?}");
                }
            }
        }
    }
}
