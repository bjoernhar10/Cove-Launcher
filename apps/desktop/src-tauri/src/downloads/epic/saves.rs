//! Epic cloud saves. Before a game starts, newer saves from the cloud replace
//! the local ones (which are backed up first); after it ends, newer local
//! saves are uploaded. Cloud saves use the same manifest + chunk format as
//! game builds, stored below `<account>/<app>/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::format::{self, read_chunk, ChunkPart, FileEntry, Guid, Manifest};
use crate::downloads::transfer::{http_error, status_error};
use crate::error::{AppError, AppResult};

/// Size of the chunks Cove packs saves into (like Epic: 1 MiB).
const CHUNK_SIZE: usize = 1 << 20;
/// Backups kept per game.
const KEEP_BACKUPS: usize = 5;

/// Mirror of `EpicSaveFile` in the C# connector.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFile {
    pub path: String,
    pub read_link: Option<String>,
    pub last_modified: Option<DateTime<Utc>>,
    pub size: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Saves {
    pub files: Vec<SaveFile>,
}

/// Folders the save-folder variables stand for (Windows paths, or inside a Proton/Wine prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDirs {
    /// `%LOCALAPPDATA%`
    pub local_app_data: PathBuf,
    /// `%USERPROFILE%`
    pub profile: PathBuf,
}

impl UserDirs {
    /// The Windows user inside a Proton prefix (`<prefix>/pfx/drive_c/users/steamuser`).
    pub fn proton(prefix: &Path) -> Self {
        Self::of_profile(prefix.join("pfx/drive_c/users/steamuser"))
    }

    pub fn of_profile(profile: PathBuf) -> Self {
        Self {
            local_app_data: profile.join("AppData").join("Local"),
            profile,
        }
    }

    /// This Windows user.
    pub fn windows() -> Option<Self> {
        Some(Self {
            local_app_data: std::env::var_os("LOCALAPPDATA")?.into(),
            profile: std::env::var_os("USERPROFILE")?.into(),
        })
    }
}

/// Fills in `{AppData}`, `{UserDir}`, `{UserProfile}`, `{UserSavedGames}`,
/// `{InstallDir}` and `{EpicID}` (any case). None when something stays unknown.
pub fn save_folder(
    template: &str,
    dirs: &UserDirs,
    install_dir: &Path,
    account_id: &str,
) -> Option<PathBuf> {
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let vars = [
        ("{appdata}", path(&dirs.local_app_data)),
        ("{userdir}", path(&dirs.profile.join("Documents"))),
        ("{userprofile}", path(&dirs.profile)),
        ("{usersavedgames}", path(&dirs.profile.join("Saved Games"))),
        ("{installdir}", path(install_dir)),
        ("{epicid}", account_id.to_owned()),
    ];
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..].find('}')? + start;
        let key = rest[start..=end].to_lowercase();
        let (_, value) = vars.iter().find(|(k, _)| *k == key)?;
        out.push_str(value);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    let normalized = out.replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR);
    let mut result = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            other => result.push(other),
        }
    }
    (!result.as_os_str().is_empty()).then_some(result)
}

/// Newest save manifest in the cloud.
pub fn newest_manifest(saves: &Saves) -> Option<&SaveFile> {
    saves
        .files
        .iter()
        .filter(|f| f.path.contains("/manifests/") && f.path.ends_with(".manifest"))
        .max_by_key(|f| (f.last_modified, f.path.clone()))
}

fn files_below(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Time of the newest local save file.
pub fn newest_local(dir: &Path) -> Option<DateTime<Utc>> {
    files_below(dir)
        .iter()
        .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .max()
        .map(DateTime::<Utc>::from)
}

/// Should the cloud saves replace the local ones?
pub fn cloud_is_newer(cloud: Option<DateTime<Utc>>, local: Option<DateTime<Utc>>) -> bool {
    match (cloud, local) {
        (Some(cloud), Some(local)) => cloud > local,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// Should the local saves be uploaded?
pub fn local_is_newer(cloud: Option<DateTime<Utc>>, local: Option<DateTime<Utc>>) -> bool {
    match (cloud, local) {
        (Some(cloud), Some(local)) => local > cloud,
        (None, Some(_)) => true,
        (_, None) => false,
    }
}

/// Copies the save folder to `backups/<time>` and keeps the newest few.
pub fn back_up(dir: &Path, backups: &Path) -> AppResult<()> {
    let files = files_below(dir);
    if files.is_empty() {
        return Ok(());
    }
    let target = backups.join(Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string());
    for file in &files {
        let Ok(relative) = file.strip_prefix(dir) else {
            continue;
        };
        let to = target.join(relative);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, &to)?;
    }
    let mut existing: Vec<PathBuf> = std::fs::read_dir(backups)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    existing.sort();
    while existing.len() > KEEP_BACKUPS {
        let oldest = existing.remove(0);
        let _ = std::fs::remove_dir_all(oldest);
    }
    Ok(())
}

async fn get(client: &reqwest::Client, url: &str) -> AppResult<Vec<u8>> {
    let response = client.get(url).send().await.map_err(http_error)?;
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    Ok(response.bytes().await.map_err(http_error)?.to_vec())
}

/// Writes the newest cloud saves into `dir`. `prefix` = `<account>/<app>/`.
pub async fn download(
    client: &reqwest::Client,
    saves: &Saves,
    prefix: &str,
    dir: &Path,
) -> AppResult<usize> {
    let Some(newest) = newest_manifest(saves) else {
        return Ok(0);
    };
    let link = |path: &str| {
        saves
            .files
            .iter()
            .find(|f| f.path == path)
            .and_then(|f| f.read_link.clone())
            .ok_or_else(|| {
                AppError::Download(tr!(
                    "Ein Teil der Cloud-Spielstände fehlt.",
                    "Part of the cloud saves is missing."
                ))
            })
    };
    let manifest = Manifest::parse(&get(client, &link(&newest.path)?).await?)?;
    let mut chunks: HashMap<Guid, Vec<u8>> = HashMap::new();
    for file in &manifest.files {
        let mut data = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        for part in &file.parts {
            let chunk = match chunks.entry(part.guid) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let info = manifest.chunks.get(&part.guid).ok_or_else(|| {
                        AppError::Download(tr!(
                            "Ein Teil der Cloud-Spielstände fehlt.",
                            "Part of the cloud saves is missing."
                        ))
                    })?;
                    let path = format!("{prefix}{}", manifest.chunk_path(info));
                    entry.insert(read_chunk(&get(client, &link(&path)?).await?, info)?)
                }
            };
            let start = usize::try_from(part.offset).unwrap_or(usize::MAX);
            let end = start.saturating_add(usize::try_from(part.size).unwrap_or(usize::MAX));
            data.extend_from_slice(chunk.get(start..end).ok_or_else(|| {
                AppError::Download(tr!(
                    "Die Cloud-Spielstände sind beschädigt.",
                    "The cloud saves are damaged."
                ))
            })?);
        }
        if format::sha1(&data) != file.sha && file.sha != [0; 20] {
            return Err(AppError::Download(tr!(
                "Die Cloud-Spielstände sind beschädigt.",
                "The cloud saves are damaged."
            )));
        }
        let path = crate::downloads::extract::safe_relative(&file.name.replace('\\', "/"))
            .map(|r| dir.join(r))
            .ok_or_else(|| {
                AppError::Download(tr!(
                    "Die Cloud-Spielstände sind beschädigt.",
                    "The cloud saves are damaged."
                ))
            })?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, data).await?;
    }
    Ok(manifest.files.len())
}

fn random_guid() -> AppResult<Guid> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| AppError::Download("no randomness".into()))?;
    let part = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Ok([part(0), part(4), part(8), part(12)])
}

/// Packs the save folder into a manifest and chunks; returns `(path, bytes)` to upload.
pub fn pack(
    dir: &Path,
    prefix: &str,
    app_name: &str,
    template: &str,
    at: DateTime<Utc>,
) -> AppResult<Vec<(String, Vec<u8>)>> {
    let mut manifest = Manifest {
        version: 18,
        app_name: app_name.to_owned(),
        custom: [("CloudSaveFolder".to_owned(), template.to_owned())]
            .into_iter()
            .collect(),
        ..Manifest::default()
    };
    let mut uploads = Vec::new();
    let mut current: Vec<u8> = Vec::with_capacity(CHUNK_SIZE);
    let mut current_guid = random_guid()?;
    let finish = |data: &mut Vec<u8>,
                  guid: Guid,
                  manifest: &mut Manifest,
                  uploads: &mut Vec<(String, Vec<u8>)>| {
        if data.is_empty() {
            return;
        }
        let (info, bytes) = format::write_chunk(guid, data);
        uploads.push((format!("{prefix}{}", manifest.chunk_path(&info)), bytes));
        manifest.chunks.insert(guid, info);
        data.clear();
    };
    for path in files_below(dir) {
        let Ok(relative) = path.strip_prefix(dir) else {
            continue;
        };
        let data = std::fs::read(&path)?;
        let mut parts = Vec::new();
        let mut rest = data.as_slice();
        while !rest.is_empty() {
            if current.len() == CHUNK_SIZE {
                finish(&mut current, current_guid, &mut manifest, &mut uploads);
                current_guid = random_guid()?;
            }
            let take = rest.len().min(CHUNK_SIZE - current.len());
            parts.push(ChunkPart {
                guid: current_guid,
                offset: u32::try_from(current.len()).unwrap_or(u32::MAX),
                size: u32::try_from(take).unwrap_or(u32::MAX),
            });
            current.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
        manifest.files.push(FileEntry {
            name: relative.to_string_lossy().replace('\\', "/"),
            sha: format::sha1(&data),
            flags: 0,
            parts,
        });
    }
    finish(&mut current, current_guid, &mut manifest, &mut uploads);
    let name = format!(
        "{prefix}manifests/{}.manifest",
        at.format("%Y.%m.%d-%H.%M.%S")
    );
    uploads.push((name, manifest.to_bytes()));
    Ok(uploads)
}

/// Uploads files to their signed links (manifest last, so it never points at missing chunks).
pub async fn upload(
    client: &reqwest::Client,
    files: Vec<(String, Vec<u8>)>,
    links: &HashMap<String, String>,
) -> AppResult<()> {
    for (path, bytes) in files {
        let url = links.get(&path).ok_or_else(|| {
            AppError::Download(tr!(
                "Epic hat keine Upload-Links für die Spielstände geliefert.",
                "Epic sent no upload links for the saves."
            ))
        })?;
        let response = client
            .put(url)
            .body(bytes)
            .send()
            .await
            .map_err(http_error)?;
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_folders_inside_a_proton_prefix() {
        let dirs = UserDirs::proton(Path::new("/data/compat/epic-sugar"));
        let install = Path::new("/games/Sugar");
        assert_eq!(
            save_folder("{AppData}/Sugar/Saved/SaveGames", &dirs, install, "acc"),
            Some(PathBuf::from("/data/compat/epic-sugar/pfx/drive_c/users/steamuser/AppData/Local/Sugar/Saved/SaveGames"))
        );
        assert_eq!(
            save_folder(
                "{USERDIR}\\My Games\\Sugar\\{EpicID}",
                &dirs,
                install,
                "acc"
            ),
            Some(PathBuf::from(
                "/data/compat/epic-sugar/pfx/drive_c/users/steamuser/Documents/My Games/Sugar/acc"
            ))
        );
        assert_eq!(
            save_folder("{InstallDir}/Saves", &dirs, install, "acc"),
            Some(PathBuf::from("/games/Sugar/Saves"))
        );
        assert_eq!(save_folder("{Unknown}/x", &dirs, install, "acc"), None);
    }

    #[test]
    fn newer_side_wins() {
        let t = |s: &str| Some(s.parse::<DateTime<Utc>>().unwrap());
        assert!(cloud_is_newer(
            t("2026-11-02T00:00:00Z"),
            t("2026-11-01T00:00:00Z")
        ));
        assert!(!cloud_is_newer(
            t("2026-11-01T00:00:00Z"),
            t("2026-11-02T00:00:00Z")
        ));
        assert!(cloud_is_newer(t("2026-11-01T00:00:00Z"), None));
        assert!(!cloud_is_newer(None, None));
        assert!(local_is_newer(None, t("2026-11-01T00:00:00Z")));
        assert!(!local_is_newer(
            t("2026-11-02T00:00:00Z"),
            t("2026-11-01T00:00:00Z")
        ));
    }

    #[test]
    fn backups_keep_the_newest_five() {
        let root = tempfile::tempdir().unwrap();
        let saves = root.path().join("saves");
        std::fs::create_dir_all(saves.join("slot")).unwrap();
        std::fs::write(saves.join("slot/1.sav"), b"one").unwrap();
        let backups = root.path().join("backups");
        for i in 0..7 {
            std::fs::create_dir_all(backups.join(format!("2020-01-0{i}_00-00-00"))).unwrap();
        }
        back_up(&saves, &backups).unwrap();
        let kept: Vec<_> = std::fs::read_dir(&backups).unwrap().flatten().collect();
        assert_eq!(kept.len(), KEEP_BACKUPS);
        assert!(
            kept.iter().any(|e| e.path().join("slot/1.sav").is_file()),
            "newest backup kept"
        );
    }

    #[tokio::test]
    async fn packed_saves_download_again() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, Request, ResponseTemplate};

        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("local");
        std::fs::create_dir_all(local.join("slot")).unwrap();
        std::fs::write(local.join("slot/game.sav"), vec![7u8; CHUNK_SIZE + 300]).unwrap();
        std::fs::write(local.join("settings.ini"), b"[a]\nb=1\n").unwrap();
        let prefix = "acc/Sugar/";
        let at = "2026-11-03T12:00:00Z".parse().unwrap();
        let files = pack(&local, prefix, "Sugar", "{AppData}/Sugar", at).unwrap();
        assert_eq!(
            files.last().unwrap().0,
            "acc/Sugar/manifests/2026.11.03-12.00.00.manifest"
        );
        assert_eq!(files.len(), 3, "two chunks and the manifest");

        // A fake storage: uploads go to /put/<path>, downloads come from /get/<path>.
        let server = MockServer::start().await;
        let stored: HashMap<String, Vec<u8>> = files.iter().cloned().collect();
        let answers = stored.clone();
        Mock::given(method("GET"))
            .respond_with(move |request: &Request| {
                let path = request.url.path().trim_start_matches("/get/").to_owned();
                answers.get(&path).map_or_else(
                    || ResponseTemplate::new(404),
                    |b| ResponseTemplate::new(200).set_body_bytes(b.clone()),
                )
            })
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let links: HashMap<String, String> = files
            .iter()
            .map(|(p, _)| (p.clone(), format!("{}/put/{p}", server.uri())))
            .collect();
        let client = reqwest::Client::new();
        upload(&client, files.clone(), &links).await.unwrap();
        let puts = server.received_requests().await.unwrap();
        assert_eq!(puts.len(), 3);
        assert!(
            puts.last().unwrap().url.path().ends_with(".manifest"),
            "manifest last"
        );

        let saves = Saves {
            files: stored
                .keys()
                .map(|p| SaveFile {
                    path: p.clone(),
                    read_link: Some(format!("{}/get/{p}", server.uri())),
                    last_modified: Some(at),
                    size: None,
                })
                .collect(),
        };
        let restored = root.path().join("restored");
        assert_eq!(
            download(&client, &saves, prefix, &restored).await.unwrap(),
            2
        );
        assert_eq!(
            std::fs::read(restored.join("slot/game.sav")).unwrap(),
            vec![7u8; CHUNK_SIZE + 300]
        );
        assert_eq!(
            std::fs::read(restored.join("settings.ini")).unwrap(),
            b"[a]\nb=1\n"
        );
    }
}
