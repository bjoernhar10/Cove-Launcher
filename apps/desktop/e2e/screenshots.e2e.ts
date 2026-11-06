import type { Game, ProxyPage } from "@cove/shared";
import { expect, test, type Page } from "@playwright/test";
import { openCove } from "./app.ts";

/**
 * README screenshots (`docs/screenshots`) with real games: titles, covers and
 * artwork come from the Cove catalogue proxy (IGDB), exactly as the app shows them.
 * Only runs on request: `COVE_SCREENSHOTS=1 npx playwright test screenshots`.
 */
test.skip(!process.env.COVE_SCREENSHOTS, "set COVE_SCREENSHOTS=1 to update the README screenshots");
test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });

const OUT = "../../docs/screenshots";
const PROXY = process.env.COVE_CATALOG_PROXY_URL ?? "https://cove-catalog-proxy.bjoernharter-github.workers.dev";

interface Demo {
  library: Game[];
  catalog: Game[];
  total: number;
}

async function browse(query: string): Promise<ProxyPage> {
  const response = await fetch(`${PROXY}/v1/browse?${query}`);
  if (!response.ok) throw new Error(`Katalog nicht erreichbar: ${String(response.status)}`);
  return (await response.json()) as ProxyPage;
}

/** A plausible library (well-known games with full artwork) and a catalogue to browse. */
async function loadDemo(): Promise<Demo> {
  const pages = await Promise.all([0, 1, 2, 3].map((page) => browse(`sort=popular&page=${String(page)}`)));
  const seen = new Set<string>();
  const games = pages
    .flatMap((p) => p.items)
    .filter((g) => g.artwork.cover.url && g.artwork.header.url && !seen.has(g.id) && seen.add(g.id));
  const library = games.filter((_, i) => i % 3 !== 2).slice(0, 36);
  return { library, catalog: games, total: pages[0]?.total ?? games.length };
}

async function settle(page: Page): Promise<void> {
  await expect(page.locator("[aria-busy=true]")).toHaveCount(0);
  // Wait until every visible cover image has loaded.
  await page.waitForFunction(() => [...document.images].every((img) => img.complete));
  await page.waitForTimeout(400);
}

async function seedDownloads(page: Page, demo: Demo): Promise<void> {
  const owned = new Set(demo.library.map((g) => g.id));
  const [active, queued, handedOff, done] = demo.catalog.filter((g) => !owned.has(g.id)).slice(-4);
  await page.evaluate(
    (games) => {
      interface Cove {
        core: { downloads: { game: unknown }[] };
        emit: (event: string, payload: unknown) => void;
        fakeDownload: (id: string, title: string, overrides: Record<string, unknown>) => { game: unknown };
      }
      const cove = (window as unknown as { __cove: Cove }).__cove;
      const mb = 1024 * 1024;
      const specs: [string, Record<string, unknown>][] = [
        ["a", { status: "downloading", totalBytes: 2100 * mb, downloadedBytes: 1365 * mb, bytesPerSecond: 18 * mb, queuePosition: 0, origin: "itch" }],
        ["b", { status: "queued", totalBytes: 840 * mb, queuePosition: 1, origin: "direct" }],
        ["c", { status: "handedOff", queuePosition: 2, origin: "gog" }],
        ["d", { status: "completed", totalBytes: 512 * mb, downloadedBytes: 512 * mb, queuePosition: null, checksumVerified: true, finishedAt: "2026-11-04T09:12:00Z", origin: "direct" }],
      ];
      specs.forEach(([id, overrides], i) => {
        const game = games[i] as { title: string } | undefined;
        if (!game) return;
        const item = cove.fakeDownload(id, game.title, overrides);
        item.game = game;
        cove.core.downloads.push(item);
      });
      cove.emit("downloads-changed", structuredClone(cove.core.downloads));
    },
    [active, queued, handedOff, done],
  );
}

let demo: Demo;
test.beforeAll(async () => {
  demo = await loadDemo();
});

const theme = "dark";
test("screenshots", async ({ page }) => {
  await page.addInitScript((data) => {
    (window as unknown as { __coveDemo: unknown }).__coveDemo = data;
  }, demo);
  await openCove(page, "/");
  await seedDownloads(page, demo);
  await settle(page);
  await page.screenshot({ path: `${OUT}/home-${theme}.png` });

  for (const [route, name] of [
    ["/library", "library"],
    [`/library/${demo.library[0]?.id ?? ""}`, "game"],
    ["/catalogue", "catalogue"],
    ["/downloads", "downloads"],
    ["/settings/accounts", "settings"],
  ] as const) {
    await page.evaluate((r) => {
      window.location.hash = `#${r}`;
    }, route);
    await settle(page);
    await page.screenshot({ path: `${OUT}/${name}-${theme}.png` });
  }
});
