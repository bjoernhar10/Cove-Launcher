import { afterEach, describe, expect, it } from "vitest";
import { formatPlaytime } from "../lib/format.ts";
import { GENRE_LABELS, SOURCE_LABELS } from "../lib/labels.ts";
import { de } from "./de.ts";
import { en } from "./en.ts";
import { language, messages, setLanguage } from "./index.ts";

/** Every key path with the kind of value behind it ("string" or "function"). */
function shape(value: unknown, path = ""): string[] {
  if (typeof value === "string" || typeof value === "function") return [`${path}:${typeof value}`];
  if (typeof value !== "object" || value === null) throw new Error(`unexpected value at ${path}`);
  return Object.entries(value).flatMap(([key, child]) => shape(child, path ? `${path}.${key}` : key));
}

afterEach(() => {
  setLanguage("de");
});

describe("i18n", () => {
  it("has the same keys and value kinds in German and English", () => {
    expect(shape(en)).toEqual(shape(de));
  });

  it("has no untranslated or empty English texts", () => {
    const strings = (value: unknown, path: string[] = []): [string, string][] => {
      if (typeof value === "string") return [[path.join(" › "), value]];
      if (typeof value !== "object" || value === null) return [];
      return Object.entries(value).flatMap(([key, child]) => strings(child, [...path, key]));
    };
    const all = strings(en);
    // Intentionally empty in both languages.
    const blank = new Set(["library › emptyGroup › all › title", "library › emptyGroup › all › description", "settings › componentImpact › core"]);
    expect(all.filter(([path, text]) => text === "" && !blank.has(path))).toEqual([]);
    // Umlauts only appear in German copy; an English text with one was left untranslated.
    expect(all.filter(([, text]) => /[äöüÄÖÜß„]/.test(text))).toEqual([]);
  });

  it("switches texts, label tables and formats at once", () => {
    setLanguage("en");
    expect(language()).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    expect(messages().common.retry).toBe("Try again");
    expect(SOURCE_LABELS.manual).toBe("Manual");
    expect(GENRE_LABELS.rpg).toBe("Role-playing");
    expect(Object.keys(SOURCE_LABELS)).toEqual(["steam", "gog", "epic", "manual"]);
    expect(formatPlaytime(450)).toBe("7.5 h");
    expect(formatPlaytime(0)).toBe("Never played");

    setLanguage("de");
    expect(SOURCE_LABELS.manual).toBe("Manuell");
    expect(formatPlaytime(450)).toBe("7,5 Std.");
  });
});
