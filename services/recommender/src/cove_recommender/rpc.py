"""Minimal JSON-RPC 2.0 dispatcher (newline-delimited JSON over stdio).

Message shapes mirror ``packages/shared`` — keep them in sync.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from typing import Any

from cove_recommender import __version__

type JsonObject = dict[str, Any]
type Handler = Callable[[JsonObject], JsonObject]
type JsonRpcId = str | int | None

PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INTERNAL_ERROR = -32603


def _ping(params: JsonObject) -> JsonObject:
    message = params.get("message", "")
    return {"service": "recommender", "version": __version__, "message": f"pong: {message}"}


HANDLERS: dict[str, Handler] = {"system.ping": _ping}


def _error(id_: JsonRpcId, code: int, message: str) -> JsonObject:
    return {"jsonrpc": "2.0", "id": id_, "error": {"code": code, "message": message}}


def handle_line(line: str) -> JsonObject | None:
    """Handle one raw input line; returns the response, or ``None`` for notifications."""
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        return _error(None, PARSE_ERROR, "Parse error")

    if not isinstance(msg, dict) or msg.get("jsonrpc") != "2.0" or "method" not in msg:
        return _error(None, INVALID_REQUEST, "Invalid Request")

    id_: JsonRpcId = msg.get("id")
    handler = HANDLERS.get(msg["method"])
    is_notification = "id" not in msg

    if handler is None:
        return None if is_notification else _error(id_, METHOD_NOT_FOUND, "Method not found")

    params = msg.get("params") or {}
    try:
        result = handler(params)
    except Exception as exc:  # report, don't crash the service
        return None if is_notification else _error(id_, INTERNAL_ERROR, str(exc))

    return None if is_notification else {"jsonrpc": "2.0", "id": id_, "result": result}
