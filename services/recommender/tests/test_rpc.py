import json

from cove_recommender.rpc import METHOD_NOT_FOUND, PARSE_ERROR, handle_line


def test_ping_returns_pong() -> None:
    line = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": "system.ping", "params": {"message": "hi"}}
    )
    response = handle_line(line)
    assert response is not None
    assert response["id"] == 1
    assert response["result"]["service"] == "recommender"
    assert response["result"]["message"] == "pong: hi"


def test_unknown_method() -> None:
    response = handle_line('{"jsonrpc":"2.0","id":2,"method":"nope"}')
    assert response is not None
    assert response["error"]["code"] == METHOD_NOT_FOUND


def test_parse_error() -> None:
    response = handle_line("{not json")
    assert response is not None
    assert response["error"]["code"] == PARSE_ERROR


def test_notification_has_no_response() -> None:
    assert handle_line('{"jsonrpc":"2.0","method":"system.ping"}') is None
