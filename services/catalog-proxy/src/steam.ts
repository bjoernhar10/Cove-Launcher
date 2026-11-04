/**
 * Steam Web API behind fixed endpoints. The API key is a Worker secret
 * (STEAM_API_KEY); clients only send a SteamID64 and an app id.
 */
import type { ProxySteamAchievement, ProxySteamAchievements, ProxySteamGame, ProxySteamOwned, ProxySteamProfile } from "@cove/shared";
import type { Env } from "./env.ts";
import { UpstreamError } from "./igdb-client.ts";

export const STEAM_API = "https://api.steampowered.com";
const ICON_BASE = "https://media.steampowered.com/steamcommunity/public/images/apps";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface SteamResult {
  status: number;
  body: unknown;
}

export class SteamClient {
  private readonly fetchFn: typeof fetch;

  constructor(
    private readonly env: Env,
    fetchFn: typeof fetch = (input, init) => fetch(input, init),
  ) {
    this.fetchFn = (input, init) => fetchFn(input, init);
  }

  /** Same global slot gate as IGDB, separate instance. */
  private async acquireSlot(): Promise<void> {
    const gate = this.env.IGDB_GATE.get(this.env.IGDB_GATE.idFromName("steam"));
    const response = await gate.fetch("https://gate/acquire", { method: "POST" });
    const { waitMs } = await response.json<{ waitMs: number }>();
    if (response.status === 503) throw new UpstreamError(503, "Steam queue is full, try again shortly.");
    if (waitMs > 0) await sleep(waitMs);
  }

  /** GET with key; returns status + parsed body (Steam reports privacy errors as 4xx with JSON). */
  async call(path: string, params: Record<string, string>): Promise<SteamResult> {
    if (!this.env.STEAM_API_KEY) throw new UpstreamError(500, "STEAM_API_KEY is not configured (wrangler secret put).");
    const url = new URL(`${STEAM_API}${path}`);
    url.searchParams.set("key", this.env.STEAM_API_KEY);
    url.searchParams.set("format", "json");
    for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
    for (let attempt = 0; ; attempt += 1) {
      await this.acquireSlot();
      const response = await this.fetchFn(url, { headers: { Accept: "application/json" } });
      if (response.status === 429 && attempt < 3) {
        await sleep(500 * (attempt + 1));
        continue;
      }
      if (response.status >= 500 || response.status === 429) {
        throw new UpstreamError(response.status === 429 ? 503 : 502, `Steam answered HTTP ${String(response.status)}`);
      }
      const text = await response.text();
      let body: unknown = null;
      try {
        body = text ? (JSON.parse(text) as unknown) : null;
      } catch {
        body = null;
      }
      return { status: response.status, body };
    }
  }
}

type Json = Record<string, unknown>;
const obj = (v: unknown): Json => (typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Json) : {});
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const str = (v: unknown): string | null => (typeof v === "string" && v ? v : typeof v === "number" ? String(v) : null);
const num = (v: unknown): number | null => {
  const n = typeof v === "string" ? Number(v) : v;
  return typeof n === "number" && Number.isFinite(n) ? n : null;
};
const iso = (unix: number | null): string | null => (unix && unix > 0 ? new Date(unix * 1000).toISOString() : null);

export async function steamProfile(steam: SteamClient, steamId: string): Promise<ProxySteamProfile | null> {
  const { body } = await steam.call("/ISteamUser/GetPlayerSummaries/v2/", { steamids: steamId });
  const player = obj(arr(obj(obj(body).response).players)[0]);
  if (!str(player.steamid)) return null;
  return {
    steamId,
    personaName: str(player.personaname) ?? steamId,
    avatarUrl: str(player.avatarfull),
    // 3 = public; anything else hides games and achievements.
    public: num(player.communityvisibilitystate) === 3,
  };
}

export async function steamOwned(steam: SteamClient, steamId: string): Promise<ProxySteamOwned> {
  const { body } = await steam.call("/IPlayerService/GetOwnedGames/v1/", {
    steamid: steamId,
    include_appinfo: "1",
    include_played_free_games: "1",
  });
  const response = obj(obj(body).response);
  // A private profile / private "game details" yields an empty response object.
  if (!("games" in response) && !("game_count" in response)) return { private: true, games: [] };
  const games: ProxySteamGame[] = arr(response.games).flatMap((raw) => {
    const g = obj(raw);
    const appId = str(g.appid);
    if (!appId) return [];
    const icon = str(g.img_icon_url);
    return [
      {
        appId,
        name: str(g.name) ?? `App ${appId}`,
        playtimeMinutes: num(g.playtime_forever) ?? 0,
        lastPlayedAt: iso(num(g.rtime_last_played)),
        iconUrl: icon ? `${ICON_BASE}/${appId}/${icon}.jpg` : null,
      },
    ];
  });
  return { private: false, games };
}

export interface AchievementSchema {
  name: string;
  displayName: string;
  description: string;
  icon: string | null;
  icongray: string | null;
  hidden: boolean;
}

export async function steamSchema(steam: SteamClient, appId: string): Promise<AchievementSchema[]> {
  const { status, body } = await steam.call("/ISteamUserStats/GetSchemaForGame/v2/", { appid: appId, l: "german" });
  if (status >= 400) return [];
  return arr(obj(obj(obj(body).game).availableGameStats).achievements).flatMap((raw) => {
    const a = obj(raw);
    const name = str(a.name);
    return name
      ? [
          {
            name,
            displayName: str(a.displayName) ?? name,
            description: str(a.description) ?? "",
            icon: str(a.icon),
            icongray: str(a.icongray),
            hidden: num(a.hidden) === 1,
          },
        ]
      : [];
  });
}

export async function steamRarity(steam: SteamClient, appId: string): Promise<Record<string, number>> {
  const { status, body } = await steam.call("/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/", { gameid: appId });
  if (status >= 400) return {};
  const result: Record<string, number> = {};
  for (const raw of arr(obj(obj(body).achievementpercentages).achievements)) {
    const a = obj(raw);
    const name = str(a.name);
    const percent = num(a.percent);
    if (name && percent !== null) result[name] = Math.round(percent * 10) / 10;
  }
  return result;
}

/** Player progress merged with schema (names, icons) and global rarity. */
export async function steamAchievements(
  steam: SteamClient,
  steamId: string,
  appId: string,
  schema: AchievementSchema[],
  rarity: Record<string, number>,
): Promise<ProxySteamAchievements> {
  const { status, body } = await steam.call("/ISteamUserStats/GetPlayerAchievements/v1/", { steamid: steamId, appid: appId });
  const stats = obj(obj(body).playerstats);
  const error = (str(stats.error) ?? "").toLowerCase();
  if (status === 403 || error.includes("not public")) return { private: true, achievements: [] };
  // No stats for this app (e.g. games without achievements).
  if (status >= 400 || stats.success === false) return { private: false, achievements: [] };
  const progress = new Map<string, number>();
  for (const raw of arr(stats.achievements)) {
    const a = obj(raw);
    const name = str(a.apiname);
    if (name && num(a.achieved) === 1) progress.set(name, num(a.unlocktime) ?? 0);
  }
  const achievements: ProxySteamAchievement[] = schema.map((s) => ({
    id: s.name,
    title: s.displayName,
    description: s.description,
    iconUrl: s.icon,
    iconLockedUrl: s.icongray,
    hidden: s.hidden,
    unlockedAt: progress.has(s.name) ? iso(progress.get(s.name) ?? null) ?? new Date(0).toISOString() : null,
    rarityPercent: rarity[s.name] ?? null,
  }));
  return { private: false, achievements };
}
