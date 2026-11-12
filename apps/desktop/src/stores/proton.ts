import type { ProtonOverview, ProtonProgress, ProtonVersionId } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";
import { UMU_ID } from "../api/proton.ts";
import { messages } from "../i18n/index.ts";
import { invalidateQueries } from "./query.ts";
import { toast } from "./toasts.ts";

/** Readable name of an install: a GE-Proton release, or umu-launcher. */
export function installName(id: ProtonVersionId): string {
  return id === UMU_ID ? "umu-launcher" : id;
}

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
      if (progress.phase !== "done" && progress.phase !== "failed" && progress.phase !== "cancelled") {
        set((s) => ({ installs: { ...s.installs, [progress.id]: progress } }));
        return;
      }
      set((s) => ({ installs: Object.fromEntries(Object.entries(s.installs).filter(([id]) => id !== progress.id)) }));
      const t = messages().proton;
      const name = installName(progress.id);
      if (progress.phase === "done") toast(t.installed(name));
      else if (progress.phase === "cancelled") toast(t.cancelled(name));
      else toast(t.installFailed(name, progress.error ?? messages().common.genericError), "danger");
      invalidateQueries();
    }),
}));

/** The running install of a build: live events, else what the core listed. */
export function useProtonInstall(id: ProtonVersionId | null, overview: ProtonOverview | undefined): ProtonProgress | null {
  const live = useProton((s) => (id === null ? undefined : s.installs[id]));
  if (id === null) return null;
  return live ?? overview?.installing.find((p) => p.id === id) ?? null;
}
