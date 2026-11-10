import AxeBuilder from "@axe-core/playwright";
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

    // Install dialog: sizes, games folder with free space, then "Installieren".
    const install = page.getByRole("dialog", { name: "Harbor Lights installieren" });
    await expect(install.getByText(/frei auf diesem Laufwerk/)).toBeVisible();
    await expect(install.getByText("Wird installiert in /home/u/Games/Harbor Lights")).toBeVisible();
    const axe = await new AxeBuilder({ page }).include("dialog[open]").withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    expect(axe.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
    await install.getByRole("button", { name: "Installieren" }).click();

    // Progress in the page and in the sidebar while it runs …
    await expect(page.getByRole("progressbar", { name: "Fortschritt Harbor Lights" })).toBeVisible();
    await expect(page.getByRole("link", { name: /Download läuft: Harbor Lights/ })).toBeVisible();
    // … then it lands in the history and in the library.
    await expect(page.getByText("Prüfsumme bestätigt")).toBeVisible({ timeout: 10_000 });
    await page.getByRole("navigation", { name: "Bereiche" }).getByRole("link", { name: /My Library/ }).click();
    await page.getByRole("button", { name: /Manuell/ }).click();
    await expect(page.getByRole("list", { name: "Spiele" }).getByRole("listitem").filter({ hasText: "Harbor Lights" })).toBeVisible();
  });

  test("Steam-Spiele übergibt Cove an Steam", async ({ page }) => {
    // game-4 is a Steam game that isn't installed.
    await openCove(page, "/library/game-4");
    await page.getByRole("button", { name: "Installieren", exact: true }).click();
    await expect(page.getByText(/Installation von „Ember Fields“ an Steam übergeben/)).toBeVisible();
  });

  test("GOG-Spiele installiert Cove selbst, mit Install-Dialog", async ({ page }) => {
    await openCove(page, "/library/game-1");
    await page.evaluate(() => {
      const cove = (
        window as unknown as {
          __cove: { core: { accounts: { gog: { connected: boolean; status: string } }; dlcs: Record<string, unknown[]> } };
        }
      ).__cove;
      Object.assign(cove.core.accounts.gog, { connected: true, status: "connected" });
      cove.core.dlcs["game-1"] = [
        { id: "200", title: "Deep Water", downloadBytes: 800_000_000, installBytes: 1_000_000_000, selected: true },
        { id: "300", title: "Soundtrack", downloadBytes: 200_000_000, installBytes: 200_000_000, selected: true },
      ];
    });
    await page.getByRole("button", { name: "Installieren", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Hollow Lantern installieren" });
    await expect(dialog.getByText(/frei auf diesem Laufwerk/)).toBeVisible();
    // DLCs: a button opens the list; unticking one updates the count.
    await dialog.getByRole("button", { name: "DLCs auswählen" }).click();
    await dialog.getByRole("checkbox", { name: /Soundtrack/ }).uncheck();
    await expect(dialog.getByText("1 von 2 DLCs werden mitinstalliert")).toBeVisible();
    const axe = await new AxeBuilder({ page }).include("dialog[open]").withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    expect(axe.violations.map((v) => v.id)).toEqual([]);
    await dialog.getByRole("button", { name: "Installieren" }).click();
    await expect(page.getByText("„Hollow Lantern“ wird heruntergeladen").first()).toBeVisible();
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
