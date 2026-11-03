/** IGDB game → Cove `Game` (same shape as the core's direct IGDB provider). */
import type { Game, Genre, Platform } from "@cove/shared";
import { GENRE_MAP, THEME_MAP } from "./igdb-query.ts";

const IMAGE_BASE = "https://images.igdb.com/igdb/image/upload";

export interface IgdbImage {
  image_id: string;
}

export interface IgdbGame {
  id: number;
  name: string;
  summary?: string;
  storyline?: string;
  genres?: number[];
  themes?: number[];
  platforms?: number[];
  first_release_date?: number;
  involved_companies?: { company?: { name: string }; developer?: boolean; publisher?: boolean }[];
  total_rating?: number;
  total_rating_count?: number;
  cover?: IgdbImage;
  screenshots?: IgdbImage[];
  artworks?: IgdbImage[];
  similar_games?: number[];
}

/** Image URLs point to images.igdb.com — the app loads them directly. */
export function imageUrl(size: string, id: string): string {
  return `${IMAGE_BASE}/${size}/${id}.jpg`;
}

/** First sentence (or ~180 chars) as teaser. */
export function teaser(text: string): string {
  const t = text.trim();
  const match = /^[\s\S]{41,}?[.!?](?=\s)/.exec(t);
  if (match && match[0].length <= 220) return match[0];
  return t.length > 180 ? `${t.slice(0, 180).trimEnd()}…` : t;
}

export function popularityFromCount(count: number): number {
  return Math.min(100, Math.max(0, Math.round(Math.log10(Math.max(0, count) + 1) * 25)));
}

const PLATFORMS: Record<number, Platform> = { 6: "windows", 3: "linux", 14: "macos" };

export function mapGame(raw: IgdbGame): Game {
  const genres: Genre[] = [];
  const mapped = [
    ...(raw.genres ?? []).map((id) => GENRE_MAP.find(([g]) => g === id)?.[1]),
    ...(raw.themes ?? []).map((id) => THEME_MAP.find(([t]) => t === id)?.[1]),
  ];
  for (const g of mapped) if (g && !genres.includes(g)) genres.push(g);
  const platforms: Platform[] = [];
  for (const id of raw.platforms ?? []) {
    const p = PLATFORMS[id];
    if (p && !platforms.includes(p)) platforms.push(p);
  }
  const company = (role: "developer" | "publisher") =>
    raw.involved_companies?.find((c) => c[role] === true && c.company)?.company?.name ?? "";
  const title = raw.name;
  const headerImage = raw.artworks?.[0] ?? raw.screenshots?.[0];
  return {
    id: `igdb-${String(raw.id)}`,
    title,
    summary: teaser(raw.summary ?? ""),
    description: [raw.summary, raw.storyline]
      .map((s) => s?.trim() ?? "")
      .filter(Boolean)
      .join("\n\n"),
    genres,
    platforms,
    releaseDate:
      raw.first_release_date === undefined ? null : new Date(raw.first_release_date * 1000).toISOString().slice(0, 10),
    developer: company("developer"),
    publisher: company("publisher"),
    rating: raw.total_rating === undefined ? null : Math.round(raw.total_rating),
    popularity: popularityFromCount(raw.total_rating_count ?? 0),
    artwork: {
      cover: { url: raw.cover ? imageUrl("t_cover_big", raw.cover.image_id) : null, alt: `Cover von ${title}` },
      header: { url: headerImage ? imageUrl("t_1080p", headerImage.image_id) : null, alt: `Titelbild von ${title}` },
      screenshots: (raw.screenshots ?? []).slice(0, 8).map((s, i) => ({
        url: imageUrl("t_screenshot_big", s.image_id),
        alt: `Screenshot ${String(i + 1)} von ${title}`,
      })),
    },
  };
}
