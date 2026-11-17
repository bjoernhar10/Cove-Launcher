-- Desktop and app menu shortcuts Cove created for a game (one per place),
-- so it can list and remove exactly its own files.
CREATE TABLE shortcuts (
    game_id    TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    place      TEXT NOT NULL CHECK (place IN ('desktop', 'menu')),
    path       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (game_id, place)
);
