import { defineConfig, devices } from "@playwright/test";

/**
 * End-to-end tests of the main flows. They drive the real UI in Chromium
 * against the fake core (`e2e.html` → `src/test/e2e.ts`); the Rust core has
 * its own IPC tests (`src-tauri/src/ipc_tests.rs`).
 */
export default defineConfig({
  testDir: "e2e",
  testMatch: "**/*.e2e.ts",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: "http://localhost:1420",
    viewport: { width: 1280, height: 800 },
    locale: "de-DE",
    timezoneId: "Europe/Berlin",
    trace: "retain-on-failure",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } },
    // Same engine family as the Linux webview (WebKitGTK): the main flows run there too.
    // CI always; locally with COVE_E2E_WEBKIT=1 (Playwright has no WebKit build for every distribution).
    ...(process.env.CI || process.env.COVE_E2E_WEBKIT
      ? [
          {
            name: "webkit",
            use: { ...devices["Desktop Safari"], viewport: { width: 1280, height: 800 } },
            testMatch: ["**/library.e2e.ts", "**/catalogue.e2e.ts", "**/downloads.e2e.ts"],
          },
        ]
      : []),
  ],
  // A production build: realistic performance, and what users actually run.
  webServer: {
    command: "vite build --mode e2e && vite preview --mode e2e --outDir dist-e2e",
    url: "http://localhost:1420/e2e.html",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
