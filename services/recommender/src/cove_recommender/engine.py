"""Hybrid recommendations: content similarity to a weighted user profile,
a popularity/rating prior and MMR diversification.

All functions are pure; the core sends library + catalogue data with every
request, nothing is stored and nothing leaves the machine.
"""

from __future__ import annotations

import math
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import datetime

import numpy as np

from cove_recommender.data import CatalogGame, LibraryGame
from cove_recommender.features import FeatureSpace, Matrix
from cove_recommender.texts import genre_label, hours, t

#: Half-life of the recency decay.
RECENCY_HALF_LIFE_DAYS = 120.0
#: A game started once, played < 30 min and not touched for this long counts as "abandoned".
ABANDONED_MAX_MINUTES = 30
ABANDONED_AFTER_DAYS = 14
ABANDONED_WEIGHT = -0.35
#: Owned but never started: mild interest.
UNPLAYED_WEIGHT = 0.2

#: score = SIMILARITY_WEIGHT * similarity + PRIOR_WEIGHT * prior
SIMILARITY_WEIGHT = 0.75
PRIOR_WEIGHT = 0.25
#: MMR trade-off between relevance (1.0) and diversity (0.0).
MMR_LAMBDA = 0.7
#: Redundancy = similarity ** this. Near-duplicates (≈1) are penalised fully,
#: related-but-different games (≈0.8 → 0.4) only mildly.
REDUNDANCY_EXPONENT = 4
#: A library game is only named as the reason if it is at least this similar.
EXPLAIN_MIN_SIMILARITY = 0.3
#: "Discover" only suggests games at least this close to the profile (if any are).
DISCOVER_MIN_SIMILARITY = 0.12


@dataclass(frozen=True)
class Recommendation:
    game_id: str
    score: float
    reason: str


# ───────────────────────── user profile ─────────────────────────


def is_abandoned(game: LibraryGame, now: datetime) -> bool:
    """Started, played < 30 min, never again (at most one session) and left alone for a while."""
    if (
        not 0 < game.playtime_minutes < ABANDONED_MAX_MINUTES
        or game.session_count > 1
        or game.favorite
    ):
        return False
    if game.last_played_at is None:
        return False
    return (now - game.last_played_at).days >= ABANDONED_AFTER_DAYS


def interaction_weight(game: LibraryGame, now: datetime) -> float:
    """How much a library game says about the user's taste (negative = dislike)."""
    if is_abandoned(game, now):
        return ABANDONED_WEIGHT
    hours = game.playtime_minutes / 60
    weight = math.log1p(hours)  # 1 h → 0.69, 10 h → 2.4, 100 h → 4.6
    if game.last_played_at is not None and weight > 0:
        days = max(0.0, (now - game.last_played_at).total_seconds() / 86_400)
        recency = 0.5 ** (days / RECENCY_HALF_LIFE_DAYS)
        weight *= 0.4 + 0.6 * recency
    if game.achievements_total > 0:
        weight += 0.8 * game.achievements_unlocked / game.achievements_total
    if game.favorite:
        weight += 1.5
    if weight == 0:
        weight = UNPLAYED_WEIGHT
    return weight


@dataclass(frozen=True)
class Profile:
    vector: Matrix
    #: (library game, weight, feature vector) for games present in the feature space.
    contributions: list[tuple[LibraryGame, float, Matrix]]

    @property
    def is_empty(self) -> bool:
        return float(np.linalg.norm(self.vector)) == 0.0


def build_profile(library: Sequence[LibraryGame], space: FeatureSpace, now: datetime) -> Profile:
    contributions: list[tuple[LibraryGame, float, Matrix]] = []
    total = np.zeros(space.matrix.shape[1])
    for game in library:
        vector = space.vector(game.game_id)
        if vector is None:
            continue
        weight = interaction_weight(game, now)
        contributions.append((game, weight, vector))
        total += weight * vector
    positive = sum(w for _, w, _ in contributions if w > 0)
    vector = total / positive if positive > 0 else np.zeros_like(total)
    # Negative signals must not flip the whole profile.
    return Profile(vector=np.maximum(vector, 0.0), contributions=contributions)


# ───────────────────────── scoring ─────────────────────────


def prior(game: CatalogGame) -> float:
    """Popularity/rating prior in 0..1; unknown ratings count as mediocre."""
    rating = (game.rating if game.rating is not None else 55.0) / 100
    return 0.6 * rating + 0.4 * min(game.popularity, 100.0) / 100


def mmr(
    candidates: list[int], relevance: Matrix, space: FeatureSpace, k: int, lam: float = MMR_LAMBDA
) -> list[int]:
    """Maximal Marginal Relevance: relevant but not near-duplicates of each other."""
    selected: list[int] = []
    pool = list(candidates)
    while pool and len(selected) < k:
        if selected:
            chosen = space.matrix[selected]
            similarity = np.clip((space.matrix[pool] @ chosen.T).max(axis=1), 0.0, 1.0)
            redundancy = similarity**REDUNDANCY_EXPONENT
        else:
            redundancy = np.zeros(len(pool))
        values = lam * relevance[pool] - (1 - lam) * redundancy
        best = int(np.argmax(values))
        selected.append(pool.pop(best))
    return selected


def explain(target: int, profile: Profile, space: FeatureSpace) -> str:
    """Short reason (in the UI language) naming the library game that contributes most."""
    return explain_anchor(target, profile, space)[0]


def explain_anchor(target: int, profile: Profile, space: FeatureSpace) -> tuple[str, bool]:
    """The reason and whether it names a game from the library."""
    target_vector = space.matrix[target]
    best: tuple[float, LibraryGame] | None = None
    for game, weight, vector in profile.contributions:
        if weight <= 0 or game.game_id == space.games[target].id:
            continue
        similarity = float(vector @ target_vector)
        if similarity < EXPLAIN_MIN_SIMILARITY:
            continue
        contribution = weight * similarity
        if best is None or contribution > best[0]:
            best = (contribution, game)
    if best is not None:
        game = best[1]
        if game.playtime_minutes >= 60:
            played = hours(game.playtime_minutes)
            return t(
                f"Weil du {played} Std. {game.title} gespielt hast",
                f"Because you played {game.title} for {played} h",
            ), True
        if game.favorite:
            return t(
                f"Weil {game.title} zu deinen Favoriten gehört",
                f"Because {game.title} is one of your favourites",
            ), True
        return t(f"Ähnlich wie {game.title}", f"Similar to {game.title}"), True
    candidate = space.games[target]
    if (candidate.rating or 0) >= 80:
        return t("Sehr gut bewertet", "Highly rated"), False
    return t("Beliebt bei vielen Spielern", "Popular with many players"), False


def _owned(library: Sequence[LibraryGame]) -> set[str]:
    return {g.game_id for g in library}


def recommend_for_user(
    library: Sequence[LibraryGame],
    catalog: Sequence[CatalogGame],
    limit: int,
    now: datetime,
    candidate_ids: Sequence[str] | None = None,
) -> list[Recommendation]:
    """Top games for the user (owned games excluded), diversified with MMR."""
    space = FeatureSpace(catalog)
    profile = build_profile(library, space, now)
    owned = _owned(library)
    allowed = set(candidate_ids) if candidate_ids is not None else None
    candidates = [
        i
        for i, g in enumerate(space.games)
        if g.id not in owned and (allowed is None or g.id in allowed)
    ]
    priors = np.array([prior(g) for g in space.games])
    if profile.is_empty:
        relevance = priors  # cold start: popular, well-rated games
    else:
        similarity = np.clip(space.similarities(profile.vector), 0.0, 1.0)
        relevance = SIMILARITY_WEIGHT * similarity + PRIOR_WEIGHT * priors
    ranked = sorted(candidates, key=lambda i: -relevance[i])
    chosen = mmr(ranked, relevance, space, limit)
    return [
        Recommendation(space.games[i].id, round(float(relevance[i]), 4), explain(i, profile, space))
        for i in chosen
    ]


def played_genres(
    library: Sequence[LibraryGame], catalog: Sequence[CatalogGame], now: datetime
) -> set[str]:
    by_id = {g.id: g for g in catalog}
    genres: set[str] = set()
    for game in library:
        entry = by_id.get(game.game_id)
        if entry is not None and interaction_weight(game, now) > UNPLAYED_WEIGHT:
            genres.update(entry.genres)
    return genres


def discover_new(
    library: Sequence[LibraryGame], catalog: Sequence[CatalogGame], limit: int, now: datetime
) -> list[Recommendation]:
    """Exploration: games from genres the user has not played yet, but close to their profile."""
    space = FeatureSpace(catalog)
    profile = build_profile(library, space, now)
    owned = _owned(library)
    known = played_genres(library, catalog, now)
    fresh = [
        i
        for i, g in enumerate(space.games)
        if g.id not in owned and g.genres and not set(g.genres) & known
    ]
    priors = np.array([prior(g) for g in space.games])
    if profile.is_empty:
        relevance = priors
    else:
        similarity = np.clip(space.similarities(profile.vector), 0.0, 1.0)
        close = [i for i in fresh if similarity[i] >= DISCOVER_MIN_SIMILARITY]
        fresh = close or fresh
        relevance = 0.6 * similarity + 0.4 * priors
    ranked = sorted(fresh, key=lambda i: -relevance[i])
    chosen = mmr(ranked, relevance, space, limit, lam=0.6)
    results = []
    for i in chosen:
        genre = genre_label(space.games[i].genres[0])
        anchor, names_a_game = explain_anchor(i, profile, space)
        reason = t(f"Neu für dich: {genre}", f"New for you: {genre}")
        if names_a_game and not profile.is_empty:
            reason += f". {anchor}"
        results.append(Recommendation(space.games[i].id, round(float(relevance[i]), 4), reason))
    return results


def similar_games(game_id: str, catalog: Sequence[CatalogGame], limit: int) -> list[Recommendation]:
    """Games similar to one catalogue entry."""
    space = FeatureSpace(catalog)
    target = space.index.get(game_id)
    if target is None:
        raise KeyError(game_id)
    similarity = np.clip(space.similarities(space.matrix[target]), 0.0, 1.0)
    priors = np.array([prior(g) for g in space.games])
    relevance = 0.85 * similarity + 0.15 * priors
    ranked = sorted((i for i in range(len(space)) if i != target), key=lambda i: -relevance[i])
    chosen = mmr(ranked, relevance, space, limit, lam=0.8)
    title = space.games[target].title
    return [
        Recommendation(
            space.games[i].id,
            round(float(relevance[i]), 4),
            t(f"Ähnlich wie {title}", f"Similar to {title}"),
        )
        for i in chosen
    ]


def explain_recommendation(
    game_id: str, library: Sequence[LibraryGame], catalog: Sequence[CatalogGame], now: datetime
) -> str:
    space = FeatureSpace(catalog)
    target = space.index.get(game_id)
    if target is None:
        raise KeyError(game_id)
    return explain(target, build_profile(library, space, now), space)
