import { describe, expect, it } from "vitest";
import { fakeItem } from "../test/fakeCore.ts";
import { DEFAULT_PICK, pickCandidates, pickGame } from "./pick.ts";

const now = new Date("2026-11-17T12:00:00Z");
const items = [
  fakeItem("new", "New", {
    installed: true,
    playtimeMinutes: 10,
    catalogId: "igdb-1",
  }),
  fakeItem("old", "Old", {
    installed: true,
    playtimeMinutes: 600,
    lastPlayedAt: "2026-06-01T10:00:00Z",
    catalogId: "igdb-2",
  }),
  fakeItem("recent", "Recent", {
    installed: true,
    playtimeMinutes: 600,
    lastPlayedAt: "2026-11-16T10:00:00Z",
  }),
  fakeItem("away", "Away", { installed: false, playtimeMinutes: 0 }),
];
const ids = (list: { game: { id: string } }[]) => list.map((i) => i.game.id);

describe("pickCandidates", () => {
  it("filters by installation, kind, collection and length", () => {
    expect(ids(pickCandidates(items, DEFAULT_PICK, {}, [], now))).toEqual(["new", "old", "recent"]);
    expect(ids(pickCandidates(items, { ...DEFAULT_PICK, installedOnly: false }, {}, [], now))).toHaveLength(4);
    expect(ids(pickCandidates(items, { ...DEFAULT_PICK, kind: "new" }, {}, [], now))).toEqual(["new"]);
    expect(ids(pickCandidates(items, { ...DEFAULT_PICK, kind: "resume" }, {}, [], now))).toEqual(["old"]);
    const collections = [{ id: "c1", name: "Friends", gameIds: ["recent", "away"] }];
    expect(ids(pickCandidates(items, { ...DEFAULT_PICK, collection: "c1" }, {}, collections, now))).toEqual(["recent"]);
    const times = {
      "igdb-1": {
        hastily: 3 * 3600,
        normally: null,
        completely: null,
        count: 4,
      },
    };
    expect(ids(pickCandidates(items, { ...DEFAULT_PICK, length: "short" }, times, [], now))).toEqual(["new"]);
  });
});

describe("pickGame", () => {
  it("skips games shown recently until none are left", () => {
    const pool = items.slice(0, 3);
    expect(pickGame([], [])).toBeNull();
    expect(pickGame(pool, ["new"], () => 0)?.game.id).toBe("old");
    expect(pickGame(pool, ["new", "old", "recent"], () => 0)?.game.id).toBe("new");
    expect(pickGame(pool, ["old", "recent", "new"], () => 0)?.game.id).toBe("old");
    expect(pickGame(pool.slice(0, 1), ["new"], () => 0.99)?.game.id).toBe("new");
  });
});
