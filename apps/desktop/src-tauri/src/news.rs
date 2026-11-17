//! News and patch notes per game: the developers' announcements on Steam.
//!
//! Steam's public news API (`ISteamNews/GetNewsForApp`, no key) for the
//! "Community Announcements" feed only, so press articles are left out. Steam
//! games use their app id; every other game the Steam app with exactly its
//! title (`ProtonDb::steam_app_id`). The announcements are Steam's `BBCode`
//! (sometimes HTML); `blocks` turns them into headings, paragraphs and list
//! items of plain text, so nothing from a news post is ever rendered as markup.
//! Answers are cached in `catalog_cache` (provider `news`, 6 h; old entries are
//! served when Steam can't be reached).

use std::time::Duration;

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{GameNews, LibraryItem, NewsBlock, NewsItem, StoreSource};
use crate::protondb::ProtonDb;

pub const STEAM_API_BASE: &str = "https://api.steampowered.com";
const CACHE_PROVIDER: &str = "news";
const FRESH_HOURS: i64 = 6;
/// Posts per game.
const COUNT: usize = 15;
/// Limits for one post (some patch notes are very long).
const MAX_BLOCKS: usize = 400;
const MAX_TEXT: usize = 4000;
/// Home: games looked at, posts shown and how old they may be.
const FEED_GAMES: usize = 12;
const FEED_POSTS: usize = 8;
const FEED_DAYS: i64 = 30;

#[derive(Deserialize)]
struct Answer {
    appnews: AppNews,
}

#[derive(Deserialize)]
struct AppNews {
    #[serde(default)]
    newsitems: Vec<RawItem>,
}

#[derive(Deserialize)]
struct RawItem {
    gid: String,
    title: String,
    url: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    contents: String,
    date: i64,
    #[serde(default)]
    tags: Vec<String>,
}

pub struct News {
    pool: SqlitePool,
    client: reqwest::Client,
    base: String,
}

impl News {
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_base(pool, STEAM_API_BASE.into())
    }

    pub fn with_base(pool: SqlitePool, base: String) -> Self {
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
        Self { pool, client, base }
    }

    /// The newest announcements of a library game; empty when it isn't on Steam.
    pub async fn for_game(
        &self,
        protondb: &ProtonDb,
        source: StoreSource,
        store_game_id: Option<&str>,
        title: &str,
    ) -> AppResult<Vec<NewsItem>> {
        let Some(app_id) = protondb.steam_app_id(source, store_game_id, title).await else {
            return Ok(Vec::new());
        };
        self.for_app(&app_id).await
    }

    /// Home: recent posts of the games played last (newest first).
    pub async fn feed(&self, protondb: &ProtonDb, library: &[LibraryItem]) -> Vec<GameNews> {
        let mut played: Vec<&LibraryItem> = library
            .iter()
            .filter(|item| item.entry.last_played_at.is_some())
            .collect();
        played.sort_by(|a, b| b.entry.last_played_at.cmp(&a.entry.last_played_at));
        let since = chrono::Utc::now() - chrono::Duration::days(FEED_DAYS);
        let mut posts = Vec::new();
        for item in played.into_iter().take(FEED_GAMES) {
            // One game Steam doesn't know (or a hiccup) never empties the feed.
            let entry = &item.entry;
            let Ok(news) = self
                .for_game(
                    protondb,
                    entry.source,
                    entry.store_game_id.as_deref(),
                    &item.game.title,
                )
                .await
            else {
                continue;
            };
            posts.extend(
                news.into_iter()
                    .filter(|post| {
                        chrono::DateTime::parse_from_rfc3339(&post.published_at)
                            .is_ok_and(|at| at >= since)
                    })
                    .take(2)
                    .map(|post| GameNews {
                        game_id: item.game.id.clone(),
                        item: post,
                    }),
            );
        }
        posts.sort_by(|a, b| b.item.published_at.cmp(&a.item.published_at));
        posts.truncate(FEED_POSTS);
        posts
    }

    async fn for_app(&self, app_id: &str) -> AppResult<Vec<NewsItem>> {
        let key = format!("news:{app_id}");
        let stale = match db::catalog_cache::get(&self.pool, &key).await? {
            Some(entry) => {
                let fresh =
                    chrono::Utc::now() - entry.fetched_at < chrono::Duration::hours(FRESH_HOURS);
                match serde_json::from_str::<Vec<NewsItem>>(&entry.value) {
                    Ok(items) if fresh => return Ok(items),
                    Ok(items) => Some(items),
                    Err(_) => None,
                }
            }
            None => None,
        };
        match (self.fetch(app_id).await, stale) {
            (Some(items), _) => {
                db::catalog_cache::put(
                    &self.pool,
                    CACHE_PROVIDER,
                    &key,
                    &serde_json::to_string(&items)?,
                )
                .await?;
                Ok(items)
            }
            (None, Some(items)) => Ok(items),
            (None, None) => Err(AppError::StoreUnavailable(tr!(
                "Steam ist gerade nicht erreichbar. Versuch es später noch einmal.",
                "Steam can't be reached right now. Please try again later."
            ))),
        }
    }

    async fn fetch(&self, app_id: &str) -> Option<Vec<NewsItem>> {
        let count = COUNT.to_string();
        let query = [
            ("appid", app_id),
            ("count", count.as_str()),
            ("maxlength", "0"),
            ("feeds", "steam_community_announcements"),
            ("format", "json"),
        ];
        let response = self
            .client
            .get(format!("{}/ISteamNews/GetNewsForApp/v2/", self.base))
            .query(&query)
            .send()
            .await
            .ok()?;
        // Unknown apps answer 403 or 404: that's "no news", not an outage.
        if matches!(response.status().as_u16(), 403 | 404) {
            return Some(Vec::new());
        }
        if !response.status().is_success() {
            return None;
        }
        let answer: Answer = response.json().await.ok()?;
        Some(answer.appnews.newsitems.into_iter().map(convert).collect())
    }
}

fn convert(raw: RawItem) -> NewsItem {
    let blocks = blocks(&raw.contents);
    let published_at = chrono::DateTime::from_timestamp(raw.date, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let title = clean(&raw.title);
    NewsItem {
        id: raw.gid,
        title,
        url: raw.url.starts_with("https://").then_some(raw.url),
        published_at,
        author: Some(clean(&raw.author)).filter(|a| !a.is_empty()),
        patch_notes: raw.tags.iter().any(|tag| tag == "patchnotes"),
        blocks,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Paragraph,
    Heading,
    Item,
}

struct Builder {
    blocks: Vec<NewsBlock>,
    text: String,
    mode: Mode,
}

impl Builder {
    fn flush(&mut self) {
        let text = clean(&self.text);
        self.text.clear();
        // A line that is only a link is one of Steam's embeds (store widget, video).
        let embed = text.starts_with("http") && !text.contains(' ');
        if text.is_empty() || embed || self.blocks.len() >= MAX_BLOCKS {
            return;
        }
        let text: String = text.chars().take(MAX_TEXT).collect();
        self.blocks.push(match self.mode {
            Mode::Heading => NewsBlock::Heading { text },
            Mode::Item => NewsBlock::Item { text },
            Mode::Paragraph => NewsBlock::Paragraph { text },
        });
    }

    fn switch(&mut self, mode: Mode) {
        self.flush();
        self.mode = mode;
    }
}

/// Tags whose content is not text (images, videos, embeds).
const SKIPPED: &[&str] = &[
    "img",
    "previewyoutube",
    "video",
    "dynamiclink",
    "script",
    "style",
];

/// `BBCode` or HTML → plain text blocks.
pub fn blocks(contents: &str) -> Vec<NewsBlock> {
    let mut out = Builder {
        blocks: Vec::new(),
        text: String::new(),
        mode: Mode::Paragraph,
    };
    let mut skipping: Option<String> = None;
    let mut rest = contents;
    while let Some(c) = rest.chars().next() {
        // `\[` is an escaped bracket in Steam's `BBCode`.
        if c == '\\' && (rest[1..].starts_with('[') || rest[1..].starts_with(']')) {
            if skipping.is_none() {
                out.text.push_str(&rest[1..2]);
            }
            rest = &rest[2..];
            continue;
        }
        if let Some(Tag {
            name: tag,
            len,
            html,
        }) = tag_at(rest)
        {
            rest = &rest[len..];
            let (closing, name) = match tag.strip_prefix('/') {
                Some(name) => (true, name.to_owned()),
                None => (false, tag.clone()),
            };
            if let Some(open) = &skipping {
                if closing && *open == name {
                    skipping = None;
                }
                continue;
            }
            if !closing && SKIPPED.contains(&name.as_str()) {
                // HTML images close themselves.
                if !(name == "img" && html) {
                    skipping = Some(name);
                }
                continue;
            }
            match (name.as_str(), closing) {
                ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => out.switch(Mode::Heading),
                ("*" | "li", false) => out.switch(Mode::Item),
                (
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "*" | "li" | "list" | "olist" | "ul"
                    | "ol",
                    true,
                ) => {
                    out.switch(Mode::Paragraph);
                }
                (
                    "p" | "br" | "hr" | "div" | "tr" | "quote" | "code" | "table" | "list"
                    | "olist" | "ul" | "ol",
                    _,
                ) => {
                    let mode = out.mode;
                    out.flush();
                    out.mode = mode;
                }
                _ => {}
            }
            continue;
        }
        let len = c.len_utf8();
        if skipping.is_some() {
            // Inside an image or video: not text.
        } else if c == '\n' {
            let mode = out.mode;
            out.flush();
            // A line break ends a list item written without [/*].
            out.mode = if mode == Mode::Heading {
                mode
            } else {
                Mode::Paragraph
            };
        } else {
            out.text.push(c);
        }
        rest = &rest[len..];
    }
    out.flush();
    out.blocks
}

/// A `BBCode` (`[name=attr]`) or HTML (`<name attr>`) tag.
struct Tag {
    /// Lower case, with a leading `/` when closing.
    name: String,
    /// Length in the text.
    len: usize,
    html: bool,
}

/// The tag at the start of `text`, if there is one.
fn tag_at(text: &str) -> Option<Tag> {
    let (open, close) = match text.chars().next()? {
        '[' => ('[', ']'),
        '<' => ('<', '>'),
        _ => return None,
    };
    let end = text[1..].find(close)? + 1;
    let inner = &text[1..end];
    if inner.is_empty() || inner.len() > 600 || inner.contains(open) || inner.contains('\n') {
        return None;
    }
    let inner = inner.trim_end_matches('/').trim();
    let split = inner
        .find(|c: char| c == '=' || c.is_whitespace())
        .unwrap_or(inner.len());
    let name = inner[..split].to_ascii_lowercase();
    let bare = name.strip_prefix('/').unwrap_or(&name);
    let valid = bare == "*"
        || (!bare.is_empty()
            && bare.len() <= 20
            && bare.chars().all(|c| c.is_ascii_alphanumeric()));
    if !valid {
        return None;
    }
    Some(Tag {
        name,
        len: end + 1,
        html: open == '<',
    })
}

/// Entities decoded, whitespace collapsed.
fn clean(text: &str) -> String {
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn texts(blocks: &[NewsBlock]) -> Vec<String> {
        blocks
            .iter()
            .map(|b| match b {
                NewsBlock::Heading { text } => format!("# {text}"),
                NewsBlock::Paragraph { text } => text.clone(),
                NewsBlock::Item { text } => format!("- {text}"),
            })
            .collect()
    }

    #[test]
    fn bbcode_becomes_headings_paragraphs_and_items() {
        let blocks = blocks(
            "We launched [i][b]Hades II[/b][/i] &amp; more!\n\nhttps://store.steampowered.com/app/1/\n[h2]About the Game[/h2]\nIt is a [url=https://x.y]rogue-like[/url].\n[img]{STEAM_CLAN_IMAGE}/4331/d12.png[/img]\n[p]\\[ MISC ][/p][list][*][p]Fixed a hang[/p][/*][*]Better [b]AVX[/b] detection[/list][previewyoutube=abc;full][/previewyoutube]",
        );
        assert_eq!(
            texts(&blocks),
            [
                "We launched Hades II & more!",
                "# About the Game",
                "It is a rogue-like.",
                "[ MISC ]",
                "- Fixed a hang",
                "- Better AVX detection",
            ]
        );
    }

    #[test]
    fn html_posts_and_plain_brackets_are_text() {
        let blocks = blocks(
            "<p>Hello <b>there</b></p><img src=\"https://cdn.example.com/a.jpg\"><ul><li>One</li><li>Two</li></ul>Score [10/10] and a < b",
        );
        assert_eq!(
            texts(&blocks),
            ["Hello there", "- One", "- Two", "Score [10/10] and a < b"]
        );
    }

    fn answer() -> serde_json::Value {
        serde_json::json!({ "appnews": { "appid": 10, "count": 2, "newsitems": [
            { "gid": "2", "title": "Update 1.2", "url": "https://store.steampowered.com/news/2", "author": "dev",
              "contents": "[list][*]Fixed saves[/list]", "date": 1_763_000_000, "tags": ["patchnotes"] },
            { "gid": "1", "title": "Hello &amp; welcome", "url": "https://store.steampowered.com/news/1", "author": "",
              "contents": "Thanks!", "date": 1_700_000_000 }
        ] } })
    }

    #[tokio::test]
    async fn steam_games_get_their_announcements_and_they_are_cached() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ISteamNews/GetNewsForApp/v2/"))
            .and(query_param("appid", "10"))
            .and(query_param("feeds", "steam_community_announcements"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer()))
            .expect(1)
            .mount(&server)
            .await;
        let pool = db::open_in_memory().await.unwrap();
        let news = News::with_base(pool.clone(), server.uri());
        let protondb = ProtonDb::with_endpoints(pool.clone(), server.uri(), server.uri());
        let posts = news
            .for_game(&protondb, StoreSource::Steam, Some("10"), "Tidewater")
            .await
            .unwrap();
        assert_eq!(posts.len(), 2);
        assert!(posts[0].patch_notes);
        assert_eq!(posts[0].published_at, "2025-11-13T02:13:20Z");
        assert_eq!(posts[1].title, "Hello & welcome");
        assert_eq!(posts[1].author, None);
        assert!(!posts[1].patch_notes);
        // Second call: from the cache (the mock expects one request).
        let again = news
            .for_game(&protondb, StoreSource::Steam, Some("10"), "Tidewater")
            .await
            .unwrap();
        assert_eq!(again, posts);
    }

    #[tokio::test]
    async fn steam_down_without_cache_is_an_error_and_unknown_apps_have_no_news() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("appid", "11"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("appid", "12"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let pool = db::open_in_memory().await.unwrap();
        let news = News::with_base(pool, server.uri());
        assert_eq!(news.for_app("11").await.unwrap(), []);
        assert!(matches!(
            news.for_app("12").await,
            Err(AppError::StoreUnavailable(_))
        ));
    }
}
