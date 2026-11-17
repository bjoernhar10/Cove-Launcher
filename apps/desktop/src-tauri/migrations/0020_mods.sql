-- Mods of a game: unpacked into Cove's folder, copied into the game folder
-- while switched on (later positions win for the same file).
CREATE TABLE mods (
    id         TEXT PRIMARY KEY,
    game_id    TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    -- File or folder name it came from.
    source     TEXT NOT NULL,
    -- Folder inside the game folder ('' = the game folder itself).
    target     TEXT NOT NULL DEFAULT '',
    enabled    INTEGER NOT NULL DEFAULT 1,
    position   INTEGER NOT NULL,
    files      INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    added_at   TEXT NOT NULL
);

CREATE INDEX idx_mods_game ON mods (game_id, position);

-- Files Cove copied into a game folder, as it wrote them (size, modification
-- time in ns): only unchanged files are taken out again.
CREATE TABLE mod_files (
    game_id  TEXT NOT NULL,
    path     TEXT NOT NULL,
    mod_id   TEXT NOT NULL,
    size     INTEGER NOT NULL,
    modified INTEGER NOT NULL,
    PRIMARY KEY (game_id, path)
);

-- Folders Cove created for mod files (removed again when empty).
CREATE TABLE mod_dirs (
    game_id TEXT NOT NULL,
    path    TEXT NOT NULL,
    PRIMARY KEY (game_id, path)
);
