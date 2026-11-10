-- Store games Cove installed itself (GOG now, Epic later): which build is on
-- disk, for update checks, and which files it installed, so an update can
-- remove files the new build no longer has.
CREATE TABLE store_installs (
    game_id      TEXT PRIMARY KEY REFERENCES games (id) ON DELETE CASCADE,
    store        TEXT NOT NULL REFERENCES sources (id),
    product_id   TEXT NOT NULL,
    build_id     TEXT NOT NULL,
    version      TEXT NOT NULL DEFAULT '',
    -- windows, linux or osx: what the installed files are built for
    platform     TEXT NOT NULL,
    -- JSON array of relative paths
    files        TEXT NOT NULL DEFAULT '[]',
    -- Store-specific start data (Epic: app name, namespace, launch command, save folder)
    meta         TEXT NOT NULL DEFAULT '{}',
    installed_at TEXT NOT NULL
);
