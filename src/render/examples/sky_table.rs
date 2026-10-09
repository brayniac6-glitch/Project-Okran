//! The ported sky light through the day: `cargo run -p kenshi-render --example sky_table -- <install>`
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;
use kenshi_render::sky::SkySettings;

fn main() {
    let install = std::env::args().nth(1).expect("install");
    let mut order = Install::new(&install).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let s = SkySettings::from_data(&data);
    println!("{s:?}");
    for h in [0.0, 4.0, 5.0, 6.0, 8.0, 11.0, 14.0, 17.0, 20.0, 22.0, 23.0] {
        let (dir, c, w, min_lum) = s.main_light(h);
        println!(
            "{h:5.1}h sun ({:+.2} {:+.2} {:+.2}) colour ({:.3} {:.3} {:.3}) w {w:.2} minLum {min_lum:.2}",
            dir.x, dir.y, dir.z, c.x, c.y, c.z
        );
    }
}
