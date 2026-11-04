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

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
