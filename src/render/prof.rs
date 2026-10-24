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
    spent: HashMap<&'static str, (f64, u32, f64)>,
    /// The last frame's start, the window's frame times (ms).
    last: Option<Instant>,
    times: Vec<f32>,
}

fn totals() -> &'static Mutex<Totals> {
    static T: OnceLock<Mutex<Totals>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(Totals { since: Instant::now(), frames: 0, spent: HashMap::new(), last: None, times: Vec::new() }))
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
    e.2 = e.2.max(secs);
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
    let now = Instant::now();
    if let Some(l) = t.last {
        t.times.push((now - l).as_secs_f32() * 1000.0);
    }
    t.last = Some(now);
    if t.since.elapsed().as_secs_f64() < 5.0 {
        return;
    }
    print_totals(&mut t);
}

/// One frame counted, with no print (headless benches: `report` at the end).
pub fn count_frame() {
    if on() {
        totals().lock().unwrap().frames += 1;
    }
}

/// The totals per frame since the last print, now (headless benches: once at the end).
pub fn report() {
    if !on() {
        return;
    }
    print_totals(&mut totals().lock().unwrap());
}

fn print_totals(t: &mut Totals) {
    let frames = t.frames.max(1) as f64;
    let mut v: Vec<_> = t.spent.iter().map(|(k, (s, n, m))| (*k, *s, *n, *m)).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    let ms = t.since.elapsed().as_secs_f64() * 1000.0 / frames;
    eprintln!("profile: {:.0} frames, {ms:.1} ms/frame", frames);
    // the frame times: median, 95th percentile, longest, and the frames over 33 / 50 / 100 ms
    let mut ft = std::mem::take(&mut t.times);
    ft.sort_by(f32::total_cmp);
    if !ft.is_empty() {
        let at = |q: f32| ft[((ft.len() - 1) as f32 * q) as usize];
        let over = |x: f32| ft.iter().filter(|&&v| v > x).count();
        eprintln!(
            "  frame times: median {:.1} ms, p95 {:.1}, max {:.1}; over 33 ms {}, 50 ms {}, 100 ms {}",
            at(0.5),
            at(0.95),
            ft[ft.len() - 1],
            over(33.0),
            over(50.0),
            over(100.0)
        );
    }
    for (k, s, n, m) in v {
        eprintln!("  {:7.2} ms/frame  x{:5.1}  max {:7.2}  {k}", s * 1000.0 / frames, n as f64 / frames, m * 1000.0);
    }
    t.spent.clear();
    t.frames = 0;
    t.since = Instant::now();
}
