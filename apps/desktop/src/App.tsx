import { QueryClientProvider } from "@tanstack/react-query";
import { LucideProvider } from "lucide-react";
import { useEffect, useState } from "react";
import { RouterProvider } from "react-router";
import { queryClient } from "./lib/queryClient.ts";
import { useLanguage } from "./i18n/index.ts";
import { createAppRouter } from "./router.tsx";
import { applyAppearance, usePreferences } from "./stores/preferences.ts";

/** Keeps <html data-theme/data-accent> in sync with the preferences. */
function useAppearance(): void {
  const accent = usePreferences((s) => s.accent);
  useEffect(() => {
    applyAppearance(accent);
  }, [accent]);
}

export function App() {
  const [router] = useState(createAppRouter);
  // A language switch re-renders the whole UI, so every text and format follows it.
  const language = useLanguage();
  useAppearance();
  return (
    // Icons: lucide-react, 18px, stroke 1.75, set once for the whole app.
    <QueryClientProvider client={queryClient}>
      <LucideProvider size={18} strokeWidth={1.75}>
        <RouterProvider key={language} router={router} />
      </LucideProvider>
    </QueryClientProvider>
  );
}
