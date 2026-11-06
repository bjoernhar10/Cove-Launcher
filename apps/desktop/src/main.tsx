import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { language } from "./i18n/index.ts";
import { applyAppearance, usePreferences } from "./stores/preferences.ts";
import "./styles/global.css";

// Apply the persisted accent before the first paint to avoid a flash.
applyAppearance(usePreferences.getState().accent);
document.documentElement.lang = language();

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing");

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
