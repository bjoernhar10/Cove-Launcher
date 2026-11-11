import type { ProtonOverview, ProtonProgress, ProtonVersionId } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";
import { messages } from "../i18n/index.ts";
import { invalidateQueries } from "./query.ts";
import { toast } from "./toasts.ts";

interface ProtonState {
  /** GE-Proton builds being installed, with their latest progress. */
  installs: Record<ProtonVersionId, ProtonProgress>;
  /** Listens for `proton-progress`. Returns the unsubscribe function. */
  connect: () => () => void;
}

export const useProton = create<ProtonState>()((set) => ({
  installs: {},
  connect: () =>
    api.proton.subscribeProgress((progress) => {
      if (progress.phase !== "done" && progress.phase !== "failed") {
        set((s) => ({ installs: { ...s.installs, [progress.id]: progress } }));
        return;
      }
      set((s) => ({ installs: Object.fromEntries(Object.entries(s.installs).filter(([id]) => id !== progress.id)) }));
      const t = messages().proton;
      if (progress.phase === "done") toast(t.installed(progress.id));
      else toast(t.installFailed(progress.id, progress.error ?? messages().common.genericError), "danger");
      invalidateQueries();
    }),
}));

/** The running install of a build: live events, else what the core listed. */
export function useProtonInstall(id: ProtonVersionId | null, overview: ProtonOverview | undefined): ProtonProgress | null {
  const live = useProton((s) => (id === null ? undefined : s.installs[id]));
  if (id === null) return null;
  return live ?? overview?.installing.find((p) => p.id === id) ?? null;
}
