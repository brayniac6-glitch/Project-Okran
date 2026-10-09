//! Every weapon's and crossbow's held meshes, their submesh materials, UV ranges and the texture
//! they get:
//! `cargo run -p kenshi-render --example weapon_tex -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use kenshi_io::resources::Resources;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let order = Install::new(&a[0]).load_order().unwrap();
    let c = GameDataContainer::load(&order).unwrap();
    let res = Resources::index(&a[0]);
    let (mut ok, mut bad) = (0, 0);
    for kind in [ItemType::Weapon, ItemType::Crossbow] {
        let mut recs: Vec<_> = c.of_type(kind).collect();
        recs.sort_by(|x, y| x.name.cmp(&y.name));
        for r in recs {
            let mat = kenshi_render::appearance_material(&c, r);
            let tex = mat.and_then(|m| m.files.get("texture map")).filter(|p| !p.is_empty()).cloned();
            let found = tex.as_deref().and_then(|t| res.find(t));
            for k in ["bare sword", "mesh"] {
                let Some(p) = r.files.get(k).filter(|p| !p.is_empty()) else { continue };
                let subs = res
                    .path(p)
                    .and_then(|p| kenshi_io::ogre_mesh::read(&p).ok())
                    .map(|m| m.submeshes.iter().map(|s| s.material.clone()).collect::<Vec<_>>());
                let good = found.is_some() && subs.is_some();
                if let Some(pp) = res.path(p) { println!("   {}", uv_range(&pp)); }
                if good { ok += 1 } else { bad += 1 }
                println!("{} {:?} [{k}] {p} subs={subs:?} mat={:?} tex={tex:?} found={}", if good { "OK " } else { "BAD" }, r.name, mat.map(|m| &m.name), found.is_some());
                break;
            }
        }
    }
    println!("ok {ok} bad {bad}");
}

fn uv_range(path: &std::path::Path) -> String {
    let m = kenshi_io::ogre_mesh::read(path).unwrap();
    let mut out = String::new();
    for s in &m.submeshes {
        let g = s.geometry.as_ref().or(m.shared.as_ref()).unwrap();
        for set in 0..3 {
            if let Some(uv) = g.uvs(set) {
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for p in &uv {
                    for k in 0..2 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                out += &format!("set{set} {lo:?}..{hi:?} ");
            }
        }
        out += &format!("elements {:?}; ", g.elements.iter().map(|e| (e.semantic, e.kind, e.index)).collect::<Vec<_>>());
    }
    out
}
