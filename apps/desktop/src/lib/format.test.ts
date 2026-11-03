import { describe, expect, it } from "vitest";
import { formatBytes, formatEta, formatPlaytime, formatRelative } from "./format.ts";

describe("format", () => {
  it("formats playtime", () => {
    expect(formatPlaytime(0)).toBe("Noch nie gespielt");
    expect(formatPlaytime(45)).toBe("45 Min.");
    expect(formatPlaytime(90)).toBe("1,5 Std.");
    expect(formatPlaytime(754)).toBe("13 Std.");
  });

  it("formats bytes with decimal units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1_500_000)).toBe("1,5 MB");
    expect(formatBytes(48_100_000_000)).toBe("48,1 GB");
  });

  it("formats remaining time", () => {
    expect(formatEta(30)).toBe("30 Sek.");
    expect(formatEta(600)).toBe("10 Min.");
    expect(formatEta(3900)).toBe("1 Std. 5 Min.");
    expect(formatEta(Number.POSITIVE_INFINITY)).toBe("—");
  });

  it("formats relative time in German", () => {
    const now = new Date("2026-11-03T12:00:00Z");
    expect(formatRelative("2026-11-01T12:00:00Z", now)).toBe("vorgestern");
    expect(formatRelative("2026-11-03T11:59:30Z", now)).toBe("gerade eben");
  });
});
