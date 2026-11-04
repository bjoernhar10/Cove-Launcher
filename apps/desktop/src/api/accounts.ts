import type {
  AchievementsSyncSummary,
  ConnectableStore,
  ConnectorLoginFinished,
  ConnectorLoginStart,
  CoveAchievement,
  ImportSummary,
  LocalStoreGame,
  StoreAccount,
  SyncProgress,
} from "@cove/shared";
import { call, subscribe } from "./transport.ts";

export function listAccounts(): Promise<StoreAccount[]> {
  return call("connectors_list", {});
}

/** Opens the store's login (browser or own window). The outcome arrives via `subscribeLoginFinished`. */
export function startLogin(store: ConnectableStore): Promise<ConnectorLoginStart> {
  return call("connector_login_start", { store });
}

/** Alternative login: the code the store showed after signing in. */
export function completeLogin(store: ConnectableStore, code: string): Promise<StoreAccount> {
  return call("connector_login_complete", { store, code });
}

export function cancelLogin(store: ConnectableStore): Promise<null> {
  return call("connector_login_cancel", { store });
}

export function refreshAccount(store: ConnectableStore): Promise<StoreAccount> {
  return call("connector_refresh", { store });
}

export function logout(store: ConnectableStore, keepGames: boolean): Promise<StoreAccount> {
  return call("connector_logout", { store, keepGames });
}

export function detectLocal(store: ConnectableStore | null = null): Promise<LocalStoreGame[]> {
  return call("connector_detect_local", { store });
}

export function importLibrary(store: ConnectableStore): Promise<ImportSummary> {
  return call("connector_import_library", { store });
}

export function syncAchievements(store: ConnectableStore | null = null): Promise<AchievementsSyncSummary> {
  return call("achievements_sync", { store });
}

export function listCoveAchievements(): Promise<CoveAchievement[]> {
  return call("cove_achievements_list", {});
}

export function subscribeLoginFinished(handler: (event: ConnectorLoginFinished) => void): () => void {
  return subscribe("connector-login-finished", handler);
}

export function subscribeSyncProgress(handler: (event: SyncProgress) => void): () => void {
  return subscribe("store-sync-progress", handler);
}

export function subscribeAchievementUnlocked(handler: (event: CoveAchievement) => void): () => void {
  return subscribe("achievement-unlocked", handler);
}
