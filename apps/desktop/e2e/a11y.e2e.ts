import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

/** Every page of Cove (dark only). */
const PAGES = [
  "/",
  "/library",
  "/library/game-0",
  "/library/game-0?tab=settings",
  "/library/game-3?tab=settings",
  "/catalogue",
  "/catalogue/igdb-1",
  "/downloads",
  "/settings/general",
  "/settings/appearance",
  "/settings/library",
  "/settings/accounts",
  "/settings/catalog",
  "/settings/downloads",
  "/settings/plugins",
  "/settings/about",
];

test.describe("WCAG 2.1 AA", () => {
  for (const route of PAGES) {
    test(route, async ({ page }) => {
      await openCove(page, route);
      await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
      await expect(page.locator("[aria-busy=true]")).toHaveCount(0);
      // Let fade-ins (150 to 200 ms) finish so colours are final.
      await page.waitForTimeout(300);
      const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
      const report = results.violations.map((v) => `${v.id}: ${v.help}\n  ${v.nodes.map((n) => n.target.join(" ")).slice(0, 5).join("\n  ")}`);
      expect(report, report.join("\n")).toEqual([]);
    });
  }
});

test("Fokus ist bei Tastaturbedienung immer sichtbar", async ({ page }) => {
  await openCove(page, "/library");
  await expect(page.getByRole("list", { name: "Spiele" })).toBeVisible();
  for (let i = 0; i < 25; i++) {
    await page.keyboard.press("Tab");
    const focus = await page.evaluate(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return null;
      const style = getComputedStyle(el);
      return { name: el.getAttribute("aria-label") ?? el.textContent.trim().slice(0, 30), outline: style.outlineStyle, width: style.outlineWidth };
    });
    if (!focus) continue;
    expect(focus.outline, `Fokusring fehlt bei „${focus.name}“`).not.toBe("none");
    expect(focus.width).not.toBe("0px");
  }
});
