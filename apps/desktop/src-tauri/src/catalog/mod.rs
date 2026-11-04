//! Catalogue: real game data from IGDB or RAWG behind the [`CatalogProvider`]
//! trait, with a SQLite response cache (TTL 7 days), offline fallback to stale
//! cache entries, a local image cache and fuzzy matching of library games.

pub mod igdb;
pub mod images;
pub mod matching;
pub mod provider;
pub mod proxy;
pub mod rate_limit;
pub mod rawg;
pub mod secrets;

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use self::images::ImageCache;
use self::matching::{decide, MatchDecision};
use self::provider::{BrowseQuery, CatalogProvider, ProviderError, ProviderPage};
use self::secrets::SecretStore;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    AppSettings, CatalogCredentials, CatalogFilter, CatalogGameDetails, CatalogPage,
    CatalogProviderId, CatalogSort, CatalogSource, CatalogStatus, CoveServiceStatus, Game,
    LibraryItem, MatchStatus,
};

pub const CACHE_TTL_DAYS: i64 = 7;
/// Bump when cached `Game` data changes shape (v2: recommender features).
pub const CACHE_VERSION: u32 = 2;
/// Entries older than this are deleted on startup (still usable offline until then).
pub const CACHE_MAX_AGE_DAYS: i64 = 60;
pub const PAGE_SIZE: u32 = 24;
const SEARCH_LIMIT: u32 = 20;

/// Base URLs, overridable in tests.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub igdb_api: String,
    pub igdb_auth: String,
    pub rawg_api: String,
    /// Cove catalogue proxy; None when the build has no proxy URL.
    pub cove_proxy: Option<String>,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            igdb_api: igdb::API_BASE.into(),
            igdb_auth: igdb::AUTH_URL.into(),
            rawg_api: rawg::API_BASE.into(),
            cove_proxy: proxy::build_time_url(),
        }
    }
}

pub struct CatalogService {
    pool: SqlitePool,
    secrets: Arc<dyn SecretStore>,
    client: reqwest::Client,
    endpoints: Endpoints,
    defaults: AppSettings,
    providers: RwLock<HashMap<CatalogProviderId, Arc<dyn CatalogProvider>>>,
    proxy: Option<Arc<dyn CatalogProvider>>,
    online: AtomicBool,
    pub images: ImageCache,
}

/// Result of a cached fetch: the value and whether it is stale data served while offline.
struct Fetched<T> {
    value: T,
    offline: bool,
}

impl CatalogService {
    pub fn new(
        pool: SqlitePool,
        secrets: Arc<dyn SecretStore>,
        image_dir: std::path::PathBuf,
        defaults: AppSettings,
        endpoints: Endpoints,
    ) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "Cove/",
                env!("CARGO_PKG_VERSION"),
                " (desktop game launcher)"
            ))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        let proxy = endpoints
            .cove_proxy
            .clone()
            .map(|url| -> Arc<dyn CatalogProvider> {
                Arc::new(proxy::CoveProxyProvider::new(client.clone(), url))
            });
        Self {
            images: ImageCache::new(image_dir, client.clone()),
            proxy,
            pool,
            secrets,
            client,
            endpoints,
            defaults,
            providers: RwLock::new(HashMap::new()),
            online: AtomicBool::new(true),
        }
    }

    async fn secret_call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn SecretStore) -> AppResult<T> + Send + 'static,
    ) -> AppResult<T> {
        // Keyring backends block (D-Bus, Keychain); keep them off the async workers.
        let store = self.secrets.clone();
        tokio::task::spawn_blocking(move || f(store.as_ref()))
            .await
            .map_err(|e| AppError::Keyring(e.to_string()))?
    }

    fn build(&self, credentials: CatalogCredentials) -> Arc<dyn CatalogProvider> {
        match credentials {
            CatalogCredentials::Igdb {
                client_id,
                client_secret,
            } => Arc::new(igdb::IgdbProvider::with_endpoints(
                self.client.clone(),
                client_id,
                client_secret,
                self.endpoints.igdb_api.clone(),
                self.endpoints.igdb_auth.clone(),
            )),
            CatalogCredentials::Rawg { api_key } => Arc::new(rawg::RawgProvider::with_base(
                self.client.clone(),
                api_key,
                self.endpoints.rawg_api.clone(),
            )),
        }
    }

    /// The provider for `id`, built from the keyring on first use. None if not configured.
    async fn provider_for(
        &self,
        id: CatalogProviderId,
    ) -> AppResult<Option<Arc<dyn CatalogProvider>>> {
        if let Some(p) = self.providers.read().await.get(&id) {
            return Ok(Some(p.clone()));
        }
        let Some(credentials) = self.secret_call(move |s| secrets::load(s, id)).await? else {
            return Ok(None);
        };
        let provider = self.build(credentials);
        self.providers.write().await.insert(id, provider.clone());
        Ok(Some(provider))
    }

    /// The Cove proxy is used when this build has a proxy URL and the user did not switch it off.
    async fn cove_enabled(&self) -> AppResult<bool> {
        Ok(db::settings::load(&self.pool, &self.defaults)
            .await?
            .catalog
            .use_cove_service)
    }

    /// IGDB data sources in fallback order: Cove proxy → own IGDB keys.
    async fn igdb_sources(&self) -> AppResult<Vec<(CatalogSource, Arc<dyn CatalogProvider>)>> {
        let mut sources: Vec<(CatalogSource, Arc<dyn CatalogProvider>)> = Vec::new();
        if let Some(proxy) = &self.proxy {
            if self.cove_enabled().await? {
                sources.push((CatalogSource::Cove, proxy.clone()));
            }
        }
        if let Some(own) = self.provider_for(CatalogProviderId::Igdb).await? {
            sources.push((CatalogSource::Igdb, own));
        }
        Ok(sources)
    }

    async fn igdb_chain(&self) -> AppResult<Option<Arc<dyn CatalogProvider>>> {
        let mut sources = self.igdb_sources().await?;
        Ok(match sources.len() {
            0 => None,
            1 => sources.pop().map(|(_, p)| p),
            _ => Some(Arc::new(proxy::FallbackProvider::new(
                sources.into_iter().map(|(_, p)| p).collect(),
            ))),
        })
    }

    /// Cove proxy / own IGDB keys (with fallback), else own RAWG key.
    async fn active(&self) -> AppResult<Arc<dyn CatalogProvider>> {
        if let Some(chain) = self.igdb_chain().await? {
            return Ok(chain);
        }
        self.provider_for(CatalogProviderId::Rawg)
            .await?
            .ok_or(AppError::CatalogNotConfigured)
    }

    pub async fn status(&self) -> AppResult<CatalogStatus> {
        let configured = self.secret_call(|s| secrets::configured(s)).await?;
        let mut sources: Vec<CatalogSource> = self
            .igdb_sources()
            .await?
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        if configured.rawg {
            sources.push(CatalogSource::Rawg);
        }
        Ok(CatalogStatus {
            sources,
            cove_service: CoveServiceStatus {
                available: self.proxy.is_some(),
                enabled: self.cove_enabled().await?,
            },
            configured,
            online: self.online.load(Ordering::Relaxed),
        })
    }

    /// Verifies the credentials with the provider, then stores them in the keyring.
    pub async fn set_credentials(
        &self,
        credentials: CatalogCredentials,
    ) -> AppResult<CatalogStatus> {
        secrets::validate(&credentials)?;
        let id = credentials.provider();
        let provider = self.build(credentials.clone());
        provider.verify().await.map_err(|e| self.map_error(e))?;
        self.secret_call(move |s| secrets::save(s, &credentials))
            .await?;
        self.providers.write().await.insert(id, provider);
        self.status().await
    }

    pub async fn clear_credentials(&self, id: CatalogProviderId) -> AppResult<CatalogStatus> {
        self.secret_call(move |s| secrets::clear(s, id)).await?;
        self.providers.write().await.remove(&id);
        self.status().await
    }

    fn map_error(&self, error: ProviderError) -> AppError {
        match error {
            ProviderError::Network(msg) => {
                self.online.store(false, Ordering::Relaxed);
                AppError::Offline(msg)
            }
            ProviderError::Auth(msg) => AppError::CatalogAuth(msg),
            other => AppError::CatalogProvider(other.to_string()),
        }
    }

    /// Fresh cache hit → cached value. Otherwise ask the provider; if that fails,
    /// fall back to any cached value (marked offline when the network was the problem).
    async fn cached<T, F, Fut>(
        &self,
        provider: CatalogProviderId,
        key: &str,
        fetch: F,
    ) -> AppResult<Fetched<T>>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, ProviderError>>,
    {
        // Versioned keys: entries written before a format change are simply not found.
        let versioned = format!("v{CACHE_VERSION}:{key}");
        let key = versioned.as_str();
        let cached = db::catalog_cache::get(&self.pool, key).await?;
        let stale = match cached {
            Some(c) => {
                let fresh =
                    chrono::Utc::now() - c.fetched_at < chrono::Duration::days(CACHE_TTL_DAYS);
                match serde_json::from_str::<T>(&c.value) {
                    Ok(value) if fresh => {
                        return Ok(Fetched {
                            value,
                            offline: false,
                        })
                    }
                    Ok(value) => Some(value),
                    Err(_) => None, // unreadable after a format change: refetch
                }
            }
            None => None,
        };
        match fetch().await {
            Ok(value) => {
                self.online.store(true, Ordering::Relaxed);
                db::catalog_cache::put(
                    &self.pool,
                    provider.as_str(),
                    key,
                    &serde_json::to_string(&value)?,
                )
                .await?;
                Ok(Fetched {
                    value,
                    offline: false,
                })
            }
            Err(error) => {
                let offline = matches!(error, ProviderError::Network(_));
                match stale {
                    Some(value) => {
                        if offline {
                            self.online.store(false, Ordering::Relaxed);
                        }
                        Ok(Fetched { value, offline })
                    }
                    None => Err(self.map_error(error)),
                }
            }
        }
    }

    async fn library_ids(&self, games: &[Game]) -> AppResult<HashMap<String, String>> {
        let mut map = HashMap::new();
        for g in games {
            if let Some(item) = db::library::get(&self.pool, &g.id).await? {
                map.insert(g.id.clone(), item.game.id);
            }
        }
        Ok(map)
    }

    pub async fn browse(
        &self,
        filter: CatalogFilter,
        sort: CatalogSort,
        page: u32,
    ) -> AppResult<CatalogPage> {
        let provider = self.active().await?;
        let query = BrowseQuery {
            filter,
            sort,
            page,
            page_size: PAGE_SIZE,
        };
        let key = format!(
            "{}:browse:{}",
            provider.id().as_str(),
            serde_json::to_string(&(&query.filter, query.sort, page))?
        );
        let fetched: Fetched<ProviderPage> = self
            .cached(provider.id(), &key, || provider.browse(&query))
            .await?;
        Ok(CatalogPage {
            reasons: std::collections::BTreeMap::new(),
            items: fetched.value.items,
            page,
            has_more: fetched.value.has_more,
            total: fetched.value.total,
            offline: fetched.offline,
        })
    }

    pub async fn search(&self, text: &str) -> AppResult<CatalogPage> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(CatalogPage {
                reasons: std::collections::BTreeMap::new(),
                items: Vec::new(),
                page: 0,
                has_more: false,
                total: Some(0),
                offline: false,
            });
        }
        let provider = self.active().await?;
        let key = format!("{}:search:{}", provider.id().as_str(), text.to_lowercase());
        let fetched: Fetched<Vec<Game>> = self
            .cached(provider.id(), &key, || provider.search(text, SEARCH_LIMIT))
            .await?;
        Ok(CatalogPage {
            reasons: std::collections::BTreeMap::new(),
            items: fetched.value,
            page: 0,
            has_more: false,
            total: None,
            offline: fetched.offline,
        })
    }

    /// Provider + native id for a namespaced catalogue id (`igdb-1942`).
    async fn provider_of(&self, catalog_id: &str) -> AppResult<(Arc<dyn CatalogProvider>, String)> {
        let (id, native) = CatalogProviderId::parse_catalog_id(catalog_id).ok_or_else(|| {
            AppError::InvalidInput(format!("Unbekannte Katalog-ID: {catalog_id}"))
        })?;
        let provider = match id {
            CatalogProviderId::Igdb => self.igdb_chain().await?,
            CatalogProviderId::Rawg => self.provider_for(id).await?,
        }
        .ok_or(AppError::CatalogNotConfigured)?;
        Ok((provider, native.to_owned()))
    }

    async fn fetch_details(&self, catalog_id: &str) -> AppResult<Fetched<Game>> {
        let (provider, native) = self.provider_of(catalog_id).await?;
        let key = format!("{catalog_id}:details");
        self.cached(provider.id(), &key, || provider.details(&native))
            .await
    }

    pub async fn details(&self, catalog_id: &str) -> AppResult<CatalogGameDetails> {
        let fetched = self.fetch_details(catalog_id).await?;
        let library_game_id = self
            .library_ids(std::slice::from_ref(&fetched.value))
            .await?
            .remove(catalog_id);
        Ok(CatalogGameDetails {
            game: fetched.value,
            library_game_id,
            offline: fetched.offline,
        })
    }

    pub async fn similar(&self, catalog_id: &str) -> AppResult<CatalogPage> {
        let (provider, native) = self.provider_of(catalog_id).await?;
        let key = format!("{catalog_id}:similar");
        let fetched: Fetched<Vec<Game>> = self
            .cached(provider.id(), &key, || provider.similar(&native))
            .await?;
        Ok(CatalogPage {
            reasons: std::collections::BTreeMap::new(),
            items: fetched.value,
            page: 0,
            has_more: false,
            total: None,
            offline: fetched.offline,
        })
    }

    /// Image URLs of a game (for offline prefetching).
    pub fn image_urls(game: &Game) -> Vec<String> {
        std::iter::once(&game.artwork.cover)
            .chain(std::iter::once(&game.artwork.header))
            .chain(game.artwork.screenshots.iter())
            .filter_map(|i| i.url.clone())
            .collect()
    }

    /// Matches a library game by title. Confident → metadata applied; unsure →
    /// candidates stored for the user; catalogue unavailable → status unchanged.
    pub async fn match_library_game(&self, game_id: &str) -> AppResult<MatchStatus> {
        let item = db::library::require(&self.pool, game_id).await?;
        if item.entry.match_status == MatchStatus::Matched {
            return Ok(MatchStatus::Matched);
        }
        let title = item.game.title.clone();
        let provider = match self.active().await {
            Ok(p) => p,
            Err(AppError::CatalogNotConfigured) => return Ok(item.entry.match_status),
            Err(e) => return Err(e),
        };
        let key = format!(
            "{}:match:{}",
            provider.id().as_str(),
            matching::normalize(&title)
        );
        let results: Fetched<Vec<Game>> = match self
            .cached(provider.id(), &key, || provider.match_candidates(&title))
            .await
        {
            Ok(r) => r,
            Err(AppError::Offline(_)) => return Ok(item.entry.match_status),
            Err(e) => return Err(e),
        };
        match decide(&title, results.value) {
            MatchDecision::Auto(candidate) => {
                self.link(game_id, &candidate.game.id).await?;
                Ok(MatchStatus::Matched)
            }
            MatchDecision::Ask(candidates) => {
                db::library::store_candidates(&self.pool, game_id, &candidates).await?;
                Ok(MatchStatus::Pending)
            }
            MatchDecision::NoMatch => {
                db::library::set_match_status(&self.pool, game_id, MatchStatus::None).await?;
                Ok(MatchStatus::None)
            }
        }
    }

    /// Links a library game to a catalogue entry (full details are fetched first).
    pub async fn link(&self, game_id: &str, catalog_id: &str) -> AppResult<LibraryItem> {
        let details = self.fetch_details(catalog_id).await?.value;
        let item = db::library::apply_catalog(&self.pool, game_id, &details).await?;
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::secrets::MemoryStore;
    use super::*;
    use crate::db::library::{add_manual, require};
    use crate::models::ManualGameInput;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn rawg_game(id: u64, name: &str) -> serde_json::Value {
        json!({ "id": id, "name": name, "released": "2020-01-01", "background_image": "https://media.rawg.io/x.jpg" })
    }

    async fn service(server: &MockServer) -> (CatalogService, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let pool = crate::db::open_in_memory().await.unwrap();
        let svc = CatalogService::new(
            pool,
            Arc::new(MemoryStore::default()),
            tmp.path().join("images"),
            crate::state::default_settings(None),
            Endpoints {
                igdb_api: format!("{}/v4", server.uri()),
                igdb_auth: format!("{}/oauth2/token", server.uri()),
                rawg_api: format!("{}/api", server.uri()),
                cove_proxy: None,
            },
        );
        (svc, tmp)
    }

    async fn mount_rawg(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/api/games"))
            .and(query_param("key", "k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "count": 2, "next": null, "results": [rawg_game(1, "Tidewater"), rawg_game(2, "Tidewater II")]
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/games/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 1, "name": "Tidewater", "description_raw": "A flooded village.",
                "developers": [{ "name": "Harbor & Pine" }], "background_image": "https://media.rawg.io/t.jpg"
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/games/1/screenshots"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn not_configured_until_credentials_are_saved() {
        let server = MockServer::start().await;
        mount_rawg(&server).await;
        let (svc, _tmp) = service(&server).await;
        assert!(matches!(
            svc.browse(CatalogFilter::default(), CatalogSort::Popular, 0)
                .await,
            Err(AppError::CatalogNotConfigured)
        ));

        let status = svc
            .set_credentials(CatalogCredentials::Rawg {
                api_key: "k".into(),
            })
            .await
            .unwrap();
        assert!(status.configured.rawg && !status.configured.igdb);
        assert_eq!(status.sources, [CatalogSource::Rawg]);
        assert!(
            !status.cove_service.available,
            "test build has no proxy URL"
        );

        let page = svc
            .browse(CatalogFilter::default(), CatalogSort::Popular, 0)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(!page.offline);

        assert!(matches!(
            svc.set_credentials(CatalogCredentials::Rawg {
                api_key: " ".into()
            })
            .await,
            Err(AppError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn serves_cache_within_ttl_and_stale_cache_when_offline() {
        let server = MockServer::start().await;
        mount_rawg(&server).await;
        let (svc, tmp) = service(&server).await;
        svc.set_credentials(CatalogCredentials::Rawg {
            api_key: "k".into(),
        })
        .await
        .unwrap();

        let first = svc.details("rawg-1").await.unwrap();
        assert_eq!(first.game.developer, "Harbor & Pine");
        let requests = server.received_requests().await.unwrap().len();
        svc.details("rawg-1").await.unwrap();
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            requests,
            "second call served from cache"
        );

        // Expire the entry, then ask through a provider that cannot be reached.
        crate::db::catalog_cache::set_fetched_at(
            &svc.pool,
            "v2:rawg-1:details",
            chrono::Utc::now() - chrono::Duration::days(8),
        )
        .await;
        let store = Arc::new(MemoryStore::default());
        secrets::save(
            store.as_ref(),
            &CatalogCredentials::Rawg {
                api_key: "k".into(),
            },
        )
        .unwrap();
        let offline_svc = CatalogService::new(
            svc.pool.clone(),
            store,
            tmp.path().join("images"),
            crate::state::default_settings(None),
            Endpoints {
                rawg_api: "http://127.0.0.1:9/api".into(),
                cove_proxy: None,
                ..Endpoints::default()
            },
        );
        let offline = offline_svc.details("rawg-1").await.unwrap();
        assert!(offline.offline);
        assert_eq!(offline.game.title, "Tidewater");
        assert!(!offline_svc.status().await.unwrap().online);

        // Nothing cached + offline → error.
        assert!(matches!(
            offline_svc.details("rawg-99").await,
            Err(AppError::Offline(_))
        ));
    }

    #[tokio::test]
    async fn matches_library_games_automatically_or_asks() {
        let server = MockServer::start().await;
        mount_rawg(&server).await;
        let (svc, _tmp) = service(&server).await;

        let input = |title: &str, exe: &str| ManualGameInput {
            title: title.into(),
            executable_path: exe.into(),
            size_bytes: None,
        };
        let exact = add_manual(&svc.pool, &input("tidewater", "/g/a"))
            .await
            .unwrap()
            .game
            .id;

        // Not configured: nothing happens.
        assert_eq!(
            svc.match_library_game(&exact).await.unwrap(),
            MatchStatus::None
        );

        svc.set_credentials(CatalogCredentials::Rawg {
            api_key: "k".into(),
        })
        .await
        .unwrap();
        assert_eq!(
            svc.match_library_game(&exact).await.unwrap(),
            MatchStatus::Matched
        );
        let linked = require(&svc.pool, &exact).await.unwrap();
        assert_eq!(linked.game.title, "Tidewater");
        assert_eq!(linked.game.developer, "Harbor & Pine");
        assert_eq!(linked.entry.catalog_id.as_deref(), Some("rawg-1"));

        let vague = add_manual(&svc.pool, &input("Tide", "/g/b"))
            .await
            .unwrap()
            .game
            .id;
        assert_eq!(
            svc.match_library_game(&vague).await.unwrap(),
            MatchStatus::Pending
        );
        let candidates = db::library::candidates(&svc.pool, &vague).await.unwrap();
        assert_eq!(candidates.len(), 2);

        let item = svc.link(&vague, "rawg-1").await.unwrap();
        assert_eq!(item.entry.match_status, MatchStatus::Matched);
    }
}

#[cfg(test)]
mod source_order_tests {
    use super::secrets::MemoryStore;
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn proxy_game(id: u64, title: &str) -> serde_json::Value {
        json!({
            "id": format!("igdb-{id}"), "title": title, "summary": "", "description": "", "genres": [],
            "platforms": [], "releaseDate": null, "developer": "", "publisher": "", "rating": null, "popularity": 0,
            "artwork": { "cover": { "url": null, "alt": "" }, "header": { "url": null, "alt": "" }, "screenshots": [] }
        })
    }

    async fn service(
        proxy: Option<String>,
        igdb: &MockServer,
    ) -> (CatalogService, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let svc = CatalogService::new(
            crate::db::open_in_memory().await.unwrap(),
            Arc::new(MemoryStore::default()),
            tmp.path().join("img"),
            crate::state::default_settings(None),
            Endpoints {
                igdb_api: format!("{}/v4", igdb.uri()),
                igdb_auth: format!("{}/oauth2/token", igdb.uri()),
                rawg_api: "http://127.0.0.1:9/api".into(),
                cove_proxy: proxy,
            },
        );
        (svc, tmp)
    }

    async fn mount_igdb(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "access_token": "t", "expires_in": 3600 })),
            )
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!([{ "id": 7, "name": "Via own keys" }])),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn cove_proxy_is_ready_to_use_without_keys() {
        let proxy = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/game/1942"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "game": proxy_game(1942, "Via proxy") })),
            )
            .mount(&proxy)
            .await;
        let igdb = MockServer::start().await;
        let (svc, _tmp) = service(Some(proxy.uri()), &igdb).await;

        let status = svc.status().await.unwrap();
        assert_eq!(status.sources, [CatalogSource::Cove]);
        assert_eq!(
            status.cove_service,
            CoveServiceStatus {
                available: true,
                enabled: true
            }
        );
        assert_eq!(
            svc.details("igdb-1942").await.unwrap().game.title,
            "Via proxy"
        );

        // Switched off in Settings → no source left.
        let mut settings = db::settings::load(&svc.pool, &svc.defaults).await.unwrap();
        settings.catalog.use_cove_service = false;
        db::settings::save(&svc.pool, &settings).await.unwrap();
        assert_eq!(
            svc.status().await.unwrap().sources,
            Vec::<CatalogSource>::new()
        );
        assert!(matches!(
            svc.details("igdb-1").await,
            Err(AppError::CatalogNotConfigured)
        ));
    }

    #[tokio::test]
    async fn order_is_proxy_then_own_keys_then_cache() {
        let igdb = MockServer::start().await;
        mount_igdb(&igdb).await;
        // Proxy configured but unreachable.
        let (svc, _tmp) = service(Some("http://127.0.0.1:9".into()), &igdb).await;
        svc.set_credentials(CatalogCredentials::Igdb {
            client_id: "id".into(),
            client_secret: "s".into(),
        })
        .await
        .unwrap();
        assert_eq!(
            svc.status().await.unwrap().sources,
            [CatalogSource::Cove, CatalogSource::Igdb]
        );

        let first = svc.details("igdb-7").await.unwrap();
        assert_eq!(first.game.title, "Via own keys");
        assert!(!first.offline);

        // Everything unreachable → stale cache with offline flag.
        crate::db::catalog_cache::set_fetched_at(
            &svc.pool,
            "v2:igdb-7:details",
            chrono::Utc::now() - chrono::Duration::days(8),
        )
        .await;
        drop(igdb);
        let (offline_svc, _tmp2) = {
            let tmp = tempfile::tempdir().unwrap();
            let store = Arc::new(MemoryStore::default());
            secrets::save(
                store.as_ref(),
                &CatalogCredentials::Igdb {
                    client_id: "id".into(),
                    client_secret: "s".into(),
                },
            )
            .unwrap();
            let svc2 = CatalogService::new(
                svc.pool.clone(),
                store,
                tmp.path().join("img"),
                crate::state::default_settings(None),
                Endpoints {
                    igdb_api: "http://127.0.0.1:9/v4".into(),
                    igdb_auth: "http://127.0.0.1:9/token".into(),
                    rawg_api: "http://127.0.0.1:9/api".into(),
                    cove_proxy: Some("http://127.0.0.1:9".into()),
                },
            );
            (svc2, tmp)
        };
        let cached = offline_svc.details("igdb-7").await.unwrap();
        assert!(cached.offline);
        assert_eq!(cached.game.title, "Via own keys");
    }
}
