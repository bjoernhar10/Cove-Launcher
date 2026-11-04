//! Personal recommendations via the Python sidecar (services/recommender).
//!
//! The core collects everything the recommender needs — library signals and
//! catalogue features — and sends it with every request; the sidecar keeps no
//! state and nothing leaves the machine.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::catalog::{CatalogService, PAGE_SIZE};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    now_iso, CatalogFilter, CatalogPage, CatalogSort, Game, LibraryItem, Recommendation,
    RecommenderGame, RecommenderLibraryGame,
};
use crate::sidecar::Sidecar;

/// Pages per sort order that form the candidate pool (cached catalogue requests).
const POOL_PAGES: u32 = 2;
/// Catalogue sort "recommended": this many pages are re-ranked personally.
pub const RERANK_PAGES: u32 = 3;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    game_id: String,
    score: f64,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct Items {
    items: Vec<Item>,
}

pub fn to_recommender_game(game: &Game, id: &str) -> RecommenderGame {
    let features = game.features.clone().unwrap_or_default();
    RecommenderGame {
        id: id.to_owned(),
        title: game.title.clone(),
        genres: game.genres.clone(),
        themes: features.themes,
        game_modes: features.game_modes,
        perspectives: features.perspectives,
        keywords: features.keywords,
        developer: game.developer.clone(),
        release_year: game
            .release_date
            .as_deref()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse().ok()),
        rating: game.rating,
        popularity: game.popularity,
    }
}

/// Library signals; linked games use their catalogue id so they match catalogue entries.
pub fn to_library_inputs(
    items: &[LibraryItem],
    sessions: &HashMap<String, i64>,
) -> (Vec<RecommenderLibraryGame>, Vec<RecommenderGame>) {
    let mut library = Vec::new();
    let mut games = Vec::new();
    for item in items {
        let id = item
            .entry
            .catalog_id
            .clone()
            .unwrap_or_else(|| item.game.id.clone());
        library.push(RecommenderLibraryGame {
            game_id: id.clone(),
            title: item.game.title.clone(),
            playtime_minutes: item.entry.playtime_minutes,
            last_played_at: item.entry.last_played_at.clone(),
            favorite: item.entry.favorite,
            achievements: item.entry.achievements,
            session_count: sessions.get(&item.game.id).copied().unwrap_or(0),
        });
        games.push(to_recommender_game(&item.game, &id));
    }
    (library, games)
}

pub struct Recommender {
    sidecar: Sidecar,
    pool: SqlitePool,
    catalog: Arc<CatalogService>,
}

struct Inputs {
    library: Vec<RecommenderLibraryGame>,
    catalog: Vec<RecommenderGame>,
    /// Full game data for every candidate, by id.
    games: HashMap<String, Game>,
}

impl Recommender {
    pub fn new(sidecar: Sidecar, pool: SqlitePool, catalog: Arc<CatalogService>) -> Self {
        Self {
            sidecar,
            pool,
            catalog,
        }
    }

    pub fn status(&self) -> crate::sidecar::SidecarStatus {
        self.sidecar.status()
    }

    pub fn is_available(&self) -> bool {
        self.sidecar.is_configured()
    }

    pub async fn warm_up(&self) -> AppResult<()> {
        self.sidecar.warm_up().await
    }

    /// Catalogue candidates from cached browse pages; unavailable pages are skipped.
    async fn candidate_pool(&self) -> Vec<Game> {
        let mut games = Vec::new();
        for sort in [
            CatalogSort::Recommended,
            CatalogSort::Popular,
            CatalogSort::Newest,
        ] {
            for page in 0..POOL_PAGES {
                match self
                    .catalog
                    .browse(CatalogFilter::default(), sort, page)
                    .await
                {
                    Ok(p) => {
                        let more = p.has_more;
                        games.extend(p.items);
                        if !more {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        games
    }

    async fn inputs(&self, extra: Vec<Game>, use_pool: bool) -> AppResult<Inputs> {
        let items = db::library::list(&self.pool).await?;
        let sessions = db::sessions::counts(&self.pool).await?;
        let (library, mut catalog) = to_library_inputs(&items, &sessions);
        let mut games: HashMap<String, Game> = HashMap::new();
        let mut seen: std::collections::HashSet<String> =
            catalog.iter().map(|g| g.id.clone()).collect();
        let pool = if use_pool {
            self.candidate_pool().await
        } else {
            Vec::new()
        };
        for game in extra.into_iter().chain(pool) {
            if seen.insert(game.id.clone()) {
                catalog.push(to_recommender_game(&game, &game.id));
            }
            games.entry(game.id.clone()).or_insert(game);
        }
        Ok(Inputs {
            library,
            catalog,
            games,
        })
    }

    async fn items(&self, method: &str, params: Value) -> AppResult<Vec<Item>> {
        let value = self.sidecar.call(method, params).await?;
        let items: Items = serde_json::from_value(value)
            .map_err(|e| AppError::Recommender(format!("unerwartete Antwort: {e}")))?;
        Ok(items.items)
    }

    fn attach(items: Vec<Item>, games: &HashMap<String, Game>) -> Vec<Recommendation> {
        items
            .into_iter()
            .filter_map(|i| {
                Some(Recommendation {
                    game: games.get(&i.game_id)?.clone(),
                    score: i.score,
                    reason: i.reason,
                })
            })
            .collect()
    }

    pub async fn for_user(&self, limit: u32) -> AppResult<Vec<Recommendation>> {
        let inputs = self.inputs(Vec::new(), true).await?;
        if inputs.games.is_empty() {
            return Ok(Vec::new());
        }
        let params = json!({ "library": inputs.library, "catalog": inputs.catalog, "limit": limit, "now": now_iso() });
        Ok(Self::attach(
            self.items("recommend_for_user", params).await?,
            &inputs.games,
        ))
    }

    pub async fn discover(&self, limit: u32) -> AppResult<Vec<Recommendation>> {
        let inputs = self.inputs(Vec::new(), true).await?;
        if inputs.games.is_empty() {
            return Ok(Vec::new());
        }
        let params = json!({ "library": inputs.library, "catalog": inputs.catalog, "limit": limit, "now": now_iso() });
        Ok(Self::attach(
            self.items("discover_new", params).await?,
            &inputs.games,
        ))
    }

    pub async fn similar(&self, catalog_id: &str, limit: u32) -> AppResult<Vec<Recommendation>> {
        let target = self.catalog.details(catalog_id).await?.game;
        let mut extra = vec![target];
        if let Ok(page) = self.catalog.similar(catalog_id).await {
            extra.extend(page.items);
        }
        let inputs = self.inputs(extra, true).await?;
        let params = json!({ "gameId": catalog_id, "catalog": inputs.catalog, "limit": limit });
        Ok(Self::attach(
            self.items("similar_games", params).await?,
            &inputs.games,
        ))
    }

    pub async fn explain(&self, catalog_id: &str) -> AppResult<String> {
        let target = self.catalog.details(catalog_id).await?.game;
        let inputs = self.inputs(vec![target], false).await?;
        let params = json!({ "gameId": catalog_id, "library": inputs.library, "catalog": inputs.catalog, "now": now_iso() });
        let value = self.sidecar.call("explain_recommendation", params).await?;
        value
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| AppError::Recommender("Antwort ohne Begründung".into()))
    }

    /// Catalogue sort "recommended": the first pages, re-ranked for this user.
    /// Returns None beyond the re-ranked window (the caller serves provider pages).
    pub async fn recommended_page(
        &self,
        filter: &CatalogFilter,
        page: u32,
    ) -> AppResult<Option<CatalogPage>> {
        if page >= RERANK_PAGES {
            return Ok(None);
        }
        let mut candidates: Vec<Game> = Vec::new();
        let mut offline = false;
        let mut total = None;
        let mut provider_has_more = false;
        for p in 0..RERANK_PAGES {
            let provider_page = self
                .catalog
                .browse(filter.clone(), CatalogSort::Recommended, p)
                .await?;
            offline |= provider_page.offline;
            total = total.or(provider_page.total);
            provider_has_more = provider_page.has_more;
            candidates.extend(provider_page.items);
            if !provider_has_more {
                break;
            }
        }
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|g| seen.insert(g.id.clone()));
        let ids: Vec<&str> = candidates.iter().map(|g| g.id.as_str()).collect();
        let inputs = self.inputs(candidates.clone(), false).await?;
        let params = json!({
            "library": inputs.library, "catalog": inputs.catalog, "limit": ids.len().clamp(1, 200),
            "now": now_iso(), "candidateIds": ids,
        });
        let ranked = self.items("recommend_for_user", params).await?;
        let mut order: Vec<Game> = Vec::with_capacity(candidates.len());
        let mut reasons = BTreeMap::new();
        for item in &ranked {
            if let Some(game) = inputs.games.get(&item.game_id) {
                order.push(game.clone());
                reasons.insert(item.game_id.clone(), item.reason.clone());
            }
        }
        // Owned games are not ranked by the recommender; keep them at the end.
        for game in candidates {
            if !reasons.contains_key(&game.id) {
                order.push(game);
            }
        }
        let size = PAGE_SIZE as usize;
        let start = page as usize * size;
        let items: Vec<Game> = order.iter().skip(start).take(size).cloned().collect();
        let has_more = start + items.len() < order.len() || provider_has_more;
        let reasons = items
            .iter()
            .filter_map(|g| reasons.get(&g.id).map(|r| (g.id.clone(), r.clone())))
            .collect();
        Ok(Some(CatalogPage {
            items,
            page,
            has_more,
            total,
            offline,
            reasons,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::tests::sample_game;
    use crate::models::{GameFeatures, ManualGameInput};

    #[tokio::test]
    async fn builds_recommender_inputs_from_the_library() {
        let pool = crate::db::open_in_memory().await.unwrap();
        let input = ManualGameInput {
            title: "Tide".into(),
            executable_path: "/g/t".into(),
            size_bytes: None,
        };
        let manual = db::library::add_manual(&pool, &input).await.unwrap();
        let mut catalog_game = sample_game("igdb-7", "Tidewater");
        catalog_game.features = Some(GameFeatures {
            keywords: vec!["boats".into()],
            ..GameFeatures::default()
        });
        db::library::apply_catalog(&pool, &manual.game.id, &catalog_game)
            .await
            .unwrap();
        let session = db::sessions::start(&pool, &manual.game.id, "2026-11-01T10:00:00Z")
            .await
            .unwrap();
        db::sessions::update(&pool, session, "2026-11-01T12:00:00Z", 7200)
            .await
            .unwrap();

        let items = db::library::list(&pool).await.unwrap();
        let sessions = db::sessions::counts(&pool).await.unwrap();
        let (library, games) = to_library_inputs(&items, &sessions);
        assert_eq!(
            library[0].game_id, "igdb-7",
            "linked games use the catalogue id"
        );
        assert_eq!(library[0].playtime_minutes, 120);
        assert_eq!(library[0].session_count, 1);
        assert_eq!(games[0].id, "igdb-7");
        assert_eq!(
            games[0].keywords,
            ["boats"],
            "features survive the round trip through SQLite"
        );
        assert_eq!(games[0].release_year, Some(2024));
    }
}

/// Contract between the core and the real sidecar binary (skipped when it is not built).
#[cfg(test)]
mod sidecar_contract {
    use super::*;
    use crate::db::library::tests::sample_game;
    use crate::models::{GameFeatures, ManualGameInput};
    use crate::sidecar::{is_placeholder, SidecarConfig};
    use std::path::PathBuf;

    fn built_sidecar() -> Option<PathBuf> {
        let triple = if cfg!(target_os = "linux") {
            "x86_64-unknown-linux-gnu"
        } else {
            return None;
        };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("binaries/cove-recommender-{triple}"));
        (!is_placeholder(&path)).then_some(path)
    }

    fn game(id: &str, title: &str, genres: &[&str], keywords: &[&str], rating: i64) -> Game {
        let mut g = sample_game(id, title);
        g.genres = genres.iter().map(|s| (*s).to_owned()).collect();
        g.rating = Some(rating);
        g.features = Some(GameFeatures {
            keywords: keywords.iter().map(|s| (*s).to_owned()).collect(),
            ..Default::default()
        });
        g
    }

    #[tokio::test]
    async fn real_binary_understands_core_requests() {
        let Some(binary) = built_sidecar() else {
            eprintln!("skipped: sidecar not built (npm run build:sidecar -w @cove/desktop)");
            return;
        };
        let pool = crate::db::open_in_memory().await.unwrap();
        let input = ManualGameInput {
            title: "HK".into(),
            executable_path: "/g/hk".into(),
            size_bytes: None,
        };
        let manual = db::library::add_manual(&pool, &input).await.unwrap();
        let hk = game(
            "igdb-1",
            "Hollow Knight",
            &["platformer", "action"],
            &["metroidvania", "souls-like"],
            90,
        );
        db::library::apply_catalog(&pool, &manual.game.id, &hk)
            .await
            .unwrap();
        let session = db::sessions::start(&pool, &manual.game.id, "2026-11-01T10:00:00Z")
            .await
            .unwrap();
        db::sessions::update(&pool, session, "2026-11-03T10:00:00Z", 40 * 3600)
            .await
            .unwrap();

        let candidates = [
            game(
                "igdb-2",
                "Silksong",
                &["platformer", "action"],
                &["metroidvania", "souls-like"],
                92,
            ),
            game("igdb-3", "Forza", &["racing"], &["cars"], 91),
            game(
                "igdb-4",
                "Skyrim",
                &["rpg", "adventure"],
                &["open world"],
                88,
            ),
        ];
        let items = db::library::list(&pool).await.unwrap();
        let sessions = db::sessions::counts(&pool).await.unwrap();
        let (library, mut catalog) = to_library_inputs(&items, &sessions);
        catalog.extend(candidates.iter().map(|g| to_recommender_game(g, &g.id)));

        let sidecar = Sidecar::new(Some(SidecarConfig::new("recommender", binary, Vec::new())));
        let value = sidecar
            .call(
                "recommend_for_user",
                json!({ "library": library, "catalog": catalog, "limit": 2, "now": now_iso() }),
            )
            .await
            .unwrap();
        let items: Items = serde_json::from_value(value).unwrap();
        assert_eq!(items.items[0].game_id, "igdb-2");
        assert_eq!(
            items.items[0].reason,
            "Weil du 40 Std. Hollow Knight gespielt hast"
        );
        assert!(
            items.items.iter().all(|i| i.game_id != "igdb-1"),
            "owned game excluded"
        );

        let discover = sidecar
            .call(
                "discover_new",
                json!({ "library": library, "catalog": catalog, "limit": 3, "now": now_iso() }),
            )
            .await
            .unwrap();
        let discover: Items = serde_json::from_value(discover).unwrap();
        assert!(discover
            .items
            .iter()
            .all(|i| i.reason.starts_with("Neu für dich: ")));
        assert!(
            discover.items.iter().all(|i| i.game_id != "igdb-2"),
            "known genres are not 'new'"
        );
    }
}
