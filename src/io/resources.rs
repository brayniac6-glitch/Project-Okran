//! Ogre-style resource lookup (port_map `ogre_assets`): Ogre finds meshes, skeletons and textures by
//! file name anywhere in its resource folders, so paths stored in game data may be stale (e.g.
//! `character/skins/hu/heads/x.dds` when the file is in `character/skins/human_male/`). Stored paths
//! are tried first; otherwise the file name is looked up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Resources {
    root: PathBuf,
    /// lowercase file name -> path relative to `root`, first found in sorted directory order
    by_name: HashMap<String, PathBuf>,
    /// The names an enabled mod has its own file for (found before the stored paths).
    overridden: std::collections::HashSet<String>,
}

/// A game-data path (`.\data\x\y.dds`) -> relative path with forward slashes (`data/x/y.dds`).
pub fn normalize(stored: &str) -> String {
    stored
        .trim_start_matches(".\\")
        .trim_start_matches("./")
        .replace('\\', "/")
}

impl Resources {
    /// Index every file under `<root>/data`.
    pub fn index(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut by_name = HashMap::new();
        let mut overridden = std::collections::HashSet::new();
        walk(&root, Path::new("data"), &mut by_name);
        // the enabled mods' files over the game's, the last mod winning
        for dir in crate::mods::Install::new(&root).mod_dirs() {
            let mut theirs = HashMap::new();
            match dir.strip_prefix(&root) {
                Ok(rel) => walk(&root, rel, &mut theirs),
                Err(_) => walk(&dir, Path::new(""), &mut theirs),
            }
            for (name, p) in theirs {
                let p = if p.is_relative() && dir.strip_prefix(&root).is_err() { dir.join(p) } else { p };
                overridden.insert(name.clone());
                by_name.insert(name, p);
            }
        }
        Resources { root, by_name, overridden }
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// A game-data path or bare file name -> path relative to the install root, with forward
    /// slashes, if the file exists.
    pub fn find(&self, stored: &str) -> Option<String> {
        let rel = normalize(stored);
        // a mod's own file first
        if let Some(name) = rel.rsplit('/').next().map(str::to_lowercase)
            && self.overridden.contains(&name)
            && let Some(p) = self.by_name.get(&name)
        {
            return Some(p.to_string_lossy().replace('\\', "/"));
        }
        if !rel.is_empty() && self.root.join(&rel).is_file() {
            return Some(rel);
        }
        let name = rel.rsplit('/').next()?.to_lowercase();
        if let Some(p) = self.by_name.get(&name) {
            return Some(p.to_string_lossy().replace('\\', "/"));
        }
        // (assumption) a "_HI" texture the install lacks (the high resolution set is not shipped
        // for every texture) is its plain one: data/newland/land/textures holds
        // Geobased_Rockloop_DIF.dds where materials name Geobased_Rockloop_DIF_HI.dds; without
        // it they drew white
        let (stem, ext) = name.rsplit_once('.')?;
        let plain = format!("{}.{ext}", stem.strip_suffix("_hi")?);
        self.by_name.get(&plain).map(|p| p.to_string_lossy().replace('\\', "/"))
    }

    pub fn path(&self, stored: &str) -> Option<PathBuf> {
        self.find(stored).map(|r| self.root.join(r))
    }
}

fn walk(root: &Path, rel: &Path, out: &mut HashMap<String, PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root.join(rel)) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.file_name()).collect();
    entries.sort();
    for name in entries {
        let r = rel.join(&name);
        if root.join(&r).is_dir() {
            walk(root, &r, out);
        } else {
            out.entry(name.to_string_lossy().to_lowercase())
                .or_insert(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_paths_fall_back_to_file_name() {
        let dir = std::env::temp_dir().join(format!("kenshi-res-{}", std::process::id()));
        let real = dir.join("data/character/skins/human_male");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("Head_HI.dds"), b"x").unwrap();
        let r = Resources::index(&dir);
        let want = Some("data/character/skins/human_male/Head_HI.dds");
        assert_eq!(
            r.find(".\\data\\character\\skins\\hu\\heads\\head_hi.dds")
                .as_deref(),
            want
        );
        assert_eq!(
            r.find("data/character/skins/human_male/Head_HI.dds")
                .as_deref(),
            want
        );
        assert_eq!(r.find("missing.dds"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
