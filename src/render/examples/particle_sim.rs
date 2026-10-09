//! Every particle system template of an install run for a while at 60 frames a second, with
//! the particles alive at each second (a check that the port runs them all):
//! `cargo run -p kenshi-render --example particle_sim -- <install> [template] [seconds]`.
use kenshi_render::particles::library::ParticleLibrary;
use kenshi_render::particles::system::System;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let lib = ParticleLibrary::load(std::path::Path::new(&a[0]));
    let seconds: u32 = a.get(2).map_or(5, |s| s.parse().unwrap());
    let mut names: Vec<&String> = lib.templates.keys().collect();
    names.sort();
    println!("{} templates, {} materials", lib.templates.len(), lib.materials.len());
    for name in names {
        if a.get(1).is_some_and(|w| w != "*" && w != name) {
            continue;
        }
        let mut s = System::instance(&lib.templates[name], 1);
        s.prepare();
        s.update(0.0);
        if std::env::var("PSIM_TRACE").is_ok() {
            eprintln!("starting");
        }
        s.start();
        let mut counts = Vec::new();
        for frame in 0..seconds * 60 {
            s.update(1.0 / 60.0);
            if std::env::var("PSIM_TRACE").is_ok() {
                eprintln!("frame {frame} {}", s.num_emitted());
            }
            if frame % 60 == 59 {
                counts.push(s.num_emitted());
            }
        }
        if std::env::var("PSIM_DUMP").is_ok() {
            s.for_each_visible(|t, visuals, order| {
                for &i in order.iter().take(3) {
                    let v = &visuals[i as usize];
                    eprintln!(
                        "{} {}: at {:.2} size {:.2}x{:.2} own {} colour {:.2}",
                        t.name, t.material, v.position, v.width, v.height, v.own_dimensions, v.colour
                    );
                }
            });
        }
        let techs: Vec<String> = s
            .techniques
            .iter()
            .map(|t| format!("{}:{}", t.name, t.pool.visual.released_len))
            .collect();
        println!("{name:32} {:?} {:?}  [{}]", s.state, counts, techs.join(" "));
    }
}
