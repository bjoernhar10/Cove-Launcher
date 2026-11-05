//! `CoveProxyProvider`: the default catalogue source. Talks to the Cove
//! catalogue proxy (services/catalog-proxy), which holds the IGDB credentials,
//! so users need no API keys. Same ids (`igdb-…`) and data as direct IGDB.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::provider::{status_error, BrowseQuery, CatalogProvider, ProviderError, ProviderPage};
use crate::models::{CatalogProviderId, CatalogSort, Game, MatchCandidate, Platform};

/// Proxy URL baked in at build time (see `build.rs`); None when not configured.
pub fn build_time_url() -> Option<String> {
    let url = env!("COVE_CATALOG_PROXY_URL");
    (!url.is_empty()).then(|| url.to_owned())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageBody {
    items: Vec<Game>,
    has_more: bool,
    total: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GameBody {
    game: Game,
}

#[derive(Debug, Deserialize)]
struct MatchBody {
    candidates: Vec<MatchCandidate>,
}

pub struct CoveProxyProvider {
    client: reqwest::Client,
    base: String,
}

fn sort_param(sort: CatalogSort) -> &'static str {
    match sort {
        CatalogSort::Recommended => "recommended",
        CatalogSort::Popular => "popular",
        CatalogSort::Newest => "newest",
        CatalogSort::Title => "title",
    }
}

/// Query parameters for `GET /v1/browse`.
pub fn browse_params(query: &BrowseQuery) -> Vec<(&'static str, String)> {
    let f = &query.filter;
    let mut params = vec![
        ("sort", sort_param(query.sort).to_owned()),
        ("page", query.page.to_string()),
    ];
    if !f.query.trim().is_empty() {
        params.push(("q", f.query.trim().to_owned()));
    }
    if !f.genres.is_empty() {
        params.push(("genre", f.genres.join(",")));
    }
    if !f.platforms.is_empty() {
        let names: Vec<&str> = f
            .platforms
            .iter()
            .map(|p| match p {
                Platform::Windows => "windows",
                Platform::Linux => "linux",
                Platform::Macos => "macos",
            })
            .collect();
        params.push(("platform", names.join(",")));
    }
    if let Some(years) = f.release_years {
        params.push(("year", format!("{}-{}", years.from, years.to)));
    }
    if let Some(min) = f.min_rating {
        params.push(("rating", min.to_string()));
    }
    params
}

impl CoveProxyProvider {
    pub fn new(client: reqwest::Client, base: String) -> Self {
        Self {
            client,
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    async fn read<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ProviderError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response.json().await?);
        }
        let body = response.text().await.unwrap_or_default();
        Err(status_error(status, &body))
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, ProviderError> {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .query(params)
            .send()
            .await?;
        Self::read(response).await
    }

    fn numeric(native_id: &str) -> Result<&str, ProviderError> {
        if native_id.is_empty() || !native_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(ProviderError::NotFound);
        }
        Ok(native_id)
    }
}

#[async_trait]
impl CatalogProvider for CoveProxyProvider {
    fn id(&self) -> CatalogProviderId {
        CatalogProviderId::Igdb
    }

    async fn verify(&self) -> Result<(), ProviderError> {
        let _: serde_json::Value = self.get("/v1/health", &[]).await?;
        Ok(())
    }

    async fn search(&self, query: &str, _limit: u32) -> Result<Vec<Game>, ProviderError> {
        let page: PageBody = self
            .get("/v1/search", &[("q", query.trim().to_owned())])
            .await?;
        Ok(page.items)
    }

    async fn browse(&self, query: &BrowseQuery) -> Result<ProviderPage, ProviderError> {
        let page: PageBody = self.get("/v1/browse", &browse_params(query)).await?;
        Ok(ProviderPage {
            items: page.items,
            total: page.total,
            has_more: page.has_more,
        })
    }

    async fn details(&self, native_id: &str) -> Result<Game, ProviderError> {
        let body: GameBody = self
            .get(&format!("/v1/game/{}", Self::numeric(native_id)?), &[])
            .await?;
        Ok(body.game)
    }

    async fn similar(&self, native_id: &str) -> Result<Vec<Game>, ProviderError> {
        let page: PageBody = self
            .get(&format!("/v1/similar/{}", Self::numeric(native_id)?), &[])
            .await?;
        Ok(page.items)
    }

    /// The proxy ranks candidates itself (`POST /v1/match`).
    async fn match_candidates(&self, title: &str) -> Result<Vec<Game>, ProviderError> {
        let response = self
            .client
            .post(format!("{}/v1/match", self.base))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await?;
        let body: MatchBody = Self::read(response).await?;
        Ok(body.candidates.into_iter().map(|c| c.game).collect())
    }
}

/// Tries providers in order (Cove proxy → own IGDB keys). A definite "not found"
/// ends the search; any other error moves on to the next provider. If all fail,
/// a network error wins so the service can fall back to cached data.
pub struct FallbackProvider {
    providers: Vec<std::sync::Arc<dyn CatalogProvider>>,
}

impl FallbackProvider {
    pub fn new(providers: Vec<std::sync::Arc<dyn CatalogProvider>>) -> Self {
        Self { providers }
    }

    async fn first_ok<T, F, Fut>(&self, call: F) -> Result<T, ProviderError>
    where
        F: Fn(std::sync::Arc<dyn CatalogProvider>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ProviderError>>,
    {
        let mut network: Option<ProviderError> = None;
        let mut last: Option<ProviderError> = None;
        for provider in &self.providers {
            match call(provider.clone()).await {
                Ok(value) => return Ok(value),
                Err(ProviderError::NotFound) => return Err(ProviderError::NotFound),
                Err(e @ ProviderError::Network(_)) => network = Some(e),
                Err(e) => last = Some(e),
            }
        }
        Err(network.or(last).unwrap_or_else(|| {
            ProviderError::Network(tr!(
                "Keine Katalogquelle verfügbar",
                "No catalogue source available"
            ))
        }))
    }
}

#[async_trait]
impl CatalogProvider for FallbackProvider {
    fn id(&self) -> CatalogProviderId {
        CatalogProviderId::Igdb
    }

    async fn verify(&self) -> Result<(), ProviderError> {
        self.first_ok(|p| async move { p.verify().await }).await
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Game>, ProviderError> {
        self.first_ok(|p| async move { p.search(query, limit).await })
            .await
    }

    async fn browse(&self, query: &BrowseQuery) -> Result<ProviderPage, ProviderError> {
        self.first_ok(|p| async move { p.browse(query).await })
            .await
    }

    async fn details(&self, native_id: &str) -> Result<Game, ProviderError> {
        self.first_ok(|p| async move { p.details(native_id).await })
            .await
    }

    async fn similar(&self, native_id: &str) -> Result<Vec<Game>, ProviderError> {
        self.first_ok(|p| async move { p.similar(native_id).await })
            .await
    }

    async fn match_candidates(&self, title: &str) -> Result<Vec<Game>, ProviderError> {
        self.first_ok(|p| async move { p.match_candidates(title).await })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::igdb::IgdbProvider;
    use crate::models::{CatalogFilter, YearRange};
    use serde_json::json;
    use std::sync::Arc;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn game_json(id: u64, title: &str) -> serde_json::Value {
        json!({
            "id": format!("igdb-{id}"), "title": title, "summary": "", "description": "", "genres": ["rpg"],
            "platforms": ["linux"], "releaseDate": null, "developer": "", "publisher": "", "rating": null,
            "popularity": 0,
            "artwork": {
                "cover": { "url": "https://images.igdb.com/igdb/image/upload/t_cover_big/x.jpg", "alt": "" },
                "header": { "url": null, "alt": "" }, "screenshots": []
            }
        })
    }

    #[test]
    fn browse_params_match_the_proxy_api() {
        let q = BrowseQuery {
            filter: CatalogFilter {
                query: " witcher ".into(),
                genres: vec!["rpg".into(), "horror".into()],
                platforms: vec![Platform::Linux, Platform::Windows],
                release_years: Some(YearRange {
                    from: 2018,
                    to: 2021,
                }),
                min_rating: Some(80),
            },
            sort: CatalogSort::Popular,
            page: 3,
            page_size: 24,
        };
        let params = browse_params(&q);
        let get = |k: &str| {
            params
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("sort"), Some("popular"));
        assert_eq!(get("page"), Some("3"));
        assert_eq!(get("q"), Some("witcher"));
        assert_eq!(get("genre"), Some("rpg,horror"));
        assert_eq!(get("platform"), Some("linux,windows"));
        assert_eq!(get("year"), Some("2018-2021"));
        assert_eq!(get("rating"), Some("80"));
    }

    #[tokio::test]
    async fn talks_to_the_fixed_proxy_endpoints() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/browse"))
            .and(query_param("sort", "newest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "items": [game_json(1, "One")], "page": 0, "hasMore": true, "total": 40 }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/game/1942"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "game": game_json(1942, "Witcher") })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/game/7"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({ "error": { "code": "not_found", "message": "x" } })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/match"))
            .and(body_json(json!({ "title": "witcher" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "candidates": [{ "game": game_json(1942, "Witcher"), "score": 0.9 }] }),
            ))
            .mount(&server)
            .await;

        let proxy = CoveProxyProvider::new(reqwest::Client::new(), format!("{}/", server.uri()));
        let page = proxy
            .browse(&BrowseQuery {
                filter: CatalogFilter::default(),
                sort: CatalogSort::Newest,
                page: 0,
                page_size: 24,
            })
            .await
            .unwrap();
        assert_eq!(
            (page.items.len(), page.total, page.has_more),
            (1, Some(40), true)
        );
        let game = proxy.details("1942").await.unwrap();
        assert_eq!(game.title, "Witcher");
        assert_eq!(
            game.artwork.cover.url.as_deref(),
            Some("https://images.igdb.com/igdb/image/upload/t_cover_big/x.jpg"),
            "image URLs come straight from IGDB"
        );
        assert!(matches!(
            proxy.details("7").await,
            Err(ProviderError::NotFound)
        ));
        assert!(matches!(
            proxy.details("../x").await,
            Err(ProviderError::NotFound)
        ));
        assert_eq!(
            proxy.match_candidates("witcher").await.unwrap()[0].id,
            "igdb-1942"
        );
    }

    async fn igdb_direct(server: &MockServer) -> Arc<dyn CatalogProvider> {
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
                    .set_body_json(json!([{ "id": 5, "name": "From own keys" }])),
            )
            .mount(server)
            .await;
        Arc::new(IgdbProvider::with_endpoints(
            reqwest::Client::new(),
            "id".into(),
            "secret".into(),
            format!("{}/v4", server.uri()),
            format!("{}/oauth2/token", server.uri()),
        ))
    }

    #[tokio::test]
    async fn falls_back_to_own_keys_when_the_proxy_fails() {
        let igdb_server = MockServer::start().await;
        let own = igdb_direct(&igdb_server).await;

        // Proxy unreachable → own keys answer.
        let unreachable: Arc<dyn CatalogProvider> = Arc::new(CoveProxyProvider::new(
            reqwest::Client::new(),
            "http://127.0.0.1:9".into(),
        ));
        let chain = FallbackProvider::new(vec![unreachable.clone(), own.clone()]);
        assert_eq!(
            chain.search("x", 5).await.unwrap()[0].title,
            "From own keys"
        );

        // Proxy rate-limited → own keys answer as well.
        let busy_server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&busy_server)
            .await;
        let busy: Arc<dyn CatalogProvider> = Arc::new(CoveProxyProvider::new(
            reqwest::Client::new(),
            busy_server.uri(),
        ));
        let chain = FallbackProvider::new(vec![busy, own]);
        assert_eq!(chain.search("x", 5).await.unwrap().len(), 1);

        // Only an unreachable proxy → network error (so cached data is used).
        let alone = FallbackProvider::new(vec![unreachable]);
        assert!(matches!(
            alone.search("x", 5).await,
            Err(ProviderError::Network(_))
        ));
    }
}
