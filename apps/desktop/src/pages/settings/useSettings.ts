import type { AppSettings } from "@cove/shared";
import { api } from "../../api/index.ts";
import { useQuery } from "../../hooks/useQuery.ts";
import { queryClient } from "../../lib/queryClient.ts";
import { usePreferences } from "../../stores/preferences.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";

/** Loads settings once and saves every change immediately. */
export function useSettings(): {
  settings: AppSettings | null;
  error: string | null;
  retry: () => void;
  save: (fn: (s: AppSettings) => AppSettings) => void;
} {
  const query = useQuery("settings", api.settings.getSettings);
  const save = (fn: (s: AppSettings) => AppSettings): void => {
    if (query.status !== "success") return;
    const { accent } = usePreferences.getState();
    const next = { ...fn(query.data), appearance: { theme: "dark" as const, accent } };
    query.update(() => next);
    const languageChanged = next.general.language !== query.data.general.language;
    api.settings.saveSettings(next).then(
      () => {
        // Texts from the core (errors, store status, reasons) follow the new language.
        if (!languageChanged) return;
        invalidateQueries();
        void queryClient.invalidateQueries();
      },
      (error: unknown) => {
        toast(errorMessage(error), "danger");
        query.retry();
      },
    );
  };
  return {
    settings: query.status === "success" ? query.data : null,
    error: query.status === "error" ? query.error : null,
    retry: query.retry,
    save,
  };
}
