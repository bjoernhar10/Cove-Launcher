//! umu-launcher: starts Proton inside the Steam Linux Runtime (the container
//! Steam uses) and applies per-game fixes (protonfixes). Cove only runs it as a
//! separate program (it is GPL-3.0); no code of it lives in this repo.
//!
//! Cove prefers an `umu-run` the system installed (package manager). Otherwise
//! it downloads the zipapp from the GitHub releases (SHA-256 from the release
//! asset's digest) into `<app data>/umu/<version>/umu-run`; the zipapp needs
//! `python3`. When umu is switched off, missing or can't be installed, Proton
//! starts directly as before.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::launcher::version_key;
use crate::models::StoreSource;

/// Id of umu-launcher in `proton-progress`, `proton_install` and `proton_cancel`.
pub const UMU_ID: &str = "umu";
pub const UMU_RELEASE_URL: &str =
    "https://api.github.com/repos/Open-Wine-Components/umu-launcher/releases/latest";
pub const PROGRAM: &str = "umu-run";

/// The newest umu-launcher release Cove can install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UmuRelease {
    pub version: String,
    pub url: String,
    pub size: u64,
    /// SHA-256 in hex.
    pub sha256: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    /// `sha256:<hex>`; GitHub computes it for every upload.
    digest: Option<String>,
}

/// Only plain version tags ("1.4.4") become folder names.
fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 32
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !version.starts_with('.')
}

/// The zipapp of a published release, when it carries a SHA-256 digest.
pub fn release_from(json: &str) -> Option<UmuRelease> {
    let release: GithubRelease = serde_json::from_str(json).ok()?;
    if release.draft || release.prerelease || !valid_version(&release.tag_name) {
        return None;
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == format!("umu-launcher-{}-zipapp.tar", release.tag_name))?;
    let sha256 = asset
        .digest
        .as_deref()?
        .strip_prefix("sha256:")?
        .to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(UmuRelease {
        version: release.tag_name,
        url: asset.browser_download_url.clone(),
        size: asset.size,
        sha256,
    })
}

/// Cove's copies, newest first: `(version, path of umu-run)`.
pub fn installed(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let version = entry.file_name().to_string_lossy().into_owned();
            let program = entry.path().join(PROGRAM);
            (valid_version(&version) && program.is_file()).then_some((version, program))
        })
        .collect();
    found.sort_by_key(|(version, _)| std::cmp::Reverse(version_key(version)));
    found
}

/// `umu-run` somewhere below `dir` (the zipapp tar has it in a subfolder).
pub fn find_program(dir: &Path, depth: u32) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if (kind.is_file() || kind.is_symlink()) && entry.file_name() == PROGRAM {
            return Some(path);
        }
        if kind.is_dir() {
            subdirs.push(path);
        }
    }
    if depth == 0 {
        return None;
    }
    subdirs.iter().find_map(|sub| find_program(sub, depth - 1))
}

/// Everything a launch through umu needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UmuLaunch {
    pub program: PathBuf,
    /// `umu-<Steam app id>` (protonfixes are keyed by it) or `umu-default`.
    pub game_id: String,
    /// The store for umu's database (`gog`, `egs`); none for Steam and other games.
    pub store: Option<&'static str>,
}

impl UmuLaunch {
    pub fn new(program: PathBuf, source: StoreSource, steam_app_id: Option<&str>) -> Self {
        let game_id = steam_app_id
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            .map_or_else(|| "umu-default".to_owned(), |id| format!("umu-{id}"));
        let store = match source {
            StoreSource::Gog => Some("gog"),
            StoreSource::Epic => Some("egs"),
            StoreSource::Amazon => Some("amazon"),
            _ => None,
        };
        Self {
            program,
            game_id,
            store,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github(tag: &str, digest: Option<&str>) -> String {
        serde_json::json!({
            "tag_name": tag,
            "assets": [
                { "name": format!("umu-launcher-{tag}.fc44.x86_64.rpm"), "size": 1,
                  "browser_download_url": "https://example.invalid/x.rpm", "digest": null },
                { "name": format!("umu-launcher-{tag}-zipapp.tar"), "size": 430_080,
                  "browser_download_url": "https://example.invalid/zipapp.tar", "digest": digest }
            ]
        })
        .to_string()
    }

    #[test]
    fn the_zipapp_with_its_digest_is_taken() {
        let hash = "AB".repeat(32);
        let release = release_from(&github("1.4.4", Some(&format!("sha256:{hash}")))).unwrap();
        assert_eq!(release.version, "1.4.4");
        assert_eq!(release.url, "https://example.invalid/zipapp.tar");
        assert_eq!(release.sha256, "ab".repeat(32));
        // No digest, a wrong one or an odd tag: nothing to install.
        assert!(release_from(&github("1.4.4", None)).is_none());
        assert!(release_from(&github("1.4.4", Some("sha256:abc"))).is_none());
        assert!(release_from(&github("../x", Some(&format!("sha256:{hash}")))).is_none());
    }

    #[test]
    fn copies_are_found_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        for version in ["1.4.3", "1.10.0", "1.4.4"] {
            std::fs::create_dir_all(dir.path().join(version)).unwrap();
            std::fs::write(dir.path().join(version).join(PROGRAM), b"").unwrap();
        }
        std::fs::create_dir_all(dir.path().join("2.0.0")).unwrap(); // empty: not a copy
        let versions: Vec<String> = installed(dir.path()).into_iter().map(|(v, _)| v).collect();
        assert_eq!(versions, ["1.10.0", "1.4.4", "1.4.3"]);

        std::fs::create_dir_all(dir.path().join("unpacked/umu")).unwrap();
        std::fs::write(dir.path().join("unpacked/umu").join(PROGRAM), b"").unwrap();
        assert_eq!(
            find_program(&dir.path().join("unpacked"), 2),
            Some(dir.path().join("unpacked/umu").join(PROGRAM))
        );
    }

    #[test]
    fn games_are_named_by_their_steam_app() {
        let steam = UmuLaunch::new(PathBuf::from("/u"), StoreSource::Steam, Some("1091500"));
        assert_eq!(steam.game_id, "umu-1091500");
        assert_eq!(steam.store, None);
        let gog = UmuLaunch::new(PathBuf::from("/u"), StoreSource::Gog, None);
        assert_eq!(gog.game_id, "umu-default");
        assert_eq!(gog.store, Some("gog"));
        let odd = UmuLaunch::new(PathBuf::from("/u"), StoreSource::Epic, Some("x; rm"));
        assert_eq!(odd.game_id, "umu-default");
    }
}
