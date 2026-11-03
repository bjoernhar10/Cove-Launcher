# cove-recommender

Recommendation service for Cove. Speaks JSON-RPC 2.0 over stdio
(newline-delimited JSON, one message per line). Logs go to stderr.

```sh
uv sync
echo '{"jsonrpc":"2.0","id":1,"method":"system.ping","params":{"message":"hi"}}' | uv run cove-recommender
```
