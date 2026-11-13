import { useState, type ReactNode } from "react";

export interface Bar {
  key: string;
  /** Short label under the bar ("Mar", "Mon"). */
  label: string;
  /** Long label for the tooltip and the table ("March 2026", "Monday"). */
  fullLabel: string;
  value: number;
}

interface BarChartProps {
  title: string;
  bars: Bar[];
  format: (value: number) => string;
  labelHeader: string;
  valueHeader: string;
}

/**
 * Single-series column chart: thin accent bars anchored to the baseline, a label
 * under each bar, a hover/focus tooltip and a visually hidden table for screen
 * readers (the same anatomy as the game page's playtime chart).
 */
export function BarChart({ title, bars, format, labelHeader, valueHeader }: BarChartProps) {
  const [hovered, setHovered] = useState<number | null>(null);
  const max = Math.max(60, ...bars.map((b) => b.value));
  const active = hovered === null ? null : bars[hovered];

  return (
    <figure className="m-0">
      <figcaption className="sr-only">{title}</figcaption>
      <div className="relative">
        <div className="absolute top-0 right-0 text-xs text-fg-subtle num" aria-hidden>
          {format(max)}
        </div>
        <div
          className="flex h-40 items-end gap-2 border-b border-border pt-6"
          aria-hidden
          onMouseLeave={() => {
            setHovered(null);
          }}
        >
          {bars.map((bar, index) => (
            <div
              key={bar.key}
              className="flex h-full flex-1 items-end justify-center"
              onMouseEnter={() => {
                setHovered(index);
              }}
            >
              <div
                className={
                  bar.value > 0
                    ? "w-full max-w-8 rounded-t-[4px] bg-accent transition-opacity duration-150 ease-out " +
                      (hovered !== null && hovered !== index ? "opacity-50" : "opacity-100")
                    : "h-px w-full max-w-8 bg-border-strong"
                }
                style={bar.value > 0 ? { height: `${String(Math.max((bar.value / max) * 100, 3))}%` } : undefined}
              />
            </div>
          ))}
        </div>
        {active && hovered !== null && (
          <div
            className="pointer-events-none absolute top-0 z-10 -translate-x-1/2 rounded-md border border-border bg-raised px-3 py-2 text-xs whitespace-nowrap"
            style={{ left: `${String(((hovered + 0.5) / bars.length) * 100)}%` }}
          >
            <p className="text-fg-muted">{active.fullLabel}</p>
            <p className="num font-medium text-fg">{format(active.value)}</p>
          </div>
        )}
        <div className="mt-2 flex gap-2 text-xs text-fg-subtle" aria-hidden>
          {bars.map((bar) => (
            <span key={bar.key} className="flex-1 truncate text-center">
              {bar.label}
            </span>
          ))}
        </div>
      </div>
      <table className="sr-only">
        <caption>{title}</caption>
        <thead>
          <tr>
            <th scope="col">{labelHeader}</th>
            <th scope="col">{valueHeader}</th>
          </tr>
        </thead>
        <tbody>
          {bars.map((bar) => (
            <tr key={bar.key}>
              <td>{bar.fullLabel}</td>
              <td>{format(bar.value)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}

/** A list row: label and value, with a thin bar for the share of the largest value. */
export function ShareRow({ label, value, amount, max }: { label: ReactNode; value: string; amount: number; max: number }) {
  return (
    <li className="flex flex-col gap-2 py-2">
      <div className="flex items-baseline justify-between gap-4 text-sm">
        <span className="min-w-0 truncate text-fg">{label}</span>
        <span className="num shrink-0 text-fg-muted">{value}</span>
      </div>
      <div className="h-1 rounded-full bg-border" aria-hidden>
        <div className="h-1 rounded-full bg-accent" style={{ width: `${String(max > 0 ? Math.max((amount / max) * 100, 1) : 0)}%` }} />
      </div>
    </li>
  );
}
