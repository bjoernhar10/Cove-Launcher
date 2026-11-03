/**
 * The ONLY bridge between UI and backend.
 *
 * - `call()` invokes a typed Tauri command of the Rust core (see `CoveCommands`).
 * - `subscribe()` listens to core events (see `CoveEvents`).
 */
import type { AppErrorKind, AppErrorPayload, CoveCommand, CoveCommands, CoveEvent, CoveEvents } from "@cove/shared";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** Error raised for every failed core call; `kind` mirrors the Rust `AppError` enum. */
export class CoveError extends Error {
  readonly kind: AppErrorKind | "unavailable";

  constructor(kind: AppErrorKind | "unavailable", message: string) {
    super(message);
    this.name = "CoveError";
    this.kind = kind;
  }
}

function isErrorPayload(value: unknown): value is AppErrorPayload {
  return typeof value === "object" && value !== null && "kind" in value && "message" in value;
}

function hasCore(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const UNAVAILABLE = "Das geht nur in der Desktop-App (npm run dev).";

export async function call<C extends CoveCommand>(command: C, args: CoveCommands[C]["args"]): Promise<CoveCommands[C]["result"]> {
  if (!hasCore()) throw new CoveError("unavailable", UNAVAILABLE);
  try {
    return await invoke<CoveCommands[C]["result"]>(command, args);
  } catch (error: unknown) {
    if (isErrorPayload(error)) throw new CoveError(error.kind, error.message);
    throw new CoveError("io", error instanceof Error ? error.message : String(error));
  }
}

/** Listens to a core event. Returns an unsubscribe function (safe to call before setup finished). */
export function subscribe<E extends CoveEvent>(event: E, handler: (payload: CoveEvents[E]) => void): () => void {
  if (!hasCore()) return () => undefined;
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  void listen<CoveEvents[E]>(event, (e) => {
    handler(e.payload);
  }).then((fn) => {
    if (cancelled) fn();
    else unlisten = fn;
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
