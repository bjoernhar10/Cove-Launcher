import { cx } from "../../lib/cx.ts";

/**
 * Cove's mark: a light rounded square with two dark rings, drawn like the app
 * icon (`src-tauri/icons/icon.svg`) but with the theme tokens instead of fixed
 * colours: square in the text colour, rings in the sidebar colour.
 */
export function CoveLogo({ className }: { className?: string }) {
  return (
    <svg viewBox="88 88 848 848" aria-hidden className={cx("size-5 shrink-0", className)}>
      <rect x="88" y="88" width="848" height="848" rx="176" className="fill-fg" />
      <g fill="none" strokeWidth="40" className="stroke-sidebar">
        <circle cx="512" cy="512" r="292" />
        <circle cx="512" cy="512" r="62" />
      </g>
    </svg>
  );
}
