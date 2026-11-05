import type { Language } from "@cove/shared";
import { useSyncExternalStore } from "react";
import { de } from "./de.ts";
import { en } from "./en.ts";

/** The shape every language must provide: `de` is the source, `en` mirrors it. */
export type Messages = typeof de;

const DICTIONARIES: Record<Language, Messages> = { de, en };
const LOCALES: Record<Language, string> = { de: "de-DE", en: "en-US" };
const STORAGE_KEY = "cove.language";

function initialLanguage(): Language {
  try {
    const stored = globalThis.localStorage.getItem(STORAGE_KEY);
    if (stored === "de" || stored === "en") return stored;
  } catch {
    // no storage (tests): fall through
  }
  return globalThis.navigator.language.toLowerCase().startsWith("de") ? "de" : "en";
}

let current: Language = initialLanguage();
const listeners = new Set<() => void>();

export function isLanguage(value: unknown): value is Language {
  return value === "de" || value === "en";
}

export function language(): Language {
  return current;
}

/** Switches the UI language; the app re-renders from the root. */
export function setLanguage(next: Language): void {
  if (next === current) return;
  current = next;
  try {
    globalThis.localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // ignore
  }
  document.documentElement.lang = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The UI texts of the current language (for code outside React components). */
export function messages(): Messages {
  return DICTIONARIES[current];
}

/** BCP 47 locale for `Intl` formatting. */
export function locale(): string {
  return LOCALES[current];
}

export function useLanguage(): Language {
  return useSyncExternalStore(subscribe, language, language);
}

/** The UI texts of the current language; re-renders on a language change. */
export function useMessages(): Messages {
  return DICTIONARIES[useLanguage()];
}

/**
 * A label table (`SOURCE_LABELS[store]`) that always answers in the current
 * language, so lookups stay plain property access.
 */
export function localized<T extends object>(select: (m: Messages) => T): T {
  return new Proxy({} as T, {
    get: (_, key) => Reflect.get(select(messages()), key),
    has: (_, key) => Reflect.has(select(messages()), key),
    ownKeys: () => Reflect.ownKeys(select(messages())),
    getOwnPropertyDescriptor: (_, key) => {
      const descriptor = Reflect.getOwnPropertyDescriptor(select(messages()), key);
      return descriptor ? { ...descriptor, configurable: true } : undefined;
    },
  });
}
