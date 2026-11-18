-- Save sync through the user's cloud: per game, what this computer last
-- matched in the cloud (the cloud manifest's revision) and how its own saves
-- looked then (fingerprint of path, size and time). A conflict holds the
-- cloud's side as JSON until the user chooses.
CREATE TABLE save_sync (
    game_id     TEXT PRIMARY KEY REFERENCES games (id) ON DELETE CASCADE,
    revision    TEXT,
    fingerprint TEXT,
    synced_at   TEXT,
    conflict    TEXT
);
