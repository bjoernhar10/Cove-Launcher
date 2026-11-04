import { expect, type Page } from "@playwright/test";

/** Opens Cove (with the fake core) at a route, e.g. `/library`. */
export async function openCove(page: Page, route = "/", params: Record<string, string> = {}): Promise<void> {
  const query = new URLSearchParams(params).toString();
  await page.goto(`/e2e.html${query ? `?${query}` : ""}#${route}`);
  await expect(page.getByRole("navigation", { name: "Bereiche" })).toBeVisible();
}

/** The toast region at the bottom of the window. */
export function toasts(page: Page) {
  return page.getByRole("status").filter({ hasText: /\S/ });
}
