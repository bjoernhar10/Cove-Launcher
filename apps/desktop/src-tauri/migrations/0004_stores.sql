-- Step 5: store logins, achievement icons, Cove's own achievements.
-- Tokens are never stored here (OS keyring only).

-- Login expired and the refresh failed: the UI shows "Erneut anmelden".
ALTER TABLE sources ADD COLUMN needs_reauth INTEGER NOT NULL DEFAULT 0;
-- Last store problem as JSON ({ kind, message, helpUrl }), e.g. a private Steam profile.
ALTER TABLE sources ADD COLUMN problem TEXT;

ALTER TABLE achievements ADD COLUMN icon_url TEXT;
ALTER TABLE achievements ADD COLUMN icon_locked_url TEXT;

-- Last time played according to the store (Cove's own sessions are in play_sessions).
ALTER TABLE library_entries ADD COLUMN store_last_played_at TEXT;

CREATE UNIQUE INDEX idx_library_entries_store
    ON library_entries (source_id, store_game_id)
    WHERE store_game_id IS NOT NULL;

-- Cove achievements (definitions are JSON: bundled + plugins); only unlocks are stored.
CREATE TABLE cove_achievements (
    id          TEXT PRIMARY KEY,
    unlocked_at TEXT NOT NULL
);

-- Games the recommender showed, for the "Entdecker" achievement.
CREATE TABLE recommendation_log (
    game_id        TEXT PRIMARY KEY,
    recommended_at TEXT NOT NULL
);
