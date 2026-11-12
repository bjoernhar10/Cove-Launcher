import type { Game, GameArtwork, Genre, Platform } from "@cove/shared";

/**
 * Invented games for the README screenshots: titles, studios and artwork are made
 * up here (flat SVG landscapes as data URLs), so the screenshots show no real game.
 */

interface Palette {
  sky: string;
  light: string;
  layers: [string, string, string];
  ink: string;
}

const PALETTES: Palette[] = [
  { sky: "#E9CDB0", light: "#F7E8D2", layers: ["#C08A72", "#8A5A62", "#4A3346"], ink: "#FBF3E8" },
  { sky: "#BCD6CF", light: "#F1EEDC", layers: ["#7FAEA2", "#477571", "#21414A"], ink: "#F4F1E4" },
  { sky: "#C9D3E6", light: "#F3F1EA", layers: ["#8C9CC0", "#5A6896", "#2E3760"], ink: "#F2F2F6" },
  { sky: "#E4D6A8", light: "#FAF1D0", layers: ["#B8A15E", "#7E7444", "#43432C"], ink: "#FBF6E2" },
  { sky: "#D8C2CF", light: "#F4E7EA", layers: ["#A88296", "#735A78", "#3E3350"], ink: "#F7EEF2" },
  { sky: "#1E2A3E", light: "#E8E2CF", layers: ["#33476A", "#283752", "#141C2C"], ink: "#E9E6DA" },
  { sky: "#CFE0E8", light: "#FFFFFF", layers: ["#9DBACB", "#6589A0", "#365569"], ink: "#F5F9FB" },
  { sky: "#2B2433", light: "#E7B98B", layers: ["#5A4256", "#3F2F42", "#201925"], ink: "#F1E2D2" },
];

type Scene = (w: number, h: number, p: Palette, seed: number) => string;

function rand(seed: number): () => number {
  let s = seed >>> 0 || 1;
  return () => {
    s = (s * 1_664_525 + 1_013_904_223) >>> 0;
    return s / 2 ** 32;
  };
}

/** A ridge line across the picture, as a closed polygon down to the bottom. */
function ridge(w: number, h: number, base: number, height: number, steps: number, r: () => number, color: string): string {
  const points = [`0,${String(h)}`];
  for (let i = 0; i <= steps; i += 1) {
    const x = (w * i) / steps;
    const y = base - (i % 2 === 0 ? r() * height * 0.4 : height * (0.6 + r() * 0.4));
    points.push(`${x.toFixed(0)},${y.toFixed(0)}`);
  }
  points.push(`${String(w)},${String(h)}`);
  return `<polygon points="${points.join(" ")}" fill="${color}"/>`;
}

/** A soft hill line (quadratic curves). */
function hills(w: number, h: number, base: number, height: number, r: () => number, color: string): string {
  const n = 3;
  let d = `M0 ${String(h)} L0 ${(base - r() * height).toFixed(0)}`;
  for (let i = 0; i < n; i += 1) {
    const x0 = (w * i) / n;
    const x1 = (w * (i + 1)) / n;
    d += ` Q${((x0 + x1) / 2).toFixed(0)} ${(base - height - r() * height).toFixed(0)} ${x1.toFixed(0)} ${(base - r() * height).toFixed(0)}`;
  }
  return `<path d="${d} L${String(w)} ${String(h)} Z" fill="${color}"/>`;
}

const mountains: Scene = (w, h, p, seed) => {
  const r = rand(seed);
  const s = Math.min(w, h);
  return [
    `<circle cx="${(w * (0.25 + r() * 0.5)).toFixed(0)}" cy="${(h * 0.32).toFixed(0)}" r="${(s * 0.16).toFixed(0)}" fill="${p.light}"/>`,
    ridge(w, h, h * 0.62, h * 0.22, 6, r, p.layers[0]),
    ridge(w, h, h * 0.76, h * 0.2, 9, r, p.layers[1]),
    hills(w, h, h * 0.9, h * 0.06, r, p.layers[2]),
  ].join("");
};

const sea: Scene = (w, h, p, seed) => {
  const r = rand(seed);
  const s = Math.min(w, h);
  const horizon = h * 0.58;
  const bands = [0, 1, 2, 3]
    .map((i) => `<rect x="${(w * (0.1 + r() * 0.5)).toFixed(0)}" y="${(horizon + 24 + i * s * 0.07).toFixed(0)}" width="${(w * (0.15 + r() * 0.3)).toFixed(0)}" height="${(s * 0.012).toFixed(1)}" rx="3" fill="${p.light}" opacity="0.45"/>`)
    .join("");
  const lx = w * (0.62 + r() * 0.2);
  const lw = s * 0.05;
  return [
    `<circle cx="${(w * 0.3).toFixed(0)}" cy="${(h * 0.3).toFixed(0)}" r="${(s * 0.13).toFixed(0)}" fill="${p.light}"/>`,
    hills(w, h, horizon, h * 0.04, r, p.layers[0]),
    `<rect x="0" y="${horizon.toFixed(0)}" width="${String(w)}" height="${(h - horizon).toFixed(0)}" fill="${p.layers[1]}"/>`,
    bands,
    `<rect x="${lx.toFixed(0)}" y="${(horizon - s * 0.3).toFixed(0)}" width="${lw.toFixed(0)}" height="${(s * 0.3).toFixed(0)}" fill="${p.layers[2]}"/>`,
    `<rect x="${(lx - lw * 0.2).toFixed(0)}" y="${(horizon - s * 0.34).toFixed(0)}" width="${(lw * 1.4).toFixed(0)}" height="${(s * 0.05).toFixed(0)}" fill="${p.light}"/>`,
  ].join("");
};

const forest: Scene = (w, h, p, seed) => {
  const r = rand(seed);
  const s = Math.min(w, h);
  const trees: string[] = [];
  for (const [row, color] of [
    [0.7, p.layers[1]],
    [0.86, p.layers[2]],
  ] as const) {
    const count = Math.round(w / (s * 0.1));
    for (let i = 0; i <= count; i += 1) {
      const x = (w * i) / count + (r() - 0.5) * s * 0.05;
      const th = s * (0.18 + r() * 0.14);
      const tw = th * 0.32;
      const y = h * row + r() * s * 0.03;
      trees.push(`<polygon points="${(x - tw).toFixed(0)},${y.toFixed(0)} ${x.toFixed(0)},${(y - th).toFixed(0)} ${(x + tw).toFixed(0)},${y.toFixed(0)}" fill="${color}"/>`);
    }
    trees.push(`<rect x="0" y="${(h * row).toFixed(0)}" width="${String(w)}" height="${String(h)}" fill="${color}"/>`);
  }
  return [
    `<circle cx="${(w * 0.68).toFixed(0)}" cy="${(h * 0.26).toFixed(0)}" r="${(s * 0.12).toFixed(0)}" fill="${p.light}"/>`,
    hills(w, h, h * 0.6, h * 0.08, r, p.layers[0]),
    ...trees,
  ].join("");
};

const dunes: Scene = (w, h, p, seed) => {
  const r = rand(seed);
  const s = Math.min(w, h);
  return [
    `<circle cx="${(w * 0.5).toFixed(0)}" cy="${(h * 0.42).toFixed(0)}" r="${(s * 0.22).toFixed(0)}" fill="${p.light}"/>`,
    hills(w, h, h * 0.62, h * 0.08, r, p.layers[0]),
    hills(w, h, h * 0.76, h * 0.07, r, p.layers[1]),
    hills(w, h, h * 0.9, h * 0.05, r, p.layers[2]),
  ].join("");
};

const space: Scene = (w, h, p, seed) => {
  const r = rand(seed);
  const s = Math.min(w, h);
  const stars = Array.from({ length: 40 }, () => `<circle cx="${(r() * w).toFixed(0)}" cy="${(r() * h * 0.7).toFixed(0)}" r="${(1 + r() * 2).toFixed(1)}" fill="${p.light}" opacity="${(0.3 + r() * 0.5).toFixed(2)}"/>`).join("");
  const cx = w * (0.35 + r() * 0.3);
  const cy = h * 0.4;
  const pr = s * 0.2;
  return [
    stars,
    `<circle cx="${cx.toFixed(0)}" cy="${cy.toFixed(0)}" r="${pr.toFixed(0)}" fill="${p.layers[0]}"/>`,
    `<ellipse cx="${cx.toFixed(0)}" cy="${cy.toFixed(0)}" rx="${(pr * 1.7).toFixed(0)}" ry="${(pr * 0.32).toFixed(0)}" fill="none" stroke="${p.light}" stroke-width="${(s * 0.012).toFixed(1)}" opacity="0.7" transform="rotate(-14 ${cx.toFixed(0)} ${cy.toFixed(0)})"/>`,
    hills(w, h, h * 0.9, h * 0.06, r, p.layers[2]),
  ].join("");
};

const city: Scene = (w, h, p, seed) => {
  const r = rand(seed);
  const s = Math.min(w, h);
  const out: string[] = [`<circle cx="${(w * 0.75).toFixed(0)}" cy="${(h * 0.22).toFixed(0)}" r="${(s * 0.09).toFixed(0)}" fill="${p.light}"/>`];
  for (const [layer, color, top] of [
    [0, p.layers[0], 0.42],
    [1, p.layers[1], 0.55],
    [2, p.layers[2], 0.7],
  ] as const) {
    let x = -10;
    while (x < w) {
      const bw = s * (0.08 + r() * 0.1);
      const by = h * (top + r() * 0.12);
      out.push(`<rect x="${x.toFixed(0)}" y="${by.toFixed(0)}" width="${(bw + 2).toFixed(0)}" height="${(h - by).toFixed(0)}" fill="${color}"/>`);
      if (layer === 2) {
        for (let wy = by + s * 0.03; wy < h - s * 0.03; wy += s * 0.05) {
          if (r() > 0.55) out.push(`<rect x="${(x + bw * 0.3).toFixed(0)}" y="${wy.toFixed(0)}" width="${(s * 0.016).toFixed(0)}" height="${(s * 0.02).toFixed(0)}" fill="${p.light}" opacity="0.8"/>`);
        }
      }
      x += bw;
    }
  }
  return out.join("");
};

const SCENES: Scene[] = [mountains, sea, forest, dunes, space, city];

function escapeXml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function svg(w: number, h: number, body: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${String(w)} ${String(h)}" width="${String(w)}" height="${String(h)}">${body}</svg>`)}`;
}

/** Title lines for the cover: at most two, split near the middle. */
function titleLines(title: string): string[] {
  const words = title.split(" ");
  if (title.length <= 12 || words.length === 1) return [title];
  let best = 1;
  for (let i = 1; i < words.length; i += 1) {
    if (Math.abs(words.slice(0, i).join(" ").length - title.length / 2) < Math.abs(words.slice(0, best).join(" ").length - title.length / 2)) best = i;
  }
  return [words.slice(0, best).join(" "), words.slice(best).join(" ")];
}

function artwork(title: string, index: number): GameArtwork {
  const palette = PALETTES[(index * 5) % PALETTES.length] ?? PALETTES[0];
  const scene = SCENES[(index * 7 + 3) % SCENES.length] ?? mountains;
  if (!palette) throw new Error("no palette");
  const seed = index * 977 + 13;
  const serif = index % 3 === 0;
  const font = serif ? "Georgia, 'DejaVu Serif', serif" : "'Inter', 'Helvetica Neue', Arial, sans-serif";
  const lines = titleLines(title.toUpperCase());
  const size = lines.some((l) => l.length > 11) ? 46 : 58;
  const text = lines
    .map((line, i) => `<text x="300" y="${String(112 + i * (size + 8))}" text-anchor="middle" font-family="${font}" font-size="${String(size)}" font-weight="${serif ? "400" : "700"}" letter-spacing="${serif ? "4" : "2"}" fill="${palette.ink}">${escapeXml(line)}</text>`)
    .join("");
  // The scene sits lower on the cover, so the title has the sky to itself.
  const cover = svg(600, 800, `<rect width="600" height="800" fill="${palette.sky}"/><g transform="translate(0 ${lines.length > 1 ? "110" : "70"})">${scene(600, 800, palette, seed)}</g>${text}`);
  const header = svg(1280, 600, `<rect width="1280" height="600" fill="${palette.sky}"/>${scene(1280, 600, palette, seed)}`);
  const shots = [1, 2, 3].map((v) => {
    const other = PALETTES[(index * 5 + v * 3) % PALETTES.length] ?? palette;
    return { url: svg(1280, 720, `<rect width="1280" height="720" fill="${other.sky}"/>${scene(1280, 720, other, seed + v * 31)}`), alt: `Screenshot ${String(v)}` };
  });
  return {
    cover: { url: cover, alt: `${title} cover` },
    header: { url: header, alt: `${title} header` },
    screenshots: shots,
  };
}

const TITLES = [
  "Tidewater Keep", "Paper Orbit", "Hollow Lantern", "Moss & Marrow", "The Glass Meridian", "Kitebound",
  "Saltmarsh Signal", "Lumen Drift", "Cinder Road", "Small Hours Society", "Granite Choir", "The Long Orchard",
  "Fernlight", "Copper Season", "Atlas Garden", "Velvet Static", "Lantern Bay", "Stillwater Station",
  "Ironbloom", "Skyward Ferry", "Quiet Harbor", "Ashfall Archive", "Beacon of Nine", "Dunewalker",
  "Orbital Gardener", "The Pale Cartographer", "Brightwater Rally", "Frostpine Lodge", "Night Tram 7", "Mirelight",
  "Hearth & Hollow", "Starfold", "The Ninth Lighthouse", "Rustbelt Racers", "Wren's Workshop", "Cloudline Express",
  "Emberwake", "Tinker Tides", "Hollowpeak Rangers", "Sunken Bells", "Glasshouse", "Driftlands",
  "Polar Static", "Sablewood", "Lowlight Detective", "Thornfield Tactics", "Arcadia Junction", "Willow & Wire",
  "Ninefold Crown", "Seabird Postal", "Clockwork Meadow", "Red Kite Rising", "Gravelight", "Bramble Knights",
  "Halcyon Drift", "Echo Basin", "Woolgather", "Silver Comet", "Ridgeback", "Lighthouse Keeper",
];

const STUDIOS = [
  "Lowtide Studio", "Pinecone Games", "Northlight Collective", "Fieldnote", "Two Moons",
  "Slow Coast Interactive", "Harbor & Hill", "Quietwork", "Paper Kite", "Mossfield",
];

const GENRES: Genre[][] = [
  ["adventure"], ["puzzle", "indie"], ["horror", "adventure"], ["rpg"], ["strategy"], ["platformer", "indie"],
  ["simulation"], ["action", "adventure"], ["racing"], ["puzzle"], ["rpg", "strategy"], ["simulation", "indie"],
];

const SUMMARIES = [
  "Keep a lighthouse running through a long winter and learn why the ships stopped coming.",
  "Fold paper planets into orbit and build a solar system that fits on a desk.",
  "Explore a sunken village by lantern light, one quiet room at a time.",
  "Tend a garden that grows back differently every season.",
  "Chart an unknown coast and trade maps with the towns you discover.",
  "A small story about trains, night shifts and the people who ride them.",
  "Rebuild a mountain town after the storm, one house and one neighbour at a time.",
  "Race restored cars across the old coast roads, from dawn until the tide comes in.",
];

const PLATFORMS: Platform[][] = [
  ["windows", "linux", "macos"], ["windows"], ["windows", "linux"], ["windows", "macos"],
];

/** Sixty invented games, with full artwork and plausible catalogue data. */
export function demoGames(): Game[] {
  return TITLES.map((title, i): Game => {
    const summary = SUMMARIES[i % SUMMARIES.length] ?? "";
    return {
      id: `igdb-${String(9000 + i)}`,
      title,
      summary,
      description: `${summary}\n\nA calm game with hand-drawn places, no timers and a story that unfolds at your own pace. Made by a small team over several years.`,
      genres: GENRES[i % GENRES.length] ?? ["adventure"],
      platforms: PLATFORMS[i % PLATFORMS.length] ?? ["windows"],
      releaseDate: `${String(2016 + (i % 10))}-${String(1 + ((i * 5) % 12)).padStart(2, "0")}-${String(3 + ((i * 7) % 25)).padStart(2, "0")}`,
      developer: STUDIOS[i % STUDIOS.length] ?? "Studio",
      publisher: STUDIOS[(i * 3 + 1) % STUDIOS.length] ?? "Studio",
      rating: 68 + ((i * 13) % 28),
      popularity: 100 - i,
      artwork: artwork(title, i),
      features: null,
    };
  });
}
