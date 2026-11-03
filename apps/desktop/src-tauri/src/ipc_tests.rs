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

async fn app() -> TestApp {
    let tmp = tempfile::tempdir().unwrap();
    let pool = crate::db::open_in_memory().await.unwrap();
    let state = AppState {
        launcher: Launcher::new(
            pool.clone(),
            Arc::new(|_| {}),
            Arc::new(|_| Ok(())),
            Timing::default(),
        ),
        host: HostEnv::detect(tmp.path().join("compat")),
        defaults: default_settings(None),
        plugin_dirs: vec![tmp.path().join("plugins")],
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
        url: "tauri://localhost".parse().unwrap(),
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

    let connect = invoke_raw(&app, "sources_connect", json!({ "store": "steam" })).unwrap_err();
    assert_eq!(connect["kind"], "notSupported");
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
        ("sources_list", json!({})),
        ("plugins_list", json!({})),
        ("downloads_list", json!({})),
        ("downloads_reorder", json!({ "orderedIds": [] })),
        ("downloads_clear_history", json!({})),
        ("system_profile", json!({})),
        ("system_about", json!({})),
    ] {
        invoke_raw(&app, cmd, args).unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
    }
}
