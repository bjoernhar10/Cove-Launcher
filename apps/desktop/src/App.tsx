import { LucideProvider } from "lucide-react";
import { useEffect, useState } from "react";
import { RouterProvider } from "react-router";
import { createAppRouter } from "./router.tsx";
import { applyAppearance, usePreferences } from "./stores/preferences.ts";

/** Keeps <html data-theme/data-accent> in sync with preferences and the OS. */
function useAppearance(): void {
  const theme = usePreferences((s) => s.theme);
  const accent = usePreferences((s) => s.accent);
  useEffect(() => {
    applyAppearance(theme, accent);
    if (theme !== "system") return;
    const media = globalThis.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      applyAppearance(theme, accent);
    };
    media.addEventListener("change", onChange);
    return () => {
      media.removeEventListener("change", onChange);
    };
  }, [theme, accent]);
}

export function App() {
  const [router] = useState(createAppRouter);
  useAppearance();
  return (
    // Icons: lucide-react, 18px, stroke 1.75 — set once for the whole app.
    <LucideProvider size={18} strokeWidth={1.75}>
      <RouterProvider router={router} />
    </LucideProvider>
  );
}
