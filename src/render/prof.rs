//! (testing) KENSHI_PROFILE: wall-clock time per named stretch of code, summed and printed every
//! five seconds. Off (one env lookup, cached) unless the variable is set.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("KENSHI_PROFILE").is_ok())
}

struct Totals {
    since: Instant,
    frames: u32,
    spent: HashMap<&'static str, (f64, u32)>,
}

fn totals() -> &'static Mutex<Totals> {
    static T: OnceLock<Mutex<Totals>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(Totals { since: Instant::now(), frames: 0, spent: HashMap::new() }))
}

/// Add `secs` to `name`'s total.
pub fn add(name: &'static str, secs: f64) {
    if !on() {
        return;
    }
    let mut t = totals().lock().unwrap();
    let e = t.spent.entry(name).or_default();
    e.0 += secs;
    e.1 += 1;
}

/// Successive marks: each adds the time since the last one to its name.
pub struct Marks(Option<Instant>);

impl Marks {
    pub fn start() -> Marks {
        Marks(on().then(Instant::now))
    }
    pub fn mark(&mut self, name: &'static str) {
        if let Some(t) = self.0 {
            let now = Instant::now();
            add(name, (now - t).as_secs_f64());
            self.0 = Some(now);
        }
    }
}

/// Once a frame: every five seconds, the totals per frame, largest first.
pub fn frame() {
    if !on() {
        return;
    }
    let mut t = totals().lock().unwrap();
    t.frames += 1;
    if t.since.elapsed().as_secs_f64() < 5.0 {
        return;
    }
    let frames = t.frames.max(1) as f64;
    let mut v: Vec<_> = t.spent.iter().map(|(k, (s, n))| (*k, *s, *n)).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    let ms = t.since.elapsed().as_secs_f64() * 1000.0 / frames;
    eprintln!("profile: {:.0} frames, {ms:.1} ms/frame", frames);
    for (k, s, n) in v {
        eprintln!("  {:7.2} ms/frame  x{:5.1}  {k}", s * 1000.0 / frames, n as f64 / frames);
    }
    t.spent.clear();
    t.frames = 0;
    t.since = Instant::now();
}
