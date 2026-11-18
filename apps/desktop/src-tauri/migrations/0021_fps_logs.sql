-- Frame rates recorded with MangoHud's logger (Linux), one row per session:
-- the summary plus up to 240 evenly spaced frame rate points (JSON array).
CREATE TABLE fps_logs (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id          TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    started_at       TEXT NOT NULL,
    duration_seconds INTEGER NOT NULL,
    average_fps      REAL NOT NULL,
    low1_fps         REAL NOT NULL,
    low01_fps        REAL NOT NULL,
    samples          TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_fps_logs_game ON fps_logs (game_id, started_at);
