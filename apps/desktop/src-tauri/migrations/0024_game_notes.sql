-- The user's own notes per game ("where was I, what's left"). Rows point at
-- games, so a game that leaves the library and comes back keeps its notes.
CREATE TABLE game_notes (
    game_id    TEXT PRIMARY KEY REFERENCES games (id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
