import { useMessages } from "../../i18n/index.ts";
import type { PlaytimeDay } from "@cove/shared";
import { useState } from "react";
import { formatDateShort, formatMinutesShort, formatWeekday } from "../../lib/format.ts";

/**
 * Single-series bar chart: minutes played per day.
 * Thin accent bars anchored to the baseline, hover/focus tooltip per bar,
 * recessive axis, and a visually hidden table for screen readers.
 */
export function PlaytimeChart({ days, title }: { days: PlaytimeDay[]; title: string }) {
  const t = useMessages();
  const [hovered, setHovered] = useState<number | null>(null);
  const max = Math.max(60, ...days.map((d) => d.minutes));
  const ceiling = Math.ceil(max / 60) * 60;
  const active = hovered === null ? null : days[hovered];

  return (
    <figure className="m-0">
      <figcaption className="sr-only">{title}</figcaption>
      <div className="relative">
        <div className="absolute top-0 right-0 text-xs text-fg-subtle num" aria-hidden>
          {formatMinutesShort(ceiling)}
        </div>
        <div className="flex h-36 items-end gap-1 border-b border-border pt-6" aria-hidden onMouseLeave={() => { setHovered(null); }}>
          {days.map((day, index) => {
            const pct = (day.minutes / ceiling) * 100;
            return (
              <div
                key={day.date}
                className="group relative flex h-full flex-1 items-end justify-center"
                onMouseEnter={() => {
                  setHovered(index);
                }}
              >
                <div
                  className={
                    day.minutes > 0
                      ? "w-full max-w-6 rounded-t-[4px] bg-accent transition-opacity duration-150 ease-out " +
                        (hovered !== null && hovered !== index ? "opacity-50" : "opacity-100")
                      : "h-px w-full max-w-6 bg-border-strong"
                  }
                  style={day.minutes > 0 ? { height: `${String(Math.max(pct, 3))}%` } : undefined}
                />
              </div>
            );
          })}
        </div>
        {active && hovered !== null && (
          <div
            className="pointer-events-none absolute top-0 z-10 -translate-x-1/2 rounded-md border border-border bg-raised px-3 py-2 text-xs"
            style={{ left: `${String(((hovered + 0.5) / days.length) * 100)}%` }}
          >
            <p className="text-fg-muted">
              {formatWeekday(active.date)}, {formatDateShort(active.date)}
            </p>
            <p className="num font-medium text-fg">{active.minutes > 0 ? formatMinutesShort(active.minutes) : t.common.notPlayed}</p>
          </div>
        )}
        <div className="mt-2 flex justify-between text-xs text-fg-subtle" aria-hidden>
          <span>{days[0] ? formatDateShort(days[0].date) : ""}</span>
          <span>{t.common.today}</span>
        </div>
      </div>
      <table className="sr-only">
        <caption>{title}</caption>
        <thead>
          <tr>
            <th scope="col">{t.common.day}</th>
            <th scope="col">{t.common.playtime}</th>
          </tr>
        </thead>
        <tbody>
          {days.map((d) => (
            <tr key={d.date}>
              <td>{formatDateShort(d.date)}</td>
              <td>{formatMinutesShort(d.minutes)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}
