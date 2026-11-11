import type { ProtonOverview, ProtonProgress, ProtonVersionId } from "@cove/shared";
import { call, subscribe } from "./transport.ts";

/** Value of `protonVersion` / `defaultProton` for "always the newest GE-Proton". */
export const LATEST_PROTON = "latest";

/** Linux: GE-Proton releases, installed builds and Steam's builds. */
export function overview(): Promise<ProtonOverview> {
  return call("proton_list", {});
}

/** Loads the release list again; a new "latest" is installed in the background. */
export function checkUpdates(): Promise<ProtonOverview> {
  return call("proton_check", {});
}

/** Starts installing a GE-Proton build; progress comes through `subscribeProgress`. */
export async function install(versionId: ProtonVersionId): Promise<void> {
  await call("proton_install", { versionId });
}

/** Deletes a GE-Proton build no game uses. */
export async function remove(versionId: ProtonVersionId): Promise<void> {
  await call("proton_remove", { versionId });
}

/** `proton-progress`: download, check and unpack of a GE-Proton build. */
export function subscribeProgress(listener: (progress: ProtonProgress) => void): () => void {
  return subscribe("proton-progress", listener);
}
