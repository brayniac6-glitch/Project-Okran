//! Instance position ranges per zone file, to relate `zone.X.Y` names to world coordinates:
//! `cargo run -p kenshi-world --example zones -- <dir with zone files>`
use kenshi_io::fcs;

fn main() {
    let dir = std::env::args().nth(1).expect("zone dir");
    let mut rows = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() != 4 || parts[0] != "zone" {
            continue;
        }
        let (zx, zy): (i32, i32) = (parts[1].parse().unwrap(), parts[2].parse().unwrap());
        let f = fcs::read(&p).unwrap();
        let pos: Vec<[f32; 3]> = f
            .records
            .iter()
            .flat_map(|r| r.instances.iter().map(|i| i.position))
            .collect();
        if pos.is_empty() {
            continue;
        }
        let mean = |k: usize| pos.iter().map(|p| p[k]).sum::<f32>() / pos.len() as f32;
        let (mx, mz) = (mean(0), mean(2));
        let lo = |k: usize| pos.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
        let hi = |k: usize| pos.iter().map(|p| p[k]).fold(f32::MIN, f32::max);
        rows.push((
            zx,
            zy,
            pos.len(),
            mx,
            mz,
            lo(0),
            hi(0),
            lo(2),
            hi(2),
            mean(1),
        ));
    }
    rows.sort_by_key(|r| (r.0, r.1));
    for r in rows.iter().take(12) {
        println!(
            "zone {:>2}.{:<2} n={:<5} mean x {:>9.0} z {:>9.0} | x {:>9.0}..{:<9.0} z {:>9.0}..{:<9.0} y {:.0}",
            r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9
        );
    }
    // least-squares fit of mean position against zone index for both axes and both pairings
    let fit = |xs: &[f32], ys: &[f32]| {
        let n = xs.len() as f32;
        let (mx, my) = (xs.iter().sum::<f32>() / n, ys.iter().sum::<f32>() / n);
        let cov: f32 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
        let var: f32 = xs.iter().map(|x| (x - mx).powi(2)).sum();
        let slope = cov / var;
        let r = cov / (var.sqrt() * ys.iter().map(|y| (y - my).powi(2)).sum::<f32>().sqrt());
        (slope, my - slope * mx, r)
    };
    let zx: Vec<f32> = rows.iter().map(|r| r.0 as f32).collect();
    let zy: Vec<f32> = rows.iter().map(|r| r.1 as f32).collect();
    let px: Vec<f32> = rows.iter().map(|r| r.3).collect();
    let pz: Vec<f32> = rows.iter().map(|r| r.4).collect();
    for (label, a, b) in [
        ("x ~ X", &zx, &px),
        ("z ~ Y", &zy, &pz),
        ("x ~ Y", &zy, &px),
        ("z ~ X", &zx, &pz),
    ] {
        let (s, c, r) = fit(a, b);
        println!("{label}: slope {s:.1} intercept {c:.1} r {r:.3}");
    }
    println!("{} zones with instances", rows.len());
}
