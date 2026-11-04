# Cove connectors

C# (.NET 9, Native AOT) sidecar that talks to the stores. JSON-RPC 2.0 over stdio, one message per line;
logs go to stderr.

## Methods

| Method | Purpose |
|---|---|
| `system.ping` | Health check |
| `store.list` | Supported stores |
| `store.loginStart` | Login URL and mode (`browser` for Steam, `webview` for GOG/Epic) |
| `store.loginComplete` | Callback URL (Steam) or code (GOG/Epic) to session |
| `store.refresh` | New tokens from the refresh token |
| `store.library` | Owned games with playtime |
| `store.achievements` | Achievements of one game (Steam) |
| `store.detectLocal` | Installed games without login (Steam, Heroic, Legendary, Epic, GOG Galaxy) |

Store failures answer with code `2000` and `error.data.kind`: `authFailed`, `privateProfile`, `unavailable`,
`network` or `invalidRequest`.

The service keeps no state. It never sees passwords; the core passes the session (tokens from the store) with
every call and stores it in the OS keyring.

## Configuration

Endpoints and client ids: `src/Cove.Connectors/Config/connectors.json` (embedded). Override at runtime with
`COVE_CONNECTORS_CONFIG=/path/to/connectors.json`. Steam data comes through the Cove catalogue proxy
(`/v1/steam/*`), which holds the Steam Web API key.

GOG and Epic use the public launcher clients that Heroic, gogdl and Legendary use. This is unofficial and may
stop working when the stores change their APIs.

## Commands

```sh
dotnet build
dotnet test
dotnet format --verify-no-changes
npm run build:connectors -w @cove/desktop   # sidecar binary for the app
```
