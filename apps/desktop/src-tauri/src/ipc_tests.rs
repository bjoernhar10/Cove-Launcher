//! End-to-end IPC tests: the exact JSON payloads the frontend sends
//! (camelCase keys, see `packages/shared/src/commands.ts`) go through Tauri's
//! real command router against an in-memory database.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

use crate::launcher::{HostEnv, Launcher, Timing};
use crate::models::{AppSettings, LibraryItem, ScanCandidate};
use crate::state::{default_settings, AppState};

struct TestApp {
    _app: tauri::App<MockRuntime>,
    webview: WebviewWindow<MockRuntime>,
    tmp: tempfile::TempDir,
}

/// Answers like the C# connectors (only what the IPC tests need).
fn fake_connectors(
    method: &str,
    params: &serde_json::Value,
) -> crate::error::AppResult<serde_json::Value> {
    use serde_json::json;
    match method {
        "store.loginComplete" if params["code"] == "bad" => Err(
            crate::error::AppError::LoginFailed("Das ist kein gültiger Epic-Anmeldecode.".into()),
        ),
        "store.loginComplete" => Ok(
            json!({ "store": params["store"], "accountId": "acc1", "displayName": "Tiny",
            "accessToken": "a", "refreshToken": "r", "expiresAt": "2099-01-01T00:00:00Z" }),
        ),
        "store.library" => Ok(json!({ "games": [
            { "storeGameId": "Sugar", "title": "Rocket League", "playtimeMinutes": 120, "lastPlayedAt": null, "imageUrl": null } ] })),
        "store.detectLocal" => Ok(json!({ "games": [
            { "store": "steam", "storeGameId": "105600", "title": "Terraria", "installPath": "/games/Terraria",
              "executablePath": null, "sizeBytes": 470_000_000, "source": "steam-manifest" } ] })),
        _ => Ok(serde_json::Value::Null),
    }
}

async fn app() -> TestApp {
    app_with(
        crate::stores::tests::FakeRpc::new(fake_connectors),
        Arc::new(crate::catalog::secrets::MemoryStore::default()),
        "http://127.0.0.1:1",
    )
    .await
}

/// The app state with exchangeable connectors, keyring and itch.io API.
#[allow(clippy::too_many_lines)] // one field per service, all of them test doubles
async fn app_with(
    connectors: Arc<dyn crate::stores::ConnectorRpc>,
    secrets: Arc<dyn crate::catalog::secrets::SecretStore>,
    itch_base: &str,
) -> TestApp {
    let tmp = tempfile::tempdir().unwrap();
    let pool = crate::db::open_in_memory().await.unwrap();
    let catalog = Arc::new(crate::catalog::CatalogService::new(
        pool.clone(),
        secrets.clone(),
        tmp.path().join("images"),
        default_settings(None),
        // Tests never talk to real services, not even the default proxy.
        crate::catalog::Endpoints {
            cove_proxy: None,
            ..crate::catalog::Endpoints::default()
        },
    ));
    let plugins = Arc::new(crate::plugins::Plugins::new(
        pool.clone(),
        tmp.path().join("plugins"),
        vec![tmp.path().join("dev-plugins")],
    ));
    let state = AppState {
        launcher: Launcher::new(
            pool.clone(),
            Arc::new(|_| {}),
            Arc::new(|_| Ok(())),
            Timing::default(),
        ),
        host: HostEnv::detect(tmp.path().join("compat")),
        defaults: default_settings(None),
        catalog: catalog.clone(),
        plugins: plugins.clone(),
        recommender: Arc::new(crate::recommend::Recommender::new(
            crate::sidecar::Sidecar::new(None),
            pool.clone(),
            catalog.clone(),
        )),
        stores: Arc::new(crate::stores::StoreService::new(
            connectors,
            pool.clone(),
            secrets.clone(),
            None,
            std::time::Duration::ZERO,
        )),
        login_flows: Arc::new(crate::stores::login::LoginFlows::default()),
        mover: Arc::new(crate::mover::Mover::new(pool.clone(), Arc::new(|_| {}))),
        cove_achievements: Arc::new(crate::cove_achievements::CoveAchievements::new(
            pool.clone(),
            plugins.clone(),
            Arc::new(|_| {}),
        )),
        open_url: Arc::new(|_| Ok(())),
        // Nothing listens there: tests never talk to GitHub.
        proton: Arc::new(crate::proton::ProtonManager::with_endpoints(
            pool.clone(),
            tmp.path().join("proton"),
            None,
            default_settings(None),
            Arc::new(|_| {}),
            "http://127.0.0.1:1/releases".into(),
            crate::proton::UmuConfig {
                root: tmp.path().join("umu"),
                release_url: "http://127.0.0.1:1/umu".into(),
                system: None,
                python: true,
            },
        )),
        // Nothing listens there: tests never talk to ProtonDB or Steam.
        // Nothing listens there: tests never download the Ludusavi manifest.
        backups: Arc::new(crate::backups::BackupService::new(
            pool.clone(),
            default_settings(None),
            tmp.path().to_path_buf(),
            "http://127.0.0.1:1/manifest.yaml".into(),
        )),
        prices: Arc::new(crate::prices::PriceService::with_base(
            pool.clone(),
            Some("http://127.0.0.1:1".into()),
        )),
        protondb: Arc::new(crate::protondb::ProtonDb::with_endpoints(
            pool.clone(),
            "http://127.0.0.1:1".into(),
            "http://127.0.0.1:1".into(),
        )),
        downloads: Arc::new(crate::downloads::DownloadManager::new(
            pool.clone(),
            default_settings(None),
            crate::downloads::itch::ItchApi::new(secrets, itch_base),
            None,
            crate::downloads::Hooks {
                changed: Arc::new(|| {}),
                progress: Arc::new(|_| {}),
                installed: Arc::new(|_| {}),
                open_url: Arc::new(|_| Ok(())),
            },
        )),
        pool,
    };
    let app = mock_builder()
        .manage(state)
        .invoke_handler(crate::cove_handlers!())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .unwrap();
    TestApp {
        _app: app,
        webview,
        tmp,
    }
}

fn invoke_raw(app: &TestApp, cmd: &str, args: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        // The app's own origin: Windows serves it as http://tauri.localhost.
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(args),
        headers: tauri::http::HeaderMap::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(&app.webview, request).map(|body| body.deserialize::<Value>().unwrap())
}

fn invoke<T: DeserializeOwned>(app: &TestApp, cmd: &str, args: Value) -> T {
    let value = invoke_raw(app, cmd, args).unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
    serde_json::from_value(value).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn library_flow_with_frontend_payloads() {
    let app = app().await;
    let exe = app.tmp.path().join("game.x86_64");
    std::fs::write(&exe, b"x").unwrap();
    let exe = exe.to_string_lossy().into_owned();

    let added: LibraryItem = invoke(
        &app,
        "library_add_manual",
        json!({ "input": { "title": "Tide", "executablePath": exe, "sizeBytes": null } }),
    );
    let id = added.game.id;

    let updated: LibraryItem = invoke(
        &app,
        "library_update",
        json!({ "gameId": id, "patch": { "favorite": true, "launchOptions": { "arguments": "-x", "compatibilityTool": "wine", "env": [] } } }),
    );
    assert!(updated.entry.favorite);
    assert_eq!(updated.entry.launch_options.arguments, "-x");

    let list: Vec<LibraryItem> = invoke(&app, "library_list", json!({}));
    assert_eq!(list.len(), 1);
    let _: Value = invoke(&app, "library_playtime", json!({ "gameId": id }));
    let wishlist: Vec<Value> = invoke(&app, "wishlist_list", json!({}));
    assert_eq!(wishlist, Vec::<Value>::new());
    let _: Value = invoke(&app, "wishlist_remove", json!({ "gameId": "igdb-1" }));
    let update = invoke_raw(
        &app,
        "wishlist_update",
        json!({ "gameId": "igdb-1", "targetPrice": 9.99, "notifyOnSale": true }),
    )
    .unwrap_err();
    assert_eq!(update["kind"], "invalidInput");
    let stats: Value = invoke(&app, "stats_overview", json!({ "year": null }));
    assert_eq!(stats["months"].as_array().map(Vec::len), Some(12));
    let year: Value = invoke(&app, "stats_overview", json!({ "year": 2026 }));
    assert_eq!(year["year"], 2026);
    let _: Value = invoke(&app, "achievements_recent", json!({ "limit": 6 }));
    let running: Vec<Value> = invoke(&app, "game_running_list", json!({}));
    assert_eq!(running, Vec::<Value>::new());

    let scan: Vec<ScanCandidate> = invoke(
        &app,
        "library_scan_folder",
        json!({ "path": app.tmp.path().to_string_lossy() }),
    );
    assert!(
        scan.iter().all(|c| c.already_in_library),
        "the added game is recognised"
    );

    let _: Value = invoke(&app, "library_remove", json!({ "gameId": id }));
    let error = invoke_raw(&app, "library_remove", json!({ "gameId": id })).unwrap_err();
    assert_eq!(error["kind"], "notInLibrary");
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_reach_the_frontend_as_kind_and_message() {
    let app = app().await;
    let missing = invoke_raw(
        &app,
        "library_add_manual",
        json!({ "input": { "title": "X", "executablePath": "/definitely/missing", "sizeBytes": null } }),
    )
    .unwrap_err();
    assert_eq!(missing["kind"], "invalidInput");
    assert_eq!(
        missing["message"],
        "Datei nicht gefunden: /definitely/missing"
    );

    let login = invoke_raw(
        &app,
        "connector_login_complete",
        json!({ "store": "epic", "code": "bad" }),
    )
    .unwrap_err();
    assert_eq!(login["kind"], "loginFailed");
    assert_eq!(login["message"], "Das ist kein gültiger Epic-Anmeldecode.");
    let not_signed_in =
        invoke_raw(&app, "connector_refresh", json!({ "store": "gog" })).unwrap_err();
    assert_eq!(not_signed_in["kind"], "invalidInput");
    let stop = invoke_raw(&app, "game_stop", json!({ "gameId": "nope" })).unwrap_err();
    assert_eq!(stop["kind"], "notRunning");
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_and_misc_commands() {
    let app = app().await;
    let mut settings: AppSettings = invoke(&app, "settings_get", json!({}));
    settings.appearance.theme = "dark".into();
    settings.library.scan_folders = vec!["/games".into()];
    let saved: AppSettings = invoke(&app, "settings_update", json!({ "settings": settings }));
    assert_eq!(saved.appearance.theme, "dark");
    let reloaded: AppSettings = invoke(&app, "settings_get", json!({}));
    assert_eq!(reloaded.library.scan_folders, ["/games"]);

    for (cmd, args) in [
        ("connectors_list", json!({})),
        ("cove_achievements_list", json!({})),
        ("plugins_list", json!({})),
        ("downloads_list", json!({})),
        ("downloads_reorder", json!({ "orderedIds": [] })),
        ("downloads_clear_history", json!({})),
        ("system_profile", json!({})),
        ("system_about", json!({})),
    ] {
        invoke_raw(&app, cmd, args).unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
    }

    // Sidecars start on first use: asking for the status must not start them.
    let about: Value = invoke(&app, "system_about", json!({}));
    assert_eq!(about["components"][0]["status"], "running");
    assert_ne!(about["components"][2]["status"], "running");

    // The test app has no updater plugin: a clear error instead of a panic.
    for cmd in ["updates_check", "updates_install"] {
        let error = invoke_raw(&app, cmd, json!({})).unwrap_err();
        assert_eq!(error["kind"], "update", "{cmd}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn catalog_commands_accept_frontend_payloads() {
    let app = app().await;
    let status: Value = invoke(&app, "catalog_status", json!({}));
    assert_eq!(
        status["configured"],
        json!({ "igdb": false, "rawg": false })
    );

    let browse = invoke_raw(
        &app,
        "catalog_browse",
        json!({ "filter": { "query": "", "genres": ["rpg"], "platforms": ["linux"], "releaseYears": null, "minRating": null }, "sort": "popular", "page": 0 }),
    )
    .unwrap_err();
    assert_eq!(browse["kind"], "catalogNotConfigured");

    let empty: Value = invoke(&app, "catalog_search", json!({ "query": "  " }));
    assert_eq!(empty["items"], json!([]));

    let creds = invoke_raw(
        &app,
        "catalog_set_credentials",
        json!({ "credentials": { "provider": "igdb", "clientId": "", "clientSecret": "" } }),
    )
    .unwrap_err();
    assert_eq!(creds["kind"], "invalidInput");
    let _: Value = invoke(
        &app,
        "catalog_clear_credentials",
        json!({ "provider": "rawg" }),
    );
    let details =
        invoke_raw(&app, "catalog_game_details", json!({ "gameId": "igdb-1" })).unwrap_err();
    assert_eq!(details["kind"], "catalogNotConfigured");
    // No catalogue: no times, but no error either (the library filter just finds nothing).
    let times: Value = invoke(
        &app,
        "catalog_time_to_beat",
        json!({ "catalogIds": ["igdb-1", "rawg-2", "x"] }),
    );
    assert_eq!(times, json!({}));

    let candidates: Vec<Value> = invoke(&app, "library_match_candidates", json!({ "gameId": "x" }));
    assert_eq!(candidates, Vec::<Value>::new());
    let reject = invoke_raw(&app, "library_reject_match", json!({ "gameId": "x" })).unwrap_err();
    assert_eq!(reject["kind"], "notInLibrary");
}

#[tokio::test(flavor = "multi_thread")]
async fn connector_commands_accept_frontend_payloads() {
    let app = app().await;
    let accounts: Vec<crate::models::StoreAccount> = invoke(&app, "connectors_list", json!({}));
    assert_eq!(accounts.len(), 4);

    // Local detection works without login; `store: null` = all stores.
    let local: Vec<crate::models::LocalStoreGame> =
        invoke(&app, "connector_detect_local", json!({ "store": null }));
    assert_eq!(
        (local[0].title.as_str(), local[0].in_library),
        ("Terraria", false)
    );

    // Pasted Epic code → signed in.
    let account: Value = invoke(
        &app,
        "connector_login_complete",
        json!({ "store": "epic", "code": "abc123" }),
    );
    assert_eq!(account["status"], "connected");
    assert_eq!(account["accountName"], "Tiny");
    assert_eq!(account["loginMode"], "webview");

    // The login already started a background import; either run may add the game.
    let summary: crate::models::ImportSummary =
        invoke(&app, "connector_import_library", json!({ "store": "epic" }));
    assert_eq!((summary.total, summary.added + summary.updated), (1, 1));
    let steam: Value = invoke(
        &app,
        "connector_import_library",
        json!({ "store": "steam" }),
    );
    assert_eq!(
        steam["added"], 1,
        "installed Steam games come in without login"
    );

    let synced: Value = invoke(&app, "achievements_sync", json!({ "store": null }));
    assert_eq!(
        synced,
        json!({ "games": 0, "achievements": 0, "newlyUnlocked": 0 })
    );

    invoke_raw(&app, "connector_login_cancel", json!({ "store": "gog" })).unwrap();
    let out: Value = invoke(
        &app,
        "connector_logout",
        json!({ "store": "epic", "keepGames": true }),
    );
    assert_eq!(out["status"], "disconnected");
    assert_eq!(out["gameCount"], 1, "games were kept");
    let out: Value = invoke(
        &app,
        "connector_logout",
        json!({ "store": "epic", "keepGames": false }),
    );
    assert_eq!(out["gameCount"], 0);

    let achievements: Vec<crate::models::CoveAchievement> =
        invoke(&app, "cove_achievements_list", json!({}));
    assert_eq!(achievements.len(), 6);
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)] // one walk through all download commands
async fn download_commands_accept_frontend_payloads() {
    let app = app().await;
    let games = app.tmp.path().join("Games").to_string_lossy().into_owned();
    let target = json!({ "folder": games, "makeDefault": false });
    let item: Value = invoke(
        &app,
        "downloads_add_url",
        json!({ "input": { "url": "http://127.0.0.1:1/files/lantern-linux.zip", "title": null, "sha256": null }, "target": target }),
    );
    assert_eq!(item["download"]["status"], "queued");
    assert_eq!(item["download"]["origin"], "direct");
    assert_eq!(item["download"]["checksumVerified"], false);

    let bad = invoke_raw(
        &app,
        "downloads_add_url",
        // The install dialog's DLC choice is part of the target.
        json!({ "input": { "url": "ftp://x/y", "title": "X", "sha256": null },
                "target": { "folder": games, "makeDefault": false, "excludedDlcs": ["200"] } }),
    )
    .unwrap_err();
    assert_eq!(bad["kind"], "invalidInput");
    let preview = invoke_raw(
        &app,
        "downloads_preview_url",
        json!({ "input": { "url": "ftp://x/y", "title": null, "sha256": null } }),
    )
    .unwrap_err();
    assert_eq!(preview["kind"], "invalidInput");

    let folder: Value = invoke(&app, "system_folder_info", json!({ "path": games }));
    assert_eq!(folder["writable"], true);
    assert_eq!(folder["needsAdmin"], false);
    assert!(folder["freeBytes"].as_i64().is_some_and(|b| b > 0));
    let relative = invoke_raw(&app, "system_folder_info", json!({ "path": "Games" })).unwrap_err();
    assert_eq!(relative["kind"], "invalidInput");

    let id = item["download"]["id"].as_str().unwrap();
    invoke_raw(
        &app,
        "downloads_set_status",
        json!({ "id": id, "status": "paused" }),
    )
    .unwrap();
    invoke_raw(
        &app,
        "downloads_set_status",
        json!({ "id": id, "status": "cancelled" }),
    )
    .unwrap();
    let list: Vec<crate::models::DownloadItem> = invoke(&app, "downloads_list", json!({}));
    assert_eq!(
        list[0].download.status,
        crate::models::DownloadStatus::Cancelled
    );

    let status: Value = invoke(&app, "itch_status", json!({}));
    assert_eq!(status, json!({ "connected": false, "userName": null }));
    let key = invoke_raw(&app, "itch_set_api_key", json!({ "apiKey": "not a key!" })).unwrap_err();
    assert_eq!(key["kind"], "invalidInput");
    let owned = invoke_raw(&app, "itch_owned_games", json!({})).unwrap_err();
    assert_eq!(owned["kind"], "itch");
    let uploads = invoke_raw(
        &app,
        "itch_uploads",
        json!({ "gameId": 1, "downloadKeyId": null }),
    )
    .unwrap_err();
    assert_eq!(uploads["kind"], "itch");
    let add = invoke_raw(
        &app,
        "downloads_add_itch",
        json!({ "gameId": 1, "uploadId": 2, "downloadKeyId": 3, "title": "Zeta", "target": target, "extras": [4] }),
    )
    .unwrap_err();
    assert_eq!(add["kind"], "itch");
    let lookup = invoke_raw(&app, "itch_lookup", json!({ "url": "nonsense" })).unwrap_err();
    assert_eq!(lookup["kind"], "invalidInput");
    invoke_raw(&app, "itch_clear_api_key", json!({})).unwrap();

    // Manual games cannot be handed to a store.
    let handoff = invoke_raw(
        &app,
        "downloads_enqueue",
        json!({ "gameId": list[0].game.id }),
    )
    .unwrap_err();
    assert_eq!(handoff["kind"], "notInLibrary");

    // Uninstall and move: the game must be in the library and installed by Cove.
    let exe = app.tmp.path().join("handmade/run");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
    let manual: Value = invoke(
        &app,
        "library_add_manual",
        json!({ "input": { "title": "Hand Made", "executablePath": exe.to_string_lossy(), "sizeBytes": null } }),
    );
    let game_id = manual["game"]["id"].as_str().unwrap();
    assert_eq!(manual["entry"]["launchVia"], "direct");
    // GOG installs, updates: only for GOG games.
    let preview = invoke_raw(
        &app,
        "downloads_preview_store",
        json!({ "gameId": game_id }),
    )
    .unwrap_err();
    assert_eq!(preview["kind"], "invalidInput");
    let check = invoke_raw(&app, "game_update_check", json!({ "gameId": game_id })).unwrap_err();
    assert_eq!(check["kind"], "invalidInput");
    let update = invoke_raw(&app, "game_update", json!({ "gameId": "nope" })).unwrap_err();
    assert_eq!(update["kind"], "notInLibrary");
    let dlcs = invoke_raw(
        &app,
        "game_set_dlcs",
        json!({ "gameId": "nope", "excludedDlcs": ["200"] }),
    )
    .unwrap_err();
    assert_eq!(dlcs["kind"], "notInLibrary");
    let enqueue = invoke_raw(
        &app,
        "downloads_enqueue",
        json!({ "gameId": game_id, "target": target }),
    )
    .unwrap_err();
    assert_eq!(enqueue["kind"], "invalidInput");
    let uninstall = invoke_raw(&app, "game_uninstall", json!({ "gameId": game_id })).unwrap_err();
    assert_eq!(uninstall["kind"], "invalidInput");
    // The DLC list is for store games only, "In Steam verwalten" for Steam games.
    for command in ["game_dlcs", "game_manage_dlcs"] {
        let error = invoke_raw(&app, command, json!({ "gameId": game_id })).unwrap_err();
        assert_eq!(error["kind"], "invalidInput", "{command}");
    }
    let moved = invoke_raw(
        &app,
        "game_move",
        json!({ "gameId": "nope", "folder": games }),
    )
    .unwrap_err();
    assert_eq!(moved["kind"], "notInLibrary");
    let moved = invoke_raw(
        &app,
        "game_move",
        json!({ "gameId": game_id, "folder": "relative/folder" }),
    )
    .unwrap_err();
    assert_eq!(moved["kind"], "invalidInput");
}

#[tokio::test(flavor = "multi_thread")]
async fn collection_commands_accept_frontend_payloads() {
    let app = app().await;
    let exe = app.tmp.path().join("hand-made");
    std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
    let game: Value = invoke(
        &app,
        "library_add_manual",
        json!({ "input": { "title": "Hand Made", "executablePath": exe.to_string_lossy(), "sizeBytes": null } }),
    );
    let game_id = game["game"]["id"].clone();

    let none: Value = invoke(&app, "collections_list", json!({}));
    assert_eq!(none, json!([]));
    let created: Value = invoke(&app, "collections_create", json!({ "name": "Koop" }));
    let id = created["id"].clone();
    assert_eq!(created, json!({ "id": id, "name": "Koop", "gameIds": [] }));
    let taken = invoke_raw(&app, "collections_create", json!({ "name": "koop" })).unwrap_err();
    assert_eq!(taken["kind"], "invalidInput");
    assert_eq!(taken["message"], "Eine Sammlung „koop“ gibt es schon.");

    let _: Value = invoke(
        &app,
        "collections_set_game",
        json!({ "collectionId": id, "gameId": game_id, "member": true }),
    );
    let renamed: Value = invoke(
        &app,
        "collections_rename",
        json!({ "collectionId": id, "name": "Mit Freunden" }),
    );
    assert_eq!(
        renamed,
        json!({ "id": id, "name": "Mit Freunden", "gameIds": [game_id] })
    );
    let listed: Value = invoke(&app, "collections_list", json!({}));
    assert_eq!(listed, json!([renamed]));

    let unknown = invoke_raw(
        &app,
        "collections_set_game",
        json!({ "collectionId": id, "gameId": "nope", "member": true }),
    )
    .unwrap_err();
    assert_eq!(unknown["kind"], "notInLibrary");
    let _: Value = invoke(
        &app,
        "collections_set_game",
        json!({ "collectionId": id, "gameId": game_id, "member": false }),
    );
    let _: Value = invoke(&app, "collections_delete", json!({ "collectionId": id }));
    let gone = invoke_raw(&app, "collections_delete", json!({ "collectionId": id })).unwrap_err();
    assert_eq!(gone["kind"], "invalidInput");
    let empty: Value = invoke(&app, "collections_list", json!({}));
    assert_eq!(empty, json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn proton_rating_accepts_frontend_payloads() {
    use tauri::Manager;

    let app = app().await;
    let add = |title: &str| -> Value {
        let exe = app.tmp.path().join(title);
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        invoke(
            &app,
            "library_add_manual",
            json!({ "input": { "title": title, "executablePath": exe.to_string_lossy(), "sizeBytes": null } }),
        )
    };
    let rated = add("Hand Made");
    let unknown = add("Never Heard");

    // What an earlier lookup left in the cache: the Steam app of that title and its rating.
    let pool = app.webview.state::<AppState>().pool.clone();
    for (key, value) in [
        ("protondb:steam-app:hand made", r#""13""#),
        (
            "protondb:rating:13",
            r#"{"tier":"gold","reports":12,"steamAppId":"13"}"#,
        ),
    ] {
        crate::db::catalog_cache::put(&pool, "protondb", key, value)
            .await
            .unwrap();
    }

    let rating: Value = invoke(
        &app,
        "game_proton_rating",
        json!({ "gameId": rated["game"]["id"] }),
    );
    assert_eq!(
        rating,
        json!({ "tier": "gold", "reports": 12, "steamAppId": "13" })
    );
    let offline = invoke_raw(
        &app,
        "game_proton_rating",
        json!({ "gameId": unknown["game"]["id"] }),
    )
    .unwrap_err();
    assert_eq!(offline["kind"], "storeUnavailable");
    let missing = invoke_raw(&app, "game_proton_rating", json!({ "gameId": "nope" })).unwrap_err();
    assert_eq!(missing["kind"], "notInLibrary");
}

#[tokio::test(flavor = "multi_thread")]
async fn backups_and_emulators_accept_frontend_payloads() {
    let app = app().await;
    // ROM folders are part of the settings; a scan adds the ROMs with a collection.
    let roms = app.tmp.path().join("roms/snes");
    std::fs::create_dir_all(&roms).unwrap();
    std::fs::write(roms.join("Space Game (USA).sfc"), b"rom").unwrap();
    let mut settings: Value = invoke(&app, "settings_get", json!({}));
    settings["emulators"] = json!({ "romFolders": [{ "id": "snes-1", "system": "snes", "path": roms.to_string_lossy(),
        "emulator": "retroarch", "program": null, "arguments": "" }] });
    settings["backups"] = json!({ "enabled": true, "directory": "", "keep": 5 });
    let _: Value = invoke(&app, "settings_update", json!({ "settings": settings }));
    let systems: Vec<Value> = invoke(&app, "emulators_systems", json!({}));
    assert!(systems
        .iter()
        .any(|s| s["id"] == "ps2" && s["emulators"][1]["id"] == "pcsx2"));
    let scan: Value = invoke(&app, "emulators_scan", json!({}));
    assert_eq!(scan, json!({ "found": 1, "added": 1 }));
    let again: Value = invoke(&app, "emulators_scan", json!({}));
    assert_eq!(again, json!({ "found": 1, "added": 0 }));
    let collections: Vec<Value> = invoke(&app, "collections_list", json!({}));
    assert_eq!(collections[0]["name"], "Super Nintendo");
    let game = collections[0]["gameIds"][0].clone();
    let bad = invoke_raw(
        &app,
        "settings_update",
        json!({ "settings": { "emulators": { "romFolders": [] } } }),
    );
    assert!(bad.is_err());

    // Backups: the manifest can't be loaded here, so the page says so.
    let overview = invoke_raw(&app, "game_backups", json!({ "gameId": game })).unwrap_err();
    assert_eq!(overview["kind"], "offline");
    let now = invoke_raw(&app, "game_backup_now", json!({ "gameId": game })).unwrap_err();
    assert_eq!(now["kind"], "offline");
    let restore = invoke_raw(
        &app,
        "game_backup_restore",
        json!({ "gameId": game, "backupId": "../x" }),
    )
    .unwrap_err();
    assert_eq!(restore["kind"], "invalidInput");
}

#[tokio::test(flavor = "multi_thread")]
async fn game_options_and_prefix_accept_frontend_payloads() {
    let app = app().await;
    let overview: Value = invoke(&app, "proton_list", json!({}));
    let exe = app.tmp.path().join("game.exe");
    std::fs::write(&exe, b"MZ").unwrap();
    let added: Value = invoke(
        &app,
        "library_add_manual",
        json!({ "input": { "title": "Windows Game", "executablePath": exe.to_string_lossy(), "sizeBytes": null } }),
    );
    let id = added["game"]["id"].clone();

    // Game options travel with the launch options; the tools are only reported.
    let tools: Value = invoke(&app, "system_game_tools", json!({}));
    assert!(tools["mangohud"].is_boolean() && tools["gamescope"].is_boolean());
    let tweaked: Value = invoke(
        &app,
        "library_update",
        json!({ "gameId": id, "patch": { "launchOptions": {
            "arguments": "", "compatibilityTool": "proton", "env": [], "protonVersion": null,
            "tweaks": { "mangohud": true, "gamemode": false, "gamescope": true, "gamescopeWidth": 1280,
                        "gamescopeHeight": 720, "gamescopeFullscreen": true, "fsr": true, "dxvk": true, "vkd3d": false } } } }),
    );
    assert_eq!(
        tweaked["entry"]["launchOptions"]["tweaks"]["gamescopeWidth"],
        1280
    );
    assert_eq!(tweaked["entry"]["launchOptions"]["tweaks"]["vkd3d"], false);

    // umu: on by default; nothing to cancel.
    assert_eq!(overview["umu"]["enabled"], true);
    let idle = invoke_raw(&app, "proton_cancel", json!({ "versionId": "umu" })).unwrap_err();
    assert_eq!(idle["kind"], "invalidInput");

    // The prefix: not there yet, then opened, reset.
    let info: Value = invoke(&app, "game_prefix_info", json!({ "gameId": id }));
    assert_eq!(info["exists"], false);
    assert_eq!(info["sizeBytes"], Value::Null);
    let not_yet = invoke_raw(
        &app,
        "game_prefix_action",
        json!({ "gameId": id, "action": "open" }),
    )
    .unwrap_err();
    assert_eq!(not_yet["kind"], "invalidInput");
    let pfx = app
        .tmp
        .path()
        .join("compat")
        .join(id.as_str().unwrap())
        .join("pfx/drive_c");
    std::fs::create_dir_all(&pfx).unwrap();
    std::fs::write(pfx.join("save.dat"), [1u8; 10]).unwrap();
    let info: Value = invoke(&app, "game_prefix_info", json!({ "gameId": id }));
    assert_eq!(info["exists"], true);
    assert_eq!(info["sizeBytes"], 10);
    let reset: Value = invoke(
        &app,
        "game_prefix_action",
        json!({ "gameId": id, "action": "reset" }),
    );
    assert_eq!(reset, Value::Null);
    assert!(!pfx.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn proton_commands_accept_frontend_payloads() {
    let app = app().await;
    // A GE-Proton build Cove installed earlier.
    let build = app.tmp.path().join("proton/GE-Proton10-1");
    std::fs::create_dir_all(&build).unwrap();
    std::fs::write(build.join("proton"), b"#!/usr/bin/env python3\n").unwrap();

    // GitHub is unreachable: only installed builds are listed.
    let overview: Value = invoke(&app, "proton_list", json!({}));
    assert_eq!(overview["offline"], true);
    assert_eq!(overview["latest"], "GE-Proton10-1");
    assert_eq!(
        overview["versions"],
        json!([{ "id": "GE-Proton10-1", "name": "GE-Proton10-1", "kind": "ge", "installed": true,
                 "downloadBytes": null, "releasedAt": null, "games": 0, "removable": true }])
    );
    let checked: Value = invoke(&app, "proton_check", json!({}));
    assert_eq!(checked["offline"], true);

    // A game pinned to it keeps it.
    let exe = app.tmp.path().join("game.exe");
    std::fs::write(&exe, b"MZ").unwrap();
    let added: Value = invoke(
        &app,
        "library_add_manual",
        json!({ "input": { "title": "Windows Game", "executablePath": exe.to_string_lossy(), "sizeBytes": null } }),
    );
    let id = added["game"]["id"].clone();
    let updated: Value = invoke(
        &app,
        "library_update",
        json!({ "gameId": id, "patch": { "launchOptions": {
            "arguments": "", "compatibilityTool": "proton", "env": [], "protonVersion": "GE-Proton10-1" } } }),
    );
    assert_eq!(
        updated["entry"]["launchOptions"]["protonVersion"],
        "GE-Proton10-1"
    );
    let in_use = invoke_raw(
        &app,
        "proton_remove",
        json!({ "versionId": "GE-Proton10-1" }),
    )
    .unwrap_err();
    assert_eq!(in_use["kind"], "invalidInput");
    let bad = invoke_raw(
        &app,
        "proton_install",
        json!({ "versionId": "../GE-Proton" }),
    )
    .unwrap_err();
    assert_eq!(bad["kind"], "invalidInput");
    if cfg!(target_os = "linux") {
        // A build Steam would have installed is not there: say so instead of starting.
        let _: Value = invoke(
            &app,
            "library_update",
            json!({ "gameId": id, "patch": { "launchOptions": {
                "arguments": "", "compatibilityTool": "proton", "env": [], "protonVersion": "steam:Proton 9.0" } } }),
        );
        let missing = invoke_raw(&app, "game_launch", json!({ "gameId": id })).unwrap_err();
        assert_eq!(missing["kind"], "compatToolNotFound");
    }

    let _: Value = invoke(
        &app,
        "library_update",
        json!({ "gameId": id, "patch": { "launchOptions": {
            "arguments": "", "compatibilityTool": "wine", "env": [], "protonVersion": null } } }),
    );
    let removed: Value = invoke(
        &app,
        "proton_remove",
        json!({ "versionId": "GE-Proton10-1" }),
    );
    assert_eq!(removed, Value::Null);
    assert!(!build.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_commands_accept_frontend_payloads() {
    let app = app().await;
    let dev = app.tmp.path().join("dev-plugins/pick");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(
        dev.join("plugin.json"),
        r#"{"id":"pick","name":"Pick","version":"1.0.0","author":"T","entry":"index.js","permissions":["storage","library.add","achievements.define"]}"#,
    )
    .unwrap();
    std::fs::write(dev.join("index.js"), "export function activate() {}").unwrap();

    let list: Vec<Value> = invoke(&app, "plugins_list", json!({}));
    assert_eq!(list[0]["status"], "needsApproval");
    assert_eq!(
        list[0]["permissions"],
        json!(["library.add", "achievements.define", "storage"])
    );
    let denied = invoke_raw(
        &app,
        "plugin_storage_set",
        json!({ "pluginId": "pick", "key": "k", "value": 1 }),
    )
    .unwrap_err();
    assert_eq!(denied["kind"], "plugin");

    let approved: Value = invoke(&app, "plugins_approve", json!({ "id": "pick" }));
    assert_eq!(approved["status"], "active");
    assert!(approved["entryUrl"]
        .as_str()
        .unwrap()
        .ends_with("/index.js"));

    invoke_raw(
        &app,
        "plugin_storage_set",
        json!({ "pluginId": "pick", "key": "last", "value": { "id": "x" } }),
    )
    .unwrap();
    let value: Value = invoke(
        &app,
        "plugin_storage_get",
        json!({ "pluginId": "pick", "key": "last" }),
    );
    assert_eq!(value, json!({ "id": "x" }));
    let keys: Vec<String> = invoke(&app, "plugin_storage_keys", json!({ "pluginId": "pick" }));
    assert_eq!(keys, ["last"]);
    invoke_raw(
        &app,
        "plugin_storage_remove",
        json!({ "pluginId": "pick", "key": "last" }),
    )
    .unwrap();

    let count: usize = invoke(
        &app,
        "plugin_define_achievements",
        json!({ "pluginId": "pick", "achievements": [{ "id": "ten", "title": "Zehn", "description": "", "rule": { "type": "libraryGames", "count": 10 } }] }),
    );
    assert_eq!(count, 1);
    let achievements: Vec<Value> = invoke(&app, "cove_achievements_list", json!({}));
    assert!(achievements.iter().any(|a| a["id"] == "pick:ten"));

    let added: LibraryItem = invoke(
        &app,
        "plugin_library_add",
        json!({ "pluginId": "pick", "game": { "title": "From CSV", "executablePath": null } }),
    );
    assert!(!added.entry.installed);

    let disabled: Value = invoke(
        &app,
        "plugins_set_enabled",
        json!({ "id": "pick", "enabled": false }),
    );
    assert_eq!(disabled["status"], "disabled");
    let preview = invoke_raw(
        &app,
        "plugins_inspect",
        json!({ "path": dev.to_string_lossy() }),
    )
    .unwrap();
    assert_eq!(preview["id"], "pick");
    let installed: Value = invoke(
        &app,
        "plugins_install",
        json!({ "path": dev.to_string_lossy() }),
    );
    assert_eq!(
        (
            installed["status"].as_str(),
            installed["removable"].as_bool()
        ),
        (Some("active"), Some(true))
    );
    invoke_raw(&app, "plugins_uninstall", json!({ "id": "pick" })).unwrap();
}

/// The real connectors sidecar for `store.loginStart`; everything that would
/// need a real store account answers like a successful login.
struct LoginStartOnly(crate::sidecar::Sidecar);

#[async_trait::async_trait]
impl crate::stores::ConnectorRpc for LoginStartOnly {
    async fn call(&self, method: &str, params: Value) -> crate::error::AppResult<Value> {
        if method == "store.loginStart" {
            return self.0.call(method, params).await;
        }
        match method {
            "store.loginComplete" | "store.refresh" => Ok(json!({
                "store": if method == "store.refresh" { &params["session"]["store"] } else { &params["store"] },
                "accountId": "acc1",
                "displayName": "Tiny", "accessToken": "a", "refreshToken": "r",
                "expiresAt": "2099-01-01T00:00:00Z" })),
            "store.library" | "store.detectLocal" => Ok(json!({ "games": [] })),
            _ => Ok(Value::Null),
        }
    }
}

/// Steam, GOG, Epic and itch.io against the real OS keyring (own test service
/// name) and the built connectors sidecar, through the async command path
/// that used to panic inside the Secret Service backend.
/// Run manually: `cargo test real_keyring -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a desktop session with a keyring and the built connectors sidecar"]
async fn real_keyring_store_and_itch_logins() {
    use wiremock::matchers::{header, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let itch = MockServer::start().await;
    Mock::given(path("/profile"))
        .and(header("Authorization", "abc123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "user": { "username": "tiny", "display_name": "Tiny" } })),
        )
        .mount(&itch)
        .await;
    let sidecar = crate::sidecar::Sidecar::with_errors(
        crate::sidecar::resolve_connectors(),
        crate::stores::CONNECTOR_ERRORS,
    );
    assert!(sidecar.is_configured(), "build the connectors first");
    let secrets = Arc::new(crate::catalog::secrets::KeyringStore::testing());
    let app = app_with(Arc::new(LoginStartOnly(sidecar)), secrets, &itch.uri()).await;

    for (store, mode) in [
        ("steam", "browser"),
        ("gog", "webview"),
        ("epic", "webview"),
    ] {
        let start: Value = invoke(&app, "connector_login_start", json!({ "store": store }));
        assert_eq!(start["mode"], mode, "{store}");
        invoke_raw(&app, "connector_login_cancel", json!({ "store": store })).unwrap();

        // What the login window / browser callback hands over: session into the keyring.
        let account: Value = invoke(
            &app,
            "connector_login_complete",
            json!({ "store": store, "code": "abc123" }),
        );
        assert_eq!(account["status"], "connected", "{store}");
        let refreshed: Value = invoke(&app, "connector_refresh", json!({ "store": store }));
        assert_eq!(
            refreshed["accountName"], "Tiny",
            "{store}: session read back"
        );
        let out: Value = invoke(
            &app,
            "connector_logout",
            json!({ "store": store, "keepGames": true }),
        );
        assert_eq!(out["status"], "disconnected", "{store}");
    }

    let status: Value = invoke(&app, "itch_status", json!({}));
    assert_eq!(status["connected"], false);
    let status: Value = invoke(&app, "itch_set_api_key", json!({ "apiKey": "abc123" }));
    assert_eq!(status, json!({ "connected": true, "userName": "Tiny" }));
    let status: Value = invoke(&app, "itch_status", json!({}));
    assert_eq!(status["connected"], true, "key read back from the keyring");
    invoke_raw(&app, "itch_clear_api_key", json!({})).unwrap();
    let status: Value = invoke(&app, "itch_status", json!({}));
    assert_eq!(status["connected"], false);
}
