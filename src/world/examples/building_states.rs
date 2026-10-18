//! The zone files' building states (GAMESTATE_BUILDING): how often "power on", "is complete",
//! "is inside building" and the "flags" bits take each value, and the buildings with power off:
//! `cargo run -p kenshi-world --example building_states -- <install>`
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut off: BTreeMap<String, usize> = BTreeMap::new();
    let mut inside: BTreeMap<String, usize> = BTreeMap::new();
    for x in 0..64 {
        for y in 0..64 {
            let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
            for r in z.records.iter().filter(|r| r.kind == 35) {
                let b = |k: &str| r.bools.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
                let i = |k: &str| r.ints.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
                let key = format!(
                    "power on {:?} complete {:?} inside {:?} flags {:?}",
                    b("power on"),
                    b("is complete"),
                    b("is inside building"),
                    i("flags")
                );
                *counts.entry(key).or_default() += 1;
                let name = || {
                    z.records
                        .iter()
                        .flat_map(|p| p.instances.iter())
                        .find(|i| i.states.iter().any(|s| *s == r.string_id))
                        .map_or("?".to_string(), |i| i.target.clone())
                };
                if b("power on") == Some(false) {
                    *off.entry(format!("{} zone {x}.{y}", name())).or_default() += 1;
                }
                if b("is inside building") == Some(true) {
                    *inside.entry(name()).or_default() += 1;
                }
            }
        }
    }
    for (k, v) in counts {
        println!("{v:6} {k}");
    }
    println!("power off:");
    for (k, v) in off.iter().take(40) {
        println!("{v:6} {k}");
    }
    println!("inside a building:");
    for (k, v) in inside.iter().take(40) {
        println!("{v:6} {k}");
    }
}
