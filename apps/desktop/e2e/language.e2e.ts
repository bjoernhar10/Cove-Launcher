import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Sprache", () => {
  test("auf Englisch umstellen: ganze Oberfläche und Barrierefreiheit", async ({ page }) => {
    await openCove(page, "/settings/general");
    await page.getByRole("combobox", { name: "Sprache" }).selectOption("en");

    await expect(page.getByRole("navigation", { name: "Sections" })).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.getByRole("link", { name: "Accounts & stores" })).toBeVisible();

    // Other pages follow without a restart.
    await page.getByRole("link", { name: "My Library" }).click();
    await expect(page.getByRole("button", { name: "Add game" })).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Grouping" })).toBeVisible();

    await page.getByRole("link", { name: "Settings" }).click();
    await page.getByRole("link", { name: "Accounts & stores" }).click();
    await expect(page.getByText("On this computer", { exact: true })).toBeVisible();
    await expect(page.locator("[aria-busy=true]")).toHaveCount(0);
    const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    expect(results.violations.map((v) => v.id)).toEqual([]);
  });
});
