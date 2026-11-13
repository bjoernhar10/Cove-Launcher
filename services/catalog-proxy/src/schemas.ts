/** Input validation for every endpoint (zod). Unknown parameters are ignored. */
import type { CatalogSort, Genre, Platform } from "@cove/shared";
import { z } from "zod";
import type { BrowseParams } from "./igdb-query.ts";

const GENRES = [
  "action",
  "adventure",
  "rpg",
  "strategy",
  "simulation",
  "puzzle",
  "platformer",
  "shooter",
  "racing",
  "sports",
  "horror",
  "indie",
] as const satisfies readonly Genre[];
const PLATFORMS = ["windows", "linux", "macos"] as const satisfies readonly Platform[];
const SORTS = ["recommended", "popular", "newest", "title"] as const satisfies readonly CatalogSort[];

const text = (max: number) => z.string().trim().max(max);

/** Comma separated list of allowed values ("rpg,horror"); empty → []. */
const list = <T extends string>(values: readonly [T, ...T[]]) =>
  z
    .string()
    .optional()
    .transform((v) => (v ? v.split(",").map((s) => s.trim()).filter(Boolean) : []))
    .pipe(z.array(z.enum(values)).max(values.length));

/** "2020" or "2018-2021". */
const yearRange = z
  .string()
  .regex(/^\d{4}(-\d{4})?$/, "year must be YYYY or YYYY-YYYY")
  .transform((v) => {
    const [from, to = from] = v.split("-").map(Number) as [number, number?];
    return { from, to };
  })
  .refine((r) => r.from >= 1970 && r.to <= 2100 && r.from <= r.to, "invalid year range")
  .optional();

export const searchQuery = z.object({ q: text(100).min(2, "q needs at least 2 characters") });

export const browseQuery = z
  .object({
    q: text(100).optional(),
    genre: list(GENRES),
    platform: list(PLATFORMS),
    year: yearRange,
    rating: z.coerce.number().int().min(0).max(100).optional(),
    sort: z.enum(SORTS).default("recommended"),
    page: z.coerce.number().int().min(0).max(200).default(0),
  })
  .transform(
    (v): BrowseParams => ({
      q: v.q ?? "",
      genres: v.genre,
      platforms: v.platform,
      years: v.year ?? null,
      minRating: v.rating ?? null,
      sort: v.sort,
      page: v.page,
    }),
  );

export const gameId = z.coerce.number().int().positive().max(10_000_000);

export const steamId = z.string().regex(/^\d{17}$/, "steamid must be a SteamID64");

export const steamAppId = z.string().regex(/^\d{1,10}$/, "appid must be numeric");

export const matchBody = z.object({ title: text(200).min(1, "title is required") });

/** Prices: a country and at most 50 games (title + the client's key for the answer). */
export const pricesBody = z.object({
  country: z.string().regex(/^[A-Z]{2}$/, "country must be a two-letter code"),
  games: z
    .array(z.object({ key: text(100).min(1, "key is required"), title: text(200).min(1, "title is required") }))
    .min(1, "games must not be empty")
    .max(50, "at most 50 games"),
});

/** At most 500 IGDB game ids (one IGDB page). */
export const timeToBeatBody = z.object({
  ids: z.array(gameId).min(1, "ids must not be empty").max(500, "at most 500 ids"),
});
