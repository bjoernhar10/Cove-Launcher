import json
from typing import Any, cast

from cove_recommender.rpc import (
    GAME_NOT_FOUND,
    INVALID_PARAMS,
    METHOD_NOT_FOUND,
    PARSE_ERROR,
    handle_line,
)

CATALOG = [
    {
        "id": "igdb-1",
        "title": "Hollow Knight",
        "genres": ["platformer", "indie"],
        "themes": ["Fantasy"],
        "gameModes": [],
        "perspectives": [],
        "keywords": ["metroidvania"],
        "developer": "Team Cherry",
        "releaseYear": 2017,
        "rating": 90,
        "popularity": 90,
    },
    {
        "id": "igdb-2",
        "title": "Silksong",
        "genres": ["platformer", "indie"],
        "themes": ["Fantasy"],
        "gameModes": [],
        "perspectives": [],
        "keywords": ["metroidvania"],
        "developer": "Team Cherry",
        "releaseYear": 2025,
        "rating": None,
        "popularity": 95,
    },
    {
        "id": "igdb-3",
        "title": "Forza",
        "genres": ["racing"],
        "themes": [],
        "gameModes": [],
        "perspectives": [],
        "keywords": ["cars"],
        "developer": "Playground",
        "releaseYear": None,
        "rating": 91,
        "popularity": 90,
    },
]
LIBRARY = [
    {
        "gameId": "igdb-1",
        "title": "Hollow Knight",
        "playtimeMinutes": 2400,
        "lastPlayedAt": "2026-11-01T10:00:00Z",
        "favorite": False,
        "achievements": {"unlocked": 3, "total": 10},
        "sessionCount": 20,
    },
]


def call(method: str, params: object, id_: int = 1) -> dict[str, Any]:
    line = json.dumps({"jsonrpc": "2.0", "id": id_, "method": method, "params": params})
    response = handle_line(line)
    assert response is not None
    return cast("dict[str, Any]", response)


def test_ping() -> None:
    result = call("system.ping", {"message": "hi"})["result"]
    assert result == {"service": "recommender", "version": "0.1.0", "message": "pong: hi"}


def test_recommend_for_user() -> None:
    params = {"library": LIBRARY, "catalog": CATALOG, "limit": 2, "now": "2026-11-04T12:00:00Z"}
    items = call("recommend_for_user", params)["result"]["items"]
    assert [i["gameId"] for i in items] == ["igdb-2", "igdb-3"]
    assert items[0]["reason"] == "Weil du 40 Std. Hollow Knight gespielt hast"
    assert 0 < items[0]["score"] <= 1


def test_discover_similar_and_explain() -> None:
    discover = call("discover_new", {"library": LIBRARY, "catalog": CATALOG, "limit": 5})["result"][
        "items"
    ]
    assert [i["gameId"] for i in discover] == ["igdb-3"]
    similar = call("similar_games", {"gameId": "igdb-1", "catalog": CATALOG, "limit": 1})["result"][
        "items"
    ]
    assert similar[0]["gameId"] == "igdb-2"
    reason = call(
        "explain_recommendation", {"gameId": "igdb-2", "library": LIBRARY, "catalog": CATALOG}
    )["result"]
    assert reason == {"reason": "Weil du 40 Std. Hollow Knight gespielt hast"}


def test_errors() -> None:
    bad_limit = call("recommend_for_user", {"library": [], "catalog": [], "limit": 0})
    assert bad_limit["error"]["code"] == INVALID_PARAMS
    bad_catalog = call("recommend_for_user", {"library": [], "catalog": [{"id": 1}]})
    assert bad_catalog["error"]["code"] == INVALID_PARAMS
    assert "catalog[0].id" in bad_catalog["error"]["message"]
    unknown = call("similar_games", {"gameId": "nope", "catalog": CATALOG})
    assert unknown["error"]["code"] == GAME_NOT_FOUND
    assert call("nope", {})["error"]["code"] == METHOD_NOT_FOUND
    parse = cast("dict[str, Any] | None", handle_line("{nope"))
    assert parse is not None
    assert parse["error"]["code"] == PARSE_ERROR


def test_notifications_get_no_response() -> None:
    assert handle_line('{"jsonrpc":"2.0","method":"system.ping"}') is None
