//! (testing) How fast the simulation steps, and a fingerprint of its state to show a change to
//! the code leaves it the same (headless):
//! `cargo run --release -p kenshi-game --example sim_bench -- <install> X Y [seconds] [extras...]`
//! (extras as `Sim::load` takes them). Prints the wall time per step every 10 game seconds with
//! a hash of everyone's position, state, plan, inventory and wounds; with `KENSHI_PROFILE` set,
//! the time per step in each of `Sim::step`'s marked sections at the end.
use kenshi_game::sim::Sim;
use std::path::Path;
use std::time::Instant;

struct Fnv(u64);
impl Fnv {
    fn eat(&mut self, b: &[u8]) {
        for &x in b {
            self.0 ^= x as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
    fn f(&mut self, v: f32) {
        self.eat(&v.to_bits().to_le_bytes());
    }
    fn u(&mut self, v: u64) {
        self.eat(&v.to_le_bytes());
    }
}

fn fingerprint(sim: &Sim) -> u64 {
    let mut h = Fnv(0xcbf29ce484222325);
    h.u(sim.rng.0);
    h.u(sim.folk.len() as u64);
    for p in &sim.folk {
        for v in p.mover.position {
            h.f(v);
        }
        for v in p.facing {
            h.f(v);
        }
        h.u(p.dead as u64 | (p.down as u64) << 1 | (p.away as u64) << 2 | (p.player as u64) << 3);
        h.f(p.speed);
        if let Some(c) = &p.ai.current {
            h.u(c.task as u64);
            h.eat(format!("{:?}", c.target).as_bytes());
        }
        for s in &p.ai.queue {
            h.u(s.task as u64);
            h.eat(format!("{:?}", s.target).as_bytes());
        }
        h.u(p.inside.map_or(u64::MAX, |b| b as u64));
        h.u(p.town.map_or(u64::MAX, |b| b as u64));
        h.eat(format!("{:?}", p.inventory.sections).as_bytes());
        let f = &sim.arena.f[p.id];
        h.u(f.active as u64);
        h.eat(ron::to_string(&f.medical).unwrap_or_default().as_bytes());
    }
    // the save's lines, sorted (its maps are written in no fixed order)
    let save = ron::ser::to_string_pretty(&sim.snapshot((0, 0), |_| None), Default::default()).unwrap_or_default();
    let mut lines: Vec<&str> = save.lines().map(|l| l.trim()).collect();
    lines.sort_unstable();
    for l in lines {
        h.eat(l.as_bytes());
    }
    h.0
}

/// Each one's state at length (BENCH_DUMP=step), to see what two runs part on.
fn dump(sim: &Sim) {
    if let Ok(path) = std::env::var("BENCH_DUMP_SAVE") {
        let _ = std::fs::write(path, ron::ser::to_string_pretty(&sim.snapshot((0, 0), |_| None), Default::default()).unwrap_or_default());
    }
    for p in &sim.folk {
        let mut inv = Fnv(0xcbf29ce484222325);
        inv.eat(format!("{:?}", p.inventory.sections).as_bytes());
        let mut med = Fnv(0xcbf29ce484222325);
        med.eat(ron::to_string(&sim.arena.f[p.id].medical).unwrap_or_default().as_bytes());
        println!(
            "  {} at {:?} facing {:?} speed {} flags {}{}{} current {:?} queue {:?} inside {:?} town {:?} inv {:016x} med {:016x}",
            p.id,
            p.mover.position,
            p.facing,
            p.speed,
            p.dead as u8,
            p.down as u8,
            p.away as u8,
            p.ai.current.as_ref().map(|c| (c.task, c.target)),
            p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
            p.inside,
            p.town,
            inv.0,
            med.0
        );
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(120.0, |s| s.parse().unwrap());
    let extras: Vec<String> = a.iter().skip(4).cloned().collect();
    let t0 = Instant::now();
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &extras);
    println!("loaded in {:.1} s: {} people", t0.elapsed().as_secs_f64(), sim.folk.len());
    // BENCH_TOWNS: the world's towns and the zone each stands in, to find a busy one
    if std::env::var("BENCH_TOWNS").is_ok() {
        for t in &sim.towns {
            let (x, y) = kenshi_world::zone_of(t.position[0], t.position[2]);
            println!("{x:2} {y:2}  {}", t.name);
        }
        return;
    }
    let (mut steps, mut spent, mut worst) = (0u64, 0.0f64, 0.0f64);
    let (mut window_steps, mut window_spent) = (0u64, 0.0f64);
    let mut next_report = 10.0;
    while sim.t < seconds {
        let s = Instant::now();
        sim.step();
        let d = s.elapsed().as_secs_f64();
        kenshi_render::prof::count_frame();
        steps += 1;
        spent += d;
        worst = worst.max(d);
        window_steps += 1;
        window_spent += d;
        // BENCH_TRACE=n: the hash after each of the first n steps (to find where two runs part)
        if std::env::var("BENCH_TRACE").ok().and_then(|v| v.parse::<u64>().ok()).is_some_and(|n| steps <= n) {
            println!("step {steps} rng {:016x} hash {:016x}", sim.rng.0, fingerprint(&sim));
        }
        if std::env::var("BENCH_DUMP").ok().and_then(|v| v.parse::<u64>().ok()) == Some(steps) {
            dump(&sim);
        }
        if sim.t >= next_report {
            next_report += 10.0;
            println!("t {:5.0}  {:6.3} ms/step  people {:4}  hash {:016x}", sim.t, window_spent * 1000.0 / window_steps as f64, sim.folk.len(), fingerprint(&sim));
            window_steps = 0;
            window_spent = 0.0;
        }
    }
    println!("final hash {:016x}", fingerprint(&sim));
    println!("{steps} steps: {:.3} ms/step, worst {:.1} ms", spent * 1000.0 / steps.max(1) as f64, worst * 1000.0);
    kenshi_render::prof::report();
}
