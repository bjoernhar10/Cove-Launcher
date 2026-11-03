/**
 * Apicalypse query building for IGDB. The client only sends parameters; every
 * query is built here from validated input (no pass-through).
 */
import type { CatalogSort, Genre, Platform } from "@cove/shared";

export const FIELDS =
  "name,summary,storyline,genres,themes,platforms,first_release_date," +
  "involved_companies.company.name,involved_companies.developer,involved_companies.publisher," +
  "total_rating,total_rating_count,cover.image_id,screenshots.image_id,artworks.image_id";

/** Main games only (no DLCs, editions, bundles). */
export const BASE_WHERE = "parent_game = null & version_parent = null";

export const PLATFORM_IDS: Record<Platform, number> = { windows: 6, linux: 3, macos: 14 };
export const DESKTOP_PLATFORMS: number[] = [6, 3, 14];

/** IGDB genre id → Cove genre. */
export const GENRE_MAP: [number, Genre][] = [
  [2, "adventure"],
  [4, "action"],
  [5, "shooter"],
  [8, "platformer"],
  [9, "puzzle"],
  [10, "racing"],
  [11, "strategy"],
  [12, "rpg"],
  [13, "simulation"],
  [14, "sports"],
  [15, "strategy"],
  [16, "strategy"],
  [24, "strategy"],
  [25, "action"],
  [31, "adventure"],
  [32, "indie"],
  [33, "action"],
];

/** IGDB theme id → Cove genre. */
export const THEME_MAP: [number, Genre][] = [
  [1, "action"],
  [19, "horror"],
];

export interface BrowseParams {
  q: string;
  genres: Genre[];
  platforms: Platform[];
  years: { from: number; to: number } | null;
  minRating: number | null;
  sort: CatalogSort;
  page: number;
}

export const PAGE_SIZE = 24;

export function escapeText(text: string): string {
  return text.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

function yearStart(year: number): number {
  return Date.UTC(year, 0, 1) / 1000;
}

export function buildWhere(p: BrowseParams, nowSeconds: number): string {
  const parts = [BASE_WHERE];
  const platforms = p.platforms.length > 0 ? p.platforms.map((x) => PLATFORM_IDS[x]) : DESKTOP_PLATFORMS;
  parts.push(`platforms = (${platforms.join(",")})`);
  if (p.genres.length > 0) {
    const genreIds = GENRE_MAP.filter(([, g]) => p.genres.includes(g)).map(([id]) => id);
    const themeIds = THEME_MAP.filter(([, g]) => p.genres.includes(g)).map(([id]) => id);
    const any: string[] = [];
    if (genreIds.length > 0) any.push(`genres = (${genreIds.join(",")})`);
    if (themeIds.length > 0) any.push(`themes = (${themeIds.join(",")})`);
    if (any.length > 0) parts.push(`(${any.join(" | ")})`);
  }
  if (p.years) {
    parts.push(`first_release_date >= ${String(yearStart(p.years.from))} & first_release_date < ${String(yearStart(p.years.to + 1))}`);
  }
  if (p.minRating !== null) parts.push(`total_rating >= ${String(p.minRating)}`);
  if (!p.q.trim()) {
    if (p.sort === "recommended") parts.push("total_rating_count >= 20");
    if (p.sort === "newest") parts.push(`first_release_date <= ${String(nowSeconds)}`);
  }
  return parts.join(" & ");
}

const SORT_CLAUSE: Record<CatalogSort, string> = {
  recommended: "total_rating desc",
  popular: "total_rating_count desc",
  newest: "first_release_date desc",
  title: "name asc",
};

/** IGDB forbids `sort` together with `search`; a text query is ranked by relevance. */
export function buildBrowseBody(p: BrowseParams, nowSeconds: number): string {
  const text = p.q.trim();
  let body = text ? `search "${escapeText(text)}"; ` : "";
  body += `fields ${FIELDS}; where ${buildWhere(p, nowSeconds)}; `;
  if (!text) body += `sort ${SORT_CLAUSE[p.sort]}; `;
  body += `limit ${String(PAGE_SIZE)}; offset ${String(p.page * PAGE_SIZE)};`;
  return body;
}

export function buildSearchBody(text: string, limit: number): string {
  return `search "${escapeText(text.trim())}"; fields ${FIELDS}; where ${BASE_WHERE}; limit ${String(limit)};`;
}

export function buildDetailsBody(id: number): string {
  return `fields ${FIELDS}; where id = ${String(id)}; limit 1;`;
}

export function buildSimilarIdsBody(id: number): string {
  return `fields similar_games; where id = ${String(id)};`;
}

export function buildGamesByIdsBody(ids: number[]): string {
  return `fields ${FIELDS}; where id = (${ids.join(",")}) & ${BASE_WHERE}; limit ${String(ids.length)};`;
}
