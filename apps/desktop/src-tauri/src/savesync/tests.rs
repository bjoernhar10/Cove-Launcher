//! Two computers sharing one fake cloud (`OneDrive` and Google Drive on
//! `wiremock`, files kept in memory), each with its own database, prefix and
//! keyring.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wiremock::matchers::any;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::*;
use crate::catalog::secrets::MemoryStore;
use crate::models::{CompatibilityTool, LaunchOptions, LibraryUpdate, ManualGameInput, Platform};

const YAML: &str = r"
Saltmarsh:
  files:
    <winDocuments>/Saltmarsh:
      tags: [save]
Cloudy:
  files:
    <winDocuments>/Cloudy:
      tags: [save]
  steam:
    id: 999
  cloud:
    steam: true
";

#[derive(Default)]
struct Store {
    /// `OneDrive`: path → content. Google: id → (name, content).
    files: HashMap<String, (String, Vec<u8>)>,
    /// Upload sessions: id → (name or file id, bytes so far).
    sessions: HashMap<String, (String, Vec<u8>)>,
    next: u32,
    refreshes: u32,
}

#[derive(Clone)]
struct FakeCloud {
    store: Arc<Mutex<Store>>,
    uri: String,
}

fn decode(text: &str) -> String {
    percent_encoding::percent_decode_str(text)
        .decode_utf8_lossy()
        .into_owned()
}

fn json(value: &serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(value)
}

impl FakeCloud {
    fn token(&self) -> ResponseTemplate {
        let mut store = self.store.lock().unwrap();
        store.refreshes += 1;
        json(&serde_json::json!({
            "access_token": "access", "accessToken": "access",
            "refresh_token": format!("refresh-{}", store.refreshes),
            "refreshToken": format!("refresh-{}", store.refreshes),
            "expires_in": 3600, "expiresIn": 3600
        }))
    }

    /// Bytes of an upload session; done = the whole file is there.
    fn chunk(&self, session: &str, request: &Request) -> Option<(String, Vec<u8>, bool)> {
        let mut store = self.store.lock().unwrap();
        let (target, data) = store.sessions.get_mut(session)?;
        data.extend_from_slice(&request.body);
        let total = request
            .headers
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(data.len());
        let done = data.len() >= total;
        Some((target.clone(), data.clone(), done))
    }
}

impl FakeCloud {
    fn dropbox(&self, path: &str, request: &Request) -> Option<ResponseTemplate> {
        let arg: serde_json::Value = request
            .headers
            .get("dropbox-api-arg")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default();
        let name = |value: &serde_json::Value| {
            value["path"]
                .as_str()
                .unwrap()
                .strip_prefix('/')
                .unwrap()
                .to_owned()
        };
        let mut store = self.store.lock().unwrap();
        let missing = || {
            ResponseTemplate::new(409)
                .set_body_json(serde_json::json!({ "error_summary": "path/not_found/" }))
        };
        Some(match path {
            "/dbapi/2/users/get_current_account" => {
                assert_eq!(request.body, b"null");
                json(&serde_json::json!({ "email": "ann@example.com" }))
            }
            "/dbcontent/2/files/download" => match store.files.get(&name(&arg)) {
                Some((_, data)) => ResponseTemplate::new(200).set_body_bytes(data.clone()),
                None => missing(),
            },
            "/dbcontent/2/files/upload" => {
                assert_eq!(arg["mode"], "overwrite");
                let name = name(&arg);
                store
                    .files
                    .insert(name.clone(), (name, request.body.clone()));
                json(&serde_json::json!({}))
            }
            "/dbcontent/2/files/upload_session/start" => {
                store.next += 1;
                let id = format!("d{}", store.next);
                store
                    .sessions
                    .insert(id.clone(), (String::new(), request.body.clone()));
                json(&serde_json::json!({ "session_id": id }))
            }
            "/dbcontent/2/files/upload_session/append_v2"
            | "/dbcontent/2/files/upload_session/finish" => {
                let id = arg["cursor"]["session_id"].as_str().unwrap().to_owned();
                let offset = arg["cursor"]["offset"].as_u64().unwrap();
                let data = &mut store.sessions.get_mut(&id).unwrap().1;
                assert_eq!(data.len() as u64, offset, "offset of {path}");
                data.extend_from_slice(&request.body);
                if path.ends_with("finish") {
                    let (_, data) = store.sessions.remove(&id).unwrap();
                    let name = name(&arg["commit"]);
                    store.files.insert(name.clone(), (name, data));
                }
                json(&serde_json::json!({}))
            }
            "/dbapi/2/files/delete_v2" => {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                match store.files.remove(&name(&body)) {
                    Some(_) => json(&serde_json::json!({})),
                    None => missing(),
                }
            }
            _ => return None,
        })
    }
}

impl Respond for FakeCloud {
    #[allow(clippy::too_many_lines)] // one fake per cloud API
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let auth = request
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        let path = request.url.path().to_owned();
        let method = request.method.as_str();
        if path == "/login/token"
            || path == "/proxy/v1/oauth/google/token"
            || path == "/dbapi/oauth2/token"
        {
            return self.token();
        }
        // Upload session URLs carry their own authorization (OneDrive).
        if let Some(session) = path.strip_prefix("/upload-session/") {
            let Some((target, data, done)) = self.chunk(session, request) else {
                return ResponseTemplate::new(404);
            };
            if !done {
                return ResponseTemplate::new(if target.starts_with("g:") { 308 } else { 202 });
            }
            let mut store = self.store.lock().unwrap();
            store.sessions.remove(session);
            match target.strip_prefix("g:") {
                Some(id) => {
                    if let Some(file) = store.files.get_mut(id) {
                        file.1 = data;
                    }
                }
                None => {
                    store.files.insert(target.clone(), (target, data));
                }
            }
            return ResponseTemplate::new(201).set_body_json(serde_json::json!({}));
        }
        if auth != Some("Bearer access") {
            return ResponseTemplate::new(401);
        }
        // ── Dropbox ──
        if let Some(response) = self.dropbox(&path, request) {
            return response;
        }
        // ── OneDrive ──
        if path == "/graph/me" {
            return json(&serde_json::json!({ "userPrincipalName": "ann@example.com" }));
        }
        if let Some(rest) = path.strip_prefix("/graph/me/drive/special/approot:/") {
            let (item, action) = rest.split_once(':').unwrap_or((rest, ""));
            let name = item.split('/').map(decode).collect::<Vec<_>>().join("/");
            let mut store = self.store.lock().unwrap();
            return match (method, action) {
                ("GET", "/content") => match store.files.get(&name) {
                    Some((_, data)) => ResponseTemplate::new(200).set_body_bytes(data.clone()),
                    None => ResponseTemplate::new(404),
                },
                ("PUT", "/content") => {
                    store
                        .files
                        .insert(name.clone(), (name, request.body.clone()));
                    ResponseTemplate::new(201).set_body_json(serde_json::json!({}))
                }
                ("POST", "/createUploadSession") => {
                    store.next += 1;
                    let id = store.next.to_string();
                    store.sessions.insert(id.clone(), (name, Vec::new()));
                    json(&serde_json::json!({
                        "uploadUrl": format!("{}/upload-session/{id}", self.uri)
                    }))
                }
                ("DELETE", "") => {
                    store.files.remove(&name);
                    ResponseTemplate::new(204)
                }
                _ => ResponseTemplate::new(400),
            };
        }
        // ── Google Drive ──
        let mut store = self.store.lock().unwrap();
        if path == "/drive/v3/about" {
            return json(&serde_json::json!({ "user": { "emailAddress": "ann@example.com" } }));
        }
        if path == "/drive/v3/files" {
            let q = request
                .url
                .query_pairs()
                .find(|(k, _)| k == "q")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            let wanted = q
                .strip_prefix("name = '")
                .and_then(|r| r.strip_suffix("' and trashed = false"))
                .unwrap_or_default()
                .replace("\\'", "'");
            let mut files: Vec<_> = store
                .files
                .iter()
                .filter(|(_, (name, _))| *name == wanted)
                .map(|(id, _)| serde_json::json!({ "id": id }))
                .collect();
            files.sort_by_key(ToString::to_string);
            return json(&serde_json::json!({ "files": files }));
        }
        if let Some(id) = path.strip_prefix("/drive/v3/files/") {
            return match method {
                "GET" => match store.files.get(id) {
                    Some((_, data)) => ResponseTemplate::new(200).set_body_bytes(data.clone()),
                    None => ResponseTemplate::new(404),
                },
                "DELETE" => {
                    store.files.remove(id);
                    ResponseTemplate::new(204)
                }
                _ => ResponseTemplate::new(400),
            };
        }
        if path == "/upload/drive/v3/files" && method == "POST" {
            let meta: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(meta["parents"][0], "appDataFolder");
            store.next += 1;
            let id = format!("f{}", store.next);
            let name = meta["name"].as_str().unwrap().to_owned();
            store.files.insert(id.clone(), (name, Vec::new()));
            store
                .sessions
                .insert(id.clone(), (format!("g:{id}"), Vec::new()));
            return ResponseTemplate::new(200)
                .insert_header("Location", format!("{}/upload-session/{id}", self.uri));
        }
        if let Some(id) = path.strip_prefix("/upload/drive/v3/files/") {
            store.next += 1;
            let session = format!("s{}", store.next);
            store
                .sessions
                .insert(session.clone(), (format!("g:{id}"), Vec::new()));
            return ResponseTemplate::new(200)
                .insert_header("Location", format!("{}/upload-session/{session}", self.uri));
        }
        ResponseTemplate::new(404)
    }
}

struct Device {
    sync: SaveSync,
    item: LibraryItem,
    host: HostEnv,
    saves: PathBuf,
    backups: Arc<BackupService>,
    notices: Arc<Mutex<Vec<Notice>>>,
    tmp: tempfile::TempDir,
}

async fn device(server: &MockServer, provider: CloudProvider, title: &str, name: &str) -> Device {
    let tmp = tempfile::tempdir().unwrap();
    let pool = db::open_in_memory().await.unwrap();
    let exe = tmp.path().join(format!("games/{title}/game.exe"));
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"MZ").unwrap();
    let item = db::library::add_manual(
        &pool,
        &ManualGameInput {
            title: title.into(),
            executable_path: exe.to_string_lossy().into_owned(),
            size_bytes: None,
        },
    )
    .await
    .unwrap();
    db::library::update(
        &pool,
        &item.game.id,
        &LibraryUpdate {
            launch_options: Some(LaunchOptions {
                compatibility_tool: CompatibilityTool::Proton,
                ..LaunchOptions::default()
            }),
            ..LibraryUpdate::default()
        },
    )
    .await
    .unwrap();
    let item = db::library::require(&pool, &item.game.id).await.unwrap();
    let defaults = crate::state::default_settings(None);
    let backups = Arc::new(BackupService::new(
        pool.clone(),
        defaults.clone(),
        tmp.path().join("data"),
        format!("{}/manifest.yaml", server.uri()),
    ));
    let mut host = HostEnv::detect(tmp.path().join("data/compat"));
    host.platform = Platform::Linux;
    host.home = Some(tmp.path().join("home"));
    let saves = tmp
        .path()
        .join("data/compat")
        .join(&item.game.id)
        .join(format!("pfx/drive_c/users/steamuser/Documents/{title}"));
    let secrets: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
    secrets
        .set(
            SESSION_KEY,
            &serde_json::to_string(&Session {
                provider,
                refresh_token: "refresh-0".into(),
                account: Some("ann@example.com".into()),
                needs_reauth: false,
            })
            .unwrap(),
        )
        .unwrap();
    let notices = Arc::new(Mutex::new(Vec::new()));
    let seen = notices.clone();
    let mut sync = SaveSync::new(
        pool,
        defaults,
        backups.clone(),
        secrets,
        endpoints(server),
        Arc::new(move |_, notice| seen.lock().unwrap().push(notice)),
    );
    sync.device = name.into();
    Device {
        sync,
        item,
        host,
        saves,
        backups,
        notices,
        tmp,
    }
}

fn endpoints(server: &MockServer) -> Endpoints {
    Endpoints {
        google_auth: format!("{}/auth", server.uri()),
        google_api: server.uri(),
        proxy: Some(format!("{}/proxy", server.uri())),
        microsoft_login: format!("{}/login", server.uri()),
        graph: format!("{}/graph", server.uri()),
        google_client_id: "gid".into(),
        onedrive_client_id: "oid".into(),
        dropbox_auth: format!("{}/dbauth", server.uri()),
        dropbox_api: format!("{}/dbapi", server.uri()),
        dropbox_content: format!("{}/dbcontent", server.uri()),
        dropbox_client_id: "did".into(),
    }
}

async fn cloud() -> (MockServer, FakeCloud) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/manifest.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(YAML))
        .with_priority(1)
        .mount(&server)
        .await;
    let fake = FakeCloud {
        store: Arc::default(),
        uri: server.uri(),
    };
    Mock::given(any())
        .respond_with(fake.clone())
        .mount(&server)
        .await;
    (server, fake)
}

fn write(dir: &Path, name: &str, data: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), data).unwrap();
}

impl Device {
    async fn after_session(&self) -> GameSaveSync {
        self.sync
            .run(&self.item, &self.host, Mode::AfterSession)
            .await
            .unwrap()
    }

    async fn before_launch(&self) -> GameSaveSync {
        self.sync
            .run(&self.item, &self.host, Mode::BeforeLaunch)
            .await
            .unwrap()
    }

    fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.saves.join(name)).unwrap()
    }
}

async fn two_computers_share_saves(provider: CloudProvider) {
    let (server, fake) = cloud().await;
    let pc = device(&server, provider, "Saltmarsh", "Desk").await;
    let deck = device(&server, provider, "Saltmarsh", "Deck").await;

    // Played on the pc: the save goes up.
    write(&pc.saves, "slot1.sav", b"chapter one");
    // Bigger than one upload piece and than OneDrive's simple upload.
    let big: Vec<u8> = (0..cloud_chunk() + 4096)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    write(&pc.saves.join("data"), "world.bin", &big);
    assert_eq!(pc.after_session().await.state, SaveSyncState::Synced);
    assert_eq!(*pc.notices.lock().unwrap(), [Notice::Changed]);

    // The deck (another prefix) gets it before the start.
    assert_eq!(deck.before_launch().await.state, SaveSyncState::Synced);
    assert_eq!(deck.read("slot1.sav"), b"chapter one");
    assert_eq!(deck.read("data/world.bin"), big);
    // Nothing changed: nothing happens.
    assert_eq!(deck.after_session().await.state, SaveSyncState::Synced);
    assert_eq!(*deck.notices.lock().unwrap(), [Notice::Changed]);

    // Played on the deck, then on the pc again.
    write(&deck.saves, "slot1.sav", b"chapter two");
    deck.after_session().await;
    pc.before_launch().await;
    assert_eq!(pc.read("slot1.sav"), b"chapter two");
    // The pc's old state was backed up before it was replaced.
    let backups = pc
        .backups
        .overview(&pc.item.game.id, &pc.host)
        .await
        .unwrap();
    assert_eq!(backups.backups.len(), 1);

    // Only what the manifest names stays in the cloud (the old blob is gone).
    let names: Vec<String> = fake
        .store
        .lock()
        .unwrap()
        .files
        .values()
        .map(|(name, _)| name.clone())
        .collect();
    let blobs = names.iter().filter(|n| n.contains("/blobs/")).count();
    assert_eq!(blobs, 2, "{names:?}");
    assert!(names
        .iter()
        .all(|n| n.starts_with("cove-saves/v1/saltmarsh-")));
}

fn cloud_chunk() -> usize {
    10 * 1024 * 1024
}

#[tokio::test]
async fn two_computers_share_saves_through_onedrive() {
    two_computers_share_saves(CloudProvider::OneDrive).await;
}

#[tokio::test]
async fn two_computers_share_saves_through_google_drive() {
    two_computers_share_saves(CloudProvider::GoogleDrive).await;
}

#[tokio::test]
async fn two_computers_share_saves_through_dropbox() {
    two_computers_share_saves(CloudProvider::Dropbox).await;
}

#[test]
fn dropbox_arguments_stay_ascii() {
    assert_eq!(
        cloud::dropbox_arg(&serde_json::json!({ "path": "/Spielstände ✓" })),
        r#"{"path":"/Spielst\u00e4nde \u2713"}"#
    );
}

#[tokio::test]
async fn a_save_rewritten_within_the_same_second_still_goes_up() {
    let (server, _) = cloud().await;
    let pc = device(&server, CloudProvider::OneDrive, "Saltmarsh", "Desk").await;
    let deck = device(&server, CloudProvider::OneDrive, "Saltmarsh", "Deck").await;
    write(&pc.saves, "slot1.sav", b"chapter one");
    pc.after_session().await;
    // Same size, same second, only the nanoseconds differ.
    let file = pc.saves.join("slot1.sav");
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    let second = before
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(&file, b"chapter two").unwrap();
    let nanos = if before
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
        == 500_000_000
    {
        250_000_000
    } else {
        500_000_000
    };
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::new(second, nanos))
        .unwrap();
    pc.after_session().await;
    deck.before_launch().await;
    assert_eq!(deck.read("slot1.sav"), b"chapter two");
}

#[tokio::test]
async fn a_conflict_waits_for_the_user_and_keeps_the_other_side() {
    let (server, _) = cloud().await;
    let pc = device(&server, CloudProvider::OneDrive, "Saltmarsh", "Desk").await;
    let deck = device(&server, CloudProvider::OneDrive, "Saltmarsh", "Deck").await;
    write(&pc.saves, "slot1.sav", b"pc");
    pc.after_session().await;
    // The deck played offline before it ever synced: two different saves.
    write(&deck.saves, "slot1.sav", b"deck");
    let status = deck.before_launch().await;
    assert_eq!(status.state, SaveSyncState::Conflict);
    let conflict = status.conflict.unwrap();
    assert_eq!(conflict.device, "Desk");
    assert_eq!(*deck.notices.lock().unwrap(), [Notice::Conflict]);
    // Nothing was overwritten, and nothing happens until the user chooses.
    assert_eq!(deck.read("slot1.sav"), b"deck");
    assert_eq!(deck.after_session().await.state, SaveSyncState::Conflict);
    assert_eq!(
        deck.sync.game(&deck.item.game.id).await.unwrap().state,
        SaveSyncState::Conflict
    );

    // The deck keeps its save; the pc's goes into a backup there.
    let resolved = deck
        .sync
        .resolve(&deck.item.game.id, SaveSyncKeep::Local, &deck.host)
        .await
        .unwrap();
    assert_eq!(resolved.state, SaveSyncState::Synced);
    let kept = deck
        .backups
        .overview(&deck.item.game.id, &deck.host)
        .await
        .unwrap();
    assert_eq!(kept.backups.len(), 1);
    pc.before_launch().await;
    assert_eq!(pc.read("slot1.sav"), b"deck");

    // Same saves on both sides is no conflict.
    let third = device(&server, CloudProvider::OneDrive, "Saltmarsh", "Laptop").await;
    write(&third.saves, "slot1.sav", b"deck");
    assert_eq!(third.before_launch().await.state, SaveSyncState::Synced);
}

#[tokio::test]
async fn steam_cloud_games_unknown_games_and_no_cloud_are_left_alone() {
    let (server, fake) = cloud().await;
    let unknown = device(&server, CloudProvider::OneDrive, "Nowhere", "Desk").await;
    assert_eq!(
        unknown.after_session().await.state,
        SaveSyncState::Unsupported
    );
    let mut steam = device(&server, CloudProvider::OneDrive, "Cloudy", "Desk").await;
    steam.item.entry.source = StoreSource::Steam;
    steam.item.entry.store_game_id = Some("999".into());
    write(&steam.saves, "a.sav", b"x");
    assert_eq!(steam.after_session().await.state, SaveSyncState::StoreCloud);
    assert!(fake.store.lock().unwrap().files.is_empty());

    unknown.sync.disconnect().await.unwrap();
    assert_eq!(
        unknown
            .sync
            .game(&unknown.item.game.id)
            .await
            .unwrap()
            .state,
        SaveSyncState::Off
    );
    assert_eq!(unknown.sync.status().await.unwrap().provider, None);
}

#[tokio::test]
async fn a_refused_refresh_token_asks_for_a_new_sign_in() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(400).set_body_json(
            serde_json::json!({ "error": { "code": "invalid_grant", "message": "x" } }),
        ))
        .mount(&server)
        .await;
    let pc = device(&server, CloudProvider::GoogleDrive, "Saltmarsh", "Desk").await;
    let error = pc
        .sync
        .run(&pc.item, &pc.host, Mode::Now)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::ReauthRequired(_)));
    let status = pc.sync.status().await.unwrap();
    assert!(status.needs_reauth);
    assert_eq!(status.account.as_deref(), Some("ann@example.com"));
}

#[tokio::test]
async fn onedrive_rotates_its_refresh_token() {
    let (server, _) = cloud().await;
    let pc = device(&server, CloudProvider::OneDrive, "Saltmarsh", "Desk").await;
    write(&pc.saves, "slot1.sav", b"x");
    pc.after_session().await;
    let session = pc.sync.session().await.unwrap().unwrap();
    assert_eq!(session.refresh_token, "refresh-1");
    assert!(!format!("{session:?}").contains("refresh-1"));
}

#[test]
fn decisions() {
    let manifest = |revision: &str| Manifest {
        format: FORMAT.into(),
        version: 1,
        revision: revision.into(),
        device: "Desk".into(),
        saved_at: String::new(),
        title: String::new(),
        files: Vec::new(),
    };
    let row = Row {
        revision: Some("r1".into()),
        fingerprint: Some("p1".into()),
        ..Row::default()
    };
    let m1 = manifest("r1");
    let m2 = manifest("r2");
    assert_eq!(decide(None, "p", true, None), Step::InSync);
    assert_eq!(decide(None, "p", false, None), Step::Push);
    assert_eq!(decide(None, "p", true, Some(&m1)), Step::Pull);
    assert_eq!(decide(None, "p", false, Some(&m1)), Step::Compare);
    assert_eq!(decide(Some(&row), "p1", false, Some(&m1)), Step::InSync);
    assert_eq!(decide(Some(&row), "p1", false, Some(&m2)), Step::Pull);
    assert_eq!(decide(Some(&row), "p2", false, Some(&m1)), Step::Push);
    assert_eq!(decide(Some(&row), "p2", false, Some(&m2)), Step::Compare);
}

#[test]
fn cloud_folders_are_stable_and_safe() {
    assert_eq!(remote_key("Saltmarsh"), remote_key("Saltmarsh"));
    assert_ne!(remote_key("Saltmarsh"), remote_key("Saltmarsh 2"));
    let key = remote_key("Tom Clancy's: \"Ghost\"/Recon");
    assert!(key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
}

#[test]
fn the_sign_in_page_asks_only_for_the_app_folder() {
    let endpoints = Endpoints {
        google_auth: "https://g/auth".into(),
        microsoft_login: "https://m".into(),
        google_client_id: "gid".into(),
        onedrive_client_id: "oid".into(),
        dropbox_auth: "https://d/authorize".into(),
        dropbox_client_id: "did".into(),
        ..Endpoints::default()
    };
    let google = cloud::authorize_url(
        &endpoints,
        CloudProvider::GoogleDrive,
        "http://127.0.0.1:1/callback",
        "st",
        "ch",
    );
    assert!(google.starts_with("https://g/auth?client_id=gid&"));
    assert!(google.contains("drive%2Eappdata"));
    assert!(google.contains("code_challenge_method=S256"));
    let onedrive = cloud::authorize_url(
        &endpoints,
        CloudProvider::OneDrive,
        "http://localhost:1/callback",
        "st",
        "ch",
    );
    assert!(onedrive.starts_with("https://m/authorize?client_id=oid&"));
    assert!(onedrive.contains("Files%2EReadWrite%2EAppFolder"));
    let dropbox = cloud::authorize_url(
        &endpoints,
        CloudProvider::Dropbox,
        "http://127.0.0.1:53682/callback",
        "st",
        "ch",
    );
    assert!(dropbox.contains("token_access_type=offline"));
    assert!(dropbox.contains("redirect_uri=http%3A%2F%2F127%2E0%2E0%2E1%3A53682%2Fcallback"));
    let (verifier, challenge) = cloud::pkce().unwrap();
    assert_eq!(verifier.len(), 43);
    assert_ne!(verifier, challenge);
}

async fn library_sync_goes_through_the_same_cloud(provider: CloudProvider) {
    let (server, fake) = cloud().await;
    let library = |device: Device| {
        let pool = device.sync.pool.clone();
        let id = device.item.game.id.clone();
        let sync = crate::librarysync::LibrarySync::new(
            pool.clone(),
            crate::state::default_settings(None),
            Arc::new(device.sync),
            Arc::new(|_| {}),
        );
        (pool, sync, id, device.tmp)
    };
    let (pc_pool, pc, id, _pc_tmp) = library(device(&server, provider, "Saltmarsh", "Desk").await);
    let (deck_pool, deck, deck_id, _deck_tmp) =
        library(device(&server, provider, "Saltmarsh", "Deck").await);
    let id = id.as_str();
    assert_eq!(id, deck_id);

    sqlx::query("UPDATE library_entries SET favorite = 1 WHERE game_id = ?")
        .bind(id)
        .execute(&pc_pool)
        .await
        .unwrap();
    db::notes::set(&pc_pool, id, "Lighthouse key")
        .await
        .unwrap();
    let status = pc.sync().await.unwrap();
    assert!(status.connected && status.last_synced_at.is_some() && status.problem.is_none());

    deck.sync().await.unwrap();
    let item = db::library::require(&deck_pool, id).await.unwrap();
    assert!(item.entry.favorite);
    assert_eq!(
        db::notes::get(&deck_pool, id).await.unwrap().unwrap().text,
        "Lighthouse key"
    );

    // Unfavoured on the deck: gone on the desk too, through a replaced file.
    sqlx::query("UPDATE library_entries SET favorite = 0 WHERE game_id = ?")
        .bind(id)
        .execute(&deck_pool)
        .await
        .unwrap();
    deck.sync().await.unwrap();
    pc.sync().await.unwrap();
    assert!(
        !db::library::require(&pc_pool, id)
            .await
            .unwrap()
            .entry
            .favorite
    );
    let names: Vec<String> = fake
        .store
        .lock()
        .unwrap()
        .files
        .values()
        .map(|(name, _)| name.clone())
        .collect();
    assert_eq!(
        names.iter().filter(|n| n.contains("library.json")).count(),
        1,
        "{names:?}"
    );

    // Switched off: nothing happens.
    let mut settings = db::settings::load(&pc_pool, &crate::state::default_settings(None))
        .await
        .unwrap();
    settings.backups.library_sync = false;
    db::settings::save(&pc_pool, &settings).await.unwrap();
    let before = db::settings::get_json::<String>(&pc_pool, crate::librarysync::AT_KEY)
        .await
        .unwrap();
    let status = pc.sync().await.unwrap();
    assert!(!status.enabled);
    assert_eq!(status.last_synced_at, before);
}

#[tokio::test]
async fn library_sync_through_onedrive() {
    library_sync_goes_through_the_same_cloud(CloudProvider::OneDrive).await;
}

#[tokio::test]
async fn library_sync_through_google_drive() {
    library_sync_goes_through_the_same_cloud(CloudProvider::GoogleDrive).await;
}

#[tokio::test]
async fn library_sync_through_dropbox() {
    library_sync_goes_through_the_same_cloud(CloudProvider::Dropbox).await;
}
