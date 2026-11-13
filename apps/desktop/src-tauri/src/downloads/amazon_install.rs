//! The download manager's Amazon part: installing and updating, the update
//! check, and what a start needs (the Amazon Games SDK and its variables).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::watch;

use super::amazon::{self, AmazonMeta};
use super::limiter::RateLimiter;
use super::transfer::{Control, Outcome, Progress};
use super::{epic, extract, DownloadManager};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::launcher::HostEnv;
use crate::models::{CompatibilityTool, LibraryItem, StoreSource};

/// Folder of the Amazon Games SDK files, next to the Proton prefixes.
pub fn sdk_dir(host: &HostEnv) -> PathBuf {
    host.compat_data_root
        .parent()
        .unwrap_or(&host.compat_data_root)
        .join("amazon-sdk")
}

impl DownloadManager {
    /// Downloads the current version of an Amazon game into the game folder.
    pub(super) async fn run_amazon(
        &self,
        record: &db::downloads::Record,
        control: watch::Receiver<Control>,
    ) -> AppResult<Outcome> {
        let (_, _, product) = self.store_game(&record.game_id).await?;
        let download = self.stores()?.amazon_download(&product).await?;
        let manifest = amazon::fetch_manifest(&self.client, &download.download_url).await?;
        let dir = Self::game_dir(record)?;
        tokio::fs::create_dir_all(&dir).await?;
        let old_files = db::store_installs::get(&self.pool, &record.game_id)
            .await?
            .map(|p| p.files)
            .unwrap_or_default();
        let progress = Arc::new(Progress::default());
        let job = amazon::Job {
            client: &self.client,
            limiter: &self.limiter,
            progress: &progress,
            control,
            manifest: &manifest,
            download_url: &download.download_url,
            dir: &dir,
        };
        let todo = job.files_to_write().await?;
        let total = i64::try_from(amazon::Job::total(&todo)).unwrap_or(i64::MAX);
        db::downloads::set_remote(
            &self.pool,
            &record.id,
            total,
            Some(&download.version_id),
            &record.title,
        )
        .await?;
        let outcome = {
            let reporter = self.report_progress(&record.id, total, progress.clone());
            let outcome = tokio::select! {
                outcome = job.run(&todo) => outcome,
                () = reporter => unreachable!("the reporter runs until dropped"),
            };
            self.save_progress(&record.id, &progress).await?;
            outcome?
        };
        match outcome {
            Outcome::Completed => {}
            Outcome::Cancelled => {
                self.remove_unfinished_game(record).await;
                return Ok(outcome);
            }
            Outcome::Paused => return Ok(outcome),
        }
        let files = amazon::installed_files(&manifest);
        epic::remove_old(&dir, &old_files, &files).await;
        self.complete_amazon(record, &download, &manifest, files, &dir)
            .await?;
        Ok(Outcome::Completed)
    }

    async fn complete_amazon(
        &self,
        record: &db::downloads::Record,
        download: &amazon::Download,
        manifest: &amazon::Manifest,
        files: Vec<String>,
        dir: &Path,
    ) -> AppResult<()> {
        let executable = amazon::fuel_start(dir).map(|s| s.command);
        let windows_game = executable
            .as_deref()
            .and_then(Path::extension)
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        let compat = if windows_game && !cfg!(windows) {
            CompatibilityTool::Proton
        } else {
            CompatibilityTool::Native
        };
        let size = {
            let path = dir.to_path_buf();
            tokio::task::spawn_blocking(move || extract::dir_size(&path))
                .await
                .unwrap_or_else(|_| manifest.install_size())
        };
        let meta = AmazonMeta {
            entitlement_id: download.entitlement_id.clone(),
            sku: download.sku.clone(),
        };
        // Last: `complete_install` marks the download as completed.
        db::library::set_compat_if_native(&self.pool, &record.game_id, compat).await?;
        db::store_installs::save(
            &self.pool,
            &db::store_installs::StoreInstall {
                game_id: record.game_id.clone(),
                store: StoreSource::Amazon,
                product_id: download.product_id.clone(),
                build_id: download.version_id.clone(),
                version: download.version_id.clone(),
                platform: "windows".into(),
                files,
                meta: serde_json::to_string(&meta)?,
            },
        )
        .await?;
        db::downloads::complete_install(
            &self.pool,
            &record.id,
            &record.game_id,
            &dir.to_string_lossy(),
            executable
                .as_deref()
                .map(|p| p.to_string_lossy())
                .as_deref(),
            i64::try_from(size).unwrap_or(i64::MAX),
            compat,
        )
        .await?;
        (self.hooks.installed)(record.game_id.clone());
        Ok(())
    }

    /// Sizes for the install dialog.
    pub(super) async fn preview_amazon(&self, product: &str) -> AppResult<(u64, u64)> {
        let download = self.stores()?.amazon_download(product).await?;
        let manifest = amazon::fetch_manifest(&self.client, &download.download_url).await?;
        // Files are downloaded whole, so download and install size are the same.
        let size = manifest.install_size();
        Ok((size, size))
    }

    /// Installed and live version of an Amazon game Cove installed.
    pub(super) async fn amazon_versions(&self, product: &str) -> AppResult<Option<String>> {
        let versions = self
            .stores()?
            .amazon_versions(&[product.to_owned()])
            .await?;
        Ok(versions.get(product).cloned())
    }

    /// The variables an Amazon game needs; the SDK files are fetched once.
    pub async fn amazon_launch_env(
        &self,
        item: &LibraryItem,
        host: &HostEnv,
    ) -> AppResult<Vec<(String, String)>> {
        let install = db::store_installs::get(&self.pool, &item.game.id)
            .await?
            .filter(|i| i.store == StoreSource::Amazon)
            .ok_or_else(|| {
                AppError::Launch(tr!(
                    "Cove hat dieses Spiel nicht installiert.",
                    "Cove didn't install this game."
                ))
            })?;
        let meta: AmazonMeta = serde_json::from_str(&install.meta).unwrap_or_default();
        let sdk = sdk_dir(host);
        // Never keep the game from starting: most games run without the SDK.
        if let Err(error) = self.ensure_amazon_sdk(&sdk).await {
            eprintln!("[cove] Amazon-SDK: {error}");
        }
        let name = db::sources::get(&self.pool, StoreSource::Amazon)
            .await?
            .account_name
            .unwrap_or_default();
        Ok(amazon::launch_env(&sdk, &meta, &name))
    }

    async fn ensure_amazon_sdk(&self, dir: &Path) -> AppResult<()> {
        let marker = dir.join(".cove-version");
        let sdk = self.stores()?.amazon_sdk().await?;
        if tokio::fs::read_to_string(&marker).await.ok().as_deref() == Some(sdk.version_id.as_str())
        {
            return Ok(());
        }
        let mut manifest = amazon::fetch_manifest(&self.client, &sdk.download_url).await?;
        manifest.files.retain(|f| amazon::is_sdk_file(&f.path));
        tokio::fs::create_dir_all(dir).await?;
        let (_tx, control) = watch::channel(Control::Run);
        let progress = Progress::default();
        let limiter = RateLimiter::new(None);
        let job = amazon::Job {
            client: &self.client,
            limiter: &limiter,
            progress: &progress,
            control,
            manifest: &manifest,
            download_url: &sdk.download_url,
            dir,
        };
        let todo = job.files_to_write().await?;
        if job.run(&todo).await? == Outcome::Completed {
            tokio::fs::write(&marker, &sdk.version_id).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::catalog::secrets::{MemoryStore, SecretStore};
    use crate::db::open_in_memory;
    use crate::downloads::itch::ItchApi;
    use crate::downloads::Hooks;
    use crate::models::{DownloadStatus, InstallTarget};
    use crate::stores::StoreService;

    async fn serve(server: &MockServer, version: &str, files: &[(&str, &[u8])]) {
        let manifest = amazon::tests::manifest_proto(files, true);
        Mock::given(method("GET"))
            .and(path(format!("/{version}/manifest.proto")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(manifest))
            .mount(server)
            .await;
        for (_, content) in files {
            let hash = <sha2::Sha256 as sha2::Digest>::digest(content);
            let hex = hash.iter().fold(String::new(), |mut out, b| {
                use std::fmt::Write as _;
                let _ = write!(out, "{b:02x}");
                out
            });
            Mock::given(method("GET"))
                .and(path(format!("/{version}/files/{hex}")))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(content.to_vec()))
                .mount(server)
                .await;
        }
    }

    async fn wait_for(pool: &sqlx::SqlitePool, id: &str, status: DownloadStatus) {
        for _ in 0..200 {
            let item = db::downloads::get(pool, id).await.unwrap();
            if item.download.status == status {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{id} never reached {status:?}");
    }

    #[allow(clippy::too_many_lines)]
    #[tokio::test(flavor = "multi_thread")]
    async fn installs_updates_and_starts_amazon_games() {
        let server = MockServer::start().await;
        let fuel = br#"{ "Main": { "Command": "Game.exe", "Args": ["-fuel"] } }"#;
        serve(
            &server,
            "v1",
            &[
                ("Game.exe", b"MZ one"),
                ("fuel.json", fuel),
                ("data\\old.pak", b"old"),
            ],
        )
        .await;
        serve(
            &server,
            "v2",
            &[("Game.exe", b"MZ two"), ("fuel.json", fuel)],
        )
        .await;
        serve(
            &server,
            "sdk",
            &[
                ("Amazon Games Services/Legacy/FuelSDK_x64.dll", b"fuel"),
                ("Amazon Games.exe", b"app"),
            ],
        )
        .await;

        let pool = open_in_memory().await.unwrap();
        let games = tempfile::tempdir().unwrap();
        let mut defaults = crate::state::default_settings(None);
        defaults.downloads.directory = games.path().to_string_lossy().into_owned();
        defaults.library.game_folders = vec![defaults.downloads.directory.clone()];
        let uri = server.uri();
        let version = std::sync::Arc::new(Mutex::new("v1".to_owned()));
        let live = version.clone();
        let rpc = crate::stores::tests::FakeRpc::new(move |method, params| {
            let version = live.lock().unwrap().clone();
            Ok(match method {
                "amazon.download" => {
                    assert_eq!(params["productId"], "amzn1.adg.product.1");
                    serde_json::json!({ "productId": "amzn1.adg.product.1", "entitlementId": "ent-1",
                        "sku": "SKU1", "versionId": version, "downloadUrl": format!("{uri}/{version}?sig=1") })
                }
                "amazon.versions" => {
                    serde_json::json!({ "versions": { "amzn1.adg.product.1": version } })
                }
                "amazon.sdk" => {
                    serde_json::json!({ "versionId": "s1", "downloadUrl": format!("{uri}/sdk") })
                }
                _ => serde_json::Value::Null,
            })
        });
        let secrets = std::sync::Arc::new(MemoryStore::default());
        secrets
            .set(
                "store-session:amazon",
                r#"{ "store": "amazon", "accountId": "DEV42", "displayName": "Alex", "accessToken": "a1",
                     "refreshToken": "r1", "expiresAt": "2099-01-01T00:00:00Z" }"#,
            )
            .unwrap();
        db::sources::set_connected(&pool, StoreSource::Amazon, "Alex")
            .await
            .unwrap();
        let stores = std::sync::Arc::new(StoreService::new(
            rpc,
            pool.clone(),
            secrets,
            None,
            Duration::ZERO,
        ));
        db::store_games::upsert(
            &pool,
            StoreSource::Amazon,
            &db::store_games::StoreGameInput {
                store_game_id: "amzn1.adg.product.1".into(),
                title: "Tidewater".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let game_id = db::library::list(&pool).await.unwrap()[0].game.id.clone();
        let hooks = Hooks {
            changed: std::sync::Arc::new(|| {}),
            progress: std::sync::Arc::new(|_| {}),
            installed: std::sync::Arc::new(|_| {}),
            open_url: std::sync::Arc::new(|_| Ok(())),
        };
        let itch = ItchApi::new(
            std::sync::Arc::new(MemoryStore::default()),
            "http://127.0.0.1:1",
        );
        let manager = std::sync::Arc::new(DownloadManager::new(
            pool.clone(),
            defaults,
            itch,
            Some(stores),
            hooks,
        ));
        manager.start();

        let preview = manager.preview_store(&game_id).await.unwrap();
        assert_eq!(
            preview.install_bytes,
            i64::try_from(6 + fuel.len() + 3).ok()
        );
        let target = InstallTarget {
            folder: games.path().to_string_lossy().into_owned(),
            make_default: false,
            excluded_dlcs: Vec::new(),
        };
        let id = manager.install_store(&game_id, &target).await.unwrap();
        wait_for(&pool, &id, DownloadStatus::Completed).await;
        let dir = games.path().join("Tidewater");
        assert_eq!(std::fs::read(dir.join("Game.exe")).unwrap(), b"MZ one");
        assert_eq!(std::fs::read(dir.join("data/old.pak")).unwrap(), b"old");
        let item = db::library::require(&pool, &game_id).await.unwrap();
        assert!(item.entry.installed_by_cove);
        assert!(item.entry.executable_path.unwrap().ends_with("Game.exe"));
        assert!(!manager.check_update(&game_id).await.unwrap().available);

        // A new version: only what changed loads, dropped files go.
        *version.lock().unwrap() = "v2".into();
        let update = manager.check_update(&game_id).await.unwrap();
        assert!(update.available);
        assert_eq!(update.latest_version, "v2");
        let id = manager.update_store(&game_id).await.unwrap();
        wait_for(&pool, &id, DownloadStatus::Completed).await;
        assert_eq!(std::fs::read(dir.join("Game.exe")).unwrap(), b"MZ two");
        assert!(!dir.join("data/old.pak").exists());
        assert!(!manager.check_update(&game_id).await.unwrap().available);

        // Start: the SDK files arrive once, the variables name them and the entitlement.
        let host = crate::launcher::HostEnv::detect(games.path().join("compat"));
        let item = db::library::require(&pool, &game_id).await.unwrap();
        let env = manager.amazon_launch_env(&item, &host).await.unwrap();
        let sdk = sdk_dir(&host);
        assert!(sdk
            .join("Amazon Games Services/Legacy/FuelSDK_x64.dll")
            .is_file());
        assert!(!sdk.join("Amazon Games.exe").exists());
        let get = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert_eq!(
            get("AMAZON_GAMES_FUEL_ENTITLEMENT_ID").as_deref(),
            Some("ent-1")
        );
        assert_eq!(
            get("AMAZON_GAMES_FUEL_PRODUCT_SKU").as_deref(),
            Some("SKU1")
        );
        assert_eq!(
            get("AMAZON_GAMES_FUEL_DISPLAY_NAME").as_deref(),
            Some("Alex")
        );
        assert_eq!(
            get("FUEL_DIR"),
            Some(
                sdk.join("Amazon Games Services")
                    .join("Legacy")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }
}
