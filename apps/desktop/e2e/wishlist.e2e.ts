import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Wunschliste", () => {
  test("Spiel aus dem Katalog vormerken, Preise sehen, Wunschpreis setzen", async ({ page }) => {
    await openCove(page, "/catalogue/igdb-1");
    // Prices on the catalogue page, with the mention of IsThereAnyDeal.
    const prices = page.getByRole("region", { name: "Preise" });
    await expect(prices.getByText("bei GOG")).toBeVisible();
    await expect(prices.getByText(/Tiefstpreis/)).toBeVisible();
    await expect(page.getByRole("button", { name: "Preise von IsThereAnyDeal" })).toBeVisible();

    await page.getByRole("button", { name: "Auf die Wunschliste" }).click();
    await expect(page.getByRole("button", { name: "Auf der Wunschliste" })).toHaveAttribute("aria-pressed", "true");

    await page.evaluate(() => {
      window.location.hash = "#/wishlist";
    });
    await expect(page.getByRole("heading", { level: 1, name: "Wunschliste" })).toBeVisible();
    const target = page.getByLabel("Wunschpreis für „Tidewater“");
    await target.fill("abc");
    await target.press("Enter");
    await expect(page.getByText("Gib einen Preis wie 9,99 ein")).toBeVisible();
    await target.fill("7,50");
    await target.press("Enter");
    // The core keeps it: away and back again.
    await page.evaluate(() => {
      window.location.hash = "#/catalogue";
    });
    await page.evaluate(() => {
      window.location.hash = "#/wishlist";
    });
    await expect(page.getByLabel("Wunschpreis für „Tidewater“")).toHaveValue("7.5");

    await page.getByRole("button", { name: "„Tidewater“ von der Wunschliste nehmen" }).click();
    await expect(page.getByText("Deine Wunschliste ist leer")).toBeVisible();
  });
});
