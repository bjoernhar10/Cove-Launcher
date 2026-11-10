-- DLCs Cove installed into a store game's folder (Epic: each DLC is an app
-- with its own build). Kept apart from the game's own files so updates and
-- uninstalls stay clean.
CREATE TABLE store_install_dlcs (
    game_id      TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    app_name     TEXT NOT NULL,
    title        TEXT NOT NULL,
    build_id     TEXT NOT NULL,
    -- JSON array of relative paths
    files        TEXT NOT NULL DEFAULT '[]',
    installed_at TEXT NOT NULL,
    PRIMARY KEY (game_id, app_name)
);
