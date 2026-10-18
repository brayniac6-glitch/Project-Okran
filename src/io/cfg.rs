//! The game's settings (port_map `gui`, the options window): Kenshi keeps them in settings.cfg
//! ("key=value" lines, F032944 reads and writes them) and its key bindings in controls.cfg
//! ("action=key", an action bound more than once listed once per key). The rewrite reads the
//! install's files and saves its own copies in %APPDATA%\kenshi-rewrite (the install is never
//! written), the copies read over the install's.

use std::path::{Path, PathBuf};

/// The rewrite's own folder (%APPDATA%\kenshi-rewrite, else the working directory).
pub fn user_dir() -> PathBuf {
    std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("."), |a| PathBuf::from(a).join("kenshi-rewrite"))
}

/// "key=value" lines in order (a key may repeat: controls.cfg binds an action to several keys).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cfg {
    pub lines: Vec<(String, String)>,
}

impl Cfg {
    pub fn parse(text: &str) -> Cfg {
        Cfg {
            lines: text
                .lines()
                .filter_map(|l| {
                    let (k, v) = l.split_once('=')?;
                    Some((k.trim().to_string(), v.trim().to_string()))
                })
                .collect(),
        }
    }

    pub fn text(&self) -> String {
        self.lines.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.lines.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    pub fn f32(&self, k: &str, default: f32) -> f32 {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    /// Every value of `k` (an action's keys).
    pub fn all(&self, k: &str) -> Vec<String> {
        self.lines.iter().filter(|(n, _)| n == k).map(|(_, v)| v.clone()).collect()
    }

    /// Set `k` (its first line; added at the end when new).
    pub fn set(&mut self, k: &str, v: &str) {
        match self.lines.iter_mut().find(|(n, _)| n == k) {
            Some(e) => e.1 = v.to_string(),
            None => self.lines.push((k.to_string(), v.to_string())),
        }
    }

    /// `k`'s values replaced by `vs`.
    pub fn set_all(&mut self, k: &str, vs: &[String]) {
        let at = self.lines.iter().position(|(n, _)| n == k).unwrap_or(self.lines.len());
        self.lines.retain(|(n, _)| n != k);
        for (i, v) in vs.iter().enumerate() {
            self.lines.insert((at + i).min(self.lines.len()), (k.to_string(), v.clone()));
        }
    }

    /// The other's lines over these (keys the other has replace these keys' lines).
    pub fn overlay(&mut self, other: &Cfg) {
        let mut keys: Vec<&str> = other.lines.iter().map(|(k, _)| k.as_str()).collect();
        keys.dedup();
        for k in keys {
            self.set_all(k, &other.all(k));
        }
    }
}

/// A cfg file of the install (`name`) with the rewrite's own copy read over it.
pub fn load(install: &Path, name: &str) -> Cfg {
    let mut c = Cfg::parse(&std::fs::read_to_string(install.join(name)).unwrap_or_default());
    if let Ok(t) = std::fs::read_to_string(user_dir().join(name)) {
        c.overlay(&Cfg::parse(&t));
    }
    c
}

/// The text of a cfg file as the game reads it: the install's with the rewrite's copy over it.
pub fn text(install: &Path, name: &str) -> String {
    load(install, name).text()
}

/// Save the rewrite's copy of `name`: written whole beside it, then put in its place, so a
/// write cut short never leaves half a file.
pub fn save(name: &str, cfg: &Cfg) -> std::io::Result<()> {
    let dir = user_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, cfg.text())?;
    std::fs::rename(&tmp, dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_what_is_not_a_setting() {
        // an old or hand-edited file: blank lines, comments, no "=", spaces round the parts
        let c = Cfg::parse("\n# a comment\nnot a line\n view distance = 9000 \nfont size2=abc\n=5\n");
        assert_eq!(c.get("view distance"), Some("9000"));
        assert_eq!(c.f32("font size2", 3.0), 3.0);
        assert_eq!(c.get("not a line"), None);
    }

    #[test]
    fn overlay_keeps_order_and_repeats() {
        let mut a = Cfg::parse("x=1\ncam=W\ncam=Up\ny=2\n");
        a.overlay(&Cfg::parse("cam=I\ny=5\n"));
        assert_eq!(a.all("cam"), vec!["I".to_string()]);
        assert_eq!(a.get("y"), Some("5"));
        assert_eq!(a.lines[0].0, "x");
        assert_eq!(a.f32("y", 0.0), 5.0);
    }
}
