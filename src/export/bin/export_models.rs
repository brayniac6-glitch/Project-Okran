//! Convert the character models of your own Kenshi install to glTF (`.glb`):
//! `cargo run --release -p kenshi-export --bin export-models -- <install> <out dir>`
//!
//! Writes, under `<out dir>`:
//! - `bodies/<race> male.glb`, `bodies/<race> female.glb`: the race's body (head cut away);
//! - `heads/<race> <sex>/<head>.glb`: each head the race offers (the body mesh's head, textured
//!   by the head record);
//! - `hair/<name>.glb`: hair and beard attachments, tinted brown, alpha-masked;
//! - `armour/<item> male.glb`, `armour/<item> female.glb`: clothing and armour;
//! - `textures/`: the textures, once each, as PNG (the models refer to them);
//! - `MANIFEST.txt`: every model written and the install files it came from.
//!
//! Models are static, in their bind pose, in metres. Kenshi's files belong to Lo-Fi Games: the
//! output is for your own use and for Kenshi mods, not to be handed on as it is.
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_export::{
    Material, Part, Texture, compact, glb, is_swizzled_normal, opaque, parts, png, split_head,
    tint_hair, unswizzle_normal,
};
use kenshi_io::{dds, mods::Install, ogre_mesh, resources::Resources};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Kenshi units are 10 cm (meshes are stored in them); glTF is in metres.
const SCALE: f32 = 0.1;
/// Hair tint (a mid brown).
const HAIR: [f32; 3] = [0.24, 0.16, 0.1];

fn safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_().'".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

struct Ctx<'a> {
    root: &'a Path,
    res: Resources,
    out: PathBuf,
    manifest: String,
    written: usize,
    failed: usize,
    /// Texture written: (source file, hair tinted) -> PNG file name.
    textures: HashMap<(PathBuf, bool), Option<String>>,
}

impl Ctx<'_> {
    fn file(&self, r: &GameData, key: &str) -> Option<PathBuf> {
        r.files
            .get(key)
            .filter(|p| !p.is_empty())
            .and_then(|p| self.res.path(p))
    }
    /// The texture of a record's file field, written once to `textures/`; referred to from a
    /// model `depth` folders below the output root.
    fn tex(&mut self, r: &GameData, key: &str, hair: bool, depth: usize) -> Option<Texture> {
        let src = self.file(r, key)?;
        let k = (src.clone(), hair);
        if !self.textures.contains_key(&k) {
            let name = dds::read(&src).ok().map(|t| {
                // hair "Mod" maps are tinted and keep their coverage alpha; normal maps are
                // unswizzled; other textures are made opaque
                let t = if hair {
                    tint_hair(&t, HAIR)
                } else if is_swizzled_normal(&t) {
                    unswizzle_normal(&t)
                } else {
                    opaque(&t)
                };
                let stem = src
                    .file_stem()
                    .map_or("texture".into(), |s| s.to_string_lossy().to_string());
                let mut name: String = stem
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                if hair {
                    name += "_hair";
                }
                // distinct files with the same stem
                let mut n = name.clone();
                let mut i = 1;
                while self
                    .textures
                    .values()
                    .any(|v| v.as_deref() == Some(&format!("{n}.png")))
                {
                    i += 1;
                    n = format!("{name}_{i}");
                }
                let file = format!("{n}.png");
                let dir = self.out.join("textures");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join(&file), png(&t));
                file
            });
            self.textures.insert(k.clone(), name);
        }
        let f = self.textures[&k].clone()?;
        Some(Texture::File(format!(
            "{}textures/{f}",
            "../".repeat(depth)
        )))
    }
    fn mesh(&self, path: &Path) -> Option<ogre_mesh::Mesh> {
        ogre_mesh::read(path).ok()
    }
    fn write(&mut self, rel: &str, name: &str, ps: &[Part], sources: &[&Path]) {
        if ps.iter().all(|p| p.indices.is_empty()) {
            return;
        }
        let path = self.out.join(rel);
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::write(&path, glb(name, ps)) {
            Ok(()) => {
                self.written += 1;
                let src: Vec<String> = sources
                    .iter()
                    .map(|s| s.strip_prefix(self.root).unwrap_or(s).display().to_string())
                    .collect();
                let _ = writeln!(self.manifest, "{rel}\t{}", src.join(", "));
            }
            Err(e) => {
                self.failed += 1;
                eprintln!("{rel}: {e}");
            }
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        eprintln!("usage: export-models <Kenshi install> <out dir>");
        std::process::exit(2);
    }
    let root = Path::new(&a[0]);
    let mut order = Install::new(root).load_order().expect("load order");
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).expect("game data");
    let mut cx = Ctx {
        root,
        res: Resources::index(root),
        out: PathBuf::from(&a[1]),
        manifest: String::from(
            "Exported from a local Kenshi install by kenshi-rewrite export-models.\n\
             Kenshi and its models are (c) Lo-Fi Games. For personal use and Kenshi mods only.\n\n\
             model\tsource files\n",
        ),
        written: 0,
        failed: 0,
        textures: HashMap::new(),
    };
    let mut hairs: BTreeSet<String> = BTreeSet::new();

    let mut races: Vec<&GameData> = data.of_type(ItemType::Race).collect();
    races.sort_by(|x, y| x.name.cmp(&y.name));
    for race in races {
        for sex in ["male", "female"] {
            let Some(mesh_path) = cx.file(race, &format!("{sex} mesh")) else {
                continue;
            };
            let Some(mesh) = cx.mesh(&mesh_path) else {
                cx.failed += 1;
                continue;
            };
            let body_mat = Material {
                name: format!("{} {sex} body", race.name),
                base: cx.tex(race, &format!("body texture {sex}"), false, 1),
                normal: cx.tex(race, &format!("nm {sex}"), false, 1),
                mask: false,
            };
            let (mut body, head) = split_head(parts(&mesh, &body_mat, SCALE));
            body.iter_mut().for_each(compact);
            let label = format!("{} {sex}", race.name);
            cx.write(
                &format!("bodies/{}.glb", safe(&label)),
                &label,
                &body,
                &[&mesh_path],
            );
            // the race's heads on the body's head geometry
            for (sid, _) in race.refs(&format!("heads {sex}")) {
                let Some(h) = data.get(sid) else { continue };
                let mat = Material {
                    name: h.name.clone(),
                    base: cx.tex(h, "texture map", false, 2),
                    normal: cx.tex(h, "normal map", false, 2),
                    mask: false,
                };
                let mut hp: Vec<Part> = head
                    .iter()
                    .map(|p| Part {
                        positions: p.positions.clone(),
                        normals: p.normals.clone(),
                        uvs: p.uvs.clone(),
                        indices: p.indices.clone(),
                        material: mat.clone(),
                    })
                    .collect();
                hp.iter_mut().for_each(compact);
                let src: Vec<PathBuf> = std::iter::once(mesh_path.clone())
                    .chain(cx.file(h, "texture map"))
                    .collect();
                let srcs: Vec<&Path> = src.iter().map(|p| p.as_path()).collect();
                cx.write(
                    &format!("heads/{}/{}.glb", safe(&label), safe(&h.name)),
                    &h.name,
                    &hp,
                    &srcs,
                );
            }
        }
        // hair and beards
        for (sid, _) in race.refs("hairs") {
            let Some(h) = data.get(sid) else { continue };
            if !hairs.insert(h.string_id.clone()) {
                continue;
            }
            let Some(mp) = cx.file(h, "mesh") else {
                continue;
            };
            let Some(mesh) = cx.mesh(&mp) else {
                cx.failed += 1;
                continue;
            };
            let mat = Material {
                name: h.name.clone(),
                base: cx.tex(h, "texture map", true, 1),
                normal: None,
                mask: true,
            };
            let ps = parts(&mesh, &mat, SCALE);
            cx.write(&format!("hair/{}.glb", safe(&h.name)), &h.name, &ps, &[&mp]);
        }
    }

    // clothing and armour
    let mut items: Vec<&GameData> = data.of_type(ItemType::Armour).collect();
    items.sort_by(|x, y| x.name.cmp(&y.name));
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for item in items {
        let mat_rec = item
            .refs("material")
            .first()
            .and_then(|(sid, _)| data.get(sid))
            .unwrap_or(item);
        for (key, sex) in [("mesh", "male"), ("mesh female", "female")] {
            let Some(mp) = cx.file(item, key) else {
                continue;
            };
            // one file per distinct mesh and material
            let mut tag = mp.clone();
            tag.push(&mat_rec.string_id);
            if !seen.insert(tag) {
                continue;
            }
            let Some(mesh) = cx.mesh(&mp) else {
                cx.failed += 1;
                continue;
            };
            let mat = Material {
                name: item.name.clone(),
                base: cx.tex(mat_rec, "texture map", false, 1),
                normal: cx.tex(mat_rec, "normal map", false, 1),
                mask: false,
            };
            let ps = parts(&mesh, &mat, SCALE);
            let label = format!("{} {sex}", item.name);
            cx.write(&format!("armour/{}.glb", safe(&label)), &label, &ps, &[&mp]);
        }
    }

    let manifest = std::mem::take(&mut cx.manifest);
    std::fs::create_dir_all(&cx.out).expect("out dir");
    std::fs::write(cx.out.join("MANIFEST.txt"), manifest).expect("manifest");
    println!(
        "{} models written to {}, {} failed",
        cx.written,
        cx.out.display(),
        cx.failed
    );
}
