//! IGDB (api.igdb.com v4) — authenticated with a Twitch app (client id + secret).
//!
//! Limits: 4 requests/second → [`RateLimiter`]; 429 responses are retried with
//! a short back-off; an expired token (401) is refreshed once.

use std::fmt::Write as _;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::Instant;

use super::provider::{
    popularity_from_count, status_error, teaser, BrowseQuery, CatalogProvider, ProviderError,
    ProviderPage,
};
use super::rate_limit::RateLimiter;
use crate::models::{
    CatalogProviderId, CatalogSort, Game, GameArtwork, GameFeatures, ImageRef, Platform,
};

pub const API_BASE: &str = "https://api.igdb.com/v4";
pub const AUTH_URL: &str = "https://id.twitch.tv/oauth2/token";
const IMAGE_BASE: &str = "https://images.igdb.com/igdb/image/upload";

const FIELDS: &str =
    "name,summary,storyline,genres,themes.name,game_modes.name,player_perspectives.name,\
keywords.name,platforms,first_release_date,\
involved_companies.company.name,involved_companies.developer,involved_companies.publisher,\
total_rating,total_rating_count,cover.image_id,screenshots.image_id,artworks.image_id";

/// Main games only (no DLCs, editions or bundles), playable on desktop.
const BASE_WHERE: &str = "parent_game = null & version_parent = null";
const DESKTOP_PLATFORMS: &[u32] = &[6, 3, 14];

/// IGDB genre ids → Cove genres.
const GENRE_MAP: &[(u32, &str)] = &[
    (2, "adventure"), // Point-and-click
    (4, "action"),    // Fighting
    (5, "shooter"),
    (8, "platformer"),
    (9, "puzzle"),
    (10, "racing"),
    (11, "strategy"), // Real Time Strategy
    (12, "rpg"),
    (13, "simulation"),
    (14, "sports"),
    (15, "strategy"),
    (16, "strategy"), // Turn-based strategy
    (24, "strategy"), // Tactical
    (25, "action"),   // Hack and slash
    (31, "adventure"),
    (32, "indie"),
    (33, "action"), // Arcade
];
/// IGDB theme ids → Cove genres (IGDB models these as themes).
const THEME_MAP: &[(u32, &str)] = &[(1, "action"), (19, "horror")];

#[derive(Debug, Deserialize)]
struct Token {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct Image {
    image_id: String,
}

#[derive(Debug, Deserialize)]
struct Company {
    name: String,
}

/// Expanded reference (`themes.name` → `{ id, name }`).
#[derive(Debug, Deserialize)]
struct Named {
    id: u32,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InvolvedCompany {
    company: Option<Company>,
    #[serde(default)]
    developer: bool,
    #[serde(default)]
    publisher: bool,
}

#[derive(Debug, Deserialize)]
struct IgdbGame {
    id: u64,
    name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    storyline: Option<String>,
    #[serde(default)]
    genres: Vec<u32>,
    #[serde(default)]
    themes: Vec<Named>,
    #[serde(default)]
    game_modes: Vec<Named>,
    #[serde(default)]
    player_perspectives: Vec<Named>,
    #[serde(default)]
    keywords: Vec<Named>,
    #[serde(default)]
    platforms: Vec<u32>,
    #[serde(default)]
    first_release_date: Option<i64>,
    #[serde(default)]
    involved_companies: Vec<InvolvedCompany>,
    #[serde(default)]
    total_rating: Option<f64>,
    #[serde(default)]
    total_rating_count: Option<f64>,
    #[serde(default)]
    cover: Option<Image>,
    #[serde(default)]
    screenshots: Vec<Image>,
    #[serde(default)]
    artworks: Vec<Image>,
    #[serde(default)]
    similar_games: Vec<u64>,
}

#[derive(Debug, Deserialize)]
struct Count {
    count: i64,
}

fn image(size: &str, id: &str) -> String {
    format!("{IMAGE_BASE}/{size}/{id}.jpg")
}

fn map_genres(game: &IgdbGame) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mapped = game
        .genres
        .iter()
        .filter_map(|g| GENRE_MAP.iter().find(|(id, _)| id == g))
        .chain(
            game.themes
                .iter()
                .filter_map(|t| THEME_MAP.iter().find(|(id, _)| *id == t.id)),
        );
    for (_, slug) in mapped {
        if !out.iter().any(|s| s == slug) {
            out.push((*slug).to_owned());
        }
    }
    out
}

fn map_game(raw: IgdbGame) -> Game {
    let genres = map_genres(&raw);
    let mut platforms: Vec<Platform> = Vec::new();
    for p in &raw.platforms {
        let mapped = match p {
            6 => Some(Platform::Windows),
            3 => Some(Platform::Linux),
            14 => Some(Platform::Macos),
            _ => None,
        };
        if let Some(m) = mapped.filter(|m| !platforms.contains(m)) {
            platforms.push(m);
        }
    }
    let company = |dev: bool| {
        raw.involved_companies
            .iter()
            .filter(|c| if dev { c.developer } else { c.publisher })
            .find_map(|c| c.company.as_ref().map(|co| co.name.clone()))
            .unwrap_or_default()
    };
    let developer = company(true);
    let publisher = company(false);
    let summary_text = raw.summary.clone().unwrap_or_default();
    let description = [raw.summary.as_deref(), raw.storyline.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let header = raw
        .artworks
        .first()
        .or_else(|| raw.screenshots.first())
        .map(|i| image("t_1080p", &i.image_id));
    let names = |list: &[Named], max: usize| -> Vec<String> {
        list.iter()
            .filter_map(|n| n.name.clone())
            .take(max)
            .collect()
    };
    let features = GameFeatures {
        themes: names(&raw.themes, 10),
        game_modes: names(&raw.game_modes, 10),
        perspectives: names(&raw.player_perspectives, 10),
        keywords: names(&raw.keywords, 30),
    };
    let title = raw.name;
    #[allow(clippy::cast_possible_truncation)]
    let rating = raw.total_rating.map(|r| r.round() as i64);
    Game {
        features: Some(features),
        id: format!("igdb-{}", raw.id),
        summary: teaser(&summary_text),
        description,
        genres,
        platforms,
        release_date: raw
            .first_release_date
            .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
            .map(|d| d.format("%Y-%m-%d").to_string()),
        developer,
        publisher,
        rating,
        popularity: popularity_from_count(raw.total_rating_count.unwrap_or(0.0)),
        artwork: GameArtwork {
            cover: ImageRef {
                url: raw.cover.map(|c| image("t_cover_big", &c.image_id)),
                alt: format!("Cover von {title}"),
            },
            header: ImageRef {
                url: header,
                alt: format!("Titelbild von {title}"),
            },
            screenshots: raw
                .screenshots
                .iter()
                .take(8)
                .enumerate()
                .map(|(i, s)| ImageRef {
                    url: Some(image("t_screenshot_big", &s.image_id)),
                    alt: format!("Screenshot {} von {title}", i + 1),
                })
                .collect(),
        },
        title,
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn id_list(ids: impl IntoIterator<Item = u32>) -> String {
    ids.into_iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Unix timestamp of Jan 1st of `year` (UTC).
fn year_start(year: i32) -> i64 {
    Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0)
        .single()
        .map_or(0, |d| d.timestamp())
}

/// `where` clause for a browse request (without the trailing `;`).
pub fn build_where(query: &BrowseQuery) -> String {
    let filter = &query.filter;
    let mut parts = vec![BASE_WHERE.to_owned()];
    let platforms: Vec<u32> = if filter.platforms.is_empty() {
        DESKTOP_PLATFORMS.to_vec()
    } else {
        filter
            .platforms
            .iter()
            .map(|p| match p {
                Platform::Windows => 6,
                Platform::Linux => 3,
                Platform::Macos => 14,
            })
            .collect()
    };
    parts.push(format!("platforms = ({})", id_list(platforms)));
    if !filter.genres.is_empty() {
        let genre_ids: Vec<u32> = GENRE_MAP
            .iter()
            .filter(|(_, slug)| filter.genres.iter().any(|g| g == slug))
            .map(|(id, _)| *id)
            .collect();
        let theme_ids: Vec<u32> = THEME_MAP
            .iter()
            .filter(|(_, slug)| filter.genres.iter().any(|g| g == slug))
            .map(|(id, _)| *id)
            .collect();
        let mut any = Vec::new();
        if !genre_ids.is_empty() {
            any.push(format!("genres = ({})", id_list(genre_ids)));
        }
        if !theme_ids.is_empty() {
            any.push(format!("themes = ({})", id_list(theme_ids)));
        }
        if !any.is_empty() {
            parts.push(format!("({})", any.join(" | ")));
        }
    }
    if let Some(years) = filter.release_years {
        parts.push(format!(
            "first_release_date >= {} & first_release_date < {}",
            year_start(years.from),
            year_start(years.to + 1)
        ));
    }
    if let Some(min) = filter.min_rating {
        parts.push(format!("total_rating >= {min}"));
    }
    if filter.query.trim().is_empty() {
        match query.sort {
            CatalogSort::Recommended => parts.push("total_rating_count >= 20".into()),
            CatalogSort::Newest => {
                parts.push(format!("first_release_date <= {}", Utc::now().timestamp()));
            }
            CatalogSort::Popular | CatalogSort::Title => {}
        }
    }
    parts.join(" & ")
}

/// Full Apicalypse body for a browse request. IGDB does not allow `sort`
/// together with `search`, so a text query is ranked by relevance.
pub fn build_browse_body(query: &BrowseQuery) -> String {
    let mut body = String::new();
    let text = query.filter.query.trim();
    if !text.is_empty() {
        let _ = write!(body, "search \"{}\"; ", escape(text));
    }
    let _ = write!(body, "fields {FIELDS}; where {}; ", build_where(query));
    if text.is_empty() {
        let sort = match query.sort {
            CatalogSort::Recommended => "total_rating desc",
            CatalogSort::Popular => "total_rating_count desc",
            CatalogSort::Newest => "first_release_date desc",
            CatalogSort::Title => "name asc",
        };
        let _ = write!(body, "sort {sort}; ");
    }
    let _ = write!(
        body,
        "limit {}; offset {};",
        query.page_size,
        query.page * query.page_size
    );
    body
}

pub struct IgdbProvider {
    client: reqwest::Client,
    client_id: String,
    client_secret: String,
    api_base: String,
    auth_url: String,
    token: Mutex<Option<(String, Instant)>>,
    limiter: RateLimiter,
}

impl IgdbProvider {
    pub fn with_endpoints(
        client: reqwest::Client,
        client_id: String,
        client_secret: String,
        api_base: String,
        auth_url: String,
    ) -> Self {
        Self {
            client,
            client_id,
            client_secret,
            api_base,
            auth_url,
            token: Mutex::new(None),
            limiter: RateLimiter::per_second(4),
        }
    }

    async fn token(&self, force_refresh: bool) -> Result<String, ProviderError> {
        let mut guard = self.token.lock().await;
        if let Some((token, valid_until)) = guard.as_ref() {
            if !force_refresh && Instant::now() < *valid_until {
                return Ok(token.clone());
            }
        }
        let response = self
            .client
            .post(&self.auth_url)
            .query(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            // Twitch answers 400/403 for unknown client id or wrong secret.
            return Err(if matches!(status.as_u16(), 400 | 401 | 403) {
                ProviderError::Auth(tr!(
                    "Client-ID oder Secret falsch",
                    "Client ID or secret is wrong"
                ))
            } else {
                status_error(status, &body)
            });
        }
        let token: Token = response.json().await?;
        // Refresh a minute early.
        let valid_until = Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(60));
        *guard = Some((token.access_token.clone(), valid_until));
        Ok(token.access_token)
    }

    async fn post<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<T, ProviderError> {
        let mut refreshed = false;
        let mut retries = 0;
        loop {
            let token = self.token(false).await?;
            self.limiter.acquire().await;
            let response = self
                .client
                .post(format!("{}/{endpoint}", self.api_base))
                .header("Client-ID", &self.client_id)
                .bearer_auth(&token)
                .header("Accept", "application/json")
                .body(body.to_owned())
                .send()
                .await?;
            let status = response.status();
            if status.is_success() {
                return Ok(response.json().await?);
            }
            if status.as_u16() == 401 && !refreshed {
                refreshed = true;
                self.token(true).await?;
                continue;
            }
            if status.as_u16() == 429 && retries < 3 {
                retries += 1;
                tokio::time::sleep(Duration::from_millis(500 * retries)).await;
                continue;
            }
            let text = response.text().await.unwrap_or_default();
            return Err(status_error(status, &text));
        }
    }

    async fn games(&self, body: &str) -> Result<Vec<Game>, ProviderError> {
        let raw: Vec<IgdbGame> = self.post("games", body).await?;
        Ok(raw.into_iter().map(map_game).collect())
    }
}

fn parse_id(native_id: &str) -> Result<u64, ProviderError> {
    native_id.parse().map_err(|_| ProviderError::NotFound)
}

#[async_trait]
impl CatalogProvider for IgdbProvider {
    fn id(&self) -> CatalogProviderId {
        CatalogProviderId::Igdb
    }

    async fn verify(&self) -> Result<(), ProviderError> {
        self.token(true).await?;
        let _: Vec<serde_json::Value> = self.post("games", "fields id; limit 1;").await?;
        Ok(())
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Game>, ProviderError> {
        let body = format!(
            "search \"{}\"; fields {FIELDS}; where {BASE_WHERE}; limit {limit};",
            escape(query.trim())
        );
        self.games(&body).await
    }

    async fn browse(&self, query: &BrowseQuery) -> Result<ProviderPage, ProviderError> {
        let items = self.games(&build_browse_body(query)).await?;
        let counts: Count = if query.filter.query.trim().is_empty() {
            self.post("games/count", &format!("where {};", build_where(query)))
                .await?
        } else {
            // `count` ignores `search`; estimate from the page instead.
            Count { count: -1 }
        };
        let offset = i64::from(query.page * query.page_size);
        let fetched = i64::try_from(items.len()).unwrap_or(0);
        let (total, has_more) = if counts.count >= 0 {
            (Some(counts.count), offset + fetched < counts.count)
        } else {
            (None, fetched == i64::from(query.page_size))
        };
        Ok(ProviderPage {
            items,
            total,
            has_more,
        })
    }

    async fn details(&self, native_id: &str) -> Result<Game, ProviderError> {
        let id = parse_id(native_id)?;
        let body = format!("fields {FIELDS}; where id = {id}; limit 1;");
        self.games(&body)
            .await?
            .into_iter()
            .next()
            .ok_or(ProviderError::NotFound)
    }

    async fn similar(&self, native_id: &str) -> Result<Vec<Game>, ProviderError> {
        let id = parse_id(native_id)?;
        let raw: Vec<IgdbGame> = self
            .post("games", &format!("fields similar_games; where id = {id};"))
            .await?;
        let ids: Vec<u64> = raw
            .into_iter()
            .next()
            .map(|g| g.similar_games)
            .unwrap_or_default();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = ids
            .iter()
            .take(10)
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        self.games(&format!(
            "fields {FIELDS}; where id = ({list}) & {BASE_WHERE}; limit 10;"
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CatalogFilter, YearRange};
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample() -> serde_json::Value {
        json!({
            "id": 1942,
            "name": "The Witcher 3: Wild Hunt",
            "summary": "An open-world RPG set in a dark fantasy world of monsters. More text follows here.",
            "storyline": "Geralt searches for Ciri.",
            "genres": [12, 31],
            "themes": [{ "id": 1, "name": "Action" }, { "id": 17, "name": "Fantasy" }],
            "game_modes": [{ "id": 1, "name": "Single player" }],
            "keywords": [{ "id": 5, "name": "open world" }],
            "platforms": [6, 48, 3],
            "first_release_date": 1_431_993_600,
            "involved_companies": [
                { "company": { "name": "CD Projekt RED" }, "developer": true, "publisher": false },
                { "company": { "name": "CD Projekt" }, "developer": false, "publisher": true }
            ],
            "total_rating": 92.6,
            "total_rating_count": 3000,
            "cover": { "image_id": "co1wyy" },
            "screenshots": [{ "image_id": "sc1" }, { "image_id": "sc2" }],
            "artworks": [{ "image_id": "ar1" }]
        })
    }

    #[test]
    fn maps_igdb_games() {
        let game = map_game(serde_json::from_value(sample()).unwrap());
        assert_eq!(game.id, "igdb-1942");
        assert_eq!(game.genres, ["rpg", "adventure", "action"]);
        assert_eq!(game.platforms, [Platform::Windows, Platform::Linux]);
        assert_eq!(game.release_date.as_deref(), Some("2015-05-19"));
        assert_eq!(game.developer, "CD Projekt RED");
        assert_eq!(game.publisher, "CD Projekt");
        assert_eq!(game.rating, Some(93));
        assert_eq!(
            game.summary,
            "An open-world RPG set in a dark fantasy world of monsters."
        );
        assert!(game.description.ends_with("Geralt searches for Ciri."));
        assert_eq!(
            game.artwork.cover.url.as_deref(),
            Some("https://images.igdb.com/igdb/image/upload/t_cover_big/co1wyy.jpg")
        );
        assert!(game
            .artwork
            .header
            .url
            .as_deref()
            .unwrap()
            .ends_with("t_1080p/ar1.jpg"));
        assert_eq!(game.artwork.screenshots.len(), 2);
        let features = game.features.clone().unwrap();
        assert_eq!(features.themes, ["Action", "Fantasy"]);
        assert_eq!(features.game_modes, ["Single player"]);
        assert_eq!(features.keywords, ["open world"]);

        let minimal = map_game(serde_json::from_value(json!({ "id": 1, "name": "Bare" })).unwrap());
        assert_eq!(minimal.release_date, None);
        assert_eq!(minimal.artwork.cover.url, None);
    }

    #[test]
    fn builds_apicalypse_queries() {
        let mut q = BrowseQuery {
            filter: CatalogFilter {
                genres: vec!["rpg".into(), "horror".into()],
                platforms: vec![Platform::Linux],
                release_years: Some(YearRange {
                    from: 2020,
                    to: 2021,
                }),
                min_rating: Some(80),
                ..CatalogFilter::default()
            },
            sort: CatalogSort::Popular,
            page: 2,
            page_size: 24,
        };
        let body = build_browse_body(&q);
        assert!(body.contains("platforms = (3)"));
        assert!(body.contains("(genres = (12) | themes = (19))"));
        assert!(body.contains("first_release_date >= 1577836800 & first_release_date < 1640995200"));
        assert!(body.contains("total_rating >= 80"));
        assert!(body.contains("sort total_rating_count desc;"));
        assert!(body.ends_with("limit 24; offset 48;"));
        assert!(!body.contains("search"));

        q.filter = CatalogFilter {
            query: "half \"life\"".into(),
            ..CatalogFilter::default()
        };
        let body = build_browse_body(&q);
        assert!(body.starts_with("search \"half \\\"life\\\"\";"));
        assert!(!body.contains("sort "), "IGDB forbids sort with search");
        assert!(body.contains("platforms = (6,3,14)"));
    }

    async fn server_with_token() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(query_param("client_id", "cid"))
            .and(query_param("client_secret", "secret"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "access_token": "tok", "expires_in": 3600 })),
            )
            .mount(&server)
            .await;
        server
    }

    fn provider(server: &MockServer, secret: &str) -> IgdbProvider {
        IgdbProvider::with_endpoints(
            reqwest::Client::new(),
            "cid".into(),
            secret.into(),
            format!("{}/v4", server.uri()),
            format!("{}/oauth2/token", server.uri()),
        )
    }

    #[tokio::test]
    async fn browse_sends_auth_headers_and_counts() {
        let server = server_with_token().await;
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .and(header("Client-ID", "cid"))
            .and(header("Authorization", "Bearer tok"))
            .and(body_string_contains("sort total_rating desc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([sample()])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v4/games/count"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "count": 30 })))
            .mount(&server)
            .await;

        let page = provider(&server, "secret")
            .browse(&BrowseQuery {
                filter: CatalogFilter::default(),
                sort: CatalogSort::Recommended,
                page: 0,
                page_size: 1,
            })
            .await
            .unwrap();
        assert_eq!(page.items[0].title, "The Witcher 3: Wild Hunt");
        assert_eq!(page.total, Some(30));
        assert!(page.has_more);
    }

    #[tokio::test]
    async fn wrong_secret_is_an_auth_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(403).set_body_string("invalid client secret"))
            .mount(&server)
            .await;
        let err = provider(&server, "wrong").verify().await.unwrap_err();
        assert!(matches!(err, ProviderError::Auth(_)), "{err:?}");
    }

    #[tokio::test]
    async fn retries_rate_limited_requests_and_resolves_similar_games() {
        let server = server_with_token().await;
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .and(body_string_contains("fields similar_games"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .and(body_string_contains("fields similar_games"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!([{ "id": 1942, "name": "x", "similar_games": [5, 6] }])),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .and(body_string_contains("where id = (5,6)"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!([{ "id": 5, "name": "Five" }, { "id": 6, "name": "Six" }]),
                ),
            )
            .mount(&server)
            .await;
        let similar = provider(&server, "secret").similar("1942").await.unwrap();
        assert_eq!(
            similar.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
            ["igdb-5", "igdb-6"]
        );
    }

    #[tokio::test]
    async fn network_errors_are_reported_as_such() {
        let p = IgdbProvider::with_endpoints(
            reqwest::Client::new(),
            "cid".into(),
            "s".into(),
            "http://127.0.0.1:9/v4".into(),
            "http://127.0.0.1:9/token".into(),
        );
        assert!(matches!(
            p.search("x", 5).await,
            Err(ProviderError::Network(_))
        ));
    }
}
