-- Catalogue cache (provider responses, TTL enforced in code) and library ↔ catalogue matching.

CREATE TABLE catalog_cache (
    key        TEXT PRIMARY KEY,
    provider   TEXT NOT NULL,
    value      TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);

CREATE INDEX idx_catalog_cache_fetched ON catalog_cache (fetched_at);

-- Catalogue id the entry is linked to (e.g. "igdb-1942"); NULL = not matched.
ALTER TABLE library_entries ADD COLUMN catalog_id TEXT;
ALTER TABLE library_entries ADD COLUMN match_status TEXT NOT NULL DEFAULT 'none'
    CHECK (match_status IN ('none', 'pending', 'matched', 'rejected'));

-- Games added from the catalogue are already matched.
UPDATE library_entries SET catalog_id = game_id, match_status = 'matched'
WHERE game_id LIKE 'igdb-%' OR game_id LIKE 'rawg-%';

-- Suggestions waiting for the user's confirmation.
CREATE TABLE match_candidates (
    game_id    TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    catalog_id TEXT NOT NULL,
    score      REAL NOT NULL,
    game       TEXT NOT NULL, -- JSON of the catalogue Game
    PRIMARY KEY (game_id, catalog_id)
);
