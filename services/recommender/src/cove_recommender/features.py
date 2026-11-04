"""Content-based feature vectors (TF-IDF + one-hot blocks, cosine similarity)."""

from __future__ import annotations

from collections.abc import Callable, Sequence

import numpy as np
from numpy.typing import NDArray
from sklearn.feature_extraction.text import TfidfVectorizer  # type: ignore[import-untyped]
from sklearn.preprocessing import MultiLabelBinarizer, normalize  # type: ignore[import-untyped]

from cove_recommender.data import CatalogGame

type Matrix = NDArray[np.float64]

#: Relative weight of each feature block (after per-block L2 normalisation).
BLOCK_WEIGHTS: dict[str, float] = {
    "genres": 1.0,
    "themes": 0.7,
    "keywords": 0.8,
    "game_modes": 0.35,
    "perspectives": 0.35,
    "developer": 0.5,
    "era": 0.3,
}


def _era(year: int | None) -> tuple[str, ...]:
    """Five-year bucket plus its neighbours, so 2014 and 2016 still overlap."""
    if year is None:
        return ()
    bucket = year // 5 * 5
    return (f"era:{bucket}", f"near:{bucket - 5}", f"near:{bucket + 5}", f"near:{bucket}")


def _identity(tokens: Sequence[str]) -> Sequence[str]:
    return tokens


def _one_hot(values: list[tuple[str, ...]]) -> Matrix:
    if not any(values):
        return np.zeros((len(values), 0))
    binarizer = MultiLabelBinarizer(sparse_output=False)
    return np.asarray(binarizer.fit_transform(values), dtype=np.float64)


def _tfidf(values: list[tuple[str, ...]]) -> Matrix:
    if not any(values):
        return np.zeros((len(values), 0))
    vectorizer = TfidfVectorizer(analyzer=_identity, lowercase=False, sublinear_tf=True)
    lowered = [tuple(v.lower() for v in vs) for vs in values]
    return np.asarray(vectorizer.fit_transform(lowered).toarray(), dtype=np.float64)


class FeatureSpace:
    """Row-normalised feature matrix for a set of catalogue games."""

    def __init__(self, games: Sequence[CatalogGame]) -> None:
        self.games: list[CatalogGame] = list(games)
        self.index: dict[str, int] = {g.id: i for i, g in enumerate(self.games)}
        builders: dict[str, Callable[[], Matrix]] = {
            "genres": lambda: _one_hot([g.genres for g in self.games]),
            "themes": lambda: _one_hot([tuple(t.lower() for t in g.themes) for g in self.games]),
            "keywords": lambda: _tfidf([g.keywords for g in self.games]),
            "game_modes": lambda: _one_hot(
                [tuple(m.lower() for m in g.game_modes) for g in self.games]
            ),
            "perspectives": lambda: _one_hot(
                [tuple(p.lower() for p in g.perspectives) for g in self.games]
            ),
            "developer": lambda: _one_hot(
                [(g.developer.lower(),) if g.developer else () for g in self.games]
            ),
            "era": lambda: _one_hot([_era(g.release_year) for g in self.games]),
        }
        blocks = []
        for name, build in builders.items():
            block = build()
            if block.shape[1] > 0:
                blocks.append(normalize(block) * BLOCK_WEIGHTS[name])
        stacked = np.hstack(blocks) if blocks else np.zeros((len(self.games), 1))
        self.matrix: Matrix = np.asarray(normalize(stacked), dtype=np.float64)

    def __len__(self) -> int:
        return len(self.games)

    def vector(self, game_id: str) -> Matrix | None:
        i = self.index.get(game_id)
        return None if i is None else self.matrix[i]

    def similarities(self, vector: Matrix) -> Matrix:
        """Cosine similarity of `vector` with every game (rows are unit length)."""
        norm = float(np.linalg.norm(vector))
        if norm == 0.0:
            return np.zeros(len(self.games))
        return np.asarray(self.matrix @ (vector / norm), dtype=np.float64)
