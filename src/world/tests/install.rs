//! Reads every shipped navmesh tile. Needs the player's copy, so it is opt-in:
//!   KENSHI_DIR="C:/Program Files (x86)/Steam/steamapps/common/Kenshi" cargo test --release -p kenshi-world -- --ignored

use kenshi_world::navmesh::{Section, TILES, load_tile, tile_uid};
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
#[ignore = "needs KENSHI_DIR pointing at a Kenshi install"]
fn every_navtile_loads_and_links_resolve() {
    let install = PathBuf::from(std::env::var("KENSHI_DIR").expect("set KENSHI_DIR"));
    let mut sections: HashMap<i64, Section> = HashMap::new();
    let mut tiles = 0;
    for x in 0..TILES {
        for y in 0..TILES {
            let s = load_tile(&install, x, y).unwrap_or_else(|e| panic!("tile {x}.{y}: {e}"));
            if s.is_empty() {
                continue;
            }
            tiles += 1;
            assert!(
                s.iter()
                    .any(|s| s.info.is_none() && s.uid == tile_uid(x, y)),
                "tile {x}.{y} ground uid"
            );
            for s in s {
                assert!(sections.insert(s.uid, s).is_none(), "uid seen twice");
            }
        }
    }
    assert!(tiles > 3000, "{tiles} tiles");
    // every streaming link names a loaded section and an edge there that links back
    let (mut links, mut missing, mut one_way, mut stale) = (0, 0, 0, 0);
    for s in sections.values() {
        for (&edge, to) in &s.links {
            let here = s
                .faces
                .iter()
                .position(|f| (f.start_edge..f.start_edge + f.num_edges).contains(&edge))
                .map(|f| (s.uid, f as u32, edge))
                .unwrap();
            for &(other, face, oe) in to {
                links += 1;
                let Some(o) = sections.get(&other) else {
                    missing += 1;
                    continue;
                };
                if face as usize >= o.faces.len() || oe as usize >= o.edges.len() {
                    stale += 1;
                    continue;
                }
                if !o.links.get(&oe).into_iter().flatten().any(|&b| b == here) {
                    one_way += 1;
                }
            }
        }
    }
    println!(
        "{tiles} tiles, {} sections, {links} links, {missing} to absent sections, {stale} out of range, {one_way} one way",
        sections.len()
    );
    // the shipped seams are slightly stale (tiles regenerated alone): ~1.7% one way, ~140 past the end
    assert!(missing == 0 && (stale + one_way) * 100 < links * 3);
}
