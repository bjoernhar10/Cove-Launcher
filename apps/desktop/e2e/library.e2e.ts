import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Bibliothek", () => {
  test("ein Spiel von Hand hinzufügen", async ({ page }) => {
    await openCove(page, "/library");
    await page.getByRole("button", { name: "Spiel hinzufügen" }).first().click();
    const dialog = page.getByRole("dialog", { name: "Spiel hinzufügen" });
    await dialog.getByLabel("Name").fill("Meine Demo");
    await dialog.getByLabel("Pfad zur .exe / Binary").fill("/games/demo/start.sh");
    await dialog.getByRole("button", { name: "Hinzufügen" }).click();
    await expect(dialog).toBeHidden();

    // Cove opens the new game's page …
    await expect(page.getByRole("heading", { name: "Meine Demo", level: 1 })).toBeVisible();
    // … and lists it under "Manuell".
    await page.getByRole("navigation", { name: "Bereiche" }).getByRole("link", { name: /My Library/ }).click();
    await page.getByRole("button", { name: /Manuell/ }).click();
    await expect(page.getByRole("list", { name: "Spiele" }).getByRole("listitem").filter({ hasText: "Meine Demo" })).toBeVisible();
  });

  test("ungültige Pfade erklärt der Dialog, statt abzustürzen", async ({ page }) => {
    await openCove(page, "/library");
    await page.getByRole("button", { name: "Spiel hinzufügen" }).first().click();
    const dialog = page.getByRole("dialog", { name: "Spiel hinzufügen" });
    await dialog.getByLabel("Name").fill("Fehlt");
    await dialog.getByLabel("Pfad zur .exe / Binary").fill("/games/missing/start");
    await dialog.getByRole("button", { name: "Hinzufügen" }).click();
    await expect(dialog.getByText("Datei nicht gefunden: /games/missing/start")).toBeVisible();
  });

  test("ein Spiel starten und beenden", async ({ page }) => {
    await openCove(page, "/library/game-0");
    await expect(page.getByRole("heading", { name: "Tidewater", level: 1 })).toBeVisible();
    await page.getByRole("button", { name: "Spielen", exact: true }).click();
    await expect(page.getByText("„Tidewater“ wird gestartet …")).toBeVisible();

    const stop = page.getByRole("button", { name: /Läuft/ });
    await expect(stop).toBeVisible();
    await stop.click();
    await expect(page.getByText("Sitzung gespeichert: 25 Min.")).toBeVisible();
    await expect(page.getByRole("button", { name: "Spielen", exact: true })).toBeVisible();
  });

  test("2500 Spiele: nur sichtbare Karten im DOM, flüssiges Scrollen", async ({ page, browserName }) => {
    await openCove(page, "/library", { games: "2500" });
    const list = page.getByRole("list", { name: "Spiele" });
    await expect(list.getByRole("listitem").first()).toBeVisible();
    expect(await list.getByRole("listitem").count()).toBeLessThan(150);
    await expect(list.getByRole("listitem").first()).toHaveAttribute("aria-setsize", "2500");

    if (browserName !== "chromium") {
      // Frame timing is measured in Chromium only (CI's software WebKit is too slow to be meaningful);
      // here it is enough that the end of the virtual list is reachable.
      await page.evaluate(() => {
        const main = document.getElementById("main");
        main?.scrollTo({ top: main.scrollHeight });
      });
      await expect(list.getByRole("listitem").filter({ hasText: "Quiet Harbor 105" })).toBeVisible();
      return;
    }

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
