import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Katalog", () => {
  test("durchsuchen, filtern und ein Spiel zur Bibliothek hinzufügen", async ({ page }) => {
    await openCove(page, "/catalogue");
    const games = page.getByRole("list", { name: "Spiele" });
    await expect(games.getByRole("listitem").first()).toBeVisible();

    await page.getByRole("searchbox", { name: "Im Katalog suchen" }).fill("Hollow");
    await page.getByRole("button", { name: /Genre/ }).click();
    await page.getByRole("checkbox", { name: "Horror" }).check();
    await page.keyboard.press("Escape");
    await expect(games.getByRole("listitem")).toHaveCount(1);

    await games.getByRole("link", { name: /Hollow Lantern/ }).click();
    await expect(page.getByRole("heading", { name: "Hollow Lantern", level: 1 })).toBeVisible();
    await page.getByRole("button", { name: "Zur Bibliothek hinzufügen" }).click();
    await expect(page.getByRole("link", { name: "In deiner Bibliothek" })).toBeVisible();
  });

  test("findet Spiele über die Befehlspalette", async ({ page }) => {
    await openCove(page, "/");
    await page.keyboard.press("Control+k");
    await page.getByRole("combobox", { name: "Suche" }).fill("paper");
    await expect(page.getByRole("option", { name: /Paper Orbit/ }).first()).toBeVisible();
  });
});
