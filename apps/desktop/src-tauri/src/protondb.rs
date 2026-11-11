//! `ProtonDB` ratings: how well a Windows game runs on Linux through Proton.
//!
//! `ProtonDB` rates Steam apps. Steam games are looked up by their app id; for
//! every other game the Steam store search finds the app with the same title
//! (no match, no rating: a wrong rating is worse than none). Both answers are
//! kept in `catalog_cache`, also the "nothing found" ones, and stale entries
//! are served when the services can't be reached.

use std::future::Future;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::catalog::matching::normalize;
use crate::catalog::CACHE_TTL_DAYS;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{ProtonRating, ProtonTier, StoreSource};

pub const PROTONDB_BASE: &str = "https://www.protondb.com";
pub const STEAM_STORE_BASE: &str = "https://store.steampowered.com";

/// Name of the cache rows in `catalog_cache.provider`.
const CACHE_PROVIDER: &str = "protondb";

/// The service could not be reached or answered with an error.
struct Unreachable;

#[derive(Deserialize)]
struct Summary {
    tier: String,
    #[serde(default)]
    total: u32,
}

#[derive(Deserialize)]
struct StoreSearch {
    #[serde(default)]
    items: Vec<StoreSearchItem>,
}

#[derive(Deserialize)]
struct StoreSearchItem {
    #[serde(rename = "type", default)]
    kind: String,
    name: String,
    id: u64,
}

pub struct ProtonDb {
    pool: SqlitePool,
    client: reqwest::Client,
    protondb: String,
    steam_store: String,
}

impl ProtonDb {
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_endpoints(pool, PROTONDB_BASE.into(), STEAM_STORE_BASE.into())
    }

    pub fn with_endpoints(pool: SqlitePool, protondb: String, steam_store: String) -> Self {
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
        Self {
            pool,
            client,
            protondb,
            steam_store,
        }
    }

    /// The rating of a library game; `None` when `ProtonDB` has none (yet) or
    /// the game is not on Steam.
    pub async fn rating(
        &self,
        source: StoreSource,
        store_game_id: Option<&str>,
        title: &str,
    ) -> AppResult<Option<ProtonRating>> {
        let app_id = match (source, store_game_id) {
            (StoreSource::Steam, Some(id)) => Some(id.to_owned()),
            _ => self.steam_app(title).await?,
        };
        // Only real app ids go into the request path.
        let Some(app_id) = app_id.filter(|id| is_app_id(id)) else {
            return Ok(None);
        };
        self.cached(&format!("protondb:rating:{app_id}"), || {
            self.fetch_rating(&app_id)
        })
        .await
    }

    /// The Steam app with exactly this title (compared like catalogue matches).
    async fn steam_app(&self, title: &str) -> AppResult<Option<String>> {
        let wanted = normalize(title);
        if wanted.is_empty() {
            return Ok(None);
        }
        self.cached(&format!("protondb:steam-app:{wanted}"), || {
            self.search_steam(title, &wanted)
        })
        .await
    }

    async fn search_steam(&self, title: &str, wanted: &str) -> Result<Option<String>, Unreachable> {
        let response = self
            .client
            .get(format!("{}/api/storesearch/", self.steam_store))
            .query(&[("term", title), ("cc", "us"), ("l", "english")])
            .send()
            .await
            .map_err(|_| Unreachable)?;
        if !response.status().is_success() {
            return Err(Unreachable);
        }
        let found: StoreSearch = response.json().await.map_err(|_| Unreachable)?;
        Ok(found
            .items
            .into_iter()
            .find(|item| item.kind == "app" && normalize(&item.name) == wanted)
            .map(|item| item.id.to_string()))
    }

    async fn fetch_rating(&self, app_id: &str) -> Result<Option<ProtonRating>, Unreachable> {
        let response = self
            .client
            .get(format!(
                "{}/api/v1/reports/summaries/{app_id}.json",
                self.protondb
            ))
            .send()
            .await
            .map_err(|_| Unreachable)?;
        // Apps without reports are a 404.
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Unreachable);
        }
        let summary: Summary = response.json().await.map_err(|_| Unreachable)?;
        // "pending" (too few reports) and anything new count as "no rating yet".
        Ok(tier(&summary.tier).map(|tier| ProtonRating {
            tier,
            reports: summary.total,
            steam_app_id: app_id.to_owned(),
        }))
    }

    /// Fresh cache entry → that. Otherwise ask; when that fails, an old entry
    /// is better than an error.
    async fn cached<T, F, Fut>(&self, key: &str, fetch: F) -> AppResult<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, Unreachable>>,
    {
        let stale = match db::catalog_cache::get(&self.pool, key).await? {
            Some(entry) => {
                let fresh =
                    chrono::Utc::now() - entry.fetched_at < chrono::Duration::days(CACHE_TTL_DAYS);
                match serde_json::from_str::<T>(&entry.value) {
                    Ok(value) if fresh => return Ok(value),
                    Ok(value) => Some(value),
                    Err(_) => None,
                }
            }
            None => None,
        };
        match (fetch().await, stale) {
            (Ok(value), _) => {
                db::catalog_cache::put(
                    &self.pool,
                    CACHE_PROVIDER,
                    key,
                    &serde_json::to_string(&value)?,
                )
                .await?;
                Ok(value)
            }
            (Err(Unreachable), Some(value)) => Ok(value),
            (Err(Unreachable), None) => Err(AppError::StoreUnavailable(tr!(
                "ProtonDB ist gerade nicht erreichbar. Versuch es später noch einmal.",
                "ProtonDB can't be reached right now. Please try again later."
            ))),
        }
    }
}

fn is_app_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())
}

fn tier(name: &str) -> Option<ProtonTier> {
    Some(match name {
        "platinum" => ProtonTier::Platinum,
        "gold" => ProtonTier::Gold,
        "silver" => ProtonTier::Silver,
        "bronze" => ProtonTier::Bronze,
        "borked" => ProtonTier::Borked,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn service(server: &MockServer) -> ProtonDb {
        let pool = db::open_in_memory().await.unwrap();
        ProtonDb::with_endpoints(pool, server.uri(), server.uri())
    }

    fn summary(tier: &str, total: u32) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "bestReportedTier": "platinum", "confidence": "strong", "score": 0.79,
            "tier": tier, "total": total, "trendingTier": "platinum"
        }))
    }

    #[tokio::test]
    async fn steam_games_are_rated_by_their_app_id_and_cached() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/reports/summaries/1245620.json"))
            .respond_with(summary("gold", 2107))
            .expect(1)
            .mount(&server)
            .await;
        let protondb = service(&server).await;

        for _ in 0..2 {
            let rating = protondb
                .rating(StoreSource::Steam, Some("1245620"), "Elden Ring")
                .await
                .unwrap();
            assert_eq!(
                rating,
                Some(ProtonRating {
                    tier: ProtonTier::Gold,
                    reports: 2107,
                    steam_app_id: "1245620".into(),
                })
            );
        }
    }

    #[tokio::test]
    async fn other_games_are_found_on_steam_by_their_exact_title() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/storesearch/"))
            .and(query_param("term", "Saltmarsh: Complete Edition"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total": 3,
                "items": [
                    { "type": "app", "name": "Saltmarsh Soundtrack", "id": 11 },
                    { "type": "sub", "name": "Saltmarsh", "id": 12 },
                    { "type": "app", "name": "Saltmarsh™", "id": 13 },
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/reports/summaries/13.json"))
            .respond_with(summary("platinum", 40))
            .mount(&server)
            .await;
        let protondb = service(&server).await;

        for _ in 0..2 {
            let rating = protondb
                .rating(
                    StoreSource::Gog,
                    Some("1207658924"),
                    "Saltmarsh: Complete Edition",
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(rating.tier, ProtonTier::Platinum);
            assert_eq!(rating.steam_app_id, "13");
        }
    }

    #[tokio::test]
    async fn no_steam_app_no_reports_or_a_pending_tier_mean_no_rating() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/storesearch/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total": 1,
                "items": [{ "type": "app", "name": "Saltmarsh 2", "id": 14 }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/reports/summaries/20.json"))
            .respond_with(ResponseTemplate::new(404).set_body_string("<html>"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/reports/summaries/21.json"))
            .respond_with(summary("pending", 2))
            .mount(&server)
            .await;
        let protondb = service(&server).await;

        for _ in 0..2 {
            // Asked once each: "nothing found" is remembered too.
            let manual = protondb.rating(StoreSource::Manual, None, "Saltmarsh");
            assert_eq!(manual.await.unwrap(), None);
            let unrated = protondb.rating(StoreSource::Steam, Some("20"), "Quiet");
            assert_eq!(unrated.await.unwrap(), None);
        }
        let pending = protondb.rating(StoreSource::Steam, Some("21"), "New");
        assert_eq!(pending.await.unwrap(), None);
        // An id that is not a Steam app id never reaches the request path.
        let odd = protondb.rating(StoreSource::Steam, Some("../x"), "Odd");
        assert_eq!(odd.await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_old_rating_is_kept_when_protondb_is_down() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/reports/summaries/620.json"))
            .respond_with(summary("platinum", 353))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let protondb = service(&server).await;

        let first = protondb.rating(StoreSource::Steam, Some("620"), "Portal 2");
        assert_eq!(first.await.unwrap().unwrap().tier, ProtonTier::Platinum);
        db::catalog_cache::set_fetched_at(
            &protondb.pool,
            "protondb:rating:620",
            chrono::Utc::now() - chrono::Duration::days(CACHE_TTL_DAYS + 1),
        )
        .await;
        let stale = protondb.rating(StoreSource::Steam, Some("620"), "Portal 2");
        assert_eq!(stale.await.unwrap().unwrap().tier, ProtonTier::Platinum);

        // Nothing cached: a plain sentence, no URL or status code.
        let error = protondb
            .rating(StoreSource::Steam, Some("400"), "Portal")
            .await
            .unwrap_err();
        assert_eq!(error.kind(), "storeUnavailable");
        assert_eq!(
            error.user_message(),
            "ProtonDB ist gerade nicht erreichbar. Versuch es später noch einmal."
        );
    }
}
