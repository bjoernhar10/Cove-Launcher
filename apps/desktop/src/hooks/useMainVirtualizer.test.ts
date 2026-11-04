import { describe, expect, it } from "vitest";
import { columnsFor } from "./useMainVirtualizer.ts";

describe("columnsFor", () => {
  it("matches CSS repeat(auto-fill, minmax(160px, 1fr)) with a 20px gap", () => {
    expect(columnsFor(0, 160, 20)).toBe(1);
    expect(columnsFor(159, 160, 20)).toBe(1);
    expect(columnsFor(340, 160, 20)).toBe(2);
    expect(columnsFor(339, 160, 20)).toBe(1);
    expect(columnsFor(1136, 160, 20)).toBe(6);
  });
});
