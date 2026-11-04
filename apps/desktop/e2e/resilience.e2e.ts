import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Ausfälle", () => {
  test("ohne Empfehlungsdienst bleibt Home nutzbar und Über Cove sagt, was aus ist", async ({ page }) => {
    await page.addInitScript(() => {
      // Runs before the app: switch the recommender off once the fake core exists.
      const timer = setInterval(() => {
        const cove = (window as unknown as { __cove?: { core: { recommender: boolean } } }).__cove;
        if (cove) {
          cove.core.recommender = false;
          clearInterval(timer);
        }
      }, 0);
    });
    await openCove(page, "/");
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    await expect(page.getByRole("region", { name: "Weiterspielen" })).toBeVisible();
    await expect(page.getByText(/Empfehlungen sind gerade nicht verfügbar|schiefgelaufen/)).toHaveCount(0);

    await page.goto("/e2e.html#/settings/about");
    await expect(page.getByText("Nicht verfügbar")).toBeVisible();
    await expect(page.getByText(/einfache Vorschläge statt persönlicher Empfehlungen/)).toBeVisible();
  });

  test("unbekannte Seiten zeigen eine freundliche Meldung", async ({ page }) => {
    await openCove(page, "/gibt-es-nicht");
    await expect(page.getByRole("heading", { name: "Diese Seite gibt es nicht" })).toBeVisible();
  });
});
