//! Which heightmap orientation puts placed buildings on the ground? Compares building heights
//! from zone files with the terrain under them for each candidate mapping:
//! `cargo run -p kenshi-world --example orient -- <install>`
use kenshi_io::fcs;
use kenshi_world::{HEIGHT_RANGE, Terrain, WORLD_SIZE};
use std::path::Path;

fn main() {
    let root = std::env::args().nth(1).expect("install dir");
    let use_state_y = std::env::args().any(|a| a == "--state-y");
    let mut t = Terrain::open(Path::new(&root)).unwrap();
    let dir = Path::new(&root).join("data/newland/leveldata/Newwworld");
    let mut pts = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if !e.file_name().to_string_lossy().starts_with("zone.") {
            continue;
        }
        let f = fcs::read(&e.path()).unwrap();
        // state records carry the building's true ground height ("world Y pos")
        let world_y = |sid: &str| {
            f.records.iter().find(|r| r.string_id == sid).and_then(|r| {
                r.floats
                    .iter()
                    .find(|(k, _)| k == "world Y pos")
                    .map(|(_, v)| *v)
            })
        };
        for r in &f.records {
            for i in &r.instances {
                // placed buildings: instances pointing at game data, not at other -INGAME records
                if i.target.contains("-INGAME") {
                    continue;
                }
                let [x, y, z] = i.position;
                let y = if use_state_y {
                    match i.states.iter().find_map(|s| world_y(s)) {
                        Some(v) => v,
                        None => continue,
                    }
                } else {
                    y
                };
                pts.push((x, y, z));
            }
        }
    }
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let step = (pts.len() / 400).max(1);
    let sample: Vec<_> = pts.into_iter().step_by(step).collect();
    let n = t.samples();
    let half = WORLD_SIZE / 2.0;
    for (label, fx, fz) in [
        ("col = x, row = z", 1.0f32, 1.0f32),
        ("col = x, row = -z", 1.0, -1.0),
        ("col = -x, row = z", -1.0, 1.0),
        ("col = -x, row = -z", -1.0, -1.0),
    ] {
        // regress building y on the heightmap value: the right mapping correlates, its slope is the
        // true height scale per 16-bit step and its intercept the vertical offset
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for &(x, y, z) in &sample {
            let col = ((fx * x + half) / t.spacing())
                .round()
                .clamp(0.0, (n - 1) as f32) as u32;
            let row = ((fz * z + half) / t.spacing())
                .round()
                .clamp(0.0, (n - 1) as f32) as u32;
            let h = t.patch(col, row, 1, 1, 1).unwrap().heights[0];
            xs.push(h / HEIGHT_RANGE * 65535.0);
            ys.push(y);
        }
        let k = xs.len() as f32;
        let (mx, my) = (xs.iter().sum::<f32>() / k, ys.iter().sum::<f32>() / k);
        let cov: f32 = xs.iter().zip(&ys).map(|(a, b)| (a - mx) * (b - my)).sum();
        let vx: f32 = xs.iter().map(|a| (a - mx).powi(2)).sum();
        let vy: f32 = ys.iter().map(|b| (b - my).powi(2)).sum();
        let slope = cov / vx;
        println!(
            "{label:<20} r {:.3}  y = {slope:.5} * value + {:.1}  (full range {:.0})",
            cov / (vx * vy).sqrt(),
            my - slope * mx,
            slope * 65535.0
        );
    }
}
