import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

// Frame times mean something only on an idle machine: this file is its own
// Playwright project ("performance") that runs after every other test.
test.describe("Leistung", () => {
  test("2500 Spiele: nur sichtbare Karten im DOM, flüssiges Scrollen", async ({ page }) => {
    await openCove(page, "/library", { games: "2500" });
    const list = page.getByRole("list", { name: "Spiele" });
    await expect(list.getByRole("listitem").first()).toBeVisible();
    expect(await list.getByRole("listitem").count()).toBeLessThan(150);
    await expect(list.getByRole("listitem").first()).toHaveAttribute("aria-setsize", "2500");

    // Scroll through the whole library and record frame times.
    const frames = await page.evaluate(async () => {
      const main = document.getElementById("main");
      if (!main) throw new Error("no #main");
      const times: number[] = [];
      let last = performance.now();
      while (main.scrollTop + main.clientHeight < main.scrollHeight - 2) {
        main.scrollBy(0, 200);
        await new Promise(requestAnimationFrame);
        const now = performance.now();
        times.push(now - last);
        last = now;
      }
      return times.sort((a, b) => a - b);
    });
    const p95 = frames[Math.floor(frames.length * 0.95)] ?? 0;
    console.log(`Raster: ${String(frames.length)} Frames, p95 ${p95.toFixed(1)} ms`);
    // 60 fps = 16.7 ms per frame; allow one dropped frame on slow CI machines.
    expect(p95).toBeLessThan(34);
    // The last of the 2500 games is reachable.
    await expect(list.getByRole("listitem").filter({ hasText: "Quiet Harbor 105" })).toBeVisible();

    await page.getByRole("radio", { name: "Liste" }).click();
    const rows = page.getByRole("table").getByRole("row");
    expect(await rows.count()).toBeLessThan(100);
    await page.evaluate(() => document.getElementById("main")?.scrollTo({ top: 0 }));
    await expect(page.getByRole("table").getByText("Tidewater", { exact: true })).toBeVisible();
  });
});
