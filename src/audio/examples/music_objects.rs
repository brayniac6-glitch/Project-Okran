//! The music objects of a bank (kinds 10 segment, 11 track, 12 switch, 13 random / sequence):
//! `cargo run -p kenshi-audio --example music_objects -- <bank.bnk> [n]` prints the bank version,
//! each music object's kind, id, size and first bytes, and the events' actions.
use kenshi_audio::bnk::Bank;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let b = Bank::parse(std::fs::read(&a[0]).unwrap()).unwrap();
    let n: usize = a.get(1).map_or(64, |v| v.parse().unwrap());
    println!("version {} id {} objects {} sounds {}", b.version, b.id, b.objects.len(), b.sounds.len());
    let mut counts = std::collections::BTreeMap::new();
    for o in &b.objects {
        *counts.entry(o.kind).or_insert(0) += 1;
    }
    println!("kinds {counts:?}");
    for o in b.objects.iter().filter(|o| (10..=13).contains(&o.kind) || o.kind == 4 || o.kind == 3) {
        let body = b.body(o);
        let hex: Vec<String> = body.iter().take(n).map(|x| format!("{x:02x}")).collect();
        println!("kind {:2} id {:10} len {:5} {}", o.kind, o.id, body.len(), hex.join(" "));
    }
}
