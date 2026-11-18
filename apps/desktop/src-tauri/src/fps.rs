//! Frame rate history (Linux): with the game option "FPS aufzeichnen",
//! `MangoHud`'s logger writes a CSV per session into
//! `<app data>/fps-logs/<game>` (see `launcher::apply_tweaks`). After the
//! session Cove reads it, keeps a summary (average, 1 % and 0.1 % lows) and a
//! small curve per session in `fps_logs`, and deletes the CSV files.

use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

use crate::error::AppResult;
use crate::models::FpsLog;

/// Sessions kept per game.
const KEEP: i64 = 50;
/// Points of the stored curve.
const CURVE_POINTS: usize = 240;
/// Fewer samples (at 100 ms each) are a launcher or a crash, not a session.
const MIN_SAMPLES: usize = 20;

pub struct FpsLogs {
    pool: SqlitePool,
    root: PathBuf,
}

/// What one `MangoHud` log holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub fps: Vec<f64>,
    pub duration_seconds: i64,
}

/// Reads a `MangoHud` CSV log: system lines, then the header of the frame
/// metrics (`fps,frametime,…,elapsed`) and one row per interval.
pub fn parse(text: &str) -> Option<Parsed> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines
        .by_ref()
        .map(|l| l.split(',').map(str::trim).collect::<Vec<_>>())
        .find(|cols| cols.first() == Some(&"fps"))?;
    let elapsed_at = header.iter().position(|c| *c == "elapsed");
    let mut fps = Vec::new();
    let (mut first, mut last) = (None::<f64>, None::<f64>);
    for line in lines {
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        let Some(value) = cols.first().and_then(|v| v.parse::<f64>().ok()) else {
            continue;
        };
        if !value.is_finite() || value <= 0.0 {
            continue;
        }
        fps.push(value);
        if let Some(at) = elapsed_at
            .and_then(|i| cols.get(i))
            .and_then(|v| v.parse::<f64>().ok())
        {
            first.get_or_insert(at);
            last = Some(at);
        }
    }
    if fps.len() < MIN_SAMPLES {
        return None;
    }
    // `elapsed` is in nanoseconds; without it every row is 100 ms.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let duration_seconds = match (first, last) {
        (Some(a), Some(b)) if b > a => ((b - a) / 1e9).round() as i64,
        _ => (fps.len() as f64 / 10.0).round() as i64,
    };
    Some(Parsed {
        fps,
        duration_seconds,
    })
}

/// The frame rate `share` of the samples were at or below (nearest rank).
fn percentile(sorted: &[f64], share: f64) -> f64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = (share * sorted.len() as f64).ceil() as usize;
    sorted
        .get(rank.saturating_sub(1).min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Average of each of at most `points` equal slices.
fn curve(fps: &[f64], points: usize) -> Vec<f64> {
    if fps.len() <= points {
        return fps.iter().copied().map(round1).collect();
    }
    #[allow(clippy::cast_precision_loss)]
    (0..points)
        .map(|i| {
            let from = i * fps.len() / points;
            let to = ((i + 1) * fps.len() / points).max(from + 1);
            let slice = &fps[from..to];
            round1(slice.iter().sum::<f64>() / slice.len() as f64)
        })
        .collect()
}

/// Summary of a parsed log: (average, 1 % low, 0.1 % low, curve).
pub fn summarize(parsed: &Parsed) -> (f64, f64, f64, Vec<f64>) {
    let mut sorted = parsed.fps.clone();
    sorted.sort_by(f64::total_cmp);
    #[allow(clippy::cast_precision_loss)]
    let average = sorted.iter().sum::<f64>() / sorted.len() as f64;
    (
        round1(average),
        round1(percentile(&sorted, 0.01)),
        round1(percentile(&sorted, 0.001)),
        curve(&parsed.fps, CURVE_POINTS),
    )
}

fn csv_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv")))
        .collect()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !id.starts_with('.')
}

impl FpsLogs {
    pub fn new(pool: SqlitePool, root: PathBuf) -> Self {
        Self { pool, root }
    }

    /// The game's empty log folder for a launch; `None` when it can't be made.
    pub fn prepare(&self, game_id: &str) -> Option<PathBuf> {
        if !valid_id(game_id) {
            return None;
        }
        let dir = self.root.join(game_id);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("[fps] Ordner für {game_id}: {error}");
            return None;
        }
        // Leftovers of a session Cove didn't see end (Cove closed meanwhile).
        for file in csv_files(&dir) {
            let _ = std::fs::remove_file(file);
        }
        Some(dir)
    }

    /// After a session: keeps the longest log of the game's folder (Proton
    /// may log a launcher too) and deletes the files.
    pub async fn collect(&self, game_id: &str) -> AppResult<Option<FpsLog>> {
        if !valid_id(game_id) {
            return Ok(None);
        }
        let dir = self.root.join(game_id);
        let parsed = tokio::task::spawn_blocking(move || {
            let files = csv_files(&dir);
            let best = files
                .iter()
                .filter(|p| {
                    !p.file_stem()
                        .is_some_and(|s| s.to_string_lossy().ends_with("_summary"))
                })
                .filter_map(|p| parse(&std::fs::read_to_string(p).ok()?))
                .max_by_key(|p| p.fps.len());
            for file in files {
                let _ = std::fs::remove_file(file);
            }
            best
        })
        .await
        .ok()
        .flatten();
        let Some(parsed) = parsed else {
            return Ok(None);
        };
        let (average, one_percent, tenth_percent, samples) = summarize(&parsed);
        let started_at: Option<String> = sqlx::query_scalar(
            "SELECT started_at FROM play_sessions WHERE game_id = ? ORDER BY started_at DESC LIMIT 1",
        )
        .bind(game_id)
        .fetch_optional(&self.pool)
        .await?;
        let started_at = started_at.unwrap_or_else(|| {
            (chrono::Utc::now() - chrono::Duration::seconds(parsed.duration_seconds)).to_rfc3339()
        });
        let id = sqlx::query(
            "INSERT INTO fps_logs (game_id, started_at, duration_seconds, average_fps, low1_fps, low01_fps, samples) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(game_id)
        .bind(&started_at)
        .bind(parsed.duration_seconds)
        .bind(average)
        .bind(one_percent)
        .bind(tenth_percent)
        .bind(serde_json::to_string(&samples)?)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();
        sqlx::query(
            "DELETE FROM fps_logs WHERE game_id = ? AND id NOT IN \
             (SELECT id FROM fps_logs WHERE game_id = ? ORDER BY started_at DESC, id DESC LIMIT ?)",
        )
        .bind(game_id)
        .bind(game_id)
        .bind(KEEP)
        .execute(&self.pool)
        .await?;
        Ok(Some(FpsLog {
            id,
            game_id: game_id.to_owned(),
            started_at,
            duration_seconds: parsed.duration_seconds,
            average_fps: average,
            low1_fps: one_percent,
            low01_fps: tenth_percent,
            samples,
        }))
    }

    /// The game's recorded sessions, newest first.
    pub async fn list(&self, game_id: &str) -> AppResult<Vec<FpsLog>> {
        let rows = sqlx::query(
            "SELECT id, game_id, started_at, duration_seconds, average_fps, low1_fps, low01_fps, samples \
             FROM fps_logs WHERE game_id = ? ORDER BY started_at DESC, id DESC",
        )
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| FpsLog {
                id: row.get("id"),
                game_id: row.get("game_id"),
                started_at: row.get("started_at"),
                duration_seconds: row.get("duration_seconds"),
                average_fps: row.get("average_fps"),
                low1_fps: row.get("low1_fps"),
                low01_fps: row.get("low01_fps"),
                samples: serde_json::from_str(row.get::<&str, _>("samples")).unwrap_or_default(),
            })
            .collect())
    }

    /// Forgets the game's recorded sessions.
    pub async fn clear(&self, game_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM fps_logs WHERE game_id = ?")
            .bind(game_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::{insert_store_game, tests::sample_game};
    use crate::db::open_in_memory;
    use crate::models::StoreSource;
    use std::fmt::Write as _;

    /// A log like `MangoHud` 0.7 writes: `rows` samples 100 ms apart.
    fn log(rows: &[f64]) -> String {
        let mut text = String::from(
            "os,cpu,gpu,ram,kernel,driver,cpuscheduler\n\
             Fedora,AMD Ryzen,Radeon,32GB,6.12,Mesa,none\n\
             --------------------FRAME METRICS--------------------\n\
             fps,frametime,cpu_load,gpu_load,cpu_temp,gpu_temp,elapsed\n",
        );
        for (i, fps) in rows.iter().enumerate() {
            writeln!(
                text,
                "{fps},{:.3},20,90,60,70,{}",
                1000.0 / fps,
                (i as u64 + 1) * 100_000_000
            )
            .unwrap();
        }
        text
    }

    #[test]
    fn logs_are_summarized_with_lows_and_a_short_curve() {
        let mut rows = vec![60.0; 990];
        rows.extend(vec![20.0; 10]);
        let parsed = parse(&log(&rows)).unwrap();
        assert_eq!(parsed.fps.len(), 1000);
        assert_eq!(parsed.duration_seconds, 100);
        let (average, one_percent, tenth_percent, samples) = summarize(&parsed);
        assert_eq!(average, 59.6);
        assert_eq!(one_percent, 20.0);
        assert_eq!(tenth_percent, 20.0);
        assert_eq!(samples.len(), CURVE_POINTS);
        assert_eq!(samples[0], 60.0);
        assert!(samples.last().unwrap() < &60.0);

        // A launcher's few frames, or no frame metrics at all, are no session.
        assert!(parse(&log(&[60.0; 5])).is_none());
        assert!(parse("os,cpu\nFedora,AMD\n").is_none());
    }

    #[tokio::test]
    async fn sessions_are_collected_and_the_files_removed() {
        let pool = open_in_memory().await.unwrap();
        insert_store_game(
            &pool,
            &sample_game("g1", "Ember"),
            StoreSource::Manual,
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO play_sessions (game_id, started_at, duration_seconds) VALUES ('g1', '2026-11-18T20:00:00Z', 60)")
            .execute(&pool)
            .await
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let logs = FpsLogs::new(pool, tmp.path().to_path_buf());
        assert!(logs.prepare("../x").is_none());
        let dir = logs.prepare("g1").unwrap();
        std::fs::write(
            dir.join("launcher_2026-11-18_20-00-00.csv"),
            log(&[30.0; 30]),
        )
        .unwrap();
        std::fs::write(
            dir.join("Ember.exe_2026-11-18_20-00-01.csv"),
            log(&[90.0; 600]),
        )
        .unwrap();
        std::fs::write(dir.join("Ember.exe_2026-11-18_20-00-01_summary.csv"), "x").unwrap();

        let saved = logs.collect("g1").await.unwrap().unwrap();
        assert_eq!(saved.average_fps, 90.0);
        assert_eq!(saved.started_at, "2026-11-18T20:00:00Z");
        assert_eq!(saved.duration_seconds, 60);
        assert_eq!(csv_files(&dir), Vec::<PathBuf>::new());
        // Nothing new: nothing stored.
        assert!(logs.collect("g1").await.unwrap().is_none());

        let list = logs.list("g1").await.unwrap();
        assert_eq!(list, [saved]);
        logs.clear("g1").await.unwrap();
        assert_eq!(logs.list("g1").await.unwrap(), []);
    }
}
