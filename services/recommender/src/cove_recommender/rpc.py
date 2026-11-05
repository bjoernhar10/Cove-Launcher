"""JSON-RPC 2.0 dispatcher (newline-delimited JSON over stdio).

Methods (params/results mirror `packages/shared`):
- system.ping
- system.setLanguage    {language: "de" | "en"}                       → {language}
- recommend_for_user    {library, catalog, limit?, now?, candidateIds?} → {items}
- discover_new          {library, catalog, limit?, now?}               → {items}
- similar_games         {gameId, catalog, limit?}                       → {items}
- explain_recommendation {gameId, library, catalog, now?}               → {reason}
"""

from __future__ import annotations

import json
from collections.abc import Callable, Mapping, Sequence
from typing import cast

from cove_recommender import __version__, engine, texts
from cove_recommender.data import (
    InvalidParamsError,
    parse_catalog,
    parse_library,
    parse_limit,
    parse_now,
)

type JsonObject = dict[str, object]
type Handler = Callable[[Mapping[str, object]], JsonObject]
type JsonRpcId = str | int | None

PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603
#: Application error: unknown game id.
GAME_NOT_FOUND = 1001


class GameNotFoundError(Exception):
    pass


def _items(recs: Sequence[engine.Recommendation]) -> JsonObject:
    return {"items": [{"gameId": r.game_id, "score": r.score, "reason": r.reason} for r in recs]}


def _game_id(params: Mapping[str, object]) -> str:
    value = params.get("gameId")
    if not isinstance(value, str) or not value:
        raise InvalidParamsError("gameId must be a non-empty string")
    return value


def _ping(params: Mapping[str, object]) -> JsonObject:
    message = params.get("message", "")
    return {"service": "recommender", "version": __version__, "message": f"pong: {message}"}


def _set_language(params: Mapping[str, object]) -> JsonObject:
    language = params.get("language")
    if language not in ("de", "en"):
        raise InvalidParamsError("language must be 'de' or 'en'")
    texts.set_language("en" if language == "en" else "de")
    return {"language": texts.language()}


def _recommend(params: Mapping[str, object]) -> JsonObject:
    raw_ids = params.get("candidateIds")
    candidate_ids: list[str] | None = None
    if raw_ids is not None:
        if not isinstance(raw_ids, list) or not all(isinstance(i, str) for i in raw_ids):
            raise InvalidParamsError("candidateIds must be an array of strings")
        candidate_ids = cast("list[str]", raw_ids)
    return _items(
        engine.recommend_for_user(
            parse_library(params.get("library")),
            parse_catalog(params.get("catalog")),
            parse_limit(params),
            parse_now(params),
            candidate_ids,
        )
    )


def _discover(params: Mapping[str, object]) -> JsonObject:
    return _items(
        engine.discover_new(
            parse_library(params.get("library")),
            parse_catalog(params.get("catalog")),
            parse_limit(params),
            parse_now(params),
        )
    )


def _similar(params: Mapping[str, object]) -> JsonObject:
    try:
        return _items(
            engine.similar_games(
                _game_id(params), parse_catalog(params.get("catalog")), parse_limit(params)
            )
        )
    except KeyError as exc:
        raise GameNotFoundError(str(exc)) from exc


def _explain(params: Mapping[str, object]) -> JsonObject:
    try:
        reason = engine.explain_recommendation(
            _game_id(params),
            parse_library(params.get("library")),
            parse_catalog(params.get("catalog")),
            parse_now(params),
        )
    except KeyError as exc:
        raise GameNotFoundError(str(exc)) from exc
    return {"reason": reason}


HANDLERS: dict[str, Handler] = {
    "system.ping": _ping,
    "system.setLanguage": _set_language,
    "recommend_for_user": _recommend,
    "discover_new": _discover,
    "similar_games": _similar,
    "explain_recommendation": _explain,
}


def _error(id_: JsonRpcId, code: int, message: str) -> JsonObject:
    return {"jsonrpc": "2.0", "id": id_, "error": {"code": code, "message": message}}


def handle_line(line: str) -> JsonObject | None:
    """Handle one raw input line; returns the response, or ``None`` for notifications."""
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        return _error(None, PARSE_ERROR, "Parse error")

    if (
        not isinstance(msg, dict)
        or msg.get("jsonrpc") != "2.0"
        or not isinstance(msg.get("method"), str)
    ):
        return _error(None, INVALID_REQUEST, "Invalid Request")

    raw_id = msg.get("id")
    id_: JsonRpcId = (
        raw_id if isinstance(raw_id, str | int) and not isinstance(raw_id, bool) else None
    )
    is_notification = "id" not in msg
    handler = HANDLERS.get(msg["method"])
    if handler is None:
        return None if is_notification else _error(id_, METHOD_NOT_FOUND, "Method not found")

    params = msg.get("params") or {}
    if not isinstance(params, dict):
        return None if is_notification else _error(id_, INVALID_PARAMS, "params must be an object")

    try:
        result = handler(cast("Mapping[str, object]", params))
    except InvalidParamsError as exc:
        response = _error(id_, INVALID_PARAMS, str(exc))
    except GameNotFoundError as exc:
        response = _error(id_, GAME_NOT_FOUND, f"Unknown game: {exc}")
    except Exception as exc:  # report, don't crash the sidecar
        response = _error(id_, INTERNAL_ERROR, f"{type(exc).__name__}: {exc}")
    else:
        response = {"jsonrpc": "2.0", "id": id_, "result": result}
    return None if is_notification else response
