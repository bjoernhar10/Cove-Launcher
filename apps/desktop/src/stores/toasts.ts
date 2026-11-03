import { create } from "zustand";

export interface Toast {
  id: number;
  message: string;
  tone: "neutral" | "danger";
}

interface ToastState {
  toasts: Toast[];
  push: (message: string, tone?: Toast["tone"]) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToasts = create<ToastState>()((set, get) => ({
  toasts: [],
  push: (message, tone = "neutral") => {
    const id = nextId++;
    set((s) => ({ toasts: [...s.toasts.slice(-2), { id, message, tone }] }));
    setTimeout(() => {
      get().dismiss(id);
    }, 3200);
  },
  dismiss: (id) => {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
}));

/** Convenience for non-React code paths. */
export function toast(message: string, tone?: Toast["tone"]): void {
  useToasts.getState().push(message, tone);
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
