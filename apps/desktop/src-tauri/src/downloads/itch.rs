//! itch.io through its official server-side API (<https://itch.io/docs/api/serverside>).
//! The user's API key lives only in the OS keyring. Download URLs are
//! resolved right before a transfer starts (they expire).

use std::sync::Arc;

use serde_json::Value;

use crate::catalog::secrets::SecretStore;
use crate::error::{AppError, AppResult};
use crate::models::{ItchGame, ItchStatus, ItchUpload, Platform};

pub const API_BASE: &str = "https://api.itch.io";
const KEY: &str = "itch-api-key";
const MAX_PAGES: u32 = 50;

pub struct ItchApi {
    client: reqwest::Client,
    /// Does not follow redirects: the download endpoint answers with the CDN location.
    no_redirect: reqwest::Client,
    secrets: Arc<dyn SecretStore>,
    base: String,
}

fn network(e: &reqwest::Error) -> AppError {
    if e.is_timeout() || e.is_connect() {
        AppError::Offline("itch.io ist nicht erreichbar.".into())
    } else {
        AppError::Itch(format!("Anfrage fehlgeschlagen ({e})"))
    }
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

impl ItchApi {
    pub fn new(secrets: Arc<dyn SecretStore>, base: &str) -> Self {
        let build = |redirect| {
            reqwest::Client::builder()
                .user_agent(concat!("Cove/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(20))
                .redirect(redirect)
                .build()
                .unwrap_or_default()
        };
        Self {
            client: build(reqwest::redirect::Policy::default()),
            no_redirect: build(reqwest::redirect::Policy::none()),
            secrets,
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    fn key(&self) -> AppResult<String> {
        self.secrets.get(KEY)?.ok_or_else(|| {
            AppError::Itch(
                "Verbinde zuerst dein itch.io-Konto (Einstellungen → Konten & Stores).".into(),
            )
        })
    }

    async fn get(&self, path: &str, key: &str) -> AppResult<Value> {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .header("Authorization", key)
            .send()
            .await
            .map_err(|e| network(&e))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(AppError::Itch(
                "Der API-Schlüssel ist ungültig oder abgelaufen.".into(),
            ));
        }
        if let Some(errors) = body.get("errors").and_then(Value::as_array) {
            let text: Vec<&str> = errors.iter().filter_map(Value::as_str).collect();
            if text.iter().any(|t| t.contains("invalid key")) {
                return Err(AppError::Itch(
                    "Der API-Schlüssel ist ungültig oder abgelaufen.".into(),
                ));
            }
            return Err(AppError::Itch(text.join(", ")));
        }
        if !status.is_success() {
            return Err(AppError::Itch(format!(
                "itch.io antwortet mit HTTP {}.",
                status.as_u16()
            )));
        }
        Ok(body)
    }

    pub async fn status(&self) -> AppResult<ItchStatus> {
        let Some(key) = self.secrets.get(KEY)? else {
            return Ok(ItchStatus {
                connected: false,
                user_name: None,
            });
        };
        let profile = self.get("/profile", &key).await?;
        let user = profile.get("user").cloned().unwrap_or(Value::Null);
        Ok(ItchStatus {
            connected: true,
            user_name: str_of(&user, "display_name").or_else(|| str_of(&user, "username")),
        })
    }

    /// Checks the key with itch.io before storing it.
    pub async fn set_key(&self, key: &str) -> AppResult<ItchStatus> {
        let key = key.trim();
        if key.is_empty() || key.len() > 200 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(AppError::InvalidInput(
                "Das sieht nicht wie ein itch.io-API-Schlüssel aus.".into(),
            ));
        }
        self.get("/profile", key).await?;
        self.secrets.set(KEY, key)?;
        self.status().await
    }

    pub fn clear_key(&self) -> AppResult<ItchStatus> {
        self.secrets.delete(KEY)?;
        Ok(ItchStatus {
            connected: false,
            user_name: None,
        })
    }

    fn game(game: &Value, download_key_id: Option<i64>) -> Option<ItchGame> {
        Some(ItchGame {
            game_id: game.get("id")?.as_i64()?,
            title: str_of(game, "title")?,
            // Served through the local image cache (CSP allows only `cove-img:`).
            cover_url: crate::catalog::images::proxied(
                str_of(game, "cover_url").or_else(|| str_of(game, "cover_image")),
            ),
            download_key_id,
            url: str_of(game, "url"),
        })
    }

    /// Games the user bought or claimed.
    pub async fn owned(&self) -> AppResult<Vec<ItchGame>> {
        let key = self.key()?;
        let mut games: Vec<ItchGame> = Vec::new();
        for page in 1..=MAX_PAGES {
            let body = self
                .get(&format!("/profile/owned-keys?page={page}"), &key)
                .await?;
            let keys = body
                .get("owned_keys")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if keys.is_empty() {
                break;
            }
            for owned in &keys {
                let id = owned.get("id").and_then(Value::as_i64);
                if let Some(game) = owned.get("game").and_then(|g| Self::game(g, id)) {
                    if !games.iter().any(|g| g.game_id == game.game_id) {
                        games.push(game);
                    }
                }
            }
            let per_page = body.get("per_page").and_then(Value::as_u64).unwrap_or(50);
            if (keys.len() as u64) < per_page {
                break;
            }
        }
        games.sort_by_key(|g| g.title.to_lowercase());
        Ok(games)
    }

    /// A game by its public page (e.g. a free game): `https://<user>.itch.io/<game>`.
    pub async fn lookup(&self, page_url: &str) -> AppResult<ItchGame> {
        let url: reqwest::Url = page_url.trim().parse().map_err(|_| {
            AppError::InvalidInput("Bitte gib die Adresse einer itch.io-Spielseite ein.".into())
        })?;
        let host_ok = url
            .host_str()
            .is_some_and(|h| h.ends_with(".itch.io") || self.base != API_BASE);
        if url.scheme() != "https" && self.base == API_BASE
            || !host_ok
            || url.path().trim_matches('/').is_empty()
        {
            return Err(AppError::InvalidInput(
                "Bitte gib die Adresse einer itch.io-Spielseite ein (https://…itch.io/…).".into(),
            ));
        }
        let data = format!("{}/data.json", url.as_str().trim_end_matches('/'));
        let body: Value = self
            .client
            .get(&data)
            .send()
            .await
            .map_err(|e| network(&e))?
            .json()
            .await
            .map_err(|_| AppError::Itch("Die Seite gehört zu keinem itch.io-Spiel.".into()))?;
        let mut game = Self::game(&body, None)
            .ok_or_else(|| AppError::Itch("Die Seite gehört zu keinem itch.io-Spiel.".into()))?;
        game.url = Some(url.to_string());
        Ok(game)
    }

    pub async fn uploads(
        &self,
        game_id: i64,
        download_key_id: Option<i64>,
    ) -> AppResult<Vec<ItchUpload>> {
        let key = self.key()?;
        let query = download_key_id
            .map(|k| format!("?download_key_id={k}"))
            .unwrap_or_default();
        let body = self
            .get(&format!("/games/{game_id}/uploads{query}"), &key)
            .await?;
        Ok(body
            .get("uploads")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|u| {
                !matches!(
                    u.get("type").and_then(Value::as_str),
                    Some("html" | "soundtrack" | "video" | "book")
                )
            })
            .filter_map(|u| {
                let mut platforms = Vec::new();
                for (flag, platform) in [
                    ("p_windows", Platform::Windows),
                    ("p_linux", Platform::Linux),
                    ("p_osx", Platform::Macos),
                ] {
                    if u.get(flag).and_then(Value::as_bool) == Some(true) {
                        platforms.push(platform);
                    }
                }
                Some(ItchUpload {
                    upload_id: u.get("id")?.as_i64()?,
                    file_name: str_of(u, "filename")?,
                    size_bytes: u.get("size").and_then(Value::as_i64),
                    platforms,
                })
            })
            .collect())
    }

    /// Fresh, short-lived CDN URL for an upload.
    pub async fn resolve_download(
        &self,
        upload_id: i64,
        download_key_id: Option<i64>,
    ) -> AppResult<String> {
        let key = self.key()?;
        let mut url = format!("{}/uploads/{upload_id}/download", self.base);
        if let Some(k) = download_key_id {
            url = format!("{url}?download_key_id={k}");
        }
        let response = self
            .no_redirect
            .get(&url)
            .header("Authorization", &key)
            .send()
            .await
            .map_err(|e| network(&e))?;
        if response.status().is_redirection() {
            return response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
                .ok_or_else(|| {
                    AppError::Itch("itch.io hat keine Download-Adresse geliefert.".into())
                });
        }
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if let Some(url) = str_of(&body, "url") {
            return Ok(url);
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(AppError::Itch(
                "Für diese Datei fehlt der Kauf oder der API-Schlüssel ist ungültig.".into(),
            ));
        }
        Err(AppError::Itch(
            "itch.io hat keine Download-Adresse geliefert.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::secrets::MemoryStore;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn api() -> (ItchApi, MockServer) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/profile"))
            .and(header("authorization", "goodkey"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "user": { "username": "kiefer", "display_name": "Kiefer" } }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/profile"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "errors": ["invalid key"] })),
            )
            .mount(&server)
            .await;
        (
            ItchApi::new(Arc::new(MemoryStore::default()), &server.uri()),
            server,
        )
    }

    #[tokio::test]
    async fn key_is_checked_before_it_is_stored() {
        let (itch, _server) = api().await;
        assert!(!itch.status().await.unwrap().connected);
        assert!(matches!(
            itch.set_key("bad key!").await,
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            itch.set_key("wrongkey").await,
            Err(AppError::Itch(_))
        ));
        assert!(!itch.status().await.unwrap().connected);
        let status = itch.set_key("goodkey").await.unwrap();
        assert_eq!(
            (status.connected, status.user_name.as_deref()),
            (true, Some("Kiefer"))
        );
        assert!(!itch.clear_key().unwrap().connected);
        assert!(matches!(itch.owned().await, Err(AppError::Itch(_))));
    }

    #[tokio::test]
    async fn owned_games_uploads_and_download_urls() {
        let (itch, server) = api().await;
        itch.set_key("goodkey").await.unwrap();
        Mock::given(method("GET"))
            .and(path("/profile/owned-keys"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "per_page": 2, "owned_keys": [
                { "id": 11, "game": { "id": 1, "title": "Zeta", "cover_url": "https://img.itch.zone/z.png", "url": "https://a.itch.io/zeta" } },
                { "id": 12, "game": { "id": 2, "title": "Alpha" } } ] })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/profile/owned-keys"))
            .and(query_param("page", "2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "per_page": 2, "owned_keys": [] })),
            )
            .mount(&server)
            .await;
        let games = itch.owned().await.unwrap();
        assert_eq!(
            games.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(),
            ["Alpha", "Zeta"]
        );
        assert_eq!(games[1].download_key_id, Some(11));

        Mock::given(method("GET"))
            .and(path("/games/1/uploads"))
            .and(query_param("download_key_id", "11"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "uploads": [
                { "id": 100, "filename": "zeta-linux.zip", "size": 1234, "p_linux": true, "type": "default" },
                { "id": 101, "filename": "ost.zip", "type": "soundtrack" } ] })))
            .mount(&server)
            .await;
        let uploads = itch.uploads(1, Some(11)).await.unwrap();
        assert_eq!(
            uploads,
            [ItchUpload {
                upload_id: 100,
                file_name: "zeta-linux.zip".into(),
                size_bytes: Some(1234),
                platforms: vec![Platform::Linux]
            }]
        );

        Mock::given(method("GET"))
            .and(path("/uploads/100/download"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "https://cdn.itch.test/zeta.zip?sig=1"),
            )
            .mount(&server)
            .await;
        assert_eq!(
            itch.resolve_download(100, Some(11)).await.unwrap(),
            "https://cdn.itch.test/zeta.zip?sig=1"
        );
    }

    #[tokio::test]
    async fn lookup_reads_the_public_game_page() {
        let (itch, server) = api().await;
        Mock::given(method("GET"))
            .and(path("/free-game/data.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 77, "title": "Free Game", "cover_image": "https://img.itch.zone/f.png" })))
            .mount(&server)
            .await;
        let game = itch
            .lookup(&format!("{}/free-game", server.uri()))
            .await
            .unwrap();
        assert_eq!(
            (game.game_id, game.title.as_str(), game.download_key_id),
            (77, "Free Game", None)
        );
        assert!(matches!(
            itch.lookup("not a url").await,
            Err(AppError::InvalidInput(_))
        ));
    }
}
