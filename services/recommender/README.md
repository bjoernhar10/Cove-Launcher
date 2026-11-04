# cove-recommender

Local recommendation engine for Cove (Python 3.12, numpy, scikit-learn). Runs as a Tauri sidecar and speaks
JSON-RPC 2.0 over stdio (one JSON message per line; logs go to stderr). Nothing leaves the machine: the core
sends library and catalogue data with every request, the sidecar keeps no state.

## Methods

| Method | Params | Result |
|---|---|---|
| `recommend_for_user` | `library`, `catalog`, `limit?`, `now?`, `candidateIds?` | `{ items: [{ gameId, score, reason }] }` |
| `discover_new` | `library`, `catalog`, `limit?`, `now?` | `{ items }` |
| `similar_games` | `gameId`, `catalog`, `limit?` | `{ items }` |
| `explain_recommendation` | `gameId`, `library`, `catalog`, `now?` | `{ reason }` |
| `system.ping` | `message?` | `{ service, version, message }` |

`catalog` entries are `RecommenderGame`, `library` entries `RecommenderLibraryGame` (see `packages/shared`).
Errors: `-32602` invalid params, `1001` unknown game id.

## Algorithm (hybrid)

1. **Content features** per game: one-hot blocks for genres, themes, game modes, perspectives, developer and
   release era, TF-IDF over keywords. Each block is normalised and weighted, then cosine similarity is used.
2. **User profile**: weighted mean of the library games' vectors. Weight = log-scaled playtime × recency decay
   (half-life 120 days) + achievement progress + favourite bonus. Games started once, played under 30 minutes
   and untouched for two weeks count slightly negative. Owned but unplayed games count a little.
3. **Prior**: rating and popularity from the catalogue (`score = 0.75 × similarity + 0.25 × prior`), so obscure
   low-rated games do not win on features alone. Without a library the prior alone ranks (cold start).
4. **Diversity**: Maximal Marginal Relevance; redundancy is `similarity⁴`, so near-duplicates are pushed down
   while related games stay.
5. **Discover**: only genres the user has not played yet, ranked by closeness to the profile.
6. **Reasons**: the library game contributing most (and at least 0.3 similar) is named, e.g.
   "Weil du 40 Std. Hollow Knight gespielt hast"; otherwise "Sehr gut bewertet" / "Beliebt bei vielen Spielern".

## Development

```sh
uv sync
uv run pytest            # plausibility tests on a small fake catalogue
uv run ruff check . && uv run ruff format --check . && uv run mypy
echo '{"jsonrpc":"2.0","id":1,"method":"system.ping","params":{}}' | uv run cove-recommender
```

## Sidecar binary

```sh
npm run build:sidecar -w @cove/desktop   # PyInstaller onefile → apps/desktop/src-tauri/binaries/
```

The build is skipped when sources, `pyproject.toml` and `uv.lock` are unchanged. `npm run tauri:build` runs it
automatically. Without a built binary, `build.rs` puts a small placeholder there so the app still builds; Cove
then reports recommendations as unavailable (debug builds fall back to `uv run` when uv is installed).
