import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  // `--mode e2e` builds the Playwright entry (e2e.html with the fake core) into its own folder;
  // the app build only ever contains index.html.
  build:
    mode === "e2e"
      ? { target: "es2022", outDir: "dist-e2e", rolldownOptions: { input: "e2e.html" } }
      : { target: "es2022", sourcemap: Boolean(process.env.TAURI_ENV_DEBUG) },
  preview: { port: 1420, strictPort: true },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test/setup.ts"],
  },
}));
