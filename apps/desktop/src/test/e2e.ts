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
import type { CoveEvents, DownloadItem, Genre, LibraryItem, StoreSource } from "@cove/shared";
import { usePreferences } from "../stores/preferences.ts";
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

const params = new URLSearchParams(window.location.search);
const count = Number(params.get("games") ?? TITLES.length);
const core = installFakeCore(Array.from({ length: count }, (_, i) => demoItem(i)));
core.coveAchievements = core.coveAchievements.map((a, i) => ({ ...a, unlockedAt: i === 0 ? "2026-11-02T18:00:00Z" : a.unlockedAt }));
FAKE_CATALOG.push(...Array.from({ length: 21 }, (_, i) => ({ ...demoItem(i + 3).game, id: `igdb-${String(i + 10)}` })));

const theme = params.get("theme");
if (theme === "dark" || theme === "light") {
  usePreferences.setState({ theme });
  core.settings.appearance.theme = theme;
}

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

internals.invoke = async (command, args, options) => {
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
