-- Amazon Games as a store. SQLite can't change the CHECK of `sources`, so the
-- table is rebuilt. Other tables point at it: their foreign key checks wait
-- until the end of the migration, when every store row is back.
PRAGMA defer_foreign_keys = ON;
CREATE TEMP TABLE sources_old AS SELECT * FROM sources;
DROP TABLE sources;
CREATE TABLE sources (
    id             TEXT PRIMARY KEY CHECK (id IN ('steam', 'gog', 'epic', 'amazon', 'manual')),
    connected      INTEGER NOT NULL DEFAULT 0,
    account_name   TEXT,
    last_synced_at TEXT,
    needs_reauth   INTEGER NOT NULL DEFAULT 0,
    problem        TEXT
);
INSERT INTO sources (id, connected, account_name, last_synced_at, needs_reauth, problem)
    SELECT id, connected, account_name, last_synced_at, needs_reauth, problem FROM sources_old;
INSERT INTO sources (id) VALUES ('amazon');
DROP TABLE sources_old;

-- Downloads may come from Amazon too (same CHECK problem, nothing points at downloads).
CREATE TABLE downloads_new (
    id                    TEXT PRIMARY KEY,
    game_id               TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    source_id             TEXT NOT NULL REFERENCES sources (id),
    origin                TEXT NOT NULL DEFAULT 'direct'
        CHECK (origin IN ('direct', 'itch', 'steam', 'gog', 'epic', 'amazon')),
    status                TEXT NOT NULL
        CHECK (status IN ('downloading', 'paused', 'queued', 'verifying', 'extracting',
                          'completed', 'failed', 'cancelled', 'handedOff')),
    total_bytes           INTEGER NOT NULL DEFAULT 0,
    downloaded_bytes      INTEGER NOT NULL DEFAULT 0,
    queue_position        INTEGER,
    queued_at             TEXT NOT NULL,
    finished_at           TEXT,
    error                 TEXT,
    url                   TEXT,
    file_name             TEXT,
    checksum_sha256       TEXT,
    checksum_verified     INTEGER NOT NULL DEFAULT 0,
    install_path          TEXT,
    executable_path       TEXT,
    etag                  TEXT,
    chunk_size            INTEGER NOT NULL DEFAULT 8388608,
    chunks_done           TEXT NOT NULL DEFAULT '[]',
    target_dir            TEXT,
    itch_game_id          INTEGER,
    itch_upload_id        INTEGER,
    itch_download_key_id  INTEGER,
    extra                 TEXT
);
INSERT INTO downloads_new SELECT id, game_id, source_id, origin, status, total_bytes, downloaded_bytes,
    queue_position, queued_at, finished_at, error, url, file_name, checksum_sha256, checksum_verified,
    install_path, executable_path, etag, chunk_size, chunks_done, target_dir, itch_game_id,
    itch_upload_id, itch_download_key_id, extra FROM downloads;
DROP TABLE downloads;
ALTER TABLE downloads_new RENAME TO downloads;
CREATE INDEX idx_downloads_queue ON downloads (queue_position);
