//! Where a game's saves are on this computer: the manifest's placeholders
//! (`<base>`, `<winDocuments>`, `<xdgData>` …) filled in for one "context"
//! (this Windows user, a Proton prefix's Windows user, or this Linux/macOS
//! user), then matched as globs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::manifest::{FileRule, IndexGame};

/// More would not be saves; such a game is refused instead of filling the disk.
pub const MAX_FILES: usize = 20_000;
pub const MAX_BYTES: u64 = 4 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Mac,
}

impl Os {
    fn name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Mac => "mac",
        }
    }
}

/// One way the game sees the computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub os: Os,
    /// `steam`, `gog`, `epic`; None for other games.
    pub store: Option<&'static str>,
    pub store_game_id: Option<String>,
    /// The game's install folder.
    pub base: Option<PathBuf>,
    /// The user's home (the Windows profile in a prefix).
    pub home: PathBuf,
    pub os_user: String,
    /// Windows folders (this user, or inside a prefix).
    pub windows: Option<WindowsDirs>,
    /// Linux: `$XDG_DATA_HOME`, `$XDG_CONFIG_HOME`.
    pub xdg_data: Option<PathBuf>,
    pub xdg_config: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsDirs {
    pub app_data: PathBuf,
    pub local_app_data: PathBuf,
    pub documents: PathBuf,
    pub public: PathBuf,
    pub program_data: PathBuf,
    pub windows: PathBuf,
}

impl WindowsDirs {
    /// The Windows user of a Proton prefix (`<prefix>/pfx/drive_c`).
    pub fn in_prefix(drive_c: &Path, user: &str) -> (PathBuf, Self) {
        let profile = drive_c.join("users").join(user);
        let dirs = Self::of_profile(&profile, drive_c);
        (profile, dirs)
    }

    pub fn of_profile(profile: &Path, drive_c: &Path) -> Self {
        Self {
            app_data: profile.join("AppData").join("Roaming"),
            local_app_data: profile.join("AppData").join("Local"),
            documents: profile.join("Documents"),
            public: drive_c.join("users").join("Public"),
            program_data: drive_c.join("ProgramData"),
            windows: drive_c.join("windows"),
        }
    }
}

impl Context {
    fn applies(&self, rule: &FileRule) -> bool {
        let os_ok = rule.os.is_empty() || rule.os.iter().any(|o| o == self.os.name());
        let store_ok =
            rule.stores.is_empty() || rule.stores.iter().any(|s| Some(s.as_str()) == self.store);
        os_ok && store_ok
    }

    /// The rule's path as a glob, None when a placeholder has no value here.
    fn pattern(&self, path: &str) -> Option<String> {
        let text = |p: &Path| glob::Pattern::escape(&p.to_string_lossy());
        let win = self.windows.as_ref();
        let base = self.base.as_deref();
        let values: [(&str, Option<String>); 16] = [
            ("<base>", base.map(text)),
            ("<root>", base.and_then(Path::parent).map(text)),
            (
                "<game>",
                base.and_then(Path::file_name)
                    .map(|n| glob::Pattern::escape(&n.to_string_lossy())),
            ),
            ("<home>", Some(text(&self.home))),
            ("<storeUserId>", Some("*".into())),
            (
                "<storeGameId>",
                self.store_game_id.as_deref().map(glob::Pattern::escape),
            ),
            ("<osUserName>", Some(glob::Pattern::escape(&self.os_user))),
            ("<winAppData>", win.map(|w| text(&w.app_data))),
            (
                "<winLocalAppDataLow>",
                win.map(|w| text(&w.local_app_data.with_file_name("LocalLow"))),
            ),
            ("<winLocalAppData>", win.map(|w| text(&w.local_app_data))),
            ("<winDocuments>", win.map(|w| text(&w.documents))),
            ("<winPublic>", win.map(|w| text(&w.public))),
            ("<winProgramData>", win.map(|w| text(&w.program_data))),
            ("<winDir>", win.map(|w| text(&w.windows))),
            ("<xdgData>", self.xdg_data.as_deref().map(text)),
            ("<xdgConfig>", self.xdg_config.as_deref().map(text)),
        ];
        let mut out = if self.os == Os::Windows {
            any_case(path)
        } else {
            path.to_owned()
        };
        for (name, value) in &values {
            if out.contains(name) {
                out = out.replace(name, value.as_ref()?);
            }
        }
        if out.contains('<') && out.contains('>') {
            return None;
        }
        // Manifest paths use `/`; Windows paths on Windows take either.
        Some(out.replace('\\', "/"))
    }

    /// Folders a match may never be (a rule matching them would copy everything).
    fn too_wide(&self, path: &Path) -> bool {
        let mut roots: Vec<&Path> = vec![&self.home, Path::new("/")];
        if let Some(base) = &self.base {
            roots.push(base);
        }
        if let Some(w) = &self.windows {
            roots.extend([
                w.app_data.as_path(),
                w.local_app_data.as_path(),
                w.documents.as_path(),
            ]);
        }
        roots
            .iter()
            .any(|root| path == *root || root.starts_with(path))
    }
}

/// `Saves/<home>` → `[sS][aA][vV][eE][sS]/<home>`: the manifest's own letters
/// match in any case (glob ignores its case option for plain path parts);
/// placeholders and the values put in for them stay as they are.
fn any_case(path: &str) -> String {
    let mut out = String::with_capacity(path.len() * 3);
    let mut in_placeholder = false;
    let mut in_class = false;
    for c in path.chars() {
        match c {
            '<' if !in_class => in_placeholder = true,
            '>' if in_placeholder => in_placeholder = false,
            '[' if !in_placeholder => in_class = true,
            ']' if in_class => in_class = false,
            _ => {}
        }
        if c.is_ascii_alphabetic() && !in_placeholder && !in_class {
            out.push('[');
            out.push(c.to_ascii_lowercase());
            out.push(c.to_ascii_uppercase());
            out.push(']');
        } else {
            out.push(c);
        }
    }
    out
}

/// Every file below a folder, without following links.
fn walk(dir: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(&entry.path(), out);
        } else if kind.is_file() {
            out.insert(entry.path());
        }
        if out.len() > MAX_FILES {
            return;
        }
    }
}

/// The game's save files in all contexts (blocking; run in `spawn_blocking`).
pub fn find_files(game: &IndexGame, contexts: &[Context]) -> BTreeSet<PathBuf> {
    let mut files = BTreeSet::new();
    for context in contexts {
        let options = glob::MatchOptions {
            // Windows games don't care about case, even inside a prefix.
            case_sensitive: context.os != Os::Windows,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        for rule in game.files.iter().filter(|r| context.applies(r)) {
            let Some(pattern) = context.pattern(&rule.path) else {
                continue;
            };
            let Ok(matches) = glob::glob_with(&pattern, options) else {
                continue;
            };
            for path in matches.flatten() {
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    if !context.too_wide(&path) {
                        walk(&path, &mut files);
                    }
                } else if meta.is_file() {
                    files.insert(path);
                }
                if files.len() > MAX_FILES {
                    return files;
                }
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(path: &str, os: &[&str]) -> FileRule {
        FileRule {
            path: path.into(),
            os: os.iter().map(|s| (*s).to_owned()).collect(),
            stores: Vec::new(),
        }
    }

    #[test]
    fn saves_are_found_in_a_prefix_and_in_the_install_folder() {
        let root = tempfile::tempdir().unwrap();
        let drive_c = root.path().join("compat/g/pfx/drive_c");
        let (profile, windows) = WindowsDirs::in_prefix(&drive_c, "steamuser");
        let docs = windows.documents.join("Saltmarsh/Saves");
        std::fs::create_dir_all(docs.join("slot1")).unwrap();
        std::fs::write(docs.join("slot1/a.sav"), b"a").unwrap();
        std::fs::write(docs.join("b.sav"), b"b").unwrap();
        let install = root.path().join("games/Saltmarsh");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("Settings.INI"), b"x").unwrap();
        std::fs::write(install.join("game.exe"), b"MZ").unwrap();

        let game = IndexGame {
            name: "Saltmarsh".into(),
            files: vec![
                rule("<winDocuments>/saltmarsh/saves", &[]),
                rule("<base>/settings.ini", &["windows"]),
                rule("<xdgData>/saltmarsh", &["linux"]),
                // A rule matching a whole base folder is ignored.
                rule("<base>", &[]),
                rule("<home>", &[]),
            ],
            steam: None,
            gog: None,
            alias: None,
        };
        let context = Context {
            os: Os::Windows,
            store: Some("gog"),
            store_game_id: Some("5678".into()),
            base: Some(install.clone()),
            home: profile,
            os_user: "steamuser".into(),
            windows: Some(windows),
            xdg_data: None,
            xdg_config: None,
        };
        let found: Vec<PathBuf> = find_files(&game, &[context]).into_iter().collect();
        assert_eq!(
            found,
            [
                docs.join("b.sav"),
                docs.join("slot1/a.sav"),
                install.join("Settings.INI")
            ]
        );
    }

    #[test]
    fn manifest_letters_match_in_any_case() {
        assert_eq!(
            any_case("<base>/Sv[0-9].ini"),
            "<base>/[sS][vV][0-9].[iI][nN][iI]"
        );
    }

    #[test]
    fn rules_follow_os_and_store() {
        let context = Context {
            os: Os::Linux,
            store: Some("steam"),
            store_game_id: Some("1234".into()),
            base: Some(PathBuf::from("/games/x")),
            home: PathBuf::from("/home/u"),
            os_user: "u".into(),
            windows: None,
            xdg_data: Some(PathBuf::from("/home/u/.local/share")),
            xdg_config: None,
        };
        assert!(context.applies(&rule("<home>/x", &[])));
        assert!(!context.applies(&rule("<home>/x", &["windows"])));
        let gog_only = FileRule {
            stores: vec!["gog".into()],
            ..rule("<home>/x", &[])
        };
        assert!(!context.applies(&gog_only));
        assert_eq!(
            context
                .pattern("<xdgData>/<storeGameId>/<storeUserId>")
                .as_deref(),
            Some("/home/u/.local/share/1234/*")
        );
        // A Windows folder has no value on Linux.
        assert_eq!(context.pattern("<winDocuments>/x"), None);
        assert_eq!(context.pattern("<unknown>/x"), None);
    }
}
