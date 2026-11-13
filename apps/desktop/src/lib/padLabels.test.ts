import { describe, expect, it } from "vitest";
import { glyphFor } from "./padLabels.ts";

describe("controller labels", () => {
  it("show the button that sits at the position", () => {
    expect(glyphFor("confirm", "xbox")).toEqual({ type: "letter", text: "A" });
    expect(glyphFor("confirm", "playstation")).toEqual({ type: "shape", shape: "cross" });
    expect(glyphFor("back", "playstation")).toEqual({ type: "shape", shape: "circle" });
    // Nintendo's letters sit the other way round: bottom is B, right is A.
    expect(glyphFor("confirm", "switch")).toEqual({ type: "letter", text: "B" });
    expect(glyphFor("back", "switch")).toEqual({ type: "letter", text: "A" });
    expect(glyphFor("tertiary", "switch")).toEqual({ type: "letter", text: "X" });
    expect(glyphFor("nextTab", "playstation")).toEqual({ type: "bumper", text: "R1" });
    expect(glyphFor("prevTab", "switch")).toEqual({ type: "bumper", text: "L" });
    expect(glyphFor("prevTab", "xbox")).toEqual({ type: "bumper", text: "LB" });
  });
});
