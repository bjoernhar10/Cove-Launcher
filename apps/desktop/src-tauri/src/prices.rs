//! Game prices for the wishlist and the catalogue (`IsThereAnyDeal` through the
//! Cove proxy, which holds the API key), and the price alerts.
//!
//! Each game's answer is cached for a few hours (`catalog_cache`, provider
//! `prices`); an old answer is served when the proxy can't be reached. A deal
//! is announced once: the wishlist remembers its price and discount, and a deal
//! that ended makes room for the next alert.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{AlertReason, Game, GamePrice, PriceAlert, WishlistItem};

const CACHE_PROVIDER: &str = "prices";
/// Prices are fresh for this long (the proxy caches 3 h as well).
const FRESH: chrono::Duration = chrono::Duration::hours(3);
/// Games per proxy request.
const BATCH: usize = 50;
/// How often the wishlist is checked in the background.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Deserialize)]
struct PricesBody {
    items: HashMap<String, Option<GamePrice>>,
}

pub struct PriceService {
    pool: SqlitePool,
    client: reqwest::Client,
    base: Option<String>,
}

fn cache_key(country: &str, game_id: &str) -> String {
    format!("prices:{country}:{game_id}")
}

fn unavailable() -> AppError {
    AppError::StoreUnavailable(tr!(
        "Preise sind gerade nicht verfügbar. Versuch es später noch einmal.",
        "Prices aren't available right now. Please try again later."
    ))
}

impl PriceService {
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_base(pool, crate::catalog::proxy::build_time_url())
    }

    pub fn with_base(pool: SqlitePool, base: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Cove/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { pool, client, base }
    }

    /// Prices of these games, fresh from the proxy (cached afterwards).
    async fn fetch(
        &self,
        games: &[&Game],
        country: &str,
    ) -> AppResult<HashMap<String, Option<GamePrice>>> {
        let base = self.base.as_deref().ok_or_else(unavailable)?;
        let mut out = HashMap::new();
        for chunk in games.chunks(BATCH) {
            let body = json!({
                "country": country,
                "games": chunk.iter().map(|g| json!({ "key": g.id, "title": g.title })).collect::<Vec<_>>(),
            });
            let response = self
                .client
                .post(format!("{}/v1/prices", base.trim_end_matches('/')))
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    eprintln!("[prices] {e}");
                    unavailable()
                })?;
            if !response.status().is_success() {
                eprintln!("[prices] proxy answered {}", response.status());
                return Err(unavailable());
            }
            let answer: PricesBody = response.json().await.map_err(|e| {
                eprintln!("[prices] {e}");
                unavailable()
            })?;
            for game in chunk {
                let price = answer.items.get(&game.id).cloned().flatten();
                let text = serde_json::to_string(&price)?;
                db::catalog_cache::put(
                    &self.pool,
                    CACHE_PROVIDER,
                    &cache_key(country, &game.id),
                    &text,
                )
                .await?;
                out.insert(game.id.clone(), price);
            }
        }
        Ok(out)
    }

    async fn cached(
        &self,
        country: &str,
        game_id: &str,
    ) -> AppResult<Option<(Option<GamePrice>, bool)>> {
        let Some(row) = db::catalog_cache::get(&self.pool, &cache_key(country, game_id)).await?
        else {
            return Ok(None);
        };
        let fresh = chrono::Utc::now() - row.fetched_at < FRESH;
        Ok(serde_json::from_str(&row.value).ok().map(|p| (p, fresh)))
    }

    /// One game's prices (catalogue page): cached, refreshed after a few hours.
    pub async fn game_price(&self, game: &Game, country: &str) -> AppResult<Option<GamePrice>> {
        let cached = self.cached(country, &game.id).await?;
        if let Some((price, true)) = &cached {
            return Ok(price.clone());
        }
        match self.fetch(&[game], country).await {
            Ok(mut prices) => Ok(prices.remove(&game.id).flatten()),
            Err(error) => cached.map(|(p, _)| p).ok_or(error),
        }
    }

    /// Checks every wishlist game now; returns the list and the alerts to announce.
    pub async fn check(&self, country: &str) -> AppResult<(Vec<WishlistItem>, Vec<PriceAlert>)> {
        let items = db::wishlist::list(&self.pool).await?;
        if items.is_empty() {
            return Ok((items, Vec::new()));
        }
        let games: Vec<&Game> = items.iter().map(|i| &i.game).collect();
        let prices = self.fetch(&games, country).await?;
        for (id, price) in &prices {
            db::wishlist::set_price(&self.pool, id, price.as_ref()).await?;
        }
        let mut alerts = Vec::new();
        for state in db::wishlist::alert_states(&self.pool).await? {
            let best = prices
                .get(&state.game_id)
                .and_then(|p| p.as_ref())
                .and_then(|p| p.best.clone());
            match decide(&state, best.as_ref()) {
                Decision::Alert(reason) => {
                    let Some(deal) = best else { continue };
                    db::wishlist::set_alerted(
                        &self.pool,
                        &state.game_id,
                        Some((deal.price.amount, deal.cut)),
                    )
                    .await?;
                    alerts.push(PriceAlert {
                        game_id: state.game_id.clone(),
                        title: state.title.clone(),
                        deal,
                        reason,
                    });
                }
                Decision::Reset => {
                    db::wishlist::set_alerted(&self.pool, &state.game_id, None).await?;
                }
                Decision::Keep => {}
            }
        }
        Ok((db::wishlist::list(&self.pool).await?, alerts))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Alert(AlertReason),
    /// The announced deal is over: the next one may alert again.
    Reset,
    Keep,
}

/// Whether the best deal deserves an alert. Each deal is announced once; a
/// cheaper price or a bigger discount later is a new one.
fn decide(state: &db::wishlist::AlertState, best: Option<&crate::models::PriceDeal>) -> Decision {
    let Some(deal) = best else {
        return Decision::Keep;
    };
    let price = deal.price.amount;
    let cheaper = state.alerted_price.is_none_or(|old| price < old - 0.005);
    let at_target = state.target_price.is_some_and(|t| price <= t + 0.005);
    if at_target {
        return if cheaper {
            Decision::Alert(AlertReason::Target)
        } else {
            Decision::Keep
        };
    }
    if deal.cut > 0 && state.notify_on_sale {
        let deeper = state.alerted_cut.is_none_or(|old| deal.cut > old);
        return if cheaper || deeper {
            Decision::Alert(AlertReason::Sale)
        } else {
            Decision::Keep
        };
    }
    if state.alerted_price.is_some() {
        Decision::Reset
    } else {
        Decision::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::models::{Money, PriceDeal};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn deal(price: f64, cut: i64) -> PriceDeal {
        let money = |amount| Money {
            amount,
            currency: "EUR".into(),
        };
        PriceDeal {
            shop: "GOG".into(),
            price: money(price),
            regular: money(20.0),
            cut,
            url: "https://itad.link/x".into(),
            expiry: None,
        }
    }

    fn state(
        target: Option<f64>,
        sale: bool,
        alerted: Option<(f64, i64)>,
    ) -> db::wishlist::AlertState {
        db::wishlist::AlertState {
            game_id: "igdb-1".into(),
            title: "Orbit".into(),
            target_price: target,
            notify_on_sale: sale,
            alerted_price: alerted.map(|a| a.0),
            alerted_cut: alerted.map(|a| a.1),
        }
    }

    #[test]
    fn each_deal_is_announced_once() {
        use Decision::{Alert, Keep, Reset};
        // A sale, then the same sale again, then a deeper one, then it ends.
        assert_eq!(
            decide(&state(None, true, None), Some(&deal(10.0, 50))),
            Alert(AlertReason::Sale)
        );
        assert_eq!(
            decide(&state(None, true, Some((10.0, 50))), Some(&deal(10.0, 50))),
            Keep
        );
        assert_eq!(
            decide(&state(None, true, Some((10.0, 50))), Some(&deal(6.0, 70))),
            Alert(AlertReason::Sale)
        );
        assert_eq!(
            decide(&state(None, true, Some((6.0, 70))), Some(&deal(20.0, 0))),
            Reset
        );
        // Sales are off: only the target counts.
        assert_eq!(
            decide(&state(None, false, None), Some(&deal(10.0, 50))),
            Keep
        );
        assert_eq!(
            decide(&state(Some(12.0), false, None), Some(&deal(10.0, 50))),
            Alert(AlertReason::Target)
        );
        assert_eq!(
            decide(
                &state(Some(12.0), true, Some((10.0, 50))),
                Some(&deal(10.0, 50))
            ),
            Keep
        );
        assert_eq!(
            decide(&state(Some(5.0), false, None), Some(&deal(10.0, 50))),
            Keep
        );
        assert_eq!(decide(&state(Some(5.0), true, None), None), Keep);
    }

    fn game(id: &str, title: &str) -> Game {
        crate::db::library::tests::sample_game(id, title)
    }

    #[tokio::test]
    async fn checks_the_wishlist_caches_and_alerts() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/prices"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": {
                "igdb-1": { "url": "https://isthereanydeal.com/game/orbit/", "best": deal(7.99, 60), "deals": [deal(7.99, 60)],
                            "historyLow": { "amount": 4.99, "currency": "EUR" }, "checkedAt": "2026-11-13T12:00:00Z" },
                "igdb-2": null
            } })))
            .expect(1)
            .mount(&server)
            .await;
        let pool = open_in_memory().await.unwrap();
        db::wishlist::add(&pool, &game("igdb-1", "Paper Orbit"))
            .await
            .unwrap();
        db::wishlist::add(&pool, &game("igdb-2", "Nowhere"))
            .await
            .unwrap();
        db::wishlist::update(&pool, "igdb-1", Some(9.0), true)
            .await
            .unwrap();
        let prices = PriceService::with_base(pool.clone(), Some(server.uri()));

        let (items, alerts) = prices.check("DE").await.unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].reason, AlertReason::Target);
        assert_eq!(alerts[0].deal.price.amount, 7.99);
        let orbit = items.iter().find(|i| i.game.id == "igdb-1").unwrap();
        assert_eq!(
            orbit
                .price
                .as_ref()
                .unwrap()
                .history_low
                .as_ref()
                .unwrap()
                .amount,
            4.99
        );
        assert!(items
            .iter()
            .find(|i| i.game.id == "igdb-2")
            .unwrap()
            .price
            .is_none());

        // The catalogue page reads the cached answer (one proxy call in total).
        let price = prices
            .game_price(&game("igdb-1", "Paper Orbit"), "DE")
            .await
            .unwrap();
        assert_eq!(price.unwrap().best.unwrap().shop, "GOG");
        assert_eq!(
            prices
                .game_price(&game("igdb-2", "Nowhere"), "DE")
                .await
                .unwrap(),
            None
        );

        // Owned games leave the list.
        db::library::add_from_catalog(&pool, &game("igdb-2", "Nowhere"))
            .await
            .unwrap();
        let list = db::wishlist::list(&pool).await.unwrap();
        assert_eq!(
            list.iter().map(|i| i.game.id.as_str()).collect::<Vec<_>>(),
            ["igdb-1"]
        );
        assert!(db::wishlist::update(&pool, "igdb-1", Some(-1.0), true)
            .await
            .is_err());
        db::wishlist::remove(&pool, "igdb-1").await.unwrap();
        assert_eq!(db::wishlist::list(&pool).await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn without_the_proxy_prices_are_unavailable() {
        let pool = open_in_memory().await.unwrap();
        let prices = PriceService::with_base(pool, None);
        assert!(prices
            .game_price(&game("igdb-1", "Orbit"), "DE")
            .await
            .is_err());
    }
}
