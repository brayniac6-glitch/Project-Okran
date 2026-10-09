//! Which FCS files a game loads, and in what order (port_map `mod_loading`).
//!
//! Spec (sheet rows F046696 initGameDataAndMods, F046684 readModsCfg):
//! `gamedata.base` first, then the core `.mod` files shipped in `data/`, then the player's mods in
//! `data/mods.cfg` order. Core files are ordered so each comes after its header dependencies (ties by
//! name): an assumption, the original's source for that order is not yet found.
//!
//! Project Okran's own: a `mods.cfg` in its folder (%APPDATA%\kenshi-rewrite) is read in place of
//! the install's when it exists, so mods can be turned on for Okran without changing Kenshi's
//! list. Each mod's folder (`mods/<name>/`, or its workshop item) also overrides the game's files
//! by name (crate::resources and the GUI's files): a mod's `foo.dds` is drawn wherever the game
//! asks for `foo.dds`, the last mod in the order winning.

use crate::fcs;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The core file every game loads first; failing to load it is fatal.
pub const CORE_BASE: &str = "gamedata.base";

/// Kenshi's Steam app id: workshop mods live in `steamapps/workshop/content/<id>/<item>/`.
pub const STEAM_APP_ID: &str = "233860";

/// `data/mods.cfg` lines -> mod names, in order (F046684): blank lines and lines starting with
/// `#`, `/` or `;` are skipped, only `*.mod` entries count, the extension is dropped, repeats are
/// ignored.
pub fn parse_mods_cfg(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', '/', ';']) {
            continue;
        }
        let Some((name, ext)) = line.rsplit_once('.') else {
            continue;
        };
        if ext.eq_ignore_ascii_case("mod") && !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
    }
    out
}

/// Project Okran's own mod list (read in place of the install's `data/mods.cfg` when it exists).
pub fn okran_mods_cfg() -> PathBuf {
    crate::cfg::user_dir().join("mods.cfg")
}

/// Whether `root` is a Kenshi install: its data (`data/gamedata.base`, `data/gui`) and its
/// game (`kenshi_x64.exe`, or another `kenshi*.exe` such as GOG's). Project Okran ships none of
/// Kenshi's files and plays only from a copy of the game the player owns.
pub fn check_install(root: &Path) -> Result<(), String> {
    let data = root.join("data");
    if !data.join(CORE_BASE).is_file() || !data.join("gui").is_dir() {
        return Err(format!("{} has no Kenshi game data (data/gamedata.base)", root.display()));
    }
    let has_exe = std::fs::read_dir(root).map(|d| {
        d.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.starts_with("kenshi") && n.ends_with(".exe")
        })
    });
    if !has_exe.unwrap_or(false) {
        return Err(format!("{} has no Kenshi game (kenshi_x64.exe)", root.display()));
    }
    Ok(())
}

/// One file to load, with the index the loader tags its records with (its position in the order).
#[derive(Debug, Clone, PartialEq)]
pub struct LoadEntry {
    pub name: String,
    pub path: PathBuf,
    pub core: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadOrder {
    pub files: Vec<LoadEntry>,
    /// Names from `mods.cfg` with no matching installed mod (the original logs and skips them).
    pub missing: Vec<String>,
}

/// A Kenshi install: the game folder, plus the Steam workshop folder when it exists.
#[derive(Debug, Clone)]
pub struct Install {
    pub root: PathBuf,
    pub workshop: Option<PathBuf>,
}

impl Install {
    /// `root` is the game folder (the one containing `data/`). The workshop folder is found relative
    /// to a Steam library layout (`steamapps/common/<game>` -> `steamapps/workshop/content/<app>`).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let workshop = root
            .parent()
            .and_then(Path::parent)
            .map(|steamapps| steamapps.join("workshop/content").join(STEAM_APP_ID))
            .filter(|p| p.is_dir());
        Install { root, workshop }
    }

    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    /// `gamedata.base`, then every `data/*.mod`, each after its dependencies.
    pub fn core_files(&self) -> Result<Vec<LoadEntry>, fcs::Error> {
        let data = self.data();
        let base = data.join(CORE_BASE);
        if !base.is_file() {
            return Err(fcs::Error::Io(format!("{} not found", base.display())));
        }
        let mut mods: BTreeMap<String, (PathBuf, Vec<String>)> = BTreeMap::new();
        let entries = std::fs::read_dir(&data)
            .map_err(|e| fcs::Error::Io(format!("{}: {e}", data.display())))?;
        for p in entries.flatten().map(|e| e.path()) {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mod")) {
                let file = p.file_name().unwrap().to_string_lossy().into_owned();
                let deps = fcs::file_list(&fcs::read_header(&p)?.dependencies)
                    .map(str::to_owned)
                    .collect();
                mods.insert(file, (p, deps));
            }
        }
        let mut out = vec![LoadEntry {
            name: CORE_BASE.into(),
            path: base,
            core: true,
        }];
        let mut done: Vec<String> = vec![CORE_BASE.into()];
        while !mods.is_empty() {
            // first by name whose core dependencies are all loaded; break cycles by name
            let ready = mods
                .iter()
                .find(|(_, (_, deps))| {
                    deps.iter()
                        .all(|d| done.contains(d) || !mods.contains_key(d))
                })
                .or_else(|| mods.iter().next())
                .map(|(k, _)| k.clone())
                .unwrap();
            let (path, _) = mods.remove(&ready).unwrap();
            out.push(LoadEntry {
                name: ready.clone(),
                path,
                core: true,
            });
            done.push(ready);
        }
        Ok(out)
    }

    /// Where an installed mod named `name` lives: `mods/<name>/<name>.mod`, then any workshop item.
    pub fn find_mod(&self, name: &str) -> Option<PathBuf> {
        let file = format!("{name}.mod");
        let local = self.root.join("mods").join(name).join(&file);
        if local.is_file() {
            return Some(local);
        }
        let ws = self.workshop.as_ref()?;
        let mut items: Vec<PathBuf> = std::fs::read_dir(ws)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .collect();
        items.sort();
        items
            .into_iter()
            .map(|d| d.join(&file))
            .find(|p| p.is_file())
    }

    /// The mod list: Project Okran's own `mods.cfg` when there is one, else the install's.
    pub fn mods_cfg_text(&self) -> String {
        std::fs::read_to_string(okran_mods_cfg())
            .or_else(|_| std::fs::read_to_string(self.data().join("mods.cfg")))
            .unwrap_or_default()
    }

    /// The enabled mods' folders in load order (each mod file's folder).
    pub fn mod_dirs(&self) -> Vec<PathBuf> {
        self.load_order()
            .map(|o| o.files.into_iter().filter(|f| !f.core).filter_map(|f| f.path.parent().map(Path::to_path_buf)).collect())
            .unwrap_or_default()
    }

    /// The order a game plays: the full order, its missing mods told; only the core files with
    /// KENSHI_NO_MODS set (testing).
    pub fn game_order(&self) -> Result<LoadOrder, fcs::Error> {
        let mut order = self.load_order()?;
        if std::env::var_os("KENSHI_NO_MODS").is_some() {
            order.files.retain(|f| f.core);
            order.missing.clear();
        }
        for f in order.files.iter().filter(|f| !f.core) {
            eprintln!("mod: {}", f.path.display());
        }
        for m in &order.missing {
            eprintln!("mod missing: {m}.mod (in the mod list, not installed)");
        }
        Ok(order)
    }

    /// The full order: core files, then `data/mods.cfg` mods (core names there are ignored).
    pub fn load_order(&self) -> Result<LoadOrder, fcs::Error> {
        let mut order = LoadOrder {
            files: self.core_files()?,
            missing: Vec::new(),
        };
        let cfg = self.mods_cfg_text();
        for name in parse_mods_cfg(&cfg) {
            let file = format!("{name}.mod");
            if order
                .files
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(&file))
            {
                continue;
            }
            match self.find_mod(&name) {
                Some(path) => order.files.push(LoadEntry {
                    name: file,
                    path,
                    core: false,
                }),
                None => order.missing.push(name),
            }
        }
        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_needs_kenshi() {
        let dir = std::env::temp_dir().join(format!("okran-install-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/gui")).unwrap();
        assert!(check_install(&dir).is_err());
        std::fs::write(dir.join("data").join(CORE_BASE), b"x").unwrap();
        assert!(check_install(&dir).is_err(), "data without the game");
        std::fs::write(dir.join("kenshi_x64.exe"), b"x").unwrap();
        assert!(check_install(&dir).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mods_cfg_rules() {
        let cfg = "A.mod\r\n# comment.mod\n/x.mod\n;y.mod\n\nnot a mod.txt\nB Mod (2).MOD\nA.mod\n  C.mod  \n";
        assert_eq!(parse_mods_cfg(cfg), ["A", "B Mod (2)", "C"]);
    }
}
