//! The statistics page: playtime of the library over a year or all time.
//!
//! A year only knows the sessions Cove tracked; all time also counts playtime
//! imported from the stores (it has no dates, so it never shows up in months,
//! weekdays or streaks). Dates are UTC, like the game page's 14 days.

use std::collections::{BTreeSet, HashMap};

use chrono::{Datelike, Duration, NaiveDate};
use sqlx::SqlitePool;

use crate::db::library;
use crate::error::AppResult;
use crate::models::{
    LibraryItem, PlayStats, StatsGame, StatsGenre, StatsMonth, StatsSession, StatsStore,
    StoreSource,
};

const TOP_GAMES: usize = 10;

struct Session {
    game_id: String,
    started_at: String,
    date: Option<NaiveDate>,
    seconds: i64,
}

/// Statistics for `year` (None = all time); `today` sets the last 12 months.
pub async fn overview(
    pool: &SqlitePool,
    year: Option<i32>,
    today: NaiveDate,
) -> AppResult<PlayStats> {
    let items = library::list(pool).await?;
    let in_library: HashMap<&str, &LibraryItem> =
        items.iter().map(|i| (i.game.id.as_str(), i)).collect();

    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT game_id, started_at, duration_seconds FROM play_sessions \
         WHERE duration_seconds > 0 ORDER BY started_at",
    )
    .fetch_all(pool)
    .await?;
    let sessions: Vec<Session> = rows
        .into_iter()
        .filter(|(game_id, _, _)| in_library.contains_key(game_id.as_str()))
        .map(|(game_id, started_at, seconds)| Session {
            date: started_at
                .get(..10)
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
            game_id,
            started_at,
            seconds,
        })
        .collect();

    let mut years: Vec<i32> = sessions
        .iter()
        .filter_map(|s| s.date.map(|d| d.year()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    years.reverse();

    let in_range: Vec<&Session> = sessions
        .iter()
        .filter(|s| year.is_none_or(|y| s.date.is_some_and(|d| d.year() == y)))
        .collect();

    // Minutes and sessions per game.
    let mut session_counts: HashMap<&str, i64> = HashMap::new();
    let mut tracked: HashMap<&str, i64> = HashMap::new();
    for s in &in_range {
        *session_counts.entry(s.game_id.as_str()).or_default() += 1;
        *tracked.entry(s.game_id.as_str()).or_default() += s.seconds;
    }
    let minutes_of = |item: &LibraryItem| -> i64 {
        if year.is_some() {
            tracked.get(item.game.id.as_str()).copied().unwrap_or(0) / 60
        } else {
            item.entry.playtime_minutes
        }
    };
    let played: Vec<(&LibraryItem, i64)> = items
        .iter()
        .map(|item| (item, minutes_of(item)))
        .filter(|(_, minutes)| *minutes > 0)
        .collect();

    let top_games = top_games(&played, &session_counts);
    let (genres, stores) = shares(&played);

    let longest_session = in_range.iter().max_by_key(|s| s.seconds).and_then(|s| {
        let item = in_library.get(s.game_id.as_str())?;
        Some(StatsSession {
            game_id: s.game_id.clone(),
            title: item.game.title.clone(),
            minutes: s.seconds / 60,
            started_at: s.started_at.clone(),
        })
    });

    let days: BTreeSet<NaiveDate> = in_range.iter().filter_map(|s| s.date).collect();
    let weekdays = weekdays(&in_range);

    let achievements_unlocked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_achievements u \
         JOIN library_entries e ON e.game_id = u.game_id \
         WHERE ? IS NULL OR substr(u.unlocked_at, 1, 4) = ?",
    )
    .bind(year.map(|y| y.to_string()))
    .bind(year.map(|y| y.to_string()))
    .fetch_one(pool)
    .await?;

    Ok(PlayStats {
        year,
        years,
        total_minutes: played.iter().map(|(_, m)| m).sum(),
        session_count: i64::try_from(in_range.len()).unwrap_or(i64::MAX),
        games_played: i64::try_from(played.len()).unwrap_or(i64::MAX),
        days_played: i64::try_from(days.len()).unwrap_or(i64::MAX),
        longest_streak_days: longest_streak(&days),
        longest_session,
        achievements_unlocked,
        months: months(&in_range, year, today),
        weekdays,
        top_games,
        genres,
        stores,
    })
}

/// The most played games with their session counts.
fn top_games(
    played: &[(&LibraryItem, i64)],
    session_counts: &HashMap<&str, i64>,
) -> Vec<StatsGame> {
    let mut top: Vec<&(&LibraryItem, i64)> = played.iter().collect();
    top.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.game.title.cmp(&b.0.game.title))
    });
    top.into_iter()
        .take(TOP_GAMES)
        .map(|(item, minutes)| StatsGame {
            game: item.game.clone(),
            minutes: *minutes,
            session_count: session_counts
                .get(item.game.id.as_str())
                .copied()
                .unwrap_or(0),
        })
        .collect()
}

/// Minutes per genre and per store, most first.
fn shares(played: &[(&LibraryItem, i64)]) -> (Vec<StatsGenre>, Vec<StatsStore>) {
    let mut genres: HashMap<&str, i64> = HashMap::new();
    let mut stores: HashMap<StoreSource, i64> = HashMap::new();
    for (item, minutes) in played {
        for genre in &item.game.genres {
            *genres.entry(genre.as_str()).or_default() += minutes;
        }
        *stores.entry(item.entry.source).or_default() += minutes;
    }
    let mut genres: Vec<StatsGenre> = genres
        .into_iter()
        .map(|(genre, minutes)| StatsGenre {
            genre: genre.to_owned(),
            minutes,
        })
        .collect();
    genres.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.genre.cmp(&b.genre))
    });
    let mut stores: Vec<StatsStore> = stores
        .into_iter()
        .map(|(store, minutes)| StatsStore { store, minutes })
        .collect();
    stores.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.store.as_str().cmp(b.store.as_str()))
    });

    (genres, stores)
}

/// Tracked minutes per weekday, Monday first.
fn weekdays(sessions: &[&Session]) -> Vec<i64> {
    let mut weekdays = vec![0_i64; 7];
    for s in sessions {
        if let Some(date) = s.date {
            if let Some(slot) = weekdays.get_mut(date.weekday().num_days_from_monday() as usize) {
                *slot += s.seconds;
            }
        }
    }
    for minutes in &mut weekdays {
        *minutes /= 60;
    }

    weekdays
}

/// The twelve months shown: the year's, or the last twelve up to `today`.
fn months(sessions: &[&Session], year: Option<i32>, today: NaiveDate) -> Vec<StatsMonth> {
    let (mut y, mut m) = if let Some(y) = year {
        (y, 1)
    } else {
        let back = i32::try_from(today.month()).unwrap_or(1) - 11;
        if back >= 1 {
            (today.year(), back)
        } else {
            (today.year() - 1, back + 12)
        }
    };
    let mut out = Vec::with_capacity(12);
    for _ in 0..12 {
        let key = format!("{y:04}-{m:02}");
        let seconds: i64 = sessions
            .iter()
            .filter(|s| s.started_at.starts_with(&key))
            .map(|s| s.seconds)
            .sum();
        out.push(StatsMonth {
            month: key,
            minutes: seconds / 60,
        });
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    out
}

fn longest_streak(days: &BTreeSet<NaiveDate>) -> i64 {
    let mut best = 0;
    let mut current = 0;
    let mut previous: Option<NaiveDate> = None;
    for day in days {
        current = match previous {
            Some(p) if *day - p == Duration::days(1) => current + 1,
            _ => 1,
        };
        best = best.max(current);
        previous = Some(*day);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::add_manual;
    use crate::db::open_in_memory;
    use crate::db::sessions;
    use crate::models::ManualGameInput;

    async fn game(pool: &SqlitePool, title: &str, genres: &str) -> String {
        let input = ManualGameInput {
            title: title.into(),
            executable_path: format!("/games/{title}"),
            size_bytes: None,
        };
        let id = add_manual(pool, &input).await.unwrap().game.id;
        sqlx::query("UPDATE games SET genres = ? WHERE id = ?")
            .bind(genres)
            .bind(&id)
            .execute(pool)
            .await
            .unwrap();
        id
    }

    async fn play(pool: &SqlitePool, id: &str, start: &str, minutes: i64) {
        let session = sessions::start(pool, id, start).await.unwrap();
        sessions::update(pool, session, start, minutes * 60)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_year_counts_its_sessions_and_all_time_the_store_playtime_too() {
        let pool = open_in_memory().await.unwrap();
        let orbit = game(&pool, "Orbit", r#"["puzzle"]"#).await;
        let tide = game(&pool, "Tide", r#"["adventure","rpg"]"#).await;
        // Imported store playtime without dates.
        sqlx::query("UPDATE library_entries SET playtime_offset_minutes = 600 WHERE game_id = ?")
            .bind(&tide)
            .execute(&pool)
            .await
            .unwrap();
        play(&pool, &orbit, "2026-03-02T20:00:00Z", 60).await; // Monday
        play(&pool, &orbit, "2026-03-03T20:00:00Z", 30).await; // Tuesday
        play(&pool, &orbit, "2026-03-04T20:00:00Z", 45).await;
        play(&pool, &tide, "2026-03-10T18:00:00Z", 120).await;
        play(&pool, &tide, "2025-12-31T18:00:00Z", 90).await;
        let today = NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();

        let year = overview(&pool, Some(2026), today).await.unwrap();
        assert_eq!(year.years, [2026, 2025]);
        assert_eq!(year.total_minutes, 255);
        assert_eq!(year.session_count, 4);
        assert_eq!(year.games_played, 2);
        assert_eq!(year.days_played, 4);
        assert_eq!(year.longest_streak_days, 3);
        assert_eq!(year.longest_session.as_ref().unwrap().minutes, 120);
        assert_eq!(year.months.len(), 12);
        assert_eq!(year.months[0].month, "2026-01");
        assert_eq!(year.months[2].minutes, 255);
        assert_eq!(year.weekdays[0], 60);
        assert_eq!(year.weekdays[1], 30 + 120);
        let top: Vec<(&str, i64, i64)> = year
            .top_games
            .iter()
            .map(|g| (g.game.title.as_str(), g.minutes, g.session_count))
            .collect();
        assert_eq!(top, [("Orbit", 135, 3), ("Tide", 120, 1)]);
        assert_eq!(year.genres[0].genre, "puzzle");
        assert_eq!(year.stores[0].minutes, 255);

        let all = overview(&pool, None, today).await.unwrap();
        assert_eq!(all.total_minutes, 135 + 600 + 210);
        assert_eq!(all.session_count, 5);
        assert_eq!(all.months[0].month, "2025-04");
        assert_eq!(all.months[8].minutes, 90);
        assert_eq!(all.top_games[0].game.title, "Tide");
    }

    #[tokio::test]
    async fn an_empty_library_has_empty_statistics() {
        let pool = open_in_memory().await.unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let stats = overview(&pool, None, today).await.unwrap();
        assert_eq!(stats.total_minutes, 0);
        assert_eq!(stats.years, Vec::<i32>::new());
        assert!(stats.longest_session.is_none());
        assert_eq!(stats.months.last().unwrap().month, "2026-11");
        assert_eq!(stats.weekdays, [0; 7]);
    }
}
