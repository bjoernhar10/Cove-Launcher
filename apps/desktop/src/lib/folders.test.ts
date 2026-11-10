import { describe, expect, it } from "vitest";
import { folderName, isArchive, joinPath } from "./folders.ts";

describe("folderName", () => {
  it("matches the folder the core creates", () => {
    expect(folderName("Saltmarsh")).toBe("Saltmarsh");
    expect(folderName("Half-Life 2: Episode One")).toBe("Half-Life 2 Episode One");
    expect(folderName("Ember & Oak")).toBe("Ember and Oak");
    expect(folderName("Baldur's Gate 3 (Deluxe)")).toBe("Baldur's Gate 3 (Deluxe)");
    expect(folderName('What?/\\*<>|"Now')).toBe("WhatNow");
    expect(folderName("Ōkami HD")).toBe("Ōkami HD");
    expect(folderName("  ...hidden.  ")).toBe("hidden");
    expect(folderName("???")).toBe("Game");
    expect(folderName("con")).toBe("con Game");
    expect(folderName("x".repeat(200))).toHaveLength(80);
  });
});

describe("joinPath", () => {
  it("keeps the separator of the folder", () => {
    expect(joinPath("/home/u/Games", "Saltmarsh")).toBe("/home/u/Games/Saltmarsh");
    expect(joinPath("D:\\Games", "Saltmarsh")).toBe("D:\\Games\\Saltmarsh");
    expect(joinPath("D:\\", "Saltmarsh")).toBe("D:\\Saltmarsh");
  });
});

describe("isArchive", () => {
  it("knows the formats the core unpacks", () => {
    expect(isArchive("zeta-linux.zip")).toBe(true);
    expect(isArchive("game.tar.gz")).toBe(true);
    expect(isArchive("Game.AppImage")).toBe(false);
    expect(isArchive("setup.exe")).toBe(false);
  });
});
