-- Step 6: real download manager. SQLite cannot change CHECK constraints, so
-- the table is rebuilt (existing rows are kept).

ALTER TABLE downloads RENAME TO downloads_old;

CREATE TABLE downloads (
    id                    TEXT PRIMARY KEY,
    game_id               TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    source_id             TEXT NOT NULL REFERENCES sources (id),
    -- Where the file comes from. Store games are handed over to their own
    -- client; direct links and itch.io are transferred by Cove.
    origin                TEXT NOT NULL DEFAULT 'direct'
        CHECK (origin IN ('direct', 'itch', 'steam', 'gog', 'epic')),
    status                TEXT NOT NULL
        CHECK (status IN ('downloading', 'paused', 'queued', 'verifying', 'extracting',
                          'completed', 'failed', 'cancelled', 'handedOff')),
    total_bytes           INTEGER NOT NULL DEFAULT 0,
    downloaded_bytes      INTEGER NOT NULL DEFAULT 0,
    queue_position        INTEGER,
    queued_at             TEXT NOT NULL,
    finished_at           TEXT,
    error                 TEXT,
    -- Direct links only. itch.io URLs are resolved fresh before every start
    -- (they expire, and the API key must never be stored here).
    url                   TEXT,
    file_name             TEXT,
    -- Expected SHA-256 (hex), if known.
    checksum_sha256       TEXT,
    checksum_verified     INTEGER NOT NULL DEFAULT 0,
    install_path          TEXT,
    executable_path       TEXT,
    -- HTTP ETag / Last-Modified: a changed file restarts the download.
    etag                  TEXT,
    chunk_size            INTEGER NOT NULL DEFAULT 8388608,
    -- Finished chunk indices (JSON array) for resuming.
    chunks_done           TEXT NOT NULL DEFAULT '[]',
    -- Fixed at enqueue time so a settings change does not break resuming.
    target_dir            TEXT,
    itch_game_id          INTEGER,
    itch_upload_id        INTEGER,
    itch_download_key_id  INTEGER
);

INSERT INTO downloads (id, game_id, source_id, origin, status, total_bytes, downloaded_bytes,
                       queue_position, queued_at, finished_at, error)
SELECT id, game_id, source_id,
       CASE WHEN source_id IN ('steam', 'gog', 'epic') THEN source_id ELSE 'direct' END,
       status, total_bytes, downloaded_bytes, queue_position, queued_at, finished_at, error
FROM downloads_old;

DROP TABLE downloads_old;

CREATE INDEX idx_downloads_queue ON downloads (queue_position);
