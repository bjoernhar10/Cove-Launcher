//! GOG cloud saves (Galaxy's cloud storage). The connector (`gog.cloudSaves`)
//! finds the game's own Galaxy client, the folders it syncs and a token for
//! that client; this module compares and moves the files.
//!
//! Like Epic: before a game starts, a folder whose cloud files are newer than
//! the local ones is backed up and then updated from the cloud; after a
//! session, a folder with newer local files is uploaded. Files are named
//! `<folder name>/<relative path>` in the cloud and compared by MD5, so
//! unchanged files are neither downloaded nor uploaded. Nothing is deleted.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, NaiveDateTime, Utc};
use md5::{Digest, Md5};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{Deserialize, Deserializer};

use super::epic::saves::{
    back_up, cloud_is_newer, files_below, local_is_newer, newest_local, UserDirs,
};
use crate::downloads::transfer::{http_error, status_error};
use crate::error::AppResult;

/// Mirror of `GogCloudSaves` in the C# connector.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSaves {
    pub enabled: bool,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub access_token: Option<String>,
    pub storage_url: Option<String>,
    pub locations: Vec<SaveLocation>,
}

// Never print the token.
impl fmt::Debug for CloudSaves {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudSaves")
            .field("enabled", &self.enabled)
            .field("client_id", &self.client_id)
            .field("storage_url", &self.storage_url)
            .field("locations", &self.locations)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SaveLocation {
    pub name: String,
    /// `<?DOCUMENTS?>/Game/Saves` and the like.
    pub location: String,
}

/// A file in the cloud storage listing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CloudFile {
    pub name: String,
    /// MD5 in hex.
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default, deserialize_with = "lenient_time")]
    pub last_modified: Option<DateTime<Utc>>,
}

/// The storage answers times with or without a time zone (then UTC).
fn lenient_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    let Some(text) = Option::<String>::deserialize(d)? else {
        return Ok(None);
    };
    Ok(DateTime::parse_from_rfc3339(&text)
        .map(|t| t.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|t| t.and_utc())
        }))
}

/// The local folder of a save location. Galaxy's variables, inside the
/// Proton prefix's Windows user on Linux. None for unknown variables (macOS
/// folders) or a path that would leave its base folder.
pub fn folder(location: &str, dirs: &UserDirs, install: &Path) -> Option<PathBuf> {
    let profile = &dirs.profile;
    let vars: [(&str, PathBuf); 6] = [
        ("<?INSTALL?>", install.to_path_buf()),
        ("<?DOCUMENTS?>", profile.join("Documents")),
        ("<?SAVED_GAMES?>", profile.join("Saved Games")),
        ("<?APPLICATION_DATA_LOCAL?>", dirs.local_app_data.clone()),
        (
            "<?APPLICATION_DATA_LOCAL_LOW?>",
            profile.join("AppData").join("LocalLow"),
        ),
        (
            "<?APPLICATION_DATA_ROAMING?>",
            profile.join("AppData").join("Roaming"),
        ),
    ];
    // Longest names first: LOCAL_LOW must not be read as LOCAL.
    let mut sorted = vars;
    sorted.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let (base, rest) = sorted
        .iter()
        .find_map(|(name, path)| location.strip_prefix(name).map(|rest| (path, rest)))?;
    if rest.contains("<?") {
        return None;
    }
    safe_join(base, rest)
}

/// `base` + a relative path with `/` or `\`; None when it would climb out.
fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for part in relative.split(['/', '\\']).filter(|p| !p.is_empty()) {
        match Path::new(part).components().next()? {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

fn file_url(saves: &CloudSaves, name: &str) -> Option<String> {
    let base = saves.storage_url.as_deref()?;
    let path: Vec<String> = name
        .split('/')
        .map(|part| utf8_percent_encode(part, SEGMENT).to_string())
        .collect();
    Some(format!("{base}/{}", path.join("/")))
}

fn token(saves: &CloudSaves) -> &str {
    saves.access_token.as_deref().unwrap_or_default()
}

/// Every file of the game in the cloud (an empty storage is a 404).
pub async fn list(client: &reqwest::Client, saves: &CloudSaves) -> AppResult<Vec<CloudFile>> {
    let Some(url) = saves.storage_url.as_deref() else {
        return Ok(Vec::new());
    };
    let response = client
        .get(url)
        .bearer_auth(token(saves))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(http_error)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    response.json().await.map_err(http_error)
}

fn md5_file(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    Some(
        Md5::digest(&data)
            .iter()
            .fold(String::with_capacity(32), |mut hex, b| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{b:02x}");
                hex
            }),
    )
}

fn set_time(path: &Path, time: DateTime<Utc>) {
    let Ok(file) = std::fs::File::options().write(true).open(path) else {
        return;
    };
    let _ = file.set_modified(SystemTime::from(time));
}

/// The cloud files of one location, without the folder name.
fn files_of<'a>(files: &'a [CloudFile], location: &SaveLocation) -> Vec<(&'a CloudFile, &'a str)> {
    let prefix = format!("{}/", location.name);
    files
        .iter()
        .filter_map(|f| f.name.strip_prefix(&prefix).map(|rest| (f, rest)))
        .filter(|(_, rest)| !rest.is_empty())
        .collect()
}

/// Before a start: folders with newer cloud files are backed up and updated.
/// Returns the number of files written.
pub async fn sync_down(
    client: &reqwest::Client,
    saves: &CloudSaves,
    dirs: &UserDirs,
    install: &Path,
    backups: &Path,
) -> AppResult<usize> {
    let files = list(client, saves).await?;
    let mut written = 0;
    for location in &saves.locations {
        let Some(dir) = folder(&location.location, dirs, install) else {
            continue;
        };
        let cloud = files_of(&files, location);
        let cloud_time = cloud.iter().filter_map(|(f, _)| f.last_modified).max();
        let local = dir.clone();
        let local_time = tokio::task::spawn_blocking(move || newest_local(&local))
            .await
            .unwrap_or(None);
        if !cloud_is_newer(cloud_time, local_time) {
            continue;
        }
        let (from, to) = (dir.clone(), backups.join(safe_name(&location.name)));
        tokio::task::spawn_blocking(move || back_up(&from, &to))
            .await
            .map_err(|_| crate::error::AppError::Download("backup".into()))??;
        for (file, relative) in cloud {
            let Some(path) = safe_join(&dir, relative) else {
                continue;
            };
            if file.hash.is_some() && md5_file(&path) == file.hash {
                continue;
            }
            let Some(url) = file_url(saves, &file.name) else {
                continue;
            };
            let response = client
                .get(&url)
                .bearer_auth(token(saves))
                .send()
                .await
                .map_err(http_error)?;
            if !response.status().is_success() {
                return Err(status_error(response.status()));
            }
            let data = response.bytes().await.map_err(http_error)?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&path, &data).await?;
            if let Some(time) = file.last_modified {
                set_time(&path, time);
            }
            written += 1;
        }
    }
    Ok(written)
}

/// Backup folders get the location's name, made safe for a folder.
fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// After a session: folders with newer local files go to the cloud (files
/// the cloud already has unchanged are skipped). Returns the number uploaded.
pub async fn sync_up(
    client: &reqwest::Client,
    saves: &CloudSaves,
    dirs: &UserDirs,
    install: &Path,
) -> AppResult<usize> {
    let files = list(client, saves).await?;
    let mut uploaded = 0;
    for location in &saves.locations {
        let Some(dir) = folder(&location.location, dirs, install) else {
            continue;
        };
        let cloud = files_of(&files, location);
        let cloud_time = cloud.iter().filter_map(|(f, _)| f.last_modified).max();
        let local = dir.clone();
        let local_files =
            tokio::task::spawn_blocking(move || (newest_local(&local), files_below(&local)))
                .await
                .unwrap_or((None, Vec::new()));
        if !local_is_newer(cloud_time, local_files.0) {
            continue;
        }
        for path in local_files.1 {
            let Ok(relative) = path.strip_prefix(&dir) else {
                continue;
            };
            let relative: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let name = format!("{}/{}", location.name, relative.join("/"));
            let Some(hash) = md5_file(&path) else {
                continue;
            };
            if cloud
                .iter()
                .any(|(f, _)| f.name == name && f.hash.as_deref() == Some(hash.as_str()))
            {
                continue;
            }
            let Some(url) = file_url(saves, &name) else {
                continue;
            };
            let modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .map_or_else(|_| Utc::now(), DateTime::<Utc>::from);
            let data = tokio::fs::read(&path).await?;
            let response = client
                .put(&url)
                .bearer_auth(token(saves))
                .header("Etag", &hash)
                .header(
                    "X-Object-Meta-LocalLastModified",
                    modified.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                )
                .header("Content-Type", "application/octet-stream")
                .body(data)
                .send()
                .await
                .map_err(http_error)?;
            if !response.status().is_success() {
                return Err(status_error(response.status()));
            }
            uploaded += 1;
        }
    }
    Ok(uploaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn dirs(root: &Path) -> UserDirs {
        UserDirs::proton(root)
    }

    #[test]
    fn locations_become_folders_in_the_prefix() {
        let prefix = Path::new("/c/game");
        let user = prefix.join("pfx/drive_c/users/steamuser");
        let install = Path::new("/games/saltmarsh");
        let d = dirs(prefix);
        assert_eq!(
            folder("<?DOCUMENTS?>/Saltmarsh/Saves", &d, install),
            Some(user.join("Documents").join("Saltmarsh").join("Saves"))
        );
        assert_eq!(
            folder("<?APPLICATION_DATA_LOCAL_LOW?>\\Studio\\Game", &d, install),
            Some(
                user.join("AppData")
                    .join("LocalLow")
                    .join("Studio")
                    .join("Game")
            )
        );
        assert_eq!(
            folder("<?APPLICATION_DATA_LOCAL?>/GOG.com", &d, install),
            Some(user.join("AppData").join("Local").join("GOG.com"))
        );
        assert_eq!(
            folder("<?INSTALL?>/save", &d, install),
            Some(install.join("save"))
        );
        // macOS folders, unknown variables and climbing out are left alone.
        assert_eq!(folder("<?APPLICATION_SUPPORT?>/x", &d, install), None);
        assert_eq!(folder("<?DOCUMENTS?>/../../etc", &d, install), None);
        assert_eq!(folder("/etc/passwd", &d, install), None);
    }

    #[test]
    fn listing_times_with_and_without_zone() {
        let files: Vec<CloudFile> = serde_json::from_value(serde_json::json!([
            { "name": "saves/a.sav", "hash": "x", "bytes": 3, "last_modified": "2026-11-01T10:00:00.123456" },
            { "name": "saves/b.sav", "last_modified": "2026-11-01T10:00:00Z" },
            { "name": "saves/c.sav", "last_modified": "yesterday" }
        ]))
        .unwrap();
        assert_eq!(
            files[0].last_modified.unwrap().to_rfc3339(),
            "2026-11-01T10:00:00.123456+00:00"
        );
        assert!(files[1].last_modified.is_some());
        assert_eq!(files[2].last_modified, None);
    }

    fn md5(data: &[u8]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, data).unwrap();
        md5_file(&file).unwrap()
    }

    fn saves(server: &MockServer) -> CloudSaves {
        CloudSaves {
            enabled: true,
            client_id: Some("5150".into()),
            user_id: Some("4242".into()),
            access_token: Some("game-token".into()),
            storage_url: Some(format!("{}/v1/4242/5150", server.uri())),
            locations: vec![SaveLocation {
                name: "saves".into(),
                location: "<?DOCUMENTS?>/Saltmarsh".into(),
            }],
        }
    }

    #[tokio::test]
    async fn newer_cloud_saves_replace_local_ones_after_a_backup() {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("compat/g");
        let folder = prefix.join("pfx/drive_c/users/steamuser/Documents/Saltmarsh");
        std::fs::create_dir_all(folder.join("slots")).unwrap();
        std::fs::write(folder.join("slots/1.sav"), b"old").unwrap();
        std::fs::write(folder.join("same.sav"), b"same").unwrap();
        let old = Utc::now() - chrono::Duration::days(2);
        set_time(&folder.join("slots/1.sav"), old);
        set_time(&folder.join("same.sav"), old);
        Mock::given(method("GET"))
            .and(path("/v1/4242/5150"))
            .and(header("Authorization", "Bearer game-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": "saves/slots/1.sav", "hash": md5(b"new"), "last_modified": Utc::now().to_rfc3339() },
                { "name": "saves/same.sav", "hash": md5(b"same"), "last_modified": old.to_rfc3339() },
                { "name": "saves/../escape.sav", "hash": "x", "last_modified": Utc::now().to_rfc3339() },
                { "name": "other/x.sav", "hash": "x", "last_modified": Utc::now().to_rfc3339() }
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/4242/5150/saves/slots/1.sav"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new".to_vec()))
            .expect(1)
            .mount(&server)
            .await;

        let backups = root.path().join("save-backups/g");
        let written = sync_down(
            &reqwest::Client::new(),
            &saves(&server),
            &dirs(&prefix),
            Path::new("/i"),
            &backups,
        )
        .await
        .unwrap();
        assert_eq!(written, 1);
        assert_eq!(std::fs::read(folder.join("slots/1.sav")).unwrap(), b"new");
        assert!(!prefix
            .join("pfx/drive_c/users/steamuser/Documents/escape.sav")
            .exists());
        // The old file is in the backup.
        let backup = std::fs::read_dir(backups.join("saves"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(std::fs::read(backup.join("slots/1.sav")).unwrap(), b"old");

        // Up to date now: nothing happens on the next start.
        let again = sync_down(
            &reqwest::Client::new(),
            &saves(&server),
            &dirs(&prefix),
            Path::new("/i"),
            &backups,
        )
        .await
        .unwrap();
        assert_eq!(again, 0);
    }

    #[tokio::test]
    async fn newer_local_saves_are_uploaded_unless_the_cloud_has_them() {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("compat/g");
        let folder = prefix.join("pfx/drive_c/users/steamuser/Documents/Saltmarsh");
        std::fs::create_dir_all(folder.join("slots")).unwrap();
        std::fs::write(folder.join("slots/2 new.sav"), b"progress").unwrap();
        std::fs::write(folder.join("same.sav"), b"same").unwrap();
        let old = Utc::now() - chrono::Duration::days(2);
        Mock::given(method("GET"))
            .and(path("/v1/4242/5150"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": "saves/same.sav", "hash": md5(b"same"), "last_modified": old.to_rfc3339() }
            ])))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1/4242/5150/saves/slots/2%20new.sav"))
            .and(header("Etag", md5(b"progress").as_str()))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let uploaded = sync_up(
            &reqwest::Client::new(),
            &saves(&server),
            &dirs(&prefix),
            Path::new("/i"),
        )
        .await
        .unwrap();
        assert_eq!(uploaded, 1);
        let put = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.method.as_str() == "PUT")
            .unwrap();
        assert_eq!(put.body, b"progress");
        assert!(put.headers.contains_key("x-object-meta-locallastmodified"));
    }

    #[test]
    fn the_token_never_shows_in_logs() {
        let saves = CloudSaves {
            enabled: true,
            client_id: Some("1".into()),
            user_id: Some("2".into()),
            access_token: Some("secret-token".into()),
            storage_url: None,
            locations: Vec::new(),
        };
        assert!(!format!("{saves:?}").contains("secret-token"));
    }
}
