//! RAWG (api.rawg.io) — authenticated with an API key query parameter.
//!
//! RAWG has no portrait covers; the landscape `background_image` is used for
//! both cover (cropped by the UI) and header.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::provider::{
    popularity_from_count, status_error, teaser, BrowseQuery, CatalogProvider, ProviderError,
    ProviderPage,
};
use super::rate_limit::RateLimiter;
use crate::models::{
    CatalogProviderId, CatalogSort, Game, GameArtwork, GameFeatures, ImageRef, Platform,
};

pub const API_BASE: &str = "https://api.rawg.io/api";

/// RAWG platform ids: PC, Linux, macOS.
const DESKTOP_PLATFORMS: &str = "4,6,5";

/// Cove genre ↔ RAWG genre slug. Horror is a RAWG *tag*, handled separately.
const GENRE_MAP: &[(&str, &str)] = &[
    ("action", "action"),
    ("adventure", "adventure"),
    ("rpg", "role-playing-games-rpg"),
    ("strategy", "strategy"),
    ("simulation", "simulation"),
    ("puzzle", "puzzle"),
    ("platformer", "platformer"),
    ("shooter", "shooter"),
    ("racing", "racing"),
    ("sports", "sports"),
    ("indie", "indie"),
];

#[derive(Debug, Deserialize)]
struct Slug {
    slug: String,
}

#[derive(Debug, Deserialize)]
struct Named {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PlatformEntry {
    platform: Slug,
}

#[derive(Debug, Deserialize)]
struct Screenshot {
    image: String,
}

#[derive(Debug, Deserialize)]
struct RawgGame {
    id: u64,
    name: String,
    #[serde(default)]
    released: Option<String>,
    #[serde(default)]
    background_image: Option<String>,
    #[serde(default)]
    background_image_additional: Option<String>,
    #[serde(default)]
    rating: Option<f64>,
    #[serde(default)]
    metacritic: Option<i64>,
    #[serde(default)]
    added: Option<f64>,
    #[serde(default)]
    genres: Vec<Slug>,
    #[serde(default)]
    tags: Vec<Slug>,
    #[serde(default)]
    platforms: Option<Vec<PlatformEntry>>,
    #[serde(default)]
    description_raw: Option<String>,
    #[serde(default)]
    developers: Vec<Named>,
    #[serde(default)]
    publishers: Vec<Named>,
    #[serde(default)]
    short_screenshots: Vec<Screenshot>,
}

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    #[serde(default)]
    count: Option<i64>,
    #[serde(default)]
    next: Option<String>,
    results: Vec<T>,
}

fn map_game(raw: RawgGame, screenshots: Option<Vec<String>>) -> Game {
    let mut genres: Vec<String> = raw
        .genres
        .iter()
        .filter_map(|g| {
            GENRE_MAP
                .iter()
                .find(|(_, slug)| *slug == g.slug)
                .map(|(ours, _)| (*ours).to_owned())
        })
        .collect();
    if raw.tags.iter().any(|t| t.slug == "horror") && !genres.iter().any(|g| g == "horror") {
        genres.push("horror".into());
    }
    let mut platforms: Vec<Platform> = Vec::new();
    for entry in raw.platforms.iter().flatten() {
        let mapped = match entry.platform.slug.as_str() {
            "pc" => Some(Platform::Windows),
            "linux" => Some(Platform::Linux),
            "macos" => Some(Platform::Macos),
            _ => None,
        };
        if let Some(m) = mapped.filter(|m| !platforms.contains(m)) {
            platforms.push(m);
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    let rating = raw.metacritic.or_else(|| {
        raw.rating
            .filter(|r| *r > 0.0)
            .map(|r| (r * 20.0).round() as i64)
    });
    let description = raw.description_raw.unwrap_or_default().trim().to_owned();
    let shots: Vec<String> =
        screenshots.unwrap_or_else(|| raw.short_screenshots.into_iter().map(|s| s.image).collect());
    let keywords: Vec<String> = raw
        .tags
        .iter()
        .map(|t| t.slug.replace('-', " "))
        .take(30)
        .collect();
    let title = raw.name;
    Game {
        features: Some(GameFeatures {
            keywords,
            ..GameFeatures::default()
        }),
        id: format!("rawg-{}", raw.id),
        summary: teaser(&description),
        description,
        genres,
        platforms,
        release_date: raw.released.filter(|d| !d.is_empty()),
        developer: raw
            .developers
            .first()
            .map(|d| d.name.clone())
            .unwrap_or_default(),
        publisher: raw
            .publishers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        rating,
        popularity: popularity_from_count(raw.added.unwrap_or(0.0)),
        artwork: GameArtwork {
            cover: ImageRef {
                url: raw.background_image.clone(),
                alt: format!("Cover von {title}"),
            },
            header: ImageRef {
                url: raw.background_image_additional.or(raw.background_image),
                alt: format!("Titelbild von {title}"),
            },
            screenshots: shots
                .into_iter()
                .filter(|url| !url.is_empty())
                .take(8)
                .enumerate()
                .map(|(i, url)| ImageRef {
                    url: Some(url),
                    alt: format!("Screenshot {} von {title}", i + 1),
                })
                .collect(),
        },
        title,
    }
}

/// Query parameters for a browse request (without the API key).
pub fn browse_params(query: &BrowseQuery) -> Vec<(String, String)> {
    let filter = &query.filter;
    let mut params = vec![
        ("page".to_owned(), (query.page + 1).to_string()),
        ("page_size".to_owned(), query.page_size.to_string()),
    ];
    let platforms = if filter.platforms.is_empty() {
        DESKTOP_PLATFORMS.to_owned()
    } else {
        filter
            .platforms
            .iter()
            .map(|p| match p {
                Platform::Windows => "4",
                Platform::Linux => "6",
                Platform::Macos => "5",
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    params.push(("platforms".into(), platforms));
    let genres: Vec<&str> = GENRE_MAP
        .iter()
        .filter(|(ours, _)| filter.genres.iter().any(|g| g == ours))
        .map(|(_, slug)| *slug)
        .collect();
    if !genres.is_empty() {
        params.push(("genres".into(), genres.join(",")));
    }
    if filter.genres.iter().any(|g| g == "horror") {
        params.push(("tags".into(), "horror".into()));
    }
    if let Some(years) = filter.release_years {
        params.push((
            "dates".into(),
            format!("{}-01-01,{}-12-31", years.from, years.to),
        ));
    }
    if let Some(min) = filter.min_rating {
        params.push(("metacritic".into(), format!("{min},100")));
    }
    let text = filter.query.trim();
    if text.is_empty() {
        let ordering = match query.sort {
            CatalogSort::Recommended => "-metacritic",
            CatalogSort::Popular => "-added",
            CatalogSort::Newest => "-released",
            CatalogSort::Title => "name",
        };
        params.push(("ordering".into(), ordering.into()));
    } else {
        params.push(("search".into(), text.to_owned()));
    }
    params
}

pub struct RawgProvider {
    client: reqwest::Client,
    api_key: String,
    api_base: String,
    limiter: RateLimiter,
}

impl RawgProvider {
    pub fn with_base(client: reqwest::Client, api_key: String, api_base: String) -> Self {
        Self {
            client,
            api_key,
            api_base,
            limiter: RateLimiter::per_second(5),
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<T, ProviderError> {
        self.limiter.acquire().await;
        let response = self
            .client
            .get(format!("{}/{endpoint}", self.api_base))
            .query(&[("key", self.api_key.as_str())])
            .query(params)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(status_error(status, &text));
        }
        Ok(response.json().await?)
    }
}

#[async_trait]
impl CatalogProvider for RawgProvider {
    fn id(&self) -> CatalogProviderId {
        CatalogProviderId::Rawg
    }

    async fn verify(&self) -> Result<(), ProviderError> {
        let _: ListResponse<serde_json::Value> = self
            .get("games", &[("page_size".into(), "1".into())])
            .await?;
        Ok(())
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Game>, ProviderError> {
        let params = vec![
            ("search".to_owned(), query.trim().to_owned()),
            ("page_size".to_owned(), limit.to_string()),
            ("platforms".to_owned(), DESKTOP_PLATFORMS.to_owned()),
        ];
        let list: ListResponse<RawgGame> = self.get("games", &params).await?;
        Ok(list
            .results
            .into_iter()
            .map(|g| map_game(g, None))
            .collect())
    }

    async fn browse(&self, query: &BrowseQuery) -> Result<ProviderPage, ProviderError> {
        let list: ListResponse<RawgGame> = self.get("games", &browse_params(query)).await?;
        Ok(ProviderPage {
            has_more: list.next.is_some(),
            total: list.count,
            items: list
                .results
                .into_iter()
                .map(|g| map_game(g, None))
                .collect(),
        })
    }

    async fn details(&self, native_id: &str) -> Result<Game, ProviderError> {
        if native_id.is_empty()
            || !native_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(ProviderError::NotFound);
        }
        let raw: RawgGame = self.get(&format!("games/{native_id}"), &[]).await?;
        let shots: ListResponse<Screenshot> = self
            .get(&format!("games/{native_id}/screenshots"), &[])
            .await?;
        Ok(map_game(
            raw,
            Some(shots.results.into_iter().map(|s| s.image).collect()),
        ))
    }

    /// RAWG's "suggested" endpoint needs a paid plan: use popular games sharing the genres instead.
    async fn similar(&self, native_id: &str) -> Result<Vec<Game>, ProviderError> {
        let raw: RawgGame = self.get(&format!("games/{native_id}"), &[]).await?;
        let genres: Vec<String> = raw.genres.iter().map(|g| g.slug.clone()).collect();
        if genres.is_empty() {
            return Ok(Vec::new());
        }
        let params = vec![
            ("genres".to_owned(), genres.join(",")),
            ("ordering".to_owned(), "-added".to_owned()),
            ("page_size".to_owned(), "11".to_owned()),
            ("platforms".to_owned(), DESKTOP_PLATFORMS.to_owned()),
        ];
        let list: ListResponse<RawgGame> = self.get("games", &params).await?;
        let own_id = format!("rawg-{native_id}");
        Ok(list
            .results
            .into_iter()
            .map(|g| map_game(g, None))
            .filter(|g| g.id != own_id)
            .take(10)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CatalogFilter, YearRange};
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample() -> serde_json::Value {
        json!({
            "id": 3328,
            "name": "The Witcher 3: Wild Hunt",
            "released": "2015-05-18",
            "background_image": "https://media.rawg.io/media/games/618/618c.jpg",
            "rating": 4.66,
            "metacritic": 92,
            "added": 21000,
            "genres": [{ "slug": "action" }, { "slug": "role-playing-games-rpg" }, { "slug": "massively-multiplayer" }],
            "tags": [{ "slug": "open-world" }, { "slug": "horror" }],
            "platforms": [{ "platform": { "slug": "pc" } }, { "platform": { "slug": "playstation4" } }],
            "short_screenshots": [{ "image": "https://media.rawg.io/media/s1.jpg" }]
        })
    }

    #[test]
    fn maps_rawg_games() {
        let game = map_game(serde_json::from_value(sample()).unwrap(), None);
        assert_eq!(game.id, "rawg-3328");
        assert_eq!(game.genres, ["action", "rpg", "horror"]);
        assert_eq!(game.platforms, [Platform::Windows]);
        assert_eq!(game.release_date.as_deref(), Some("2015-05-18"));
        assert_eq!(game.rating, Some(92));
        assert_eq!(
            game.artwork.cover.url.as_deref(),
            Some("https://media.rawg.io/media/games/618/618c.jpg")
        );
        assert_eq!(game.artwork.screenshots.len(), 1);

        let no_meta = map_game(
            serde_json::from_value(json!({ "id": 1, "name": "x", "rating": 3.5 })).unwrap(),
            None,
        );
        assert_eq!(no_meta.rating, Some(70));
    }

    #[test]
    fn builds_query_params() {
        let q = BrowseQuery {
            filter: CatalogFilter {
                genres: vec!["rpg".into(), "horror".into()],
                platforms: vec![Platform::Linux, Platform::Macos],
                release_years: Some(YearRange {
                    from: 2018,
                    to: 2021,
                }),
                min_rating: Some(80),
                ..CatalogFilter::default()
            },
            sort: CatalogSort::Newest,
            page: 1,
            page_size: 24,
        };
        let params = browse_params(&q);
        let get = |k: &str| {
            params
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("page"), Some("2"));
        assert_eq!(get("platforms"), Some("6,5"));
        assert_eq!(get("genres"), Some("role-playing-games-rpg"));
        assert_eq!(get("tags"), Some("horror"));
        assert_eq!(get("dates"), Some("2018-01-01,2021-12-31"));
        assert_eq!(get("metacritic"), Some("80,100"));
        assert_eq!(get("ordering"), Some("-released"));
        assert_eq!(get("search"), None);
    }

    #[tokio::test]
    async fn browse_details_and_auth_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/games"))
            .and(query_param("key", "good"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "count": 50, "next": "x", "results": [sample()] })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/games/3328"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 3328, "name": "The Witcher 3", "description_raw": "Long story. With details.",
                "developers": [{ "name": "CD PROJEKT RED" }], "publishers": [{ "name": "CD PROJEKT" }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/games/3328/screenshots"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [{ "image": "https://media.rawg.io/a.jpg" }, { "image": "https://media.rawg.io/b.jpg" }] })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/games"))
            .and(query_param("key", "bad"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(json!({ "error": "The key parameter is not provided" })),
            )
            .mount(&server)
            .await;

        let p = RawgProvider::with_base(
            reqwest::Client::new(),
            "good".into(),
            format!("{}/api", server.uri()),
        );
        let page = p
            .browse(&BrowseQuery {
                filter: CatalogFilter::default(),
                sort: CatalogSort::Popular,
                page: 0,
                page_size: 24,
            })
            .await
            .unwrap();
        assert_eq!(
            (page.total, page.has_more, page.items.len()),
            (Some(50), true, 1)
        );

        let details = p.details("3328").await.unwrap();
        assert_eq!(details.developer, "CD PROJEKT RED");
        assert_eq!(details.summary, "Long story. With details.");
        assert_eq!(details.artwork.screenshots.len(), 2);
        assert!(matches!(
            p.details("../etc").await,
            Err(ProviderError::NotFound)
        ));

        let bad = RawgProvider::with_base(
            reqwest::Client::new(),
            "bad".into(),
            format!("{}/api", server.uri()),
        );
        assert!(matches!(bad.verify().await, Err(ProviderError::Auth(_))));
    }
}
