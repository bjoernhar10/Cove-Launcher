import { messages } from "../i18n/index.ts";
import { create } from "zustand";

export interface Toast {
  id: number;
  message: string;
  tone: "neutral" | "danger";
  /** Small heading above the message (e.g. "Achievement freigeschaltet"). */
  title?: string;
  kind?: "achievement";
  /** Link shown next to the message (e.g. "Öffnen" → a game page). */
  action?: { label: string; to: string };
}

export interface ToastOptions {
  title?: string;
  kind?: "achievement";
  action?: { label: string; to: string };
  durationMs?: number;
}

interface ToastState {
  toasts: Toast[];
  push: (message: string, tone?: Toast["tone"], options?: ToastOptions) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToasts = create<ToastState>()((set, get) => ({
  toasts: [],
  push: (message, tone = "neutral", options = {}) => {
    const id = nextId++;
    const { durationMs = 3200, ...rest } = options;
    set((s) => ({ toasts: [...s.toasts.slice(-2), { id, message, tone, ...rest }] }));
    setTimeout(() => {
      get().dismiss(id);
    }, durationMs);
  },
  dismiss: (id) => {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
}));

/** Convenience for non-React code paths. */
export function toast(message: string, tone?: Toast["tone"], options?: ToastOptions): void {
  useToasts.getState().push(message, tone, options);
}

/** An error whose message is written for the user (in the UI language). */
export class UserFacingError extends Error {
  override readonly name = "UserFacingError";
}

/**
 * Text for the UI. Messages from the core (`CoveError`) and `UserFacingError`
 * are already written for people; anything else is a programming error whose
 * details belong in the console, not in front of the user.
 */
export function errorMessage(error: unknown): string {
  if (error instanceof Error && (error.name === "CoveError" || error instanceof UserFacingError)) return error.message;
  console.error("[cove]", error);
  return messages().common.genericError;
}
