import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import type { AnySchema } from "ajv";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS, FAKE_CATALOG, FAKE_SCAN, fakeItem, installFakeCore, type FakeCore } from "../test/fakeCore.ts";
import { api, CoveError, DEFAULT_CATALOG_FILTER } from "./index.ts";

const require = createRequire(import.meta.url);
const schema = JSON.parse(readFileSync(require.resolve("@cove/shared/schemas/models.schema.json"), "utf8")) as AnySchema;
const { default: Ajv } = await import("ajv");
const ajv = new Ajv({ allErrors: true, strict: false });
ajv.addSchema(schema);

function expectValid(definition: string, value: unknown): void {
  const validate = ajv.getSchema(`https://cove.local/schemas/models.schema.json#/definitions/${definition}`);
  if (!validate) throw new Error(`Unknown definition ${definition}`);
  expect(validate(value), JSON.stringify(validate.errors, null, 2)).toBe(true);
}

describe("test fake stays honest", () => {
  it("produces schema-valid core data", () => {
    expectValid("LibraryItem", fakeItem("x", "X"));
    expectValid("AppSettings", DEFAULT_SETTINGS);
    for (const c of FAKE_SCAN) expectValid("ScanCandidate", c);
    for (const g of FAKE_CATALOG) expectValid("Game", g);
  });
});

describe("core commands", () => {
  let core: FakeCore;
  beforeEach(() => {
    core = installFakeCore();
  });
  afterEach(() => {
    core.calls = [];
  });

  it("uses the command names and camelCase args of the Rust core", async () => {
    await api.library.listLibrary();
    await api.library.updateGame("tidewater", { favorite: true });
    await api.library.scanFolder("/games");
    await api.downloads.reorderQueue(["a", "b"]);
    expect(core.calls.map((c) => c.command)).toEqual([
      "library_list",
      "library_update",
      "library_scan_folder",
      "downloads_reorder",
    ]);
    expect(core.calls[1]?.args).toEqual({ gameId: "tidewater", patch: { favorite: true } });
    expect(core.calls[3]?.args).toEqual({ orderedIds: ["a", "b"] });
  });

  it("turns core errors into CoveError with kind", async () => {
    const error: unknown = await api.library.removeGame("nope").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CoveError);
    expect((error as CoveError).kind).toBe("notInLibrary");
  });

  it("adds confirmed scan results and reports failures per game", async () => {
    const result = await api.library.addScannedGames([
      FAKE_SCAN[0] ?? fail(),
      { ...(FAKE_SCAN[0] ?? fail()), title: "Broken", executablePath: "/missing/x" },
    ]);
    expect(result.added.map((i) => i.game.title)).toEqual(["Hollow Lantern"]);
    expect(result.failed).toEqual([{ title: "Broken", message: "Datei nicht gefunden: /missing/x" }]);
    expect(core.calls.find((c) => c.command === "library_add_manual")?.args).toEqual({
      input: { title: "Hollow Lantern", executablePath: "/games/hl/HollowLantern.exe", sizeBytes: 3_000_000 },
    });
  });

  it("sends catalogue filter, sort and page as separate arguments", async () => {
    const page = await api.catalog.browseCatalog({ ...DEFAULT_CATALOG_FILTER, genres: ["horror"] }, "newest", 2);
    expectValid("CatalogPage", page);
    expect(page.items.map((g) => g.title)).toEqual(["Hollow Lantern"]);
    expect(core.calls.at(-1)).toEqual({
      command: "catalog_browse",
      args: { filter: { ...DEFAULT_CATALOG_FILTER, genres: ["horror"] }, sort: "newest", page: 2 },
    });
  });

  it("reports a missing catalogue source as its own error kind", async () => {
    core.catalog.cove = false;
    const error: unknown = await api.catalog.browseCatalog(DEFAULT_CATALOG_FILTER, "popular", 0).catch((e: unknown) => e);
    expect((error as CoveError).kind).toBe("catalogNotConfigured");
    // Home stays usable: catalogue rows are simply empty.
    expect(await api.home.getRecommendations()).toEqual([]);
    expect(await api.home.getDiscover()).toEqual([]);
  });

  it("links library games to catalogue entries", async () => {
    const item = await api.library.linkCatalog("tidewater", "igdb-1");
    expect(item.entry.catalogId).toBe("igdb-1");
    expect(core.calls.at(-1)?.args).toEqual({ gameId: "tidewater", catalogId: "igdb-1" });
  });

  it("builds Home from the real library and the catalogue", async () => {
    const continuePlaying = await api.home.getContinuePlaying();
    expect(continuePlaying.map((i) => i.game.id)).toEqual(["tidewater"]);
    core.library[0] = fakeItem("tidewater", "Tidewater", { catalogId: "igdb-1", matchStatus: "matched" });
    const recommendations = await api.home.getRecommendations();
    // Same genre as the owned game ranks first.
    expect(recommendations.map((r) => r.game.id)).toEqual(["igdb-3", "igdb-2"]);
  });
});

function fail(): never {
  throw new Error("fixture missing");
}
