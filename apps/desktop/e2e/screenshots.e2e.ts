import type { Game } from "@cove/shared";
import { expect, test, type Page } from "@playwright/test";
import { LINUX_USER_AGENT } from "./app.ts";
import { demoGames } from "./demoGames.ts";

/**
 * README screenshots (`docs/screenshots`), in English, on Linux, with invented
 * games and drawn artwork (`demoGames.ts`): no real game or store artwork.
 * Only runs on request: `COVE_SCREENSHOTS=1 npx playwright test screenshots`.
 */
test.skip(!process.env.COVE_SCREENSHOTS, "set COVE_SCREENSHOTS=1 to update the README screenshots");
test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, locale: "en-US", userAgent: LINUX_USER_AGENT });

const OUT = "../../docs/screenshots";

interface Demo {
  library: Game[];
  catalog: Game[];
  total: number;
}

function loadDemo(): Demo {
  const games = demoGames();
  const library = games.filter((_, i) => i % 3 !== 2).slice(0, 36);
  return { library, catalog: games, total: games.length };
}

async function settle(page: Page): Promise<void> {
  await expect(page.locator("[aria-busy=true]")).toHaveCount(0);
  // Every image in view has loaded (lazy ones further down never start).
  await page.waitForFunction(() =>
    [...document.images].every((img) => {
      const box = img.getBoundingClientRect();
      return img.complete || box.top > window.innerHeight || box.bottom < 0;
    }),
  );
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
        ["a", { status: "downloading", totalBytes: 2100 * mb, downloadedBytes: 1365 * mb, bytesPerSecond: 18 * mb, queuePosition: 0, origin: "gog", source: "gog" }],
        ["b", { status: "queued", totalBytes: 840 * mb, queuePosition: 1, origin: "itch" }],
        ["c", { status: "handedOff", queuePosition: 2, origin: "steam", source: "steam" }],
        ["d", { status: "completed", totalBytes: 512 * mb, downloadedBytes: 512 * mb, queuePosition: null, checksumVerified: true, finishedAt: "2026-11-11T09:12:00Z", origin: "direct" }],
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

async function go(page: Page, route: string): Promise<void> {
  await page.evaluate((r) => {
    window.location.hash = `#${r}`;
  }, route);
  await settle(page);
}

test("screenshots", async ({ page }) => {
  test.setTimeout(120_000);
  const demo = loadDemo();
  await page.addInitScript((data) => {
    localStorage.setItem("cove.language", "en");
    (window as unknown as { __coveDemo: unknown }).__coveDemo = data;
  }, demo);
  await page.goto("/e2e.html#/");
  await expect(page.getByRole("navigation", { name: "Sections" })).toBeVisible();
  await seedDownloads(page, demo);
  await settle(page);
  await page.screenshot({ path: `${OUT}/home.png` });

  const first = demo.library[0]?.id ?? "";
  const shots: [string, string][] = [
    ["/library", "library"],
    [`/library/${first}`, "game"],
    ["/catalogue", "catalogue"],
    ["/downloads", "downloads"],
    ["/stats", "stats"],
    ["/wishlist", "wishlist"],
    ["/settings/emulators", "emulators"],
  ];
  for (const [route, name] of shots) {
    await go(page, route);
    await page.screenshot({ path: `${OUT}/${name}.png` });
  }

  // The TV mode with a PlayStation controller, the focus on the first game.
  await go(page, "/tv");
  await page.evaluate(() => {
    (window as unknown as { __cove: { emit: (event: string, payload: unknown) => void } }).__cove.emit("gamepad-input", { action: "down", kind: "playstation" });
  });
  await settle(page);
  await page.screenshot({ path: `${OUT}/tv.png` });
  await go(page, "/library");

  // The Proton game's settings tab: Proton version, game options, prefix and save backups.
  const protonId = await page.evaluate(() => {
    interface Item {
      game: { id: string };
      entry: { launchOptions: { compatibilityTool: string } };
    }
    const cove = (window as unknown as { __cove: { core: { library: Item[] } } }).__cove;
    return cove.core.library.find((i) => i.entry.launchOptions.compatibilityTool === "proton")?.game.id ?? "";
  });
  await go(page, `/library/${protonId}?tab=settings`);
  await page.getByRole("combobox", { name: "Proton version" }).evaluate((el) => {
    el.scrollIntoView({ block: "start" });
    document.getElementById("main")?.scrollBy(0, -180);
  });
  await settle(page);
  await page.screenshot({ path: `${OUT}/game-options.png` });
  await page.getByRole("heading", { name: "Save backups" }).evaluate((el) => {
    el.scrollIntoView({ block: "start" });
    document.getElementById("main")?.scrollBy(0, -48);
  });
  await settle(page);
  await page.screenshot({ path: `${OUT}/backups.png` });
});
