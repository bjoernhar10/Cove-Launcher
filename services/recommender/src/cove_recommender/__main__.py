"""Entry point: read JSON-RPC requests from stdin, write responses to stdout."""

from __future__ import annotations

import json
import sys

from cove_recommender import __version__
from cove_recommender.rpc import handle_line


def main() -> None:
    print(f"cove-recommender {__version__} ready", file=sys.stderr)
    for line in sys.stdin:
        if not line.strip():
            continue
        response = handle_line(line)
        if response is not None:
            sys.stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
