import { expect, test } from "@playwright/test";
import { LINUX_USER_AGENT, openCove, toasts } from "./app.ts";

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

  test("ein Spiel per Rechtsklick in eine eigene Sammlung legen", async ({ page }) => {
    await openCove(page, "/library");
    const games = page.getByRole("list", { name: "Spiele" });
    const groups = page.getByRole("navigation", { name: "Gruppierung" });
    await games.getByRole("listitem").filter({ hasText: "Tidewater" }).click({ button: "right" });
    const menu = page.getByRole("dialog", { name: "Sammlungen für Tidewater" });
    await expect(menu.getByRole("checkbox", { name: "Mit Freunden" })).not.toBeChecked();

    // A new collection from the menu gets the game right away.
    await menu.getByLabel("Name der neuen Sammlung").fill("Angefangen");
    await page.keyboard.press("Enter");
    await expect(menu.getByRole("checkbox", { name: "Angefangen" })).toBeChecked();
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();

    await groups.getByRole("button", { name: /Angefangen/ }).click();
    await expect(games.getByRole("listitem")).toHaveCount(1);
    await expect(games.getByRole("listitem").filter({ hasText: "Tidewater" })).toBeVisible();

    // Renaming and deleting sit next to the view switch; the games stay.
    await page.getByRole("button", { name: "Sammlung umbenennen" }).click();
    const rename = page.getByRole("dialog", { name: "Sammlung umbenennen" });
    await rename.getByLabel("Name").fill("Mit Freunden");
    await rename.getByRole("button", { name: "Speichern" }).click();
    await expect(rename.getByText("Eine Sammlung „Mit Freunden“ gibt es schon.")).toBeVisible();
    await rename.getByLabel("Name").fill("Durchgespielt");
    await rename.getByRole("button", { name: "Speichern" }).click();
    await expect(groups.getByRole("button", { name: /Durchgespielt/ })).toHaveAttribute("aria-pressed", "true");

    await page.getByRole("button", { name: "Sammlung löschen" }).click();
    await page.getByRole("dialog", { name: "Sammlung „Durchgespielt“ löschen?" }).getByRole("button", { name: "Sammlung löschen" }).click();
    await expect(toasts(page).getByText("Sammlung „Durchgespielt“ gelöscht.")).toBeVisible();
    await expect(groups.getByRole("button", { name: /Alle/ })).toHaveAttribute("aria-pressed", "true");
    await expect(games.getByRole("listitem").filter({ hasText: "Tidewater" })).toBeVisible();
  });

  test("die Spielseite legt ein Spiel in Sammlungen", async ({ page }) => {
    await openCove(page, "/library/game-0");
    await page.getByRole("button", { name: "Sammlungen", exact: true }).click();
    const menu = page.getByRole("dialog", { name: "Sammlungen für Tidewater" });
    await menu.getByRole("checkbox", { name: "Mit Freunden" }).check();
    await expect(page.getByRole("button", { name: /^Sammlungen\s*1$/ })).toBeVisible();
    // A press outside closes the menu.
    await page.getByRole("heading", { name: "Tidewater", level: 1 }).click();
    await expect(menu).toBeHidden();

    await page.getByRole("navigation", { name: "Bereiche" }).getByRole("link", { name: /My Library/ }).click();
    await page.getByRole("navigation", { name: "Gruppierung" }).getByRole("button", { name: /Mit Freunden/ }).click();
    await expect(page.getByRole("list", { name: "Spiele" }).getByRole("listitem")).toHaveCount(3);
  });

  test("ohne Linux gibt es keine ProtonDB-Bewertung", async ({ page }) => {
    // The test browsers report Windows (Chromium) and macOS (WebKit).
    await openCove(page, "/library/game-0");
    await expect(page.getByRole("region", { name: "Informationen" })).toBeVisible();
    await expect(page.getByRole("region", { name: "Linux-Kompatibilität" })).toHaveCount(0);
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

test.describe("Bibliothek unter Linux", () => {
  test.use({ userAgent: LINUX_USER_AGENT });

  test("die Spielseite zeigt die ProtonDB-Bewertung", async ({ page }) => {
    await openCove(page, "/library/game-0");
    const rating = page.getByRole("region", { name: "Linux-Kompatibilität" });
    await expect(rating.getByText("Gold", { exact: true })).toBeVisible();
    await expect(rating.getByText("2.107 Berichte auf ProtonDB")).toBeVisible();
    await expect(rating.getByText("Läuft nach kleinen Anpassungen einwandfrei.")).toBeVisible();
    // Steam game: the rating is for exactly this version.
    await expect(rating.getByText("Die Bewertung gilt für die Steam-Version des Spiels.")).toHaveCount(0);
    await expect(rating.getByRole("button", { name: "Auf ProtonDB ansehen" })).toBeVisible();

    // A GOG game is rated through its Steam version; a game without reports says so.
    await page.evaluate(() => {
      window.location.hash = "#/library/game-1";
    });
    await expect(rating.getByText("Platin", { exact: true })).toBeVisible();
    await expect(rating.getByText("Die Bewertung gilt für die Steam-Version des Spiels.")).toBeVisible();
    await page.evaluate(() => {
      window.location.hash = "#/library/game-4";
    });
    await expect(rating.getByText("Zu diesem Spiel gibt es auf ProtonDB noch keine Bewertung.")).toBeVisible();
  });
});
