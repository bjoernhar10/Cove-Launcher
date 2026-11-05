//! Cove's own achievements. Definitions are JSON: the bundled file plus what
//! active plugins define through the plugin API (`achievements.define`, ids
//! prefixed with the plugin id). Rules are evaluated locally against the
//! database; only unlock times are stored (`cove_achievements`).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone, Timelike};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::models::{now_iso, CoveAchievement};

const BUNDLED: &str = include_str!("../resources/cove-achievements.json");

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Rule {
    /// Hours played in sessions tracked by Cove.
    PlaytimeHours { hours: i64 },
    /// Games in the library.
    LibraryGames { count: i64 },
    /// A session overlapped this local time window (e.g. 2–5 at night).
    #[serde(rename_all = "camelCase")]
    PlayedBetween { from_hour: u32, to_hour: u32 },
    /// Recommended games the user played afterwards.
    RecommendedPlayed { count: i64 },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Definition {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Optional English texts; without them the texts above are used in every language.
    #[serde(default)]
    pub en: Option<Texts>,
    pub rule: Rule,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Texts {
    pub title: String,
    pub description: String,
}

impl Definition {
    /// Title and description in the UI language.
    fn texts(&self) -> (String, String) {
        match &self.en {
            Some(en) if crate::i18n::is_english() => (en.title.clone(), en.description.clone()),
            _ => (self.title.clone(), self.description.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct DefinitionFile {
    achievements: Vec<Definition>,
}

pub fn parse(json: &str) -> AppResult<Vec<Definition>> {
    Ok(serde_json::from_str::<DefinitionFile>(json)?.achievements)
}

/// The definitions that ship with Cove.
pub fn bundled() -> Vec<Definition> {
    parse(BUNDLED).unwrap_or_default()
}

/// Facts the rules look at, read once per evaluation.
#[derive(Debug, Default)]
struct Facts {
    tracked_minutes: i64,
    library_games: i64,
    /// (start, end) of every finished or running session, UTC.
    sessions: Vec<(DateTime<chrono::Utc>, DateTime<chrono::Utc>)>,
    recommended_played: i64,
}

async fn facts(pool: &SqlitePool) -> AppResult<Facts> {
    let seconds: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(duration_seconds), 0) FROM play_sessions")
            .fetch_one(pool)
            .await?;
    let library_games: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM library_entries")
        .fetch_one(pool)
        .await?;
    let rows: Vec<(String, Option<String>, i64)> =
        sqlx::query_as("SELECT started_at, ended_at, duration_seconds FROM play_sessions")
            .fetch_all(pool)
            .await?;
    let sessions = rows
        .into_iter()
        .filter_map(|(start, end, duration)| {
            let start = DateTime::parse_from_rfc3339(&start).ok()?.to_utc();
            let end = end
                .and_then(|e| DateTime::parse_from_rfc3339(&e).ok())
                .map_or_else(|| start + Duration::seconds(duration), |e| e.to_utc());
            Some((start, end))
        })
        .collect();
    let recommended_played: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM recommendation_log r WHERE EXISTS \
           (SELECT 1 FROM play_sessions s WHERE s.game_id = r.game_id AND s.started_at >= r.recommended_at)",
    )
    .fetch_one(pool)
    .await?;
    Ok(Facts {
        tracked_minutes: seconds / 60,
        library_games,
        sessions,
        recommended_played,
    })
}

/// True if `[start, end]` overlaps `from_hour..to_hour` local time on any day.
pub fn overlaps_local_window<Tz: TimeZone>(
    tz: &Tz,
    start: DateTime<chrono::Utc>,
    end: DateTime<chrono::Utc>,
    from_hour: u32,
    to_hour: u32,
) -> bool {
    let (Some(from), Some(to)) = (
        NaiveTime::from_hms_opt(from_hour, 0, 0),
        NaiveTime::from_hms_opt(to_hour, 0, 0),
    ) else {
        return false;
    };
    let local_start = start.with_timezone(tz);
    let local_end = end.with_timezone(tz);
    if local_start.hour() >= from_hour && local_start.hour() < to_hour {
        return true;
    }
    let mut day = local_start.date_naive();
    while day <= local_end.date_naive() {
        let window_start = day.and_time(from);
        let window_end = day.and_time(to);
        if local_start.naive_local() < window_end && local_end.naive_local() > window_start {
            return true;
        }
        day = match day.succ_opt() {
            Some(d) => d,
            None => break,
        };
    }
    false
}

fn progress(rule: &Rule, facts: &Facts) -> (i64, i64) {
    match rule {
        Rule::PlaytimeHours { hours } => ((facts.tracked_minutes / 60).min(*hours), *hours),
        Rule::LibraryGames { count } => (facts.library_games.min(*count), *count),
        Rule::RecommendedPlayed { count } => (facts.recommended_played.min(*count), *count),
        Rule::PlayedBetween { from_hour, to_hour } => {
            let hit = facts
                .sessions
                .iter()
                .any(|(s, e)| overlaps_local_window(&Local, *s, *e, *from_hour, *to_hour));
            (i64::from(hit), 1)
        }
    }
}

async fn unlocked(pool: &SqlitePool) -> AppResult<HashMap<String, String>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, unlocked_at FROM cove_achievements")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}

/// All achievements with progress; unlocked first.
pub async fn list(
    pool: &SqlitePool,
    definitions: &[Definition],
) -> AppResult<Vec<CoveAchievement>> {
    let facts = facts(pool).await?;
    let unlocked = unlocked(pool).await?;
    let mut items: Vec<CoveAchievement> = definitions
        .iter()
        .map(|d| {
            let (current, target) = progress(&d.rule, &facts);
            let unlocked_at = unlocked.get(&d.id).cloned();
            let (title, description) = d.texts();
            CoveAchievement {
                id: d.id.clone(),
                title,
                description,
                current: if unlocked_at.is_some() {
                    target
                } else {
                    current
                },
                target,
                unlocked_at,
            }
        })
        .collect();
    items.sort_by_key(|a| a.unlocked_at.is_none());
    Ok(items)
}

/// Unlocks every achievement whose rule is met now; returns the new ones.
pub async fn evaluate(
    pool: &SqlitePool,
    definitions: &[Definition],
) -> AppResult<Vec<CoveAchievement>> {
    let facts = facts(pool).await?;
    let already = unlocked(pool).await?;
    let now = now_iso();
    let mut fresh = Vec::new();
    for definition in definitions {
        let (current, target) = progress(&definition.rule, &facts);
        if already.contains_key(&definition.id) || current < target {
            continue;
        }
        let inserted =
            sqlx::query("INSERT OR IGNORE INTO cove_achievements (id, unlocked_at) VALUES (?, ?)")
                .bind(&definition.id)
                .bind(&now)
                .execute(pool)
                .await?;
        if inserted.rows_affected() > 0 {
            let (title, description) = definition.texts();
            fresh.push(CoveAchievement {
                id: definition.id.clone(),
                title,
                description,
                unlocked_at: Some(now.clone()),
                current: target,
                target,
            });
        }
    }
    Ok(fresh)
}

/// Remembers recommended games (first time shown) for the "Entdecker" rule.
pub async fn log_recommended(pool: &SqlitePool, game_ids: &[String]) -> AppResult<()> {
    let now = now_iso();
    for id in game_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO recommendation_log (game_id, recommended_at) VALUES (?, ?)",
        )
        .bind(id)
        .bind(&now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Evaluates in the background and reports unlocks (toast in the UI).
pub struct CoveAchievements {
    pool: SqlitePool,
    plugins: Arc<crate::plugins::Plugins>,
    notify: Arc<dyn Fn(CoveAchievement) + Send + Sync>,
    lock: tokio::sync::Mutex<()>,
}

impl CoveAchievements {
    pub fn new(
        pool: SqlitePool,
        plugins: Arc<crate::plugins::Plugins>,
        notify: Arc<dyn Fn(CoveAchievement) + Send + Sync>,
    ) -> Self {
        Self {
            pool,
            plugins,
            notify,
            lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn definitions(&self) -> AppResult<Vec<Definition>> {
        let mut all = bundled();
        all.extend(self.plugins.achievement_definitions().await?);
        let mut seen = std::collections::HashSet::new();
        all.retain(|d| seen.insert(d.id.clone()));
        Ok(all)
    }

    pub async fn list(&self) -> AppResult<Vec<CoveAchievement>> {
        list(&self.pool, &self.definitions().await?).await
    }

    /// Evaluates now; unlocks are announced through `notify`.
    pub async fn check(&self) -> AppResult<Vec<CoveAchievement>> {
        let _guard = self.lock.lock().await;
        let fresh = evaluate(&self.pool, &self.definitions().await?).await?;
        for achievement in &fresh {
            (self.notify)(achievement.clone());
        }
        Ok(fresh)
    }

    pub fn check_in_background(self: &Arc<Self>) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = this.check().await {
                eprintln!("[cove] Cove-Achievements: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{library, open_in_memory, store_games};
    use chrono::{FixedOffset, Utc};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().to_utc()
    }

    #[test]
    fn bundled_definitions_have_english_texts() {
        for d in parse(BUNDLED).unwrap() {
            assert!(d.en.is_some(), "{} has no English texts", d.id);
            let german = d.texts();
            let english = crate::i18n::in_english(|| d.texts());
            assert_ne!(german.0, english.0, "{}", d.id);
        }
    }

    #[test]
    fn bundled_definitions_parse() {
        let defs = parse(BUNDLED).unwrap();
        let ids: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "first-hour",
                "ten-hours",
                "fifty-hours",
                "night-owl",
                "collector",
                "explorer"
            ]
        );
        assert_eq!(
            defs[3].rule,
            Rule::PlayedBetween {
                from_hour: 2,
                to_hour: 5
            }
        );
    }

    #[test]
    fn night_window_in_local_time() {
        let berlin = FixedOffset::east_opt(3600).unwrap();
        // 01:30–02:10 local: overlaps.
        assert!(overlaps_local_window(
            &berlin,
            utc("2026-11-04T00:30:00Z"),
            utc("2026-11-04T01:10:00Z"),
            2,
            5
        ));
        // 22:00–01:59 local: does not.
        assert!(!overlaps_local_window(
            &berlin,
            utc("2026-11-03T21:00:00Z"),
            utc("2026-11-04T00:59:00Z"),
            2,
            5
        ));
        // Started at 03:00 local.
        assert!(overlaps_local_window(
            &berlin,
            utc("2026-11-04T02:00:00Z"),
            utc("2026-11-04T02:30:00Z"),
            2,
            5
        ));
        // A long session from the evening until the next morning.
        assert!(overlaps_local_window(
            &berlin,
            utc("2026-11-03T19:00:00Z"),
            utc("2026-11-04T07:00:00Z"),
            2,
            5
        ));
    }

    async fn session(pool: &SqlitePool, game: &str, start: &str, seconds: i64) {
        sqlx::query("INSERT INTO play_sessions (game_id, started_at, ended_at, duration_seconds) VALUES (?, ?, ?, ?)")
            .bind(game)
            .bind(start)
            .bind(start)
            .bind(seconds)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn evaluates_rules_once_and_reports_progress() {
        let pool = open_in_memory().await.unwrap();
        let defs = parse(BUNDLED).unwrap();
        library::add_from_catalog(&pool, &library::tests::sample_game("g1", "Eins"))
            .await
            .unwrap();
        // Imported store playtime does not count for Cove's playtime achievements.
        store_games::upsert(
            &pool,
            crate::models::StoreSource::Steam,
            &store_games::StoreGameInput {
                store_game_id: "1".into(),
                title: "Steam".into(),
                playtime_minutes: Some(9000),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(evaluate(&pool, &defs).await.unwrap(), []);

        session(&pool, "g1", "2026-11-03T12:00:00Z", 3700).await;
        let fresh = evaluate(&pool, &defs).await.unwrap();
        assert_eq!(
            fresh.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["first-hour"]
        );
        assert!(
            evaluate(&pool, &defs).await.unwrap().is_empty(),
            "only once"
        );

        let list = list(&pool, &defs).await.unwrap();
        assert_eq!(list[0].id, "first-hour");
        let ten = list.iter().find(|a| a.id == "ten-hours").unwrap();
        assert_eq!(
            (ten.current, ten.target, ten.unlocked_at.is_none()),
            (1, 10, true)
        );
        let collector = list.iter().find(|a| a.id == "collector").unwrap();
        assert_eq!((collector.current, collector.target), (2, 50));
    }

    #[tokio::test]
    async fn explorer_counts_recommended_games_played_afterwards() {
        let pool = open_in_memory().await.unwrap();
        let defs = vec![Definition {
            id: "explorer".into(),
            title: "Entdecker".into(),
            description: String::new(),
            en: None,
            rule: Rule::RecommendedPlayed { count: 2 },
        }];
        for id in ["a", "b", "c"] {
            library::add_from_catalog(&pool, &library::tests::sample_game(id, id))
                .await
                .unwrap();
        }
        session(&pool, "a", "2020-01-01T10:00:00Z", 60).await; // played before it was recommended
        log_recommended(&pool, &["a".into(), "b".into(), "c".into()])
            .await
            .unwrap();
        session(&pool, "b", "2099-01-01T10:00:00Z", 60).await;
        assert_eq!(evaluate(&pool, &defs).await.unwrap(), []);
        session(&pool, "a", "2099-01-02T10:00:00Z", 60).await;
        assert_eq!(evaluate(&pool, &defs).await.unwrap().len(), 1);
    }
}
