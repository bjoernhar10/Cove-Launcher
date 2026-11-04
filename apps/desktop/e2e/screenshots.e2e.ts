import { expect, test, type Page } from "@playwright/test";
import { openCove } from "./app.ts";

/**
 * README screenshots (`docs/screenshots`). Only runs on request:
 * `COVE_SCREENSHOTS=1 npx playwright test screenshots`.
 */
test.skip(!process.env.COVE_SCREENSHOTS, "set COVE_SCREENSHOTS=1 to update the README screenshots");
test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });

const OUT = "../../docs/screenshots";

async function settle(page: Page): Promise<void> {
  await expect(page.locator("[aria-busy=true]")).toHaveCount(0);
  await page.waitForTimeout(400);
}

async function seedDownloads(page: Page): Promise<void> {
  await page.evaluate(() => {
    interface Cove {
      core: { downloads: unknown[] };
      emit: (event: string, payload: unknown) => void;
      fakeDownload: (id: string, title: string, overrides: Record<string, unknown>) => unknown;
    }
    const cove = (window as unknown as { __cove: Cove }).__cove;
    const mb = 1024 * 1024;
    cove.core.downloads.push(
      cove.fakeDownload("a", "Ember Fields", { status: "downloading", totalBytes: 2100 * mb, downloadedBytes: 1365 * mb, bytesPerSecond: 18 * mb, queuePosition: 0, origin: "itch" }),
      cove.fakeDownload("b", "Glass Meridian", { status: "queued", totalBytes: 840 * mb, queuePosition: 1, origin: "direct" }),
      cove.fakeDownload("c", "Northwind", { status: "handedOff", queuePosition: 2, origin: "gog" }),
      cove.fakeDownload("d", "Small Hours", { status: "completed", totalBytes: 512 * mb, downloadedBytes: 512 * mb, queuePosition: null, checksumVerified: true, finishedAt: "2026-11-04T09:12:00Z", origin: "direct" }),
    );
    cove.emit("downloads-changed", structuredClone(cove.core.downloads));
  });
}

for (const theme of ["light", "dark"] as const) {
  test(`screenshots (${theme})`, async ({ page }) => {
    await openCove(page, "/", { theme, games: "60" });
    await seedDownloads(page);
    await settle(page);
    await page.screenshot({ path: `${OUT}/home-${theme}.png` });

    for (const [route, name] of [
      ["/library", "library"],
      ["/library/game-2", "game"],
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
}
