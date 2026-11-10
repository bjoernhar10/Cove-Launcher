import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { openCove } from "./app.ts";

test.describe("Installierte Spiele", () => {
  test("zeigt die Startart und verschiebt ein Spiel in einen anderen Spiele-Ordner", async ({ page }) => {
    // game-3 is a manual game installed in /games/game-3.
    await openCove(page, "/library/game-3?tab=settings");
    await expect(page.getByText("Direkt").first()).toBeVisible();
    const section = page.getByRole("region", { name: "Spiel verschieben" });
    await expect(section.getByLabel("Zielordner")).toHaveValue("/home/u/Games");
    await section.getByRole("button", { name: "Verschieben" }).click();
    await expect(section.getByRole("progressbar")).toBeVisible();
    await expect(page.getByText(/wurde verschoben/).first()).toBeVisible();
    await page.getByRole("tab", { name: "Übersicht" }).click();
    await expect(page.getByText(/^\/home\/u\/Games\//)).toBeVisible();
  });

  test("ändert die DLCs eines Spiels, das Cove installiert hat", async ({ page }) => {
    await openCove(page, "/downloads");
    await page.evaluate(() => {
      interface Entry {
        source: string;
        storeGameId: string | null;
        installed: boolean;
        installedByCove: boolean;
      }
      const cove = (
        window as unknown as {
          __cove: {
            core: {
              accounts: { epic: { connected: boolean; status: string } };
              library: { game: { id: string }; entry: Entry }[];
              dlcs: Record<string, unknown[]>;
            };
          };
        }
      ).__cove;
      Object.assign(cove.core.accounts.epic, { connected: true, status: "connected" });
      const item = cove.core.library.find((i) => i.game.id === "game-1");
      if (item) Object.assign(item.entry, { source: "epic", storeGameId: "Sugar", installed: true, installedByCove: true });
      cove.core.dlcs["game-1"] = [
        { id: "Island", title: "Sugar Island", downloadBytes: 1_000_000_000, installBytes: 2_000_000_000, selected: true },
        { id: "Market", title: "Night Market", downloadBytes: 500_000_000, installBytes: 500_000_000, selected: false },
      ];
      window.location.hash = "#/library/game-1";
    });
    const section = page.getByRole("region", { name: "DLCs" });
    await section.getByRole("button", { name: "DLCs ändern" }).click();
    const dialog = page.getByRole("dialog", { name: /^DLCs von / });
    await dialog.getByRole("checkbox", { name: /Night Market/ }).check();
    const axe = await new AxeBuilder({ page }).include("dialog[open]").withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    expect(axe.violations.map((v) => v.id)).toEqual([]);
    await dialog.getByRole("button", { name: "Übernehmen" }).click();
    await expect(page.getByText(/werden angepasst/).first()).toBeVisible();
  });
});
