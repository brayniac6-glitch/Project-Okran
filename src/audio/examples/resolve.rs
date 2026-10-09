//! What events play: `cargo run -p kenshi-audio --example resolve -- <install> <event> [group=value ...]`
use kenshi_audio::{bnk::{name_id, Bank}, tree::{Action, Library, Pick, Switches}};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::Path::new(&a[0]).join("data/audio");
    let mut lib = Library::default();
    let mut banks = Vec::new();
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = f.path();
        if p.extension().is_some_and(|e| e == "bnk") && !p.ends_with("SFX_Bank.bnk") {
            let b = Bank::parse(std::fs::read(&p).unwrap()).unwrap();
            lib.add(&b);
            banks.push(b);
        }
    }
    let mut sw = Switches::new();
    for kv in &a[2..] {
        let (k, v) = kv.split_once('=').unwrap();
        sw.insert(name_id(k), name_id(v));
    }
    let mut seed = 12345u32;
    let mut rand = move || { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; (seed % 10000) as f32 / 10000.0 };
    let ev = name_id(&a[1]);
    let acts = lib.events.get(&ev).cloned().unwrap_or_default();
    println!("{} nodes; {} -> {:?}", lib.nodes.len(), a[1], acts);
    for act in acts {
        if let Action::Play(t) = act {
            println!("  target {t}: {:?}", lib.nodes.get(&t).map(|n| (&n.kind, n.children.len())));
            for _ in 0..4 {
                let mut p = Pick::default();
                lib.pick(t, &sw, &mut rand, &mut p, 0);
                let s: Vec<String> = p.sounds.iter().map(|(src, node)| {
                    let (v, pi) = lib.mix(*node);
                    let bank = banks.iter().position(|b| b.sounds.contains_key(src));
                    format!("{src} (bank {bank:?}, {v:.0} dB, {pi:.0} c, loop {})", lib.looped(*node))
                }).collect();
                println!("    {s:?}");
            }
        }
    }
}
