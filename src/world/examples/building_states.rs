//! The zone files' building states (GAMESTATE_BUILDING): how often "power on" and "is complete"
//! take each value, and the buildings with power on:
//! `cargo run -p kenshi-world --example building_states -- <install>`
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut powered: BTreeMap<String, usize> = BTreeMap::new();
    for x in 0..64 {
        for y in 0..64 {
            let Some(z) = kenshi_world::read_zone(install, x, y) else { continue };
            for r in z.records.iter().filter(|r| r.kind == 35) {
                let b = |k: &str| r.bools.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
                let key = format!("power on {:?} complete {:?}", b("power on"), b("is complete"));
                *counts.entry(key).or_default() += 1;
                if b("power on") == Some(false) {
                    // the building this state belongs to: the record placing it
                    let name = z
                        .records
                        .iter()
                        .flat_map(|p| p.instances.iter())
                        .find(|i| i.states.iter().any(|s| *s == r.string_id))
                        .map_or("?".to_string(), |i| i.target.clone());
                    *powered.entry(format!("{name} zone {x}.{y}")).or_default() += 1;
                }
            }
        }
    }
    for (k, v) in counts {
        println!("{v:6} {k}");
    }
    println!("power off:");
    for (k, v) in powered.iter().take(40) {
        println!("{v:6} {k}");
    }
}
