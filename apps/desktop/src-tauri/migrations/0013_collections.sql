-- The user's own collections ("Mit Freunden", "Angefangen" ...). A game can
-- be in several. Rows point at games, not at library entries: like playtime,
-- a game that leaves the library and comes back is in its collections again.
CREATE TABLE collections (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE collection_games (
    collection_id INTEGER NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
    game_id       TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    added_at      TEXT NOT NULL,
    PRIMARY KEY (collection_id, game_id)
);

CREATE INDEX idx_collection_games_game ON collection_games (game_id);
