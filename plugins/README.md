# Cove plugins

Example plugins. Each folder has a `plugin.json`, an entry module and `node:test` tests.

| Plugin | Permissions | What it shows |
|---|---|---|
| `playtime-goals` | `achievements.define` | Achievements with playtime rules |
| `random-pick` | `library.read`, `ui.addSidebarItem`, `ui.notify`, `storage` | Sidebar item, library, storage, notifications |
| `csv-import` | `library.add`, `ui.addSidebarItem`, `ui.notify` | File picking from a sidebar item, adding games |

Development builds of Cove list this folder in Settings → Plugins (as "Wartet auf deine Freigabe").
How to write a plugin: [docs/plugins.md](../docs/plugins.md).
