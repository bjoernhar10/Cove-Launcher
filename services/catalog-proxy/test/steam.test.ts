import schemaJson from "@cove/shared/schemas/models.schema.json" with { type: "json" };
import { describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";
import { FakeCache, fakeEnv, fakeNetwork } from "./fakes.ts";

const { default: Ajv } = await import("ajv");
const ajv = new Ajv({ allErrors: true, strict: false });
ajv.addSchema(schemaJson);

function expectShape(definition: string, value: unknown): void {
  const validate = ajv.getSchema(`https://cove.local/schemas/models.schema.json#/definitions/${definition}`);
  if (!validate) throw new Error(definition);
  expect(validate(value), JSON.stringify(validate.errors)).toBe(true);
}

const PUBLIC = "76561197960287930";
const PRIVATE = "76561197960287931";

function steamFake(url: URL): Response {
  const id = url.searchParams.get("steamid") ?? url.searchParams.get("steamids");
  const appId = url.searchParams.get("appid") ?? url.searchParams.get("gameid");
  expect(url.searchParams.get("key")).toBe("steamkey");
  switch (url.pathname) {
    case "/ISteamUser/GetPlayerSummaries/v2/":
      return Response.json({
        response: { players: id === PUBLIC ? [{ steamid: PUBLIC, personaname: "Gabe", avatarfull: "https://a/av.jpg", communityvisibilitystate: 3 }] : [] },
      });
    case "/IPlayerService/GetOwnedGames/v1/":
      return Response.json(
        id === PRIVATE
          ? { response: {} }
          : { response: { game_count: 2, games: [
              { appid: 620, name: "Portal 2", playtime_forever: 754, rtime_last_played: 1_790_000_000, img_icon_url: "abc" },
              { appid: 400, name: "Portal", playtime_forever: 0, rtime_last_played: 0, img_icon_url: "" },
            ] } },
      );
    case "/ISteamUserStats/GetSchemaForGame/v2/":
      return Response.json({ game: { availableGameStats: { achievements: [
        { name: "ACH_A", displayName: "Erster Schritt", description: "Fang an", icon: "https://i/a.jpg", icongray: "https://i/a_g.jpg", hidden: 0 },
        { name: "ACH_B", displayName: "Geheim", description: "", icon: "https://i/b.jpg", icongray: "https://i/b_g.jpg", hidden: 1 },
      ] } } });
    case "/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/":
      return Response.json({ achievementpercentages: { achievements: [{ name: "ACH_A", percent: "81.46" }, { name: "ACH_B", percent: 3.2 }] } });
    case "/ISteamUserStats/GetPlayerAchievements/v1/":
      if (id === PRIVATE) return Response.json({ playerstats: { error: "Profile is not public", success: false } }, { status: 403 });
      if (appId === "999") return Response.json({ playerstats: { error: "Requested app has no stats", success: false } }, { status: 400 });
      return Response.json({ playerstats: { success: true, achievements: [
        { apiname: "ACH_A", achieved: 1, unlocktime: 1_790_000_000 },
        { apiname: "ACH_B", achieved: 0, unlocktime: 0 },
      ] } });
    default:
      return new Response("unknown", { status: 404 });
  }
}

function setup(options: Parameters<typeof fakeEnv>[0] = {}) {
  const net = fakeNetwork(() => [], steamFake);
  const bindings = fakeEnv(options);
  const app = createApp({ fetch: net.fetchFn, cache: new FakeCache() });
  const get = (path: string) => app.request(path, { headers: { "CF-Connecting-IP": "203.0.113.9" } }, bindings.env);
  return { net, get, ...bindings };
}

describe("Steam endpoints", () => {
  it("profile reports name and visibility", async () => {
    const { get } = setup();
    const body = await (await get(`/v1/steam/profile/${PUBLIC}`)).json<unknown>();
    expectShape("ProxySteamProfile", body);
    expect(body).toEqual({ steamId: PUBLIC, personaName: "Gabe", avatarUrl: "https://a/av.jpg", public: true });
    expect((await get("/v1/steam/profile/76561197960287999")).status).toBe(404);
  });

  it("owned games include playtime and detect private profiles", async () => {
    const { get } = setup();
    const owned = await (await get(`/v1/steam/owned/${PUBLIC}`)).json<{ games: { appId: string; playtimeMinutes: number; lastPlayedAt: string | null; iconUrl: string | null }[] }>();
    expectShape("ProxySteamOwned", owned);
    expect(owned.games[0]).toMatchObject({ appId: "620", playtimeMinutes: 754, iconUrl: "https://media.steampowered.com/steamcommunity/public/images/apps/620/abc.jpg" });
    expect(owned.games[0]?.lastPlayedAt).toBe(new Date(1_790_000_000 * 1000).toISOString());
    expect(owned.games[1]).toMatchObject({ lastPlayedAt: null, iconUrl: null });
    expect(await (await get(`/v1/steam/owned/${PRIVATE}`)).json()).toEqual({ private: true, games: [] });
  });

  it("achievements merge progress, schema and rarity", async () => {
    const { get, net } = setup();
    const body = await (await get(`/v1/steam/achievements/${PUBLIC}/620`)).json<{ private: boolean; achievements: Record<string, unknown>[] }>();
    expectShape("ProxySteamAchievements", body);
    expect(body.achievements[0]).toEqual({
      id: "ACH_A",
      title: "Erster Schritt",
      description: "Fang an",
      iconUrl: "https://i/a.jpg",
      iconLockedUrl: "https://i/a_g.jpg",
      hidden: false,
      unlockedAt: new Date(1_790_000_000 * 1000).toISOString(),
      rarityPercent: 81.5,
    });
    expect(body.achievements[1]).toMatchObject({ hidden: true, unlockedAt: null, rarityPercent: 3.2 });

    // Schema and rarity are cached separately (7 days) from the player's progress (1 h).
    await get(`/v1/steam/achievements/${PUBLIC}/620`);
    expect(net.steamCalls.filter((u) => u.pathname.includes("GetSchemaForGame"))).toHaveLength(1);
  });

  it("private profiles and games without stats", async () => {
    const { get } = setup();
    expect(await (await get(`/v1/steam/achievements/${PRIVATE}/620`)).json()).toEqual({ private: true, achievements: [] });
    expect(await (await get(`/v1/steam/achievements/${PUBLIC}/999`)).json()).toEqual({ private: false, achievements: [] });
  });

  it("validates ids and never forwards anything else", async () => {
    const { get, net } = setup();
    for (const path of ["/v1/steam/owned/123", "/v1/steam/owned/abc", `/v1/steam/achievements/${PUBLIC}/x`, `/v1/steam/achievements/${PUBLIC}/1;drop`]) {
      expect((await get(path)).status).toBe(400);
    }
    expect((await get("/v1/steam/ISteamUser/GetPlayerSummaries")).status).toBe(404);
    expect(net.steamCalls).toHaveLength(0);
  });

  it("reports a missing Steam key", async () => {
    const { get } = setup({ secrets: false });
    const res = await get(`/v1/steam/owned/${PUBLIC}`);
    expect(res.status).toBe(500);
    expect((await res.json<{ error: { code: string } }>()).error.code).toBe("not_configured");
  });
});
