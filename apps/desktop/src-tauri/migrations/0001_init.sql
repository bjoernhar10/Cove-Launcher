-- Cove library database. Timestamps are ISO-8601 UTC strings, JSON columns hold arrays/objects.

CREATE TABLE sources (
    id             TEXT PRIMARY KEY CHECK (id IN ('steam', 'gog', 'epic', 'manual')),
    connected      INTEGER NOT NULL DEFAULT 0,
    account_name   TEXT,
    last_synced_at TEXT
);

INSERT INTO sources (id) VALUES ('steam'), ('gog'), ('epic'), ('manual');

CREATE TABLE games (
    id           TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    summary      TEXT NOT NULL DEFAULT '',
    description  TEXT NOT NULL DEFAULT '',
    genres       TEXT NOT NULL DEFAULT '[]',
    platforms    TEXT NOT NULL DEFAULT '[]',
    release_date TEXT NOT NULL,
    developer    TEXT NOT NULL DEFAULT '',
    publisher    TEXT NOT NULL DEFAULT '',
    rating       INTEGER,
    popularity   INTEGER NOT NULL DEFAULT 0,
    cover_url    TEXT,
    header_url   TEXT,
    screenshots  TEXT NOT NULL DEFAULT '[]',
    created_at   TEXT NOT NULL
);

CREATE TABLE library_entries (
    game_id                 TEXT PRIMARY KEY REFERENCES games (id) ON DELETE CASCADE,
    source_id               TEXT NOT NULL REFERENCES sources (id),
    -- Store-specific id, e.g. the Steam app id (used for URL-scheme launches)
    store_game_id           TEXT,
    installed               INTEGER NOT NULL DEFAULT 0,
    install_path            TEXT,
    executable_path         TEXT,
    install_size_bytes      INTEGER,
    favorite                INTEGER NOT NULL DEFAULT 0,
    -- Playtime imported from stores (sessions tracked by Cove are added on top)
    playtime_offset_minutes INTEGER NOT NULL DEFAULT 0,
    launch_arguments        TEXT NOT NULL DEFAULT '',
    compatibility_tool      TEXT NOT NULL DEFAULT 'native'
        CHECK (compatibility_tool IN ('native', 'proton', 'proton-experimental', 'wine')),
    env                     TEXT NOT NULL DEFAULT '[]',
    added_at                TEXT NOT NULL
);

CREATE INDEX idx_library_entries_executable ON library_entries (executable_path);

CREATE TABLE play_sessions (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id          TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    started_at       TEXT NOT NULL,
    -- Updated by a heartbeat while running, so a crash loses at most one interval
    ended_at         TEXT,
    duration_seconds INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_play_sessions_game ON play_sessions (game_id, started_at);

CREATE TABLE achievements (
    game_id        TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    id             TEXT NOT NULL,
    title          TEXT NOT NULL,
    description    TEXT NOT NULL DEFAULT '',
    hidden         INTEGER NOT NULL DEFAULT 0,
    rarity_percent REAL,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (game_id, id)
);

CREATE TABLE user_achievements (
    game_id        TEXT NOT NULL,
    achievement_id TEXT NOT NULL,
    unlocked_at    TEXT NOT NULL,
    PRIMARY KEY (game_id, achievement_id),
    FOREIGN KEY (game_id, achievement_id) REFERENCES achievements (game_id, id) ON DELETE CASCADE
);

CREATE TABLE downloads (
    id               TEXT PRIMARY KEY,
    game_id          TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    source_id        TEXT NOT NULL REFERENCES sources (id),
    status           TEXT NOT NULL
        CHECK (status IN ('downloading', 'paused', 'queued', 'completed', 'failed', 'cancelled')),
    total_bytes      INTEGER NOT NULL DEFAULT 0,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    queue_position   INTEGER,
    queued_at        TEXT NOT NULL,
    finished_at      TEXT,
    error            TEXT
);

-- Key/value store; values are JSON documents (one per settings section).
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
