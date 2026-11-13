import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Statistik", () => {
  test("öffnet sich über das Profil und wechselt das Jahr", async ({ page }) => {
    await openCove(page, "/");
    await page.getByRole("link", { name: /Lokales Profil/ }).click();
    await expect(page.getByRole("heading", { level: 1, name: "Statistik" })).toBeVisible();
    await expect(page.getByText("Gesamt enthält die aus Steam")).toBeVisible();
    const top = page.getByRole("region", { name: "Am meisten gespielt" });
    await expect(top.getByRole("listitem")).toHaveCount(10);
    await expect(page.getByRole("table", { name: "Spielzeit pro Monat" })).toBeAttached();

    await page.getByLabel("Zeitraum").selectOption("2025");
    await expect(page.getByText("Gesamt enthält die aus Steam")).toBeHidden();
    await expect(page.getByRole("heading", { level: 2, name: "Spielzeit pro Monat" })).toBeVisible();

    // The choice is remembered.
    await page.reload();
    await expect(page.getByLabel("Zeitraum")).toHaveValue("2025");
  });
});
