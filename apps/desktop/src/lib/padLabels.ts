import type { ControllerKind, PadAction } from "@cove/shared";

/** How a button looks: a letter in a circle, a PlayStation shape, a bumper, an icon. */
export type Glyph =
  | { type: "letter"; text: string }
  | { type: "shape"; shape: "cross" | "circle" | "square" | "triangle" }
  | { type: "bumper"; text: string }
  | { type: "menu" | "view" | "home" | "dpad" };

const FACE: Record<"confirm" | "back" | "secondary" | "tertiary", Record<ControllerKind, Glyph>> = {
  // Bottom button.
  confirm: {
    xbox: { type: "letter", text: "A" },
    steam: { type: "letter", text: "A" },
    generic: { type: "letter", text: "A" },
    playstation: { type: "shape", shape: "cross" },
    switch: { type: "letter", text: "B" },
  },
  // Right button.
  back: {
    xbox: { type: "letter", text: "B" },
    steam: { type: "letter", text: "B" },
    generic: { type: "letter", text: "B" },
    playstation: { type: "shape", shape: "circle" },
    switch: { type: "letter", text: "A" },
  },
  // Left button.
  secondary: {
    xbox: { type: "letter", text: "X" },
    steam: { type: "letter", text: "X" },
    generic: { type: "letter", text: "X" },
    playstation: { type: "shape", shape: "square" },
    switch: { type: "letter", text: "Y" },
  },
  // Top button.
  tertiary: {
    xbox: { type: "letter", text: "Y" },
    steam: { type: "letter", text: "Y" },
    generic: { type: "letter", text: "Y" },
    playstation: { type: "shape", shape: "triangle" },
    switch: { type: "letter", text: "X" },
  },
};

const BUMPERS: Record<ControllerKind, [string, string]> = {
  xbox: ["LB", "RB"],
  steam: ["L1", "R1"],
  generic: ["LB", "RB"],
  playstation: ["L1", "R1"],
  switch: ["L", "R"],
};

/** The button for an action on this controller (keyboard-like defaults without one). */
export function glyphFor(action: PadAction, kind: ControllerKind): Glyph {
  switch (action) {
    case "confirm":
    case "back":
    case "secondary":
    case "tertiary":
      return FACE[action][kind];
    case "prevTab":
      return { type: "bumper", text: BUMPERS[kind][0] };
    case "nextTab":
      return { type: "bumper", text: BUMPERS[kind][1] };
    case "menu":
      return { type: "menu" };
    case "view":
      return { type: "view" };
    case "home":
      return { type: "home" };
    default:
      return { type: "dpad" };
  }
}

/** The button's name for screen readers ("Kreuz", "A", "R1"). */
export function glyphName(glyph: Glyph, names: { cross: string; circle: string; square: string; triangle: string; menu: string; view: string; home: string; dpad: string }): string {
  switch (glyph.type) {
    case "letter":
    case "bumper":
      return glyph.text;
    case "shape":
      return names[glyph.shape];
    default:
      return names[glyph.type];
  }
}
