# Writing Cove plugins

Plugins are plain JavaScript ES modules without a build step. Each plugin is a folder with a
`plugin.json` and an entry module. Cove runs every plugin in its own Web Worker: no DOM, no Tauri,
no network. A plugin talks to Cove only through the small, versioned `cove` API, and only with the
permissions the user confirmed.

## Quick start

```
my-plugin/
  plugin.json
  index.js
```

`plugin.json`:

```json
{
  "id": "my-plugin",
  "name": "Mein Plugin",
  "version": "1.0.0",
  "author": "Du",
  "description": "What it does, in one sentence.",
  "apiVersion": 1,
  "permissions": ["library.read", "ui.notify"],
  "entry": "index.js"
}
```

`index.js`:

```js
/** @param {import("@cove/shared/plugin-api").CoveApi} cove */
export async function activate(cove) {
  const games = await cove.library.list();
  await cove.ui.notify(`Du hast ${games.length} Spiele.`);
}
```

Install it in Cove: Settings → Plugins → "Plugin installieren", enter the folder, confirm the
permissions. Or put the folder into Cove's plugin folder ("Plugin-Ordner öffnen"); Cove then shows it
as "Wartet auf deine Freigabe" until you confirm.

## Manifest

| Field | Required | Rules |
|---|---|---|
| `id` | yes | lowercase letters, digits and `-`, at most 64 characters |
| `name` | yes | at most 60 characters |
| `version` | yes | `major.minor.patch`, e.g. `1.2.0` |
| `author` | no | shown in Settings |
| `description` | no | one sentence |
| `apiVersion` | no | must be `1` (default) |
| `permissions` | no | see below; ask only for what you use |
| `entry` | yes | `.js` or `.mjs` file inside the plugin folder |

An entry module can import other files of the same plugin with relative paths
(`import { x } from "./lib/util.js"`). Files of other plugins and anything from the internet cannot
be imported.

## Permissions

| Permission | Allows |
|---|---|
| `library.read` | `cove.library.list()`: titles, genres, playtime, install state; no file paths |
| `library.add` | `cove.library.add({ title, executablePath? })` |
| `catalog.search` | `cove.catalog.search(query)`: up to 20 catalogue results |
| `achievements.define` | `cove.achievements.define([...])`: Cove achievements with fixed rules |
| `ui.addSidebarItem` | `cove.ui.addSidebarItem({...})`: up to 3 entries under "Plugins" |
| `ui.notify` | `cove.ui.notify(message, { gameId? })`: small toast, at most 3 per 10 seconds |
| `storage` | `cove.storage.get/set/remove/keys`: JSON values, private to the plugin, 1 MB in total, 64 KB per value |

The user confirms the permissions when installing. An update that asks for more permissions stops
the plugin until the user confirms again. Calls without permission fail with an error whose
`kind` is `"plugin"`.

## API (version 1)

The full types are in `packages/shared/src/plugin-api.ts`; with the JSDoc line above, editors offer
autocomplete for plain JavaScript.

```ts
cove.apiVersion                    // 1
cove.plugin                        // { id, name, version, permissions }
cove.library.list()                // Promise<PluginGame[]>
cove.library.add({ title, executablePath })
cove.catalog.search(query)         // Promise<PluginCatalogGame[]>
cove.achievements.define(list)     // replaces all achievements of this plugin
cove.ui.addSidebarItem({ id, label, icon, pickFile, onClick })
cove.ui.notify(message, { gameId })
cove.storage.get(key) / set(key, value) / remove(key) / keys()
```

- **Achievements** use the same rules as Cove's own ones:
  `{ type: "playtimeHours", hours }`, `{ type: "libraryGames", count }`,
  `{ type: "playedBetween", fromHour, toHour }` (local time) and `{ type: "recommendedPlayed", count }`.
  Ids are prefixed with the plugin id. At most 50 per plugin. Define them in `activate` on every
  start; it replaces the previous list.
- **Sidebar items**: `icon` is one of `puzzle`, `dice`, `trophy`, `upload`, `sparkles`, `list`,
  `star`, `clock`. With `pickFile: { accept: ".csv" }` Cove opens a file dialog when the item is
  clicked and passes `{ file: { name, text } }` to `onClick` (text files up to 5 MB).
- **Errors** thrown by the API have `kind` and `message`.
- `activate` may be `async`. If it throws, Cove shows "Plugin-Fehler" and stops the plugin.

## Isolation

- Each plugin runs in a dedicated worker created from a `blob:` URL, so it inherits Cove's Content
  Security Policy: scripts only from `cove-plugin:`, no network connections.
- Before the plugin loads, the worker removes `fetch`, `XMLHttpRequest`, `WebSocket`,
  `EventSource`, `importScripts`, `indexedDB`, `caches`, `Worker`, `SharedWorker` and
  `BroadcastChannel`.
- The core serves plugin files through `cove-plugin://localhost/<token>/<path>`. The token is random
  per plugin and app start, so a plugin can only load its own files. Paths outside the folder and
  symlinks are refused; only approved, enabled plugins are served.
- The UI host checks every call against the granted permissions; the core checks storage,
  achievements and `library.add` a second time.

## Examples

See `plugins/` in the repository:

- `playtime-goals`: achievements for 5, 25, 100, 250 and 500 hours played in Cove.
- `random-pick`: a "Was soll ich spielen?" sidebar item.
- `csv-import`: imports games from a CSV file (columns "Titel" and optional "Pfad").

## Testing

Plugins are plain modules, so `node --test` works with a fake `cove` object (see the
`*.test.js` files of the examples). `npm run test` at the repository root runs them.
