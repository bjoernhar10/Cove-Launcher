/**
 * Data access layer. The UI imports ONLY from here — never from `src/mocks`
 * and never `invoke()` directly.
 *
 *   import { api } from "../api";
 *   const items = await api.library.listLibrary();
 */
import * as accounts from "./accounts.ts";
import * as catalog from "./catalog.ts";
import * as collections from "./collections.ts";
import * as downloads from "./downloads.ts";
import * as home from "./home.ts";
import * as library from "./library.ts";
import * as plugins from "./plugins.ts";
import * as settings from "./settings.ts";
import * as system from "./system.ts";

export const api = { accounts, catalog, collections, downloads, home, library, plugins, settings, system } as const;

export { DEFAULT_CATALOG_FILTER, DEFAULT_CATALOG_SORT } from "./catalog.ts";
export { CoveError } from "./transport.ts";
