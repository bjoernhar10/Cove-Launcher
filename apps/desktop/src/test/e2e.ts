/**
 * Entry point for the Playwright tests (`e2e.html`, dev server only; never part of a build).
 *
 * Playwright cannot drive the Tauri webview, so the end-to-end tests run the
 * real UI in Chromium against the fake core, extended here with the live
 * behaviour of the Rust core: running games emit `game-state-changed`, direct
 * downloads report progress and land in the library.
 *
 * URL parameters: `?games=2500` seeds a large library, `?theme=dark` forces dark mode.
 */
import type { CoveEvents, DownloadItem, Game, Genre, LibraryItem, Recommendation, StoreSource } from "@cove/shared";
import { GENRE_LABELS } from "../lib/labels.ts";
import { emitCoreEvent, fakeDownload, fakeGame, fakeItem, installFakeCore, FAKE_CATALOG } from "./fakeCore.ts";

const TITLES = [
  "Tidewater", "Hollow Lantern", "Paper Orbit", "Quiet Harbor", "Ember Fields", "Glass Meridian",
  "Northwind", "Salt & Signal", "Lumen Drift", "Moss Kingdom", "Cinder Road", "Small Hours",
  "Kitebound", "Low Tide Radio", "Granite Choir", "The Long Orchard", "Fernlight", "Signal Hill",
  "Copper Season", "Atlas Garden", "Driftwood", "Velvet Static", "Lantern Bay", "Stillwater",
];
const GENRES: Genre[] = ["adventure", "puzzle", "rpg", "strategy", "platformer", "simulation"];
const SOURCES: StoreSource[] = ["steam", "gog", "epic", "manual"];

function demoItem(index: number): LibraryItem {
  const base = TITLES[index % TITLES.length] ?? "Spiel";
  const title = index < TITLES.length ? base : `${base} ${String(Math.floor(index / TITLES.length) + 1)}`;
  const id = `game-${String(index)}`;
  const installed = index % 3 !== 1;
  const source = SOURCES[index % SOURCES.length] ?? "manual";
  const item = fakeItem(id, title, {
    source,
    storeGameId: source === "manual" ? null : String(1000 + index),
    installed,
    installPath: installed ? `/games/${id}` : null,
    executablePath: installed || source !== "manual" ? `/games/${id}/${id}` : null,
    favorite: index % 7 === 0,
    playtimeMinutes: (index * 37) % 4000,
    lastPlayedAt: index % 4 === 0 ? null : new Date(Date.UTC(2026, 10, 3) - index * 3_600_000 * 7).toISOString(),
    achievements: index % 2 === 0 ? { unlocked: (index * 3) % 40, total: 40 } : { unlocked: 0, total: 0 },
  });
  item.game = {
    ...fakeGame(id, title),
    genres: [GENRES[index % GENRES.length] ?? "puzzle"],
    releaseDate: `${String(2014 + (index % 12))}-0${String(1 + (index % 9))}-15`,
    summary: "Ein ruhiges Spiel über Licht, Wasser und Wege, die sich erst beim zweiten Hinsehen zeigen.",
  };
  return item;
}

/**
 * Real catalogue data for the README screenshots (`e2e/screenshots.e2e.ts` fetches it
 * from the Cove proxy and injects it as `window.__coveDemo`). Without it the tests use
 * the invented titles above, so they never depend on the network.
 */
interface Demo {
  library: Game[];
  catalog: Game[];
  total: number;
}
const demo = (window as unknown as { __coveDemo?: Demo }).__coveDemo;

/** Deterministic pseudo-random number 0..1 per game and purpose. */
function rand(id: string, salt: number): number {
  let h = salt * 7919;
  for (let i = 0; i < id.length; i += 1) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return (h % 10_000) / 10_000;
}

function realItem(game: Game, index: number): LibraryItem {
  const sources: StoreSource[] = ["steam", "steam", "gog", "epic", "steam", "manual"];
  const source = sources[index % sources.length] ?? "steam";
  const installed = index < 6 || rand(game.id, 1) > 0.45;
  const total = Math.round(rand(game.id, 2) ** 2 * 9000) + 30;
  const achievements = rand(game.id, 3) > 0.3 ? Math.round(20 + rand(game.id, 4) * 60) : 0;
  return {
    game,
    entry: {
      ...fakeItem(game.id, game.title).entry,
      source,
      storeGameId: source === "manual" ? null : String(100_000 + index),
      installed,
      installPath: installed ? `/home/alex/Games/${game.title}` : null,
      executablePath: installed || source !== "manual" ? `/home/alex/Games/${game.title}/start` : null,
      installSizeBytes: installed ? Math.round((4 + rand(game.id, 5) * 90) * 1024 ** 3) : null,
      favorite: index % 5 === 1,
      playtimeMinutes: total,
      lastPlayedAt: new Date(Date.UTC(2026, 10, 4, 19) - index ** 1.6 * 3_600_000 * 9).toISOString(),
      achievements: { unlocked: Math.round(achievements * rand(game.id, 6)), total: achievements },
      catalogId: game.id,
      matchStatus: "matched",
    },
  };
}

const params = new URLSearchParams(window.location.search);
const count = Number(params.get("games") ?? TITLES.length);
const core = installFakeCore(demo ? demo.library.map(realItem) : Array.from({ length: count }, (_, i) => demoItem(i)));
core.coveAchievements = core.coveAchievements.map((a, i) => ({ ...a, unlockedAt: i === 0 ? "2026-11-02T18:00:00Z" : a.unlockedAt }));
if (demo) {
  FAKE_CATALOG.splice(0, FAKE_CATALOG.length, ...demo.catalog);
  const games = (store: StoreSource) => core.library.filter((i) => i.entry.source === store).length;
  const synced = "2026-11-04T18:42:00Z";
  for (const store of ["steam", "gog", "epic"] as const) {
    Object.assign(core.accounts[store], { status: "connected", connected: true, accountName: "alex", gameCount: games(store), lastSyncedAt: synced });
  }
}
else FAKE_CATALOG.push(...Array.from({ length: 21 }, (_, i) => ({ ...demoItem(i + 3).game, id: `igdb-${String(i + 10)}` })));


// ── live behaviour of the Rust core ──

interface TauriInternals {
  invoke: (command: string, args?: Record<string, unknown>, options?: unknown) => Promise<unknown>;
}
const internals = (window as unknown as { __TAURI_INTERNALS__: TauriInternals }).__TAURI_INTERNALS__;
const invoke = internals.invoke.bind(internals);
const running = new Map<string, string>();

function emit<E extends keyof CoveEvents>(event: E, payload: CoveEvents[E]): void {
  void emitCoreEvent(event, payload);
}

/** Simulates the download manager: progress events, verification, install into the library. */
function simulateDownload(item: DownloadItem): void {
  const total = 48 * 1024 * 1024;
  const { download } = item;
  let downloaded = 0;
  const set = (patch: Partial<DownloadItem["download"]>) => {
    Object.assign(download, patch);
    emit("downloads-changed", structuredClone(core.downloads));
  };
  set({ status: "downloading", totalBytes: total });
  const timer = setInterval(() => {
    if (download.status === "paused") return;
    if (download.status === "cancelled") {
      clearInterval(timer);
      return;
    }
    downloaded = Math.min(total, downloaded + total / 8);
    Object.assign(download, { downloadedBytes: downloaded, bytesPerSecond: total / 2 });
    emit("download-progress", { id: download.id, status: "downloading", downloadedBytes: downloaded, totalBytes: total, bytesPerSecond: total / 2 });
    if (downloaded < total) return;
    clearInterval(timer);
    set({ status: "verifying" });
    setTimeout(() => {
      const gameId = `installed-${download.id}`;
      core.library.push(fakeItem(gameId, item.game.title, { playtimeMinutes: 0, lastPlayedAt: null }));
      set({
        status: "completed",
        queuePosition: null,
        checksumVerified: true,
        finishedAt: new Date().toISOString(),
        installPath: `/games/${gameId}`,
        executablePath: `/games/${gameId}/start`,
        gameId,
      });
    }, 300);
  }, 250);
}

/** Answers that make the screenshots look like a real profile (demo mode only). */
function demoAnswer(command: string, args: Record<string, unknown> | undefined): { value: unknown } | null {
  if (!demo) return null;
  const owned = new Set(core.library.map((i) => i.game.id));
  const fresh = demo.catalog.filter((g) => !owned.has(g.id));
  const favourite = [...core.library].sort((a, b) => b.entry.playtimeMinutes - a.entry.playtimeMinutes)[0];
  const limit = Number(args?.limit ?? 12);
  switch (command) {
    case "system_profile":
      return { value: { id: "local", displayName: "Alex", avatarUrl: null } };
    case "recommendations_for_user":
      return {
        value: fresh.slice(0, limit).map((game, i): Recommendation => {
          const anchor = core.library[(i * 3) % core.library.length];
          return {
            game,
            score: 1 - i / 20,
            reason: i % 2 === 0 && anchor ? `Ähnlich wie ${anchor.game.title}` : `Weil du gern ${GENRE_LABELS[game.genres[0] ?? "adventure"]} spielst`,
          };
        }),
      };
    case "recommendations_discover":
      return {
        value: fresh.slice(limit, limit * 2).map((game, i): Recommendation => ({ game, score: 0.5 - i / 40, reason: `Neu für dich: ${GENRE_LABELS[game.genres[0] ?? "adventure"]}` })),
      };
    case "catalog_browse": {
      const page = Number(args?.page ?? 0);
      const items = demo.catalog.slice(page * 24, page * 24 + 24);
      const reasons = Object.fromEntries(
        items.map((g, i) => {
          const anchor = core.library[(i * 5 + 2) % core.library.length] ?? favourite;
          return [g.id, i % 3 === 1 ? `Beliebt bei ${GENRE_LABELS[g.genres[0] ?? "adventure"]}` : `Ähnlich wie ${anchor?.game.title ?? ""}`];
        }),
      );
      const sort = (args?.sort as string | undefined) ?? "recommended";
      return {
        value: { items, page, hasMore: (page + 1) * 24 < demo.catalog.length, total: demo.total, offline: false, reasons: sort === "recommended" ? reasons : {} },
      };
    }
    case "library_playtime": {
      const gameId = String(args?.gameId);
      const days = Array.from({ length: 14 }, (_, i) => ({
        date: new Date(Date.UTC(2026, 9, 22 + i)).toISOString().slice(0, 10),
        minutes: rand(gameId, 20 + i) > 0.55 ? Math.round(20 + rand(gameId, 40 + i) * 160) : 0,
      }));
      const recent = days.reduce((sum, d) => sum + d.minutes, 0);
      const total = core.library.find((i) => i.game.id === gameId)?.entry.playtimeMinutes ?? recent;
      return { value: { gameId, totalMinutes: Math.max(total, recent), last14DaysMinutes: recent, sessionCount: 12 + Math.round(rand(gameId, 9) * 80), days } };
    }
    default:
      return null;
  }
}

internals.invoke = async (command, args, options) => {
  const answer = demoAnswer(command, args);
  if (answer) return structuredClone(answer.value);
  if (command === "game_launch") {
    const gameId = String(args?.gameId);
    const startedAt = new Date().toISOString();
    running.set(gameId, startedAt);
    const result = await invoke(command, args, options);
    setTimeout(() => {
      emit("game-state-changed", { gameId, state: "running", startedAt, sessionSeconds: null });
    }, 100);
    return result;
  }
  if (command === "game_stop") {
    const gameId = String(args?.gameId);
    const startedAt = running.get(gameId);
    if (startedAt) {
      running.delete(gameId);
      setTimeout(() => {
        emit("game-state-changed", { gameId, state: "stopped", startedAt, sessionSeconds: 25 * 60 });
      }, 100);
      return null;
    }
  }
  if (command === "game_running_list") {
    return [...running].map(([gameId, startedAt]) => ({ gameId, startedAt }));
  }
  const result = await invoke(command, args, options);
  if (command === "downloads_add_url") {
    const item = core.downloads.at(-1);
    if (item) setTimeout(() => {
      simulateDownload(item);
    }, 200);
  }
  return result;
};

/** Handles for Playwright (`window.__cove`). */
Object.assign(window, { __cove: { core, emit, fakeDownload } });

await import("../main.tsx");
