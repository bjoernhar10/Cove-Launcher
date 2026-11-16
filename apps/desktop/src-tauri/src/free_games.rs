//! Games that are free to keep for a while: Epic's weekly free games and GOG
//! giveaways from the stores' public endpoints, and giveaways everywhere else
//! (Steam, itch.io, `IndieGala`, Prime Gaming …) from the free `GamerPower` API
//! (no login, no keys; its terms ask for a credit with a link, shown on Home).
//! The list is cached for a few hours (`catalog_cache`, provider `free-games`)
//! and served stale when the stores can't be reached. Each game is announced
//! once (the ids are kept in the settings table as `freeGamesSeen`).

use std::time::Duration;

use serde_json::Value;
use sqlx::SqlitePool;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{FreeGame, ImageRef};

const CACHE_PROVIDER: &str = "free-games";
const FRESH: chrono::Duration = chrono::Duration::hours(3);
/// How often the list is checked in the background.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const SEEN_KEY: &str = "freeGamesSeen";
/// Ids remembered as announced (the newest).
const SEEN_MAX: usize = 200;

pub const EPIC_URL: &str = "https://store-site-backend-static.epicgames.com/freeGamesPromotions";
pub const GOG_URL: &str = "https://catalog.gog.com/v1/catalog";
pub const GAMERPOWER_URL: &str = "https://www.gamerpower.com/api/giveaways";

pub struct FreeGames {
    pool: SqlitePool,
    client: reqwest::Client,
    epic_url: String,
    gog_url: String,
    gamerpower_url: String,
}

fn unavailable() -> AppError {
    AppError::StoreUnavailable(tr!(
        "Gratis-Spiele sind gerade nicht abrufbar. Versuch es später noch einmal.",
        "Free games can't be loaded right now. Please try again later."
    ))
}

/// The store page language: Epic and GOG know `de` and English.
fn locale(language: &str) -> (&'static str, &'static str) {
    if language == "de" {
        ("de", "de-DE")
    } else {
        ("en-US", "en-US")
    }
}

impl FreeGames {
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_urls(pool, EPIC_URL, GOG_URL, GAMERPOWER_URL)
    }

    pub fn with_urls(
        pool: SqlitePool,
        epic_url: &str,
        gog_url: &str,
        gamerpower_url: &str,
    ) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Cove/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Self {
            pool,
            client,
            epic_url: epic_url.to_owned(),
            gog_url: gog_url.to_owned(),
            gamerpower_url: gamerpower_url.to_owned(),
        }
    }

    async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> Option<Value> {
        let response = match self.client.get(url).query(query).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                eprintln!("[free-games] {url} answered {}", response.status());
                return None;
            }
            Err(error) => {
                eprintln!("[free-games] {error}");
                return None;
            }
        };
        response.json().await.ok()
    }

    /// All sources, fresh. Fails only when none answers.
    async fn fetch(&self, country: &str, language: &str) -> AppResult<Vec<FreeGame>> {
        let (epic_locale, gog_locale) = locale(language);
        let epic_query = [
            ("locale", epic_locale),
            ("country", country),
            ("allowCountries", country),
        ];
        let gog_query = [
            ("limit", "48"),
            ("price", "between:0,0"),
            ("discounted", "eq:true"),
            ("productType", "in:game,pack"),
            ("locale", gog_locale),
            ("countryCode", country),
        ];
        let epic = self.get_json(&self.epic_url, &epic_query);
        let gog = self.get_json(&self.gog_url, &gog_query);
        let gamerpower = self.get_json(
            &self.gamerpower_url,
            &[("type", "game"), ("platform", "pc")],
        );
        let (epic, gog, gamerpower) = tokio::join!(epic, gog, gamerpower);
        if epic.is_none() && gog.is_none() && gamerpower.is_none() {
            return Err(unavailable());
        }
        let now = chrono::Utc::now();
        let mut games = epic
            .map(|v| parse_epic(&v, epic_locale, now))
            .unwrap_or_default();
        games.extend(gog.map(|v| parse_gog(&v)).unwrap_or_default());
        games.extend(gamerpower.map(|v| parse_gamerpower(&v)).unwrap_or_default());
        Ok(games)
    }

    /// Free games now and soon (cached for a few hours), without the games already in the library.
    pub async fn list(
        &self,
        country: &str,
        language: &str,
        forced: bool,
    ) -> AppResult<Vec<FreeGame>> {
        let key = format!("free-games:{country}:{language}");
        let cached = db::catalog_cache::get(&self.pool, &key).await?;
        let fresh = cached
            .as_ref()
            .filter(|row| !forced && chrono::Utc::now() - row.fetched_at < FRESH)
            .and_then(|row| serde_json::from_str::<Vec<FreeGame>>(&row.value).ok());
        let games = if let Some(games) = fresh {
            games
        } else {
            match self.fetch(country, language).await {
                Ok(games) => {
                    let text = serde_json::to_string(&games)?;
                    db::catalog_cache::put(&self.pool, CACHE_PROVIDER, &key, &text).await?;
                    games
                }
                Err(error) => cached
                    .and_then(|row| serde_json::from_str(&row.value).ok())
                    .ok_or(error)?,
            }
        };
        let owned: std::collections::HashSet<String> = db::library::list(&self.pool)
            .await?
            .into_iter()
            .map(|item| crate::catalog::matching::normalize(&item.game.title))
            .collect();
        Ok(games
            .into_iter()
            .filter(|g| !owned.contains(&crate::catalog::matching::normalize(&g.title)))
            .collect())
    }

    /// Games free right now that were not announced yet; remembers them.
    pub async fn new_ones(&self, games: &[FreeGame]) -> AppResult<Vec<FreeGame>> {
        let mut seen: Vec<String> = db::settings::get_json(&self.pool, SEEN_KEY)
            .await?
            .unwrap_or_default();
        let fresh: Vec<FreeGame> = games
            .iter()
            .filter(|g| !g.upcoming && !seen.contains(&g.id))
            .cloned()
            .collect();
        if !fresh.is_empty() {
            seen.extend(fresh.iter().map(|g| g.id.clone()));
            let skip = seen.len().saturating_sub(SEEN_MAX);
            seen.drain(..skip);
            db::settings::set_json(&self.pool, SEEN_KEY, &seen).await?;
        }
        Ok(fresh)
    }
}

/// Offers in `promotions.<field>[].promotionalOffers[]` that make the game free.
fn free_offer<'a>(element: &'a Value, field: &str) -> Option<&'a Value> {
    element["promotions"][field]
        .as_array()?
        .iter()
        .filter_map(|group| group["promotionalOffers"].as_array())
        .flatten()
        .find(|offer| offer["discountSetting"]["discountPercentage"].as_i64() == Some(0))
}

fn epic_slug(element: &Value) -> Option<String> {
    let mapping = |list: &Value| {
        let list = list.as_array()?;
        list.iter()
            .find(|m| m["pageType"].as_str() == Some("productHome"))
            .or_else(|| list.first())
            .and_then(|m| m["pageSlug"].as_str())
            .map(str::to_owned)
    };
    mapping(&element["catalogNs"]["mappings"])
        .or_else(|| mapping(&element["offerMappings"]))
        .or_else(|| {
            element["productSlug"]
                .as_str()
                .map(|s| s.trim_end_matches("/home").to_owned())
        })
        .filter(|s| !s.is_empty() && s != "[]")
}

fn epic_cover(element: &Value) -> Option<String> {
    let images = element["keyImages"].as_array()?;
    [
        "OfferImageTall",
        "DieselStoreFrontTall",
        "Thumbnail",
        "OfferImageWide",
    ]
    .iter()
    .find_map(|kind| {
        images
            .iter()
            .find(|i| i["type"].as_str() == Some(kind))
            .and_then(|i| i["url"].as_str())
    })
    .map(str::to_owned)
}

/// Epic's `freeGamesPromotions`: games free now (a promotion at 0 %) and next week's.
pub fn parse_epic(
    value: &Value,
    locale: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<FreeGame> {
    let Some(elements) = value["data"]["Catalog"]["searchStore"]["elements"].as_array() else {
        return Vec::new();
    };
    let time = |offer: &Value, field: &str| offer[field].as_str().map(str::to_owned);
    let parse_time = |s: &Option<String>| {
        s.as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
    };
    elements
        .iter()
        .filter(|e| e["offerType"].as_str() != Some("ADD_ON"))
        .filter_map(|element| {
            let (offer, upcoming) = match free_offer(element, "promotionalOffers") {
                Some(offer) => (offer, false),
                None => (free_offer(element, "upcomingPromotionalOffers")?, true),
            };
            let (starts_at, ends_at) = (time(offer, "startDate"), time(offer, "endDate"));
            if !upcoming {
                // Defensive: a promotion that is over or not started yet.
                if parse_time(&ends_at).is_some_and(|end| end <= now)
                    || parse_time(&starts_at).is_some_and(|start| start > now)
                {
                    return None;
                }
            }
            let title = element["title"].as_str()?.trim().to_owned();
            let id = element["id"].as_str()?;
            let slug = epic_slug(element)?;
            Some(FreeGame {
                id: format!("epic:{id}"),
                shop: "Epic Games".into(),
                url: format!("https://store.epicgames.com/{locale}/p/{slug}"),
                cover: ImageRef {
                    url: epic_cover(element),
                    alt: String::new(),
                },
                title,
                starts_at,
                ends_at,
                upcoming,
            })
        })
        .collect()
}

/// GOG's catalogue filtered to price 0 with a discount: giveaways.
pub fn parse_gog(value: &Value) -> Vec<FreeGame> {
    let Some(products) = value["products"].as_array() else {
        return Vec::new();
    };
    let zero = |v: &Value| {
        v.as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .is_some_and(|n| n == 0.0)
    };
    products
        .iter()
        .filter(|p| {
            zero(&p["price"]["finalMoney"]["amount"]) && !zero(&p["price"]["baseMoney"]["amount"])
        })
        .filter_map(|p| {
            Some(FreeGame {
                id: format!("gog:{}", p["id"].as_str()?),
                shop: "GOG".into(),
                title: p["title"].as_str()?.trim().to_owned(),
                url: p["storeLink"].as_str()?.to_owned(),
                cover: ImageRef {
                    url: p["coverVertical"].as_str().map(str::to_owned),
                    alt: String::new(),
                },
                starts_at: None,
                ends_at: None,
                upcoming: false,
            })
        })
        .collect()
}

/// Shops named by the platforms of a giveaway, when the title doesn't say.
const GAMERPOWER_SHOPS: &[(&str, &str)] = &[
    ("Steam", "Steam"),
    ("Itch.io", "itch.io"),
    ("Ubisoft", "Ubisoft"),
    ("Origin", "EA"),
    ("Battle.net", "Battle.net"),
    ("DRM-Free", "DRM-free"),
];

/// "Knightica (Epic Games) Giveaway" → ("Knightica", Some("Epic Games")).
fn split_giveaway_title(raw: &str) -> (String, Option<String>) {
    let mut title = raw.trim();
    for suffix in [" Key Giveaway", " Giveaway"] {
        if let Some(rest) = title.strip_suffix(suffix) {
            title = rest.trim_end();
            break;
        }
    }
    if let Some(open) = title.rfind(" (").filter(|_| title.ends_with(')')) {
        let shop = &title[open + 2..title.len() - 1];
        return (
            title[..open].trim().to_owned(),
            Some(shop.trim().to_owned()),
        );
    }
    (title.to_owned(), None)
}

/// `GamerPower`'s giveaways (full games for PC). Epic and GOG come straight from
/// the stores, so their giveaways here are left out.
pub fn parse_gamerpower(value: &Value) -> Vec<FreeGame> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|g| g["status"].as_str().is_none_or(|s| s == "Active"))
        .filter_map(|g| {
            let platforms = g["platforms"].as_str().unwrap_or_default();
            if platforms.contains("Epic Games Store") || platforms.contains("GOG") {
                return None;
            }
            let (title, named) = split_giveaway_title(g["title"].as_str()?);
            if title.is_empty() {
                return None;
            }
            let shop = named.unwrap_or_else(|| {
                GAMERPOWER_SHOPS
                    .iter()
                    .find(|(platform, _)| platforms.contains(platform))
                    .map_or("GamerPower", |(_, shop)| shop)
                    .to_owned()
            });
            let ends_at = g["end_date"]
                .as_str()
                .and_then(|d| chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M:%S").ok())
                .map(|d| {
                    d.and_utc()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                });
            Some(FreeGame {
                id: format!("gp:{}", g["id"].as_i64()?),
                shop,
                title,
                url: g["open_giveaway_url"]
                    .as_str()
                    .or_else(|| g["gamerpower_url"].as_str())?
                    .to_owned(),
                cover: ImageRef {
                    url: g["image"]
                        .as_str()
                        .or_else(|| g["thumbnail"].as_str())
                        .map(str::to_owned),
                    alt: String::new(),
                },
                starts_at: None,
                ends_at,
                upcoming: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn epic_body() -> Value {
        let promo = |start: &str, end: &str, percent: i64| {
            json!([{ "promotionalOffers": [{
                "startDate": start, "endDate": end,
                "discountSetting": { "discountType": "PERCENTAGE", "discountPercentage": percent }
            }] }])
        };
        json!({ "data": { "Catalog": { "searchStore": { "elements": [
            {
                "title": "Lantern Keeper", "id": "a1", "namespace": "ns1", "offerType": "BASE_GAME",
                "productSlug": null,
                "catalogNs": { "mappings": [{ "pageSlug": "lantern-keeper-3f2a", "pageType": "productHome" }] },
                "keyImages": [
                    { "type": "OfferImageWide", "url": "https://cdn1.epicgames.com/wide.jpg" },
                    { "type": "OfferImageTall", "url": "https://cdn1.epicgames.com/tall.jpg" }
                ],
                "promotions": {
                    "promotionalOffers": promo("2026-11-12T16:00:00.000Z", "2026-11-19T16:00:00.000Z", 0),
                    "upcomingPromotionalOffers": []
                }
            },
            {
                "title": "Tidewater", "id": "b2", "offerType": "BASE_GAME",
                "productSlug": "tidewater/home", "catalogNs": { "mappings": [] },
                "keyImages": [],
                "promotions": {
                    "promotionalOffers": [],
                    "upcomingPromotionalOffers": promo("2026-11-19T16:00:00.000Z", "2026-11-26T16:00:00.000Z", 0)
                }
            },
            {
                "title": "Half price", "id": "c3", "offerType": "BASE_GAME", "productSlug": "half",
                "keyImages": [],
                "promotions": { "promotionalOffers": promo("2026-11-12T16:00:00.000Z", "2026-11-19T16:00:00.000Z", 50) }
            },
            {
                "title": "A free DLC", "id": "d4", "offerType": "ADD_ON", "productSlug": "dlc",
                "keyImages": [],
                "promotions": { "promotionalOffers": promo("2026-11-12T16:00:00.000Z", "2026-11-19T16:00:00.000Z", 0) }
            },
            { "title": "No promotions", "id": "e5", "productSlug": "x", "promotions": null }
        ] } } } })
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        "2026-11-16T12:00:00Z".parse().unwrap()
    }

    #[test]
    fn reads_epic_free_games_now_and_next() {
        let games = parse_epic(&epic_body(), "de", now());
        assert_eq!(games.len(), 2, "{games:?}");
        assert_eq!(games[0].title, "Lantern Keeper");
        assert_eq!(
            games[0].url,
            "https://store.epicgames.com/de/p/lantern-keeper-3f2a"
        );
        assert_eq!(
            games[0].cover.url.as_deref(),
            Some("https://cdn1.epicgames.com/tall.jpg")
        );
        assert!(!games[0].upcoming);
        assert_eq!(
            games[0].ends_at.as_deref(),
            Some("2026-11-19T16:00:00.000Z")
        );
        assert_eq!(games[1].url, "https://store.epicgames.com/de/p/tidewater");
        assert!(games[1].upcoming);

        // A week later the first promotion is over.
        let later = "2026-11-20T12:00:00Z".parse().unwrap();
        assert!(parse_epic(&epic_body(), "de", later)
            .iter()
            .all(|g| g.title != "Lantern Keeper"));
        assert_eq!(
            parse_epic(&json!({ "errors": [] }), "de", now()),
            Vec::<FreeGame>::new()
        );
    }

    fn gog_body() -> Value {
        json!({ "products": [
            { "id": "11", "title": "Woolgather", "storeLink": "https://www.gog.com/de/game/woolgather",
              "coverVertical": "https://images.gog-statics.com/w.jpg",
              "price": { "finalMoney": { "amount": "0.00" }, "baseMoney": { "amount": "9.99" } } },
            { "id": "12", "title": "Always free", "storeLink": "https://www.gog.com/de/game/free",
              "price": { "finalMoney": { "amount": "0.00" }, "baseMoney": { "amount": "0.00" } } }
        ] })
    }

    #[test]
    fn reads_gog_giveaways_but_not_free_to_play_games() {
        let games = parse_gog(&gog_body());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, "gog:11");
        assert_eq!(games[0].shop, "GOG");
    }

    fn gamerpower_body() -> Value {
        json!([
            { "id": 3850, "title": "Knightica (Epic Games) Giveaway", "platforms": "PC, Epic Games Store",
              "open_giveaway_url": "https://www.gamerpower.com/open/knightica", "status": "Active", "end_date": "N/A" },
            { "id": 3851, "title": "Dwarven Realms (Steam) Key Giveaway", "platforms": "PC, Steam",
              "image": "https://www.gamerpower.com/offers/1b/d.jpg",
              "open_giveaway_url": "https://www.gamerpower.com/open/dwarven", "status": "Active",
              "end_date": "2026-11-22 23:59:00" },
            { "id": 3852, "title": "Free itch.io Games Bundle", "platforms": "PC, Itch.io, DRM-Free",
              "open_giveaway_url": "https://www.gamerpower.com/open/bundle", "status": "Active", "end_date": "N/A" },
            { "id": 3853, "title": "Old (Steam) Giveaway", "platforms": "PC, Steam",
              "open_giveaway_url": "https://www.gamerpower.com/open/old", "status": "Expired" }
        ])
    }

    #[test]
    fn reads_giveaways_of_other_shops() {
        let games = parse_gamerpower(&gamerpower_body());
        assert_eq!(games.len(), 2, "{games:?}");
        assert_eq!(games[0].id, "gp:3851");
        assert_eq!(games[0].title, "Dwarven Realms");
        assert_eq!(games[0].shop, "Steam");
        assert_eq!(games[0].ends_at.as_deref(), Some("2026-11-22T23:59:00Z"));
        assert_eq!(
            games[0].cover.url.as_deref(),
            Some("https://www.gamerpower.com/offers/1b/d.jpg")
        );
        assert_eq!(games[1].title, "Free itch.io Games Bundle");
        assert_eq!(games[1].shop, "itch.io");
        assert_eq!(games[1].ends_at, None);
    }

    #[tokio::test]
    async fn lists_caches_and_announces_each_game_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/epic"))
            .and(query_param("country", "DE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(epic_body()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gog"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gog_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gamerpower"))
            .and(query_param("type", "game"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gamerpower_body()))
            .mount(&server)
            .await;
        let pool = db::open_in_memory().await.unwrap();
        let service = FreeGames::with_urls(
            pool.clone(),
            &format!("{}/epic", server.uri()),
            &format!("{}/gog", server.uri()),
            &format!("{}/gamerpower", server.uri()),
        );
        let games = service.list("DE", "de", false).await.unwrap();
        // Today's Epic game is over in the real clock's future, so only compare what's stable.
        assert!(games.iter().any(|g| g.id == "gog:11"));
        assert!(games.iter().any(|g| g.id == "gp:3851"));
        // Second call: from the cache (the Epic mock expects one request).
        assert_eq!(service.list("DE", "de", false).await.unwrap(), games);

        let first = service.new_ones(&games).await.unwrap();
        assert!(first.iter().all(|g| !g.upcoming));
        assert!(first.iter().any(|g| g.id == "gog:11"));
        assert_eq!(
            service.new_ones(&games).await.unwrap(),
            Vec::<FreeGame>::new()
        );
    }

    #[tokio::test]
    async fn leaves_out_games_in_the_library_and_fails_without_any_store() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gog"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gog_body()))
            .mount(&server)
            .await;
        let pool = db::open_in_memory().await.unwrap();
        db::library::add_manual(
            &pool,
            &crate::models::ManualGameInput {
                title: "Woolgather".into(),
                executable_path: "/games/woolgather/run".into(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        let service = FreeGames::with_urls(
            pool.clone(),
            &format!("{}/epic", server.uri()),
            &format!("{}/gog", server.uri()),
            &format!("{}/gamerpower", server.uri()),
        );
        assert_eq!(
            service.list("DE", "en", false).await.unwrap(),
            Vec::<FreeGame>::new()
        );

        let down = FreeGames::with_urls(
            pool,
            "http://127.0.0.1:1/epic",
            "http://127.0.0.1:1/gog",
            "http://127.0.0.1:1/gamerpower",
        );
        assert!(matches!(
            down.list("US", "en", false).await,
            Err(AppError::StoreUnavailable(_))
        ));
    }
}
