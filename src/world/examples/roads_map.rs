//! The road network drawn over the map, with the route between two towns:
//! `cargo run -p kenshi-world --example roads_map -- <install> <out.png> [<town> <town> [road preference]]`
//! Roads grey (hidden ones darker), junctions white, the route red.
use kenshi_world::longpath::{DEFAULT_ROAD_PREFERENCE, WorldGrid};
use kenshi_world::{Terrain, WORLD_SIZE, town::towns};

const PX: u32 = 2048;

fn to_px(p: [f32; 3]) -> (f32, f32) {
    let s = PX as f32 / WORLD_SIZE;
    ((p[0] + WORLD_SIZE / 2.0) * s, (p[2] + WORLD_SIZE / 2.0) * s)
}

fn line(img: &mut image::RgbImage, a: [f32; 3], b: [f32; 3], c: [u8; 3]) {
    let (x0, y0) = to_px(a);
    let (x1, y1) = to_px(b);
    let n = ((x1 - x0).abs().max((y1 - y0).abs()) as i32).max(1);
    for k in 0..=n {
        let t = k as f32 / n as f32;
        let (x, y) = ((x0 + (x1 - x0) * t) as i32, (y0 + (y1 - y0) * t) as i32);
        if x >= 0 && y >= 0 && (x as u32) < PX && (y as u32) < PX {
            img.put_pixel(x as u32, y as u32, image::Rgb(c));
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let mut terrain = Terrain::open(install).unwrap();
    // the heightmap as the backdrop
    let step = terrain.samples() / PX;
    let patch = terrain.patch(0, 0, PX, PX, step).unwrap();
    let mut img = image::RgbImage::new(PX, PX);
    for y in 0..PX {
        for x in 0..PX {
            let h = patch.at(x, y);
            let v = if h < 100.0 { [20, 40, 70] } else { let g = (40.0 + h / 9800.0 * 180.0) as u8; [g, g, (g as f32 * 0.8) as u8] };
            img.put_pixel(x, y, image::Rgb(v));
        }
    }
    let grid = WorldGrid::build(&mut terrain).unwrap();
    for r in &grid.roads.roads {
        let c = if r.hidden { [120, 120, 120] } else { [235, 200, 120] };
        for w in r.markers.windows(2) {
            line(&mut img, w[0].position, w[1].position, c);
        }
    }
    for j in 0..grid.roads.junctions.len() {
        if grid.roads.junctions[j].roads.is_empty() {
            continue;
        }
        let (x, y) = to_px(grid.roads.junction_position(j));
        if x >= 0.0 && y >= 0.0 && (x as u32) < PX && (y as u32) < PX {
            img.put_pixel(x as u32, y as u32, image::Rgb([255, 255, 255]));
        }
    }
    if a.len() >= 4 {
        let mut order = kenshi_io::mods::Install::new(install).load_order().unwrap();
        order.files.retain(|f| f.core);
        let data = kenshi_data::GameDataContainer::load(&order).unwrap();
        let ts = towns(install, &data);
        let find = |n: &str| ts.iter().find(|t| t.name == n).unwrap_or_else(|| panic!("no town {n}"));
        let (from, to) = (find(&a[2]), find(&a[3]));
        let pref: f32 = a.get(4).map_or(DEFAULT_ROAD_PREFERENCE, |v| v.parse().unwrap());
        let route = grid.route_with(from.position, to.position, pref);
        let mut p = from.position;
        for q in &route {
            line(&mut img, p, *q, [255, 40, 40]);
            p = *q;
        }
        println!("{} waypoints, {:.0} units", route.len(), WorldGrid::length(from.position, &route));
    }
    img.save(&a[1]).unwrap();
}
