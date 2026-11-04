import assert from "node:assert/strict";
import { test } from "node:test";
import { activate, pickGame } from "./index.js";

/** @param {string} id @param {Partial<import("@cove/shared/plugin-api").PluginGame>} [over] */
const game = (id, over = {}) => ({
  id,
  title: id.toUpperCase(),
  installed: true,
  favorite: false,
  playtimeMinutes: 0,
  lastPlayedAt: null,
  genres: [],
  source: /** @type {const} */ ("manual"),
  ...over,
});

test("picks only installed games and avoids repeating the last pick", () => {
  const games = [game("a"), game("b"), game("c", { installed: false })];
  for (let i = 0; i < 20; i += 1) {
    const picked = pickGame(games, "a", Math.random);
    assert.equal(picked?.id, "b");
  }
  assert.equal(pickGame([game("a")], "a")?.id, "a", "a single game may repeat");
  assert.equal(pickGame([game("x", { installed: false })], null), null);
});

test("games not played recently are favoured", () => {
  const now = Date.parse("2026-11-04T12:00:00Z");
  const fresh = game("fresh", { lastPlayedAt: "2026-11-04T10:00:00Z" });
  const old = game("old", { lastPlayedAt: "2026-08-01T10:00:00Z" });
  // fresh weighs ~1, old 2: a roll in the upper two thirds lands on "old".
  assert.equal(pickGame([fresh, old], null, () => 0.9, now)?.id, "old");
  assert.equal(pickGame([fresh, old], null, () => 0.1, now)?.id, "fresh");
});

test("the sidebar item suggests a game and remembers it", async () => {
  /** @type {any} */
  let item;
  /** @type {Record<string, unknown>} */
  const storage = {};
  /** @type {unknown[][]} */
  const notes = [];
  await activate(/** @type {any} */ ({
    ui: {
      addSidebarItem: async (i) => { item = i; },
      notify: async (...args) => { notes.push(args); },
    },
    library: { list: async () => [game("tidewater")] },
    storage: { get: async (k) => storage[k] ?? null, set: async (k, v) => { storage[k] = v; } },
  }));
  assert.equal(item.label, "Was soll ich spielen?");
  await item.onClick({ file: null });
  assert.deepEqual(notes[0], ["Wie wäre es mit „TIDEWATER“?", { gameId: "tidewater" }]);
  assert.equal(storage.lastPick, "tidewater");
});
