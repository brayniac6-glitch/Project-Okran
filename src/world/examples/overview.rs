//! Coarse ASCII height overview of the whole map:
//! `cargo run -p kenshi-world --example overview -- <install>`
fn main() {
    let root = std::env::args().nth(1).expect("install dir");
    let mut t = kenshi_world::Terrain::open(std::path::Path::new(&root)).unwrap();
    let n = 32;
    let step = (t.samples() - 1) / (n - 1);
    let p = t.patch(0, 0, n, n, step).unwrap();
    let (lo, hi) = p
        .heights
        .iter()
        .fold((f32::MAX, f32::MIN), |a, &h| (a.0.min(h), a.1.max(h)));
    println!(
        "{} samples, spacing {} units, heights {lo:.0}..{hi:.0}",
        t.samples(),
        t.spacing()
    );
    let ramp = b" .:-=+*#%@";
    for j in 0..n {
        // two characters per sample so the map is roughly square in a terminal
        let row: String = (0..n)
            .map(|i| ramp[(((p.at(i, j) - lo) / (hi - lo)) * 9.99) as usize] as char)
            .flat_map(|c| [c, c])
            .collect();
        println!("{row}");
    }
}
