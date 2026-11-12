//! The `CatalogProvider` abstraction: IGDB and RAWG implement it, the
//! service adds caching, fallback and offline handling on top.

use async_trait::async_trait;

use std::collections::HashMap;

use crate::models::{CatalogFilter, CatalogProviderId, CatalogSort, Game, TimeToBeat};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BrowseQuery {
    pub filter: CatalogFilter,
    pub sort: CatalogSort,
    /// 0-based.
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderPage {
    pub items: Vec<Game>,
    pub total: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// No connection / timeout — the service falls back to cached data.
    #[error("Keine Verbindung: {0}")]
    Network(String),
    #[error("Zugangsdaten ungültig: {0}")]
    Auth(String),
    #[error("Zu viele Anfragen – bitte kurz warten.")]
    RateLimited,
    #[error("Eintrag nicht gefunden")]
    NotFound,
    #[error("HTTP {0}: {1}")]
    Http(u16, String),
    #[error("Unerwartete Antwort: {0}")]
    Decode(String),
}

impl ProviderError {
    /// The text for the UI (the `Display` form stays for the log).
    pub fn user_text(&self) -> String {
        match self {
            Self::Network(detail) => tr!("Keine Verbindung: {detail}", "No connection: {detail}"),
            Self::Auth(detail) => tr!(
                "Zugangsdaten ungültig: {detail}",
                "Invalid credentials: {detail}"
            ),
            Self::RateLimited => tr!(
                "Zu viele Anfragen – bitte kurz warten.",
                "Too many requests. Please wait a moment."
            ),
            Self::NotFound => tr!("Eintrag nicht gefunden", "Entry not found"),
            Self::Http(status, detail) => format!("HTTP {status}: {detail}"),
            Self::Decode(detail) => tr!(
                "Unerwartete Antwort: {detail}",
                "Unexpected answer: {detail}"
            ),
        }
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        // Never show the URL: it can hold the user's RAWG key or Twitch client secret.
        let e = e.without_url();
        if e.is_decode() {
            Self::Decode(e.to_string())
        } else if let Some(status) = e.status() {
            Self::Http(status.as_u16(), e.to_string())
        } else {
            // connect, timeout, DNS, TLS, body read …
            Self::Network(e.to_string())
        }
    }
}

/// Maps a non-success HTTP status to an error.
pub fn status_error(status: reqwest::StatusCode, body: &str) -> ProviderError {
    match status.as_u16() {
        401 | 403 => ProviderError::Auth(format!("HTTP {status}")),
        404 => ProviderError::NotFound,
        429 => ProviderError::RateLimited,
        code => ProviderError::Http(code, body.chars().take(200).collect()),
    }
}

/// A source of catalogue data. Game ids are namespaced (`igdb-1942`); methods
/// taking an id receive the provider-native part (`1942`).
#[async_trait]
pub trait CatalogProvider: Send + Sync {
    fn id(&self) -> CatalogProviderId;
    /// Cheap request that fails with `Auth` for wrong credentials.
    async fn verify(&self) -> Result<(), ProviderError>;
    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Game>, ProviderError>;
    async fn browse(&self, query: &BrowseQuery) -> Result<ProviderPage, ProviderError>;
    async fn details(&self, native_id: &str) -> Result<Game, ProviderError>;
    async fn similar(&self, native_id: &str) -> Result<Vec<Game>, ProviderError>;
    /// Catalogue entries that may be the game called `title` (used for matching).
    async fn match_candidates(&self, title: &str) -> Result<Vec<Game>, ProviderError> {
        self.search(title, 10).await
    }
    /// How long games take, per native id; ids without reports are left out.
    /// Sources without such data answer with nothing.
    async fn time_to_beat(
        &self,
        native_ids: &[String],
    ) -> Result<HashMap<String, TimeToBeat>, ProviderError> {
        let _ = native_ids;
        Ok(HashMap::new())
    }
}

/// At most this many games per time-to-beat request (IGDB's page limit).
pub const TIME_TO_BEAT_BATCH: usize = 500;

/// First sentence (or ~180 chars) of a longer text, used as `Game.summary`.
pub fn teaser(text: &str) -> String {
    let text = text.trim();
    let end = text
        .char_indices()
        .find(|&(i, c)| {
            matches!(c, '.' | '!' | '?')
                && i > 40
                && text[i + c.len_utf8()..].starts_with(char::is_whitespace)
        })
        .map(|(i, c)| i + c.len_utf8());
    match end {
        Some(end) if end <= 220 => text[..end].to_owned(),
        _ if text.chars().count() > 180 => {
            format!("{}…", text.chars().take(180).collect::<String>().trim_end())
        }
        _ => text.to_owned(),
    }
}

/// Popularity 0–100 from a raw count (ratings, follows …), logarithmic.
pub fn popularity_from_count(count: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let value = ((count.max(0.0) + 1.0).log10() * 25.0).round() as i64;
    value.clamp(0, 100)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn network_errors_never_contain_the_url() {
        let error = reqwest::get("http://127.0.0.1:1/games?key=supersecret")
            .await
            .unwrap_err();
        let mapped = super::ProviderError::from(error).to_string();
        assert!(
            !mapped.contains("supersecret") && !mapped.contains("127.0.0.1"),
            "{mapped}"
        );
    }

    use super::*;

    #[test]
    fn teaser_cuts_at_sentence_end() {
        assert_eq!(teaser("Short."), "Short.");
        let long =
            "Explore a drowned city full of secrets and quiet stories. Then do more things here.";
        assert_eq!(
            teaser(long),
            "Explore a drowned city full of secrets and quiet stories."
        );
        let no_stop = "a".repeat(300);
        assert!(teaser(&no_stop).ends_with('…'));
    }

    #[test]
    fn popularity_is_logarithmic_and_bounded() {
        assert_eq!(popularity_from_count(0.0), 0);
        assert_eq!(popularity_from_count(9.0), 25);
        assert_eq!(popularity_from_count(99_999.0), 100);
        assert_eq!(popularity_from_count(1e12), 100);
    }
}
