import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { LINUX_USER_AGENT, openCove } from "./app.ts";

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
  "/wishlist",
  "/stats",
  "/settings/general",
  "/settings/appearance",
  "/settings/library",
  "/settings/backups",
  "/settings/emulators",
  "/settings/accounts",
  "/settings/catalog",
  "/settings/downloads",
  "/settings/plugins",
  "/settings/about",
];

async function expectAccessible(page: Page, route: string): Promise<void> {
  await openCove(page, route);
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("[aria-busy=true]")).toHaveCount(0);
  await expectNoViolations(page);
}

async function expectNoViolations(page: Page): Promise<void> {
  // Let fade-ins (150 to 200 ms) finish so colours are final.
  await page.waitForTimeout(300);
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
  const report = results.violations.map((v) => `${v.id}: ${v.help}\n  ${v.nodes.map((n) => n.target.join(" ")).slice(0, 5).join("\n  ")}`);
  expect(report, report.join("\n")).toEqual([]);
}

test.describe("WCAG 2.1 AA", () => {
  for (const route of PAGES) {
    test(route, async ({ page }) => {
      await expectAccessible(page, route);
    });
  }
});

test.describe("WCAG 2.1 AA unter Linux", () => {
  test.use({ userAgent: LINUX_USER_AGENT });

  // The game page shows the ProtonDB rating there.
  test("/library/game-0", async ({ page }) => {
    await expectAccessible(page, "/library/game-0");
    await expect(page.getByRole("region", { name: "Linux-Kompatibilität" }).getByText("Gold", { exact: true })).toBeVisible();
  });

  // Proton: the settings tab and the version of a Windows game.
  for (const route of ["/settings/proton", "/library/game-11?tab=settings"]) {
    test(route, async ({ page }) => {
      await expectAccessible(page, route);
    });
  }
});

test("das Sammlungen-Menü ist zugänglich und mit der Tastatur bedienbar", async ({ page }) => {
  await openCove(page, "/library/game-0");
  // The button also counts the game's collections ("Sammlungen 1").
  const button = page.getByRole("button", { name: /^Sammlungen/ });
  await button.focus();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("dialog", { name: "Sammlungen für Tidewater" });
  // Focus starts on the first collection and stays inside the menu.
  await expect(menu.getByRole("checkbox", { name: "Mit Freunden" })).toBeFocused();
  await expectNoViolations(page);
  await page.keyboard.press("Space");
  await expect(menu.getByRole("checkbox", { name: "Mit Freunden" })).toBeChecked();
  await page.keyboard.press("Tab");
  await expect(menu.getByLabel("Name der neuen Sammlung")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(menu.getByRole("checkbox", { name: "Mit Freunden" })).toBeFocused();
  // Esc closes it and focus goes back to the button.
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(button).toBeFocused();
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
