//! Every sound of every bank made into Ogg Vorbis and decoded (lewton), counting failures:
//! `cargo run -p kenshi-audio --example decode_all -- <install> [bank filter] [out.ogg]`
use kenshi_audio::{bnk::Bank, wem};
use std::io::Cursor;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::Path::new(&a[0]).join("data/audio");
    let filter = a.get(1).cloned().unwrap_or_default();
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "bnk")).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if !name.contains(&filter) {
            continue;
        }
        let bank = Bank::parse(std::fs::read(&p).unwrap()).unwrap();
        let (mut ok, mut bad, mut secs) = (0, 0, 0.0f64);
        let mut ids: Vec<_> = bank.sounds.keys().copied().collect();
        ids.sort();
        for id in ids {
            let w = bank.sound(id).unwrap();
            match wem::to_ogg(w) {
                Ok(ogg) => {
                    if let Some(out) = a.get(2) && ok == 0 {
                        std::fs::write(out, &ogg).unwrap();
                    }
                    match lewton::inside_ogg::OggStreamReader::new(Cursor::new(ogg)) {
                        Ok(mut r) => {
                            let (ch, rate) = (r.ident_hdr.audio_channels as f64, r.ident_hdr.audio_sample_rate as f64);
                            let mut n = 0usize;
                            let mut fail = None;
                            loop {
                                match r.read_dec_packet_itl() {
                                    Ok(Some(p)) => n += p.len(),
                                    Ok(None) => break,
                                    Err(e) => { fail = Some(e); break }
                                }
                            }
                            match fail {
                                None => { ok += 1; secs += n as f64 / ch / rate; }
                                Some(e) => { bad += 1; if bad <= 3 { eprintln!("  {id}: decode {e:?} after {n}"); } }
                            }
                        }
                        Err(e) => { bad += 1; if bad <= 3 { eprintln!("  {id}: open {e:?}"); } }
                    }
                }
                Err(e) => { bad += 1; if bad <= 3 { eprintln!("  {id}: convert {e}"); } }
            }
        }
        println!("{name}: v{} {} sounds, {} objects: {ok} ok ({secs:.0} s), {bad} failed", bank.version, bank.sounds.len(), bank.objects.len());
    }
}
