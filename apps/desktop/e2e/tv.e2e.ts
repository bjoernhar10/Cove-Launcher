import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { openCove } from "./app.ts";

async function pad(page: Page, action: string, kind = "xbox"): Promise<void> {
  await page.evaluate(
    ([a, k]) => {
      (window as unknown as { __cove: { emit: (event: string, payload: unknown) => void } }).__cove.emit("gamepad-input", { action: a, kind: k });
    },
    [action, kind],
  );
}

test.describe("TV-Modus", () => {
  test("mit dem Controller durch Cove: Spiel öffnen, zurück, Bereiche wechseln, Tasten passend zum Controller", async ({ page }) => {
    await openCove(page, "/");
    await page.getByRole("link", { name: "TV-Modus" }).click();
    const sections = page.getByRole("navigation", { name: "TV-Bereiche" });
    await expect(sections).toBeVisible();
    await expect(page.getByRole("region", { name: "Weiterspielen" })).toBeVisible();

    // The first press puts the focus on the first game, the next ones move it.
    await pad(page, "down");
    const first = page.getByRole("region", { name: "Weiterspielen" }).getByRole("link").first();
    await expect(first).toBeFocused();
    await pad(page, "right");
    const second = page.getByRole("region", { name: "Weiterspielen" }).getByRole("link").nth(1);
    await expect(second).toBeFocused();
    const title = (await second.locator("p").last().textContent()) ?? "";
    await pad(page, "confirm");
    await expect(page.getByRole("heading", { level: 1, name: title })).toBeVisible();
    await pad(page, "back");
    await expect(page.getByRole("region", { name: "Weiterspielen" })).toBeVisible();

    // RB/R1 switches to the library; the labels follow the controller.
    await pad(page, "nextTab", "playstation");
    await expect(sections.getByRole("link", { name: "Bibliothek" })).toHaveAttribute("aria-current", "page");
    const hints = page.getByRole("list", { name: "Controller-Tasten" });
    await expect(hints.getByRole("img", { name: "Taste Kreuz" })).toBeVisible();
    await expect(hints.getByRole("img", { name: "Taste Kreis" })).toBeVisible();
    await pad(page, "prevTab", "switch");
    // Nintendo: the bottom button is B, the right one A.
    await expect(hints.getByRole("img", { name: "Taste B" })).toBeVisible();
    await expect(sections.getByRole("img", { name: "Taste L" })).toBeVisible();

    // Search with the on-screen keyboard.
    await pad(page, "tertiary", "xbox");
    const keyboard = page.getByRole("group", { name: "Bildschirmtastatur" });
    for (const key of ["T", "I", "D"]) await keyboard.getByRole("button", { name: key, exact: true }).click();
    await expect(page.getByRole("link", { name: "Tidewater" })).toBeVisible();

    // Start opens the settings, from there back to the desktop.
    await pad(page, "menu");
    await page.getByRole("button", { name: "Zum Desktop-Modus" }).click();
    await expect(page.getByRole("navigation", { name: "Bereiche" })).toBeVisible();
  });

  test("der Controller bedient auch den Desktop und schließt Dialoge", async ({ page }) => {
    await openCove(page, "/library");
    await page.getByRole("button", { name: "Spiel hinzufügen" }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await pad(page, "back");
    await expect(page.getByRole("dialog")).toBeHidden();
    await pad(page, "home");
    await expect(page.getByRole("navigation", { name: "TV-Bereiche" })).toBeVisible();
  });

  for (const route of ["/tv", "/tv/library", "/tv/search", "/tv/downloads", "/tv/settings", "/tv/game/game-0"]) {
    test(`WCAG 2.1 AA: ${route}`, async ({ page }) => {
      await openCove(page, "/");
      await page.evaluate((r) => {
        window.location.hash = `#${r}`;
      }, route);
      await expect(page.getByRole("navigation", { name: "TV-Bereiche" })).toBeVisible();
      await expect(page.locator("[aria-busy=true]")).toHaveCount(0);
      await page.waitForTimeout(300);
      const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
      const report = results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(", ")}`);
      expect(report, report.join("\n")).toEqual([]);
    });
  }
});
