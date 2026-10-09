//! Zones whose middle's ground lies between two heights (for finding weather effects' ground):
//! `cargo run --release -p kenshi-world --example zone_heights -- <install> MIN MAX [x0 y0 x1 y1]`
use kenshi_world::Terrain;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut t = Terrain::open(std::path::Path::new(&a[0])).unwrap();
    let (lo, hi): (f32, f32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let r: Vec<i32> = a.iter().skip(3).map(|v| v.parse().unwrap()).collect();
    let (x0, y0, x1, y1) = if r.len() == 4 { (r[0], r[1], r[2], r[3]) } else { (0, 0, 63, 63) };
    let c = |i: i32| (i as f32 + 0.5) * kenshi_world::ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let h = t.height_at(c(x), c(y)).unwrap_or(0.0);
            if lo <= h && h <= hi {
                println!("{x} {y} {h:.0}");
            }
        }
    }
}
