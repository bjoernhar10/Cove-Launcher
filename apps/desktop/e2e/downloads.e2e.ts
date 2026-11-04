import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Downloads", () => {
  test("ein DRM-freies Spiel per Link laden und installieren", async ({ page }) => {
    await openCove(page, "/downloads");
    await page.getByRole("button", { name: "Download per Link" }).first().click();
    const dialog = page.getByRole("dialog", { name: "Download per Link" });
    await dialog.getByLabel("Link").fill("https://example.com/files/harbor-lights.zip");
    await dialog.getByLabel("Name (optional)").fill("Harbor Lights");
    await dialog.getByRole("button", { name: "Herunterladen" }).click();

    // Progress in the page and in the sidebar while it runs …
    await expect(page.getByRole("progressbar", { name: "Fortschritt Harbor Lights" })).toBeVisible();
    await expect(page.getByRole("link", { name: /Download läuft: Harbor Lights/ })).toBeVisible();
    // … then it lands in the history and in the library.
    await expect(page.getByText("Prüfsumme bestätigt")).toBeVisible({ timeout: 10_000 });
    await page.getByRole("navigation", { name: "Bereiche" }).getByRole("link", { name: /My Library/ }).click();
    await page.getByRole("button", { name: /Manuell/ }).click();
    await expect(page.getByRole("list", { name: "Spiele" }).getByRole("listitem").filter({ hasText: "Harbor Lights" })).toBeVisible();
  });

  test("Store-Spiele übergibt Cove an den Store", async ({ page }) => {
    await openCove(page, "/library/game-1");
    await page.getByRole("button", { name: "Installieren", exact: true }).click();
    await expect(page.getByText(/Installation von „Hollow Lantern“ an GOG übergeben/)).toBeVisible();
  });

  test("ungültige Links meldet der Dialog verständlich", async ({ page }) => {
    await openCove(page, "/downloads");
    await page.getByRole("button", { name: "Download per Link" }).first().click();
    const dialog = page.getByRole("dialog", { name: "Download per Link" });
    await dialog.getByLabel("Link").fill("ftp://example.com/x.zip");
    await dialog.getByRole("button", { name: "Herunterladen" }).click();
    await expect(dialog.getByText(/kein gültiger Link|https/)).toBeVisible();
  });
});
