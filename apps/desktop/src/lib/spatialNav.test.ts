import { describe, expect, it } from "vitest";
import { pick, type Box } from "./spatialNav.ts";

const box = (left: number, top: number, w = 100, h = 100): Box => ({ left, top, right: left + w, bottom: top + h });

describe("spatial navigation", () => {
  // A 3×2 grid of cards with 20px gaps, and a wide button above it.
  const grid = [box(0, 200), box(120, 200), box(240, 200), box(0, 320), box(120, 320), box(240, 320)];
  const header = box(0, 0, 340, 40);
  const all = [header, ...grid];

  it("moves through a grid like a person expects", () => {
    const from = grid[1] ?? box(0, 0);
    expect(all[pick(from, all, "right")]).toBe(grid[2]);
    expect(all[pick(from, all, "left")]).toBe(grid[0]);
    expect(all[pick(from, all, "down")]).toBe(grid[4]);
    expect(all[pick(from, all, "up")]).toBe(header);
  });

  it("stays in the row before jumping to another one", () => {
    // A card a bit lower but further right loses to the one in the same row.
    const row = [box(120, 0), box(240, 30), box(600, 0)];
    expect(pick(box(0, 0), row, "right")).toBe(0);
    expect(pick(box(240, 0), [box(0, 0)], "right")).toBe(-1);
  });
});
