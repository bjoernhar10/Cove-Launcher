"""Input data sent by the Cove core, parsed and validated.

Shapes mirror `packages/shared/src/models.ts` (`RecommenderGame`, `RecommenderLibraryGame`).
"""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import cast

type Json = object


class InvalidParamsError(ValueError):
    """Raised for malformed request parameters (JSON-RPC -32602)."""


@dataclass(frozen=True)
class CatalogGame:
    id: str
    title: str
    genres: tuple[str, ...]
    themes: tuple[str, ...]
    game_modes: tuple[str, ...]
    perspectives: tuple[str, ...]
    keywords: tuple[str, ...]
    developer: str
    release_year: int | None
    rating: float | None
    popularity: float


@dataclass(frozen=True)
class LibraryGame:
    game_id: str
    title: str
    playtime_minutes: int
    last_played_at: datetime | None
    favorite: bool
    achievements_unlocked: int
    achievements_total: int
    session_count: int


def _obj(value: Json, where: str) -> Mapping[str, Json]:
    if not isinstance(value, dict):
        raise InvalidParamsError(f"{where} must be an object")
    return cast("Mapping[str, Json]", value)


def _list(value: Json, where: str) -> Sequence[Json]:
    if not isinstance(value, list):
        raise InvalidParamsError(f"{where} must be an array")
    return cast("Sequence[Json]", value)


def _str(obj: Mapping[str, Json], key: str, where: str, default: str | None = None) -> str:
    value = obj.get(key, default)
    if not isinstance(value, str):
        raise InvalidParamsError(f"{where}.{key} must be a string")
    return value


def _strings(obj: Mapping[str, Json], key: str, where: str) -> tuple[str, ...]:
    raw = obj.get(key, [])
    items = _list(raw, f"{where}.{key}")
    if not all(isinstance(i, str) for i in items):
        raise InvalidParamsError(f"{where}.{key} must contain strings")
    return tuple(cast("Sequence[str]", items))


def _number(
    obj: Mapping[str, Json], key: str, where: str, *, optional: bool = False
) -> float | None:
    value = obj.get(key)
    if value is None and optional:
        return None
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise InvalidParamsError(f"{where}.{key} must be a number")
    return float(value)


def parse_datetime(value: Json, where: str) -> datetime | None:
    if value is None:
        return None
    if not isinstance(value, str):
        raise InvalidParamsError(f"{where} must be an ISO date string or null")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise InvalidParamsError(f"{where} is not an ISO date") from exc
    return parsed if parsed.tzinfo else parsed.replace(tzinfo=UTC)


def parse_catalog(value: Json) -> list[CatalogGame]:
    games: list[CatalogGame] = []
    for i, raw in enumerate(_list(value, "catalog")):
        where = f"catalog[{i}]"
        obj = _obj(raw, where)
        year = _number(obj, "releaseYear", where, optional=True)
        games.append(
            CatalogGame(
                id=_str(obj, "id", where),
                title=_str(obj, "title", where),
                genres=_strings(obj, "genres", where),
                themes=_strings(obj, "themes", where),
                game_modes=_strings(obj, "gameModes", where),
                perspectives=_strings(obj, "perspectives", where),
                keywords=_strings(obj, "keywords", where),
                developer=_str(obj, "developer", where, ""),
                release_year=int(year) if year is not None else None,
                rating=_number(obj, "rating", where, optional=True),
                popularity=_number(obj, "popularity", where, optional=True) or 0.0,
            )
        )
    return games


def parse_library(value: Json) -> list[LibraryGame]:
    games: list[LibraryGame] = []
    for i, raw in enumerate(_list(value, "library")):
        where = f"library[{i}]"
        obj = _obj(raw, where)
        achievements = _obj(
            obj.get("achievements", {"unlocked": 0, "total": 0}), f"{where}.achievements"
        )
        games.append(
            LibraryGame(
                game_id=_str(obj, "gameId", where),
                title=_str(obj, "title", where),
                playtime_minutes=int(_number(obj, "playtimeMinutes", where) or 0),
                last_played_at=parse_datetime(obj.get("lastPlayedAt"), f"{where}.lastPlayedAt"),
                favorite=bool(obj.get("favorite", False)),
                achievements_unlocked=int(
                    _number(achievements, "unlocked", f"{where}.achievements") or 0
                ),
                achievements_total=int(
                    _number(achievements, "total", f"{where}.achievements") or 0
                ),
                session_count=int(_number(obj, "sessionCount", where, optional=True) or 0),
            )
        )
    return games


def parse_limit(params: Mapping[str, Json], default: int = 10, maximum: int = 200) -> int:
    value = params.get("limit", default)
    if isinstance(value, bool) or not isinstance(value, int) or not 1 <= value <= maximum:
        raise InvalidParamsError(f"limit must be an integer between 1 and {maximum}")
    return value


def parse_now(params: Mapping[str, Json]) -> datetime:
    return parse_datetime(params.get("now"), "now") or datetime.now(UTC)
