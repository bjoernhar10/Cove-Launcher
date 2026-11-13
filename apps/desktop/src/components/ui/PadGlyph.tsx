import type { PadAction } from "@cove/shared";
import { House, Menu, Move, SquareStack } from "lucide-react";
import { useMessages } from "../../i18n/index.ts";
import { glyphFor, glyphName } from "../../lib/padLabels.ts";
import { useGamepads } from "../../stores/gamepad.ts";

/** A controller button as on the connected controller: A, ✕, B on a Switch … (drawn, no brand marks). */
export function PadGlyph({ action }: { action: PadAction }) {
  const t = useMessages();
  const kind = useGamepads((s) => s.kind) ?? "xbox";
  const glyph = glyphFor(action, kind);
  const name = t.pad.button(glyphName(glyph, t.pad.names));
  const ring = "inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-border-strong text-xs font-semibold text-fg";
  switch (glyph.type) {
    case "letter":
      return (
        <span role="img" aria-label={name} className={ring}>
          {glyph.text}
        </span>
      );
    case "bumper":
      return (
        <span role="img" aria-label={name} className="inline-flex h-6 min-w-8 shrink-0 items-center justify-center rounded-md border border-border-strong px-1 text-xs font-semibold text-fg">
          {glyph.text}
        </span>
      );
    case "shape":
      return (
        <span role="img" aria-label={name} className={ring}>
          <svg viewBox="0 0 16 16" width={12} height={12} fill="none" stroke="currentColor" strokeWidth={1.75} aria-hidden>
            {glyph.shape === "cross" && <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" strokeLinecap="round" />}
            {glyph.shape === "circle" && <circle cx="8" cy="8" r="5" />}
            {glyph.shape === "square" && <rect x="3.5" y="3.5" width="9" height="9" rx="1" />}
            {glyph.shape === "triangle" && <path d="M8 3l5.5 9.5h-11z" strokeLinejoin="round" />}
          </svg>
        </span>
      );
    default: {
      const Icon = glyph.type === "menu" ? Menu : glyph.type === "view" ? SquareStack : glyph.type === "home" ? House : Move;
      return (
        <span role="img" aria-label={name} className={ring}>
          <Icon aria-hidden />
        </span>
      );
    }
  }
}

/** The hint bar: what each button does right now. */
export function PadHints({ hints }: { hints: { action: PadAction; label: string }[] }) {
  const t = useMessages();
  return (
    <ul aria-label={t.pad.hints} className="flex flex-wrap items-center justify-end gap-6">
      {hints.map((hint) => (
        <li key={hint.action} className="flex items-center gap-2 text-sm text-fg-muted">
          <PadGlyph action={hint.action} />
          {hint.label}
        </li>
      ))}
    </ul>
  );
}
