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
    ["/storage", "storage"],
    ["/settings/emulators", "emulators"],
  ];
  for (const [route, name] of shots) {
    await go(page, route);
    await page.screenshot({ path: `${OUT}/${name}.png` });
  }

  // Downloads at night (after the downloads page, which would show the waiting queue).
  await page.evaluate(() => {
    interface Cove {
      core: { settings: { downloads: { schedule: { enabled: boolean; from: string; until: string } } } };
    }
    (window as unknown as { __cove: Cove }).__cove.core.settings.downloads.schedule = { enabled: true, from: "01:00", until: "07:00" };
  });
  await go(page, "/settings/downloads");
  await page.screenshot({ path: `${OUT}/schedule.png` });

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

  // Cloud sync: Google Drive connected, and the same game played on a Steam Deck too.
  await page.evaluate((id) => {
    interface Core {
      saveSync: Record<string, unknown>;
      saveSyncGames: Record<string, unknown>;
    }
    const core = (window as unknown as { __cove: { core: Core } }).__cove.core;
    core.saveSync = { ...core.saveSync, provider: "googleDrive", account: "alex@example.com", device: "Desktop" };
    (core as unknown as { librarySyncedAt: string }).librarySyncedAt = new Date(Date.now() - 4 * 60_000).toISOString();
    core.saveSyncGames[id] = { state: "conflict", syncedAt: null, conflict: { device: "Steam Deck", savedAt: new Date(Date.now() - 3 * 3600_000).toISOString() } };
  }, protonId);
  await go(page, "/settings/backups");
  await page.getByRole("heading", { name: "Cloud sync" }).evaluate((el) => {
    el.scrollIntoView({ block: "start" });
    document.getElementById("main")?.scrollBy(0, -320);
  });
  await settle(page);
  await page.screenshot({ path: `${OUT}/cloud-sync.png` });
  await go(page, `/library/${protonId}?tab=settings`);
  await page.getByRole("heading", { name: "Cloud sync" }).evaluate((el) => {
    el.scrollIntoView({ block: "start" });
    document.getElementById("main")?.scrollBy(0, -48);
  });
  await settle(page);
  await page.screenshot({ path: `${OUT}/save-conflict.png` });

  // Frame rate history of the first game (MangoHud, Linux).
  await go(page, `/library/${first}`);
  // Scroll only the content (the page is short: scrollIntoView would move the whole window).
  await page.getByRole("heading", { name: "Frame rate" }).evaluate((el) => {
    const main = document.getElementById("main");
    if (!main) return;
    main.scrollTop += el.getBoundingClientRect().top - main.getBoundingClientRect().top - 420;
    window.scrollTo(0, 0);
  });
  await settle(page);
  await page.screenshot({ path: `${OUT}/frame-rate.png` });

  // Notes on the first game's overview, and its Screenshots tab (the drawn demo artwork).
  const firstGame = demo.library[0];
  if (!firstGame) throw new Error("no demo games");
  await page.evaluate((game) => {
    interface Core {
      notes: Record<string, unknown>;
      screenshots: Record<string, unknown>;
    }
    const core = (window as unknown as { __cove: { core: Core } }).__cove.core;
    core.notes[game.id] = {
      text: "Chapter 4: the lighthouse. Second key is behind the waterfall.\nStill to do: the north island and the last two shrines.",
      updatedAt: new Date(Date.now() - 2 * 86_400_000).toISOString(),
    };
    core.screenshots[game.id] = [...game.artwork.screenshots, game.artwork.header].map((image, i) => ({
      url: image.url,
      fileName: `2026111${String(9 - i)}_2015${String(i)}0.jpg`,
      takenAt: new Date(Date.now() - (i + 1) * 86_400_000).toISOString(),
      sizeBytes: 400_000,
      source: i === 0 ? "game" : "steam",
    }));
  }, firstGame);
  // Away and back, so the game page loads the note.
  await go(page, "/library");
  await go(page, `/library/${first}`);
  await page.getByRole("heading", { name: "Notes" }).evaluate((el) => {
    const main = document.getElementById("main");
    if (!main) return;
    main.scrollTop += el.getBoundingClientRect().top - main.getBoundingClientRect().top - 360;
    window.scrollTo(0, 0);
  });
  await settle(page);
  await page.screenshot({ path: `${OUT}/notes.png` });
  await go(page, `/library/${first}?tab=screenshots`);
  await page.screenshot({ path: `${OUT}/screenshots.png` });

  // Games from other launchers in the add-game dialog (invented titles).
  await page.evaluate(() => {
    const game = (id: string, launcher: string, title: string, launchUrl: string | null, winePrefix: string | null = null) => ({
      id,
      launcher,
      title,
      executablePath: `/games/${id}.exe`,
      windows: winePrefix !== null,
      proton: false,
      winePrefix,
      wineBinary: null,
      arguments: "",
      playtimeMinutes: 0,
      installPath: null,
      launchUrl,
      inLibrary: false,
    });
    const cove = (window as unknown as { __cove: { core: { externalGames: unknown[] } } }).__cove;
    cove.core.externalGames = [
      game("ubisoft:101", "ubisoft", "Harbour Lights", "uplay://launch/101/0"),
      game("battlenet:north", "battleNet", "Northwind Keep", "battlenet://NWK"),
      game("xbox:Studio.PaleOrchard", "xbox", "Pale Orchard", "shell:AppsFolder\\Studio.PaleOrchard_8wekyb3d8bbwe!Game"),
      game("ea:{7}", "ea", "Glass Reef Racing", null),
      game("bottles:Games/a1", "bottles", "Copper Moth", null, "/home/alex/.local/share/bottles/bottles/Games"),
    ];
  });
  await go(page, "/library");
  await page.getByRole("button", { name: "Add game" }).click();
  await page.getByRole("radio", { name: "Other launchers" }).click();
  await settle(page);
  await page.screenshot({ path: `${OUT}/launchers.png` });
});
